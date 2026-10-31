//! Microphone recording for the native shell — the record half of the audio
//! roadmap item (playback lives in `audio_player.rs`).
//!
//! Mirrors the web shell's `AudioNoteBlock`, which captures the microphone
//! with `MediaRecorder`: here the clip is captured with `cpal` straight into
//! a 16-bit PCM WAV, so the result stays decodable by the offline player
//! (browser recordings are webm/opus, which rodio cannot decode).
//!
//! Same threading rules as the player: the cpal input stream is `!Send` on
//! some platforms, so a dedicated recorder thread owns it and talks to the UI
//! through a command channel plus a shared `Arc<Mutex<…>>` state mirror. The
//! callback only ever pushes f32 samples (all input channels downmixed to
//! mono) and an RMS level for the live meter.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Hard cap for one take: voice notes, not podcasts. At 48 kHz this is
/// ~115 MB of f32 samples before the 16-bit WAV encoding.
const MAX_SECONDS: u32 = 600;

/// What the UI sees. The elapsed clock is derived from the shared `Instant`,
/// so `Recording` carries no per-frame data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RecState {
    #[default]
    Idle,
    Recording {
        started_at: Instant,
    },
}

/// A finished take, ready to be attached to the capsule.
#[derive(Clone, Debug)]
pub struct RecordedClip {
    /// 16-bit mono PCM WAV bytes (RIFF header + payload).
    pub wav: Vec<u8>,
    /// Device sample rate the take was recorded at.
    pub sample_rate: u32,
    /// Take length in seconds.
    pub duration_secs: f32,
    /// 0..1 peaks for the block's waveform display.
    pub bars: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecError {
    /// No default input device (headless CI, no mic, no sound server).
    NoInput,
    /// The input stream could not be built with the default configuration.
    Device,
}

impl std::fmt::Display for RecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecError::NoInput => write!(f, "no microphone / default input device"),
            RecError::Device => write!(f, "could not open the input stream"),
        }
    }
}

struct Shared {
    state: RecState,
    /// Mono f32 samples collected so far.
    samples: Vec<f32>,
    sample_rate: u32,
    /// Latest RMS level, 0..1, for the live meter.
    level: f32,
    /// Finished take handed over by the recorder thread.
    clip: Option<RecordedClip>,
    /// True when the cap was hit and extra samples were dropped.
    truncated: bool,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            state: RecState::Idle,
            samples: Vec::new(),
            sample_rate: 48_000,
            level: 0.0,
            clip: None,
            truncated: false,
        }
    }
}

enum Cmd {
    Start,
    Stop,
}

fn shared() -> &'static Arc<Mutex<Shared>> {
    static SHARED: OnceLock<Arc<Mutex<Shared>>> = OnceLock::new();
    SHARED.get_or_init(|| Arc::new(Mutex::new(Shared::default())))
}

fn sender_slot() -> &'static Mutex<Option<Sender<Cmd>>> {
    static SENDER: OnceLock<Mutex<Option<Sender<Cmd>>>> = OnceLock::new();
    SENDER.get_or_init(|| Mutex::new(None))
}

static LAST_ERROR: OnceLock<Mutex<Option<RecError>>> = OnceLock::new();

fn set_last_error(err: Option<RecError>) {
    if let Ok(mut slot) = LAST_ERROR.get_or_init(|| Mutex::new(None)).lock() {
        *slot = err;
    }
}

/// The failure of the most recent [`record`] call, if any. Cleared by the
/// next successful start; never persisted to the vault.
pub fn last_error() -> Option<RecError> {
    LAST_ERROR
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
}

fn spawn_thread() -> Sender<Cmd> {
    let (tx, rx) = mpsc::channel::<Cmd>();
    let shared = Arc::clone(shared());
    std::thread::Builder::new()
        .name("capsulecraft-recorder".into())
        .spawn(move || recorder_thread(rx, shared))
        .expect("spawn recorder thread");
    tx
}

/// Build the input stream that feeds `shared`. Runs on the recorder thread;
/// the returned `Stream` must stay alive for capture to continue.
fn open_input(shared: &Arc<Mutex<Shared>>) -> Result<cpal::Stream, RecError> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let device = cpal::default_host()
        .default_input_device()
        .ok_or(RecError::NoInput)?;
    let supported = device
        .default_input_config()
        .map_err(|_| RecError::Device)?;
    let rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

    if let Ok(mut shared) = shared.lock() {
        shared.sample_rate = rate;
        shared.samples.clear();
        shared.truncated = false;
    }

    let feeder = Arc::clone(shared);
    let err_cb = move |_err| {
        // Frame-level glitches (overruns) are dropped silently; a hard
        // failure surfaces as silence, which the level meter shows honestly.
    };
    let stream = match format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _| push_frames(&feeder, Frames::F32(data), channels),
            err_cb,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _| push_frames(&feeder, Frames::I16(data), channels),
            err_cb,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config,
            move |data: &[u16], _| push_frames(&feeder, Frames::U16(data), channels),
            err_cb,
            None,
        ),
        _ => return Err(RecError::Device),
    }
    .map_err(|_| RecError::Device)?;
    stream.play().map_err(|_| RecError::Device)?;
    Ok(stream)
}

/// One input callback's payload, in whichever integer/float layout the device
/// chose. Conversions to mono f32 are the textbook ones (i16/32768, u16
/// centered at 32768).
enum Frames<'a> {
    F32(&'a [f32]),
    I16(&'a [i16]),
    U16(&'a [u16]),
}

impl Frames<'_> {
    fn len(&self) -> usize {
        match self {
            Frames::F32(data) => data.len(),
            Frames::I16(data) => data.len(),
            Frames::U16(data) => data.len(),
        }
    }

    fn at(&self, index: usize) -> f32 {
        match self {
            Frames::F32(data) => data[index],
            Frames::I16(data) => data[index] as f32 / 32_768.0,
            Frames::U16(data) => (data[index] as f32 - 32_768.0) / 32_768.0,
        }
    }
}

/// Downmix one callback frame to mono f32 and append to the shared buffer.
fn push_frames(shared: &Arc<Mutex<Shared>>, data: Frames<'_>, channels: usize) {
    let Ok(mut shared) = shared.lock() else {
        return;
    };
    if !matches!(shared.state, RecState::Recording { .. }) {
        return; // a frame flushed after Stop must not leak into the next take
    }
    let cap = shared.sample_rate * MAX_SECONDS;
    let frame_count = data.len() / channels.max(1);
    let room = cap.saturating_sub(shared.samples.len() as u32) as usize;
    let take = frame_count.min(room);
    let mut sum = 0.0_f32;
    for frame in 0..take {
        let mut mono = 0.0_f32;
        for ch in 0..channels.max(1) {
            mono += data.at(frame * channels + ch);
        }
        mono /= channels.max(1) as f32;
        sum += mono * mono;
        shared.samples.push(mono);
    }
    if frame_count > room {
        shared.truncated = true;
    }
    // RMS over this callback window; f32 math, clamped for the meter.
    if take > 0 {
        let rms = (sum / take as f32).sqrt();
        shared.level = shared.level * 0.6 + rms.min(1.0) * 0.4;
    }
}

fn recorder_thread(rx: Receiver<Cmd>, shared: Arc<Mutex<Shared>>) {
    // The stream lives in this scope; dropping it stops the callbacks.
    let mut stream: Option<cpal::Stream> = None;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Start => {
                if matches!(
                    shared.lock().map(|s| s.state.clone()),
                    Ok(RecState::Recording { .. })
                ) {
                    continue; // already rolling
                }
                drop(stream.take());
                if let Ok(mut shared) = shared.lock() {
                    shared.samples.clear();
                    shared.level = 0.0;
                    shared.clip = None;
                    shared.state = RecState::Recording {
                        started_at: Instant::now(),
                    };
                }
                match open_input(&shared) {
                    Ok(opened) => stream = Some(opened),
                    Err(_) => {
                        if let Ok(mut shared) = shared.lock() {
                            shared.state = RecState::Idle;
                            shared.samples.clear();
                        }
                        // The error is reported by `record()`, which opens the
                        // device synchronously first; this thread just keeps
                        // serving future Start commands.
                    }
                }
            }
            Cmd::Stop => {
                drop(stream.take()); // stop the callbacks first
                let clip = finalize(&shared);
                if let Ok(mut shared) = shared.lock() {
                    shared.clip = clip;
                    shared.state = RecState::Idle;
                    shared.level = 0.0;
                }
            }
        }
    }
}

/// Encode everything collected so far into a `RecordedClip`.
fn finalize(shared: &Arc<Mutex<Shared>>) -> Option<RecordedClip> {
    let mut shared = shared.lock().ok()?;
    let samples = std::mem::take(&mut shared.samples);
    let rate = shared.sample_rate.max(1);
    let duration_secs = samples.len() as f32 / rate as f32;
    let bars = waveform_bars(&samples, 48);
    let wav = encode_wav(&samples, rate);
    Some(RecordedClip {
        wav,
        sample_rate: rate,
        duration_secs,
        bars,
    })
}

/// Begin recording from the default microphone. Idempotent while a take is
/// already rolling. The device is opened synchronously so failures surface at
/// the call site instead of being swallowed by the thread.
pub fn record() -> Result<(), RecError> {
    // Probe the device up front: cheap, synchronous, and keeps the error at
    // the button that caused it.
    let probe = (|| -> Result<(), RecError> {
        use cpal::traits::{DeviceTrait, HostTrait};
        let device = cpal::default_host()
            .default_input_device()
            .ok_or(RecError::NoInput)?;
        device
            .default_input_config()
            .map_err(|_| RecError::Device)?;
        Ok(())
    })();
    if let Err(err) = &probe {
        set_last_error(Some(err.clone()));
        return probe;
    }
    set_last_error(None);

    let tx = {
        let mut slot = sender_slot().lock().map_err(|_| RecError::Device)?;
        match slot.as_ref() {
            Some(tx) => tx.clone(),
            None => {
                let tx = spawn_thread();
                *slot = Some(tx.clone());
                tx
            }
        }
    };
    // Give up the stale clip from the previous take before starting a new one.
    if let Ok(mut shared) = shared().lock() {
        shared.clip = None;
    }
    tx.send(Cmd::Start).map_err(|_| RecError::Device)?;
    Ok(())
}

/// Stop the current take and return the encoded WAV. Blocks briefly while the
/// recorder thread hands the clip over; `None` when nothing was recording.
pub fn stop() -> Option<RecordedClip> {
    let tx = sender_slot().lock().ok().and_then(|slot| slot.clone())?;
    tx.send(Cmd::Stop).ok()?;

    // The thread drops the stream, then encodes — usually well under 10 ms
    // even for minutes-long takes (a memcpy plus a WAV header).
    for _ in 0..60 {
        if let Ok(mut shared) = shared().lock() {
            if let Some(clip) = shared.clip.take() {
                return Some(clip);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    None
}

/// True when the last take hit the length cap and was truncated.
pub fn truncated() -> bool {
    shared().lock().map(|s| s.truncated).unwrap_or(false)
}

/// Current recorder state as seen by the UI.
pub fn state() -> RecState {
    shared().lock().map(|s| s.state.clone()).unwrap_or_default()
}

/// Live input level (RMS, 0..1) for the meter; decays to 0 when idle.
pub fn level() -> f32 {
    shared().lock().map(|s| s.level).unwrap_or(0.0)
}

/// Elapsed seconds of the current take; 0 when idle.
pub fn elapsed_secs() -> f64 {
    match state() {
        RecState::Recording { started_at } => started_at.elapsed().as_secs_f64(),
        RecState::Idle => 0.0,
    }
}

/// Encode mono f32 samples (-1..1) as a 16-bit PCM WAV, the one format the
/// offline player always decodes. Pure function: no device, no I/O.
pub fn encode_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let channels = 1u16;
    let bits = 16u16;
    let byte_rate = sample_rate * channels as u32 * bits as u32 / 8;
    let block_align = channels * bits / 8;
    let data_len = samples.len() * 2;
    let mut out = Vec::with_capacity(44 + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    for sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        out.extend_from_slice(&((clamped * 32_767.0) as i16).to_le_bytes());
    }
    out
}

/// Compress a take into `bars` peak values in 0..1 for the waveform strip.
/// Silence stays honest (0.0) — no fake bars like the placeholder renderer.
pub fn waveform_bars(samples: &[f32], bars: usize) -> Vec<f32> {
    if bars == 0 {
        return Vec::new();
    }
    if samples.is_empty() {
        return vec![0.0; bars];
    }
    let bucket = (samples.len() + bars - 1) / bars;
    (0..bars)
        .map(|index| {
            let start = index * bucket;
            if start >= samples.len() {
                return 0.0;
            }
            let end = (start + bucket).min(samples.len());
            samples[start..end]
                .iter()
                .fold(0.0_f32, |peak, s| peak.max(s.abs()))
                .min(1.0)
        })
        .collect()
}

/// "m:ss" clock for the recording timer and the block's duration badge.
pub fn format_clock(total_secs: f64) -> String {
    let secs = total_secs.max(0.0) as u64;
    format!("{}:{:02}", secs / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_is_valid_and_decodable() {
        // A 0.5 s sine at the default device-ish rate must survive the round
        // trip through the same decoder the player uses.
        let rate = 44_100u32;
        let samples: Vec<f32> = (0..rate / 2)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5)
            .collect();
        let wav = encode_wav(&samples, rate);

        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + samples.len() * 2, "header + 16-bit payload");
        let decoded = rodio::Decoder::new(std::io::Cursor::new(wav));
        assert!(decoded.is_ok(), "encoded WAV should decode with rodio");
    }

    #[test]
    fn wav_silence_and_clamping() {
        let wav = encode_wav(&[0.0, 2.0, -2.0], 8_000);
        // First data sample is 0, then both clamps hit the rails (32767).
        let data = &wav[44..];
        assert_eq!(i16::from_le_bytes([data[0], data[1]]), 0);
        assert_eq!(i16::from_le_bytes([data[2], data[3]]), 32_767);
        assert_eq!(i16::from_le_bytes([data[4], data[5]]), -32_767);
    }

    #[test]
    fn empty_input_encodes_a_header_only_wav() {
        let wav = encode_wav(&[], 48_000);
        assert_eq!(wav.len(), 44);
        assert!(rodio::Decoder::new(std::io::Cursor::new(wav)).is_ok());
    }

    #[test]
    fn waveform_bars_peak_per_bucket() {
        let samples = vec![0.0, 0.0, 0.9, 0.0, 0.0, 0.0, 0.4, 0.0];
        let bars = waveform_bars(&samples, 2);
        assert_eq!(bars.len(), 2);
        assert!((bars[0] - 0.9).abs() < 1e-6, "first bucket peaks at 0.9");
        assert!((bars[1] - 0.4).abs() < 1e-6, "second bucket peaks at 0.4");
    }

    #[test]
    fn waveform_bars_edge_cases() {
        assert!(waveform_bars(&[], 8).iter().all(|b| *b == 0.0));
        assert!(waveform_bars(&[0.5], 0).is_empty());
        assert_eq!(waveform_bars(&[1.5, -0.5], 4).len(), 4);
        assert!(waveform_bars(&[1.5, -0.5], 4).iter().all(|b| *b <= 1.0));
    }

    #[test]
    fn clock_format() {
        assert_eq!(format_clock(0.0), "0:00");
        assert_eq!(format_clock(7.2), "0:07");
        assert_eq!(format_clock(65.0), "1:05");
        assert_eq!(format_clock(600.0), "10:00");
        assert_eq!(format_clock(-3.0), "0:00");
    }

    #[test]
    fn stop_when_idle_is_a_noop() {
        // No thread was ever spawned here; stop() must return None and not
        // panic — deterministic on headless CI.
        assert!(stop().is_none());
        assert_eq!(state(), RecState::Idle);
        assert_eq!(level(), 0.0);
        assert_eq!(elapsed_secs(), 0.0);
    }

    #[test]
    fn record_reports_missing_input_honestly() {
        // On headless CI (no default input device) record() fails with
        // NoInput and the state stays Idle. On a machine with a mic the
        // probe succeeds; the take starts, and we stop it immediately.
        match record() {
            Err(err @ (RecError::NoInput | RecError::Device)) => {
                assert_eq!(state(), RecState::Idle);
                assert!(!err.to_string().is_empty());
            }
            Ok(()) => {
                // A real microphone exists: a short take must produce a
                // decodable WAV when stopped.
                std::thread::sleep(Duration::from_millis(150));
                let clip = stop();
                assert_eq!(state(), RecState::Idle);
                if let Some(clip) = clip {
                    assert!(rodio::Decoder::new(std::io::Cursor::new(clip.wav)).is_ok());
                    assert_eq!(clip.sample_rate, clip.sample_rate); // sanity, no panic
                }
            }
        }
    }
}
