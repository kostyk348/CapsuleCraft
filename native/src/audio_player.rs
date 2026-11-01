//! Audio playback for the native shell (roadmap item: play MIME audio
//! attachments without leaving the app).
//!
//! cpal's output stream is `!Send`, so it cannot live in a static — instead
//! the first `play()` spawns a dedicated audio thread that owns the stream
//! and the sink, and follows commands through a channel. UI state is mirrored
//! into a shared `Arc<Mutex<Playback>>` and polled with a small timeout so a
//! finished track flips back to `Stopped` without UI intervention.
//!
//! Decoding relies on rodio's built-in decoders (WAV/MP3/Vorbis/FLAC);
//! anything else — notably the browser-recorded webm/opus notes — is reported
//! as unsupported instead of silently failing.

use std::io::Cursor;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rodio::{Decoder, OutputStream, Sink, Source};

/// What the player is doing right now (mirrored by the audio thread).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Playback {
    #[default]
    Stopped,
    Playing {
        cid: String,
    },
    Paused {
        cid: String,
    },
}

enum Cmd {
    Play(String, Vec<u8>),
    Pause(String),
    Resume(String),
    Stop,
    /// Jump the current source to an absolute offset.
    Seek(String, Duration),
    /// Session volume, 0.0..=1.0, applied to this and future tracks.
    Volume(f32),
}

struct Shared {
    state: Playback,
    /// Position of the current source, mirrored from the sink on every poll
    /// so the UI can draw a progress bar without touching the audio thread.
    position: Duration,
    /// Total length of the current source (from the decoder), if known.
    total: Duration,
    volume: f32,
}

fn shared() -> &'static Arc<Mutex<Shared>> {
    static SHARED: OnceLock<Arc<Mutex<Shared>>> = OnceLock::new();
    SHARED.get_or_init(|| {
        Arc::new(Mutex::new(Shared {
            state: Playback::Stopped,
            position: Duration::ZERO,
            total: Duration::ZERO,
            volume: 1.0,
        }))
    })
}

fn sender_slot() -> &'static Mutex<Option<Sender<Cmd>>> {
    static SENDER: OnceLock<Mutex<Option<Sender<Cmd>>>> = OnceLock::new();
    SENDER.get_or_init(|| Mutex::new(None))
}

static LAST_ERROR: OnceLock<Mutex<Option<(String, PlayError)>>> = OnceLock::new();

fn last_error_slot() -> &'static Mutex<Option<(String, PlayError)>> {
    LAST_ERROR.get_or_init(|| Mutex::new(None))
}

fn record_error(cid: &str, err: PlayError) {
    if let Ok(mut slot) = last_error_slot().lock() {
        *slot = Some((cid.to_string(), err));
    }
}

fn clear_error() {
    if let Ok(mut slot) = last_error_slot().lock() {
        *slot = None;
    }
}

fn set_shared(state: Playback) {
    if let Ok(mut shared) = shared().lock() {
        shared.state = state;
    }
}

fn spawn_thread() -> Sender<Cmd> {
    let (tx, rx) = mpsc::channel::<Cmd>();
    let shared = Arc::clone(shared());
    std::thread::Builder::new()
        .name("capsulecraft-audio".into())
        .spawn(move || audio_thread(rx, shared))
        .expect("spawn audio thread");
    tx
}

fn audio_thread(rx: Receiver<Cmd>, shared: Arc<Mutex<Shared>>) {
    let setup = || -> Result<(OutputStream, Sink), PlayError> {
        let (stream, handle) = OutputStream::try_default()?;
        let sink = Sink::try_new(&handle)?;
        sink.pause(); // nothing queued yet
        Ok((stream, sink))
    };

    let mut device = setup();
    let mut current_cid = String::new();
    let mut current_volume = shared
        .lock()
        .map(|shared| shared.volume)
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);

    loop {
        // Poll commands with a timeout so natural end-of-track is detected.
        let cmd = if device.is_ok() {
            rx.recv_timeout(Duration::from_millis(400)).ok()
        } else {
            // No device: block until the next Play attempt retries setup.
            let cmd = rx.recv().ok();
            device = setup();
            cmd
        };

        match cmd {
            Some(Cmd::Play(cid, bytes)) => {
                current_cid.clear();
                match &device {
                    Ok((_stream, sink)) => match Decoder::new(Cursor::new(bytes)) {
                        Ok(mut source) => {
                            // Note: the decoder is appended un-buffered —
                            // `Buffered` cannot seek, and seek is a feature.
                            let total = source.total_duration().unwrap_or_default();
                            sink.clear();
                            sink.append(source);
                            sink.set_volume(current_volume);
                            sink.play();
                            current_cid = cid.clone();
                            if let Ok(mut shared) = shared.lock() {
                                shared.state = Playback::Playing { cid };
                                shared.position = Duration::ZERO;
                                shared.total = total;
                            }
                        }
                        Err(_) => {
                            record_error(&cid, PlayError::Unsupported);
                            if let Ok(mut shared) = shared.lock() {
                                shared.state = Playback::Stopped;
                            }
                        }
                    },
                    Err(err) => {
                        record_error(&cid, err.clone());
                        if let Ok(mut shared) = shared.lock() {
                            shared.state = Playback::Stopped;
                        }
                    }
                }
            }
            Some(Cmd::Pause(cid)) => {
                if let Ok((_stream, sink)) = &device {
                    if current_cid == cid {
                        sink.pause();
                        let position = sink.get_pos();
                        if let Ok(mut shared) = shared.lock() {
                            shared.state = Playback::Paused { cid };
                            shared.position = position;
                        }
                    }
                }
            }
            Some(Cmd::Resume(cid)) => {
                if let Ok((_stream, sink)) = &device {
                    if current_cid == cid {
                        sink.play();
                        if let Ok(mut shared) = shared.lock() {
                            shared.state = Playback::Playing { cid };
                        }
                    }
                }
            }
            Some(Cmd::Seek(cid, pos)) => {
                if let Ok((_stream, sink)) = &device {
                    if current_cid == cid {
                        // A failed seek just leaves the playhead where it was.
                        let _ = sink.try_seek(pos);
                        let position = sink.get_pos();
                        if let Ok(mut shared) = shared.lock() {
                            shared.position = position;
                        }
                    }
                }
            }
            Some(Cmd::Volume(value)) => {
                current_volume = value.clamp(0.0, 1.0);
                if let Ok((_stream, sink)) = &device {
                    sink.set_volume(current_volume);
                }
                if let Ok(mut shared) = shared.lock() {
                    shared.volume = current_volume;
                }
            }
            Some(Cmd::Stop) => {
                if let Ok((_stream, sink)) = &device {
                    sink.stop();
                    sink.clear();
                }
                current_cid.clear();
                if let Ok(mut shared) = shared.lock() {
                    shared.state = Playback::Stopped;
                    shared.position = Duration::ZERO;
                    shared.total = Duration::ZERO;
                }
            }
            None => {
                // Timeout: mirror the playhead and detect natural end.
                if !current_cid.is_empty() {
                    if let Ok((_stream, sink)) = &device {
                        if sink.is_paused() {
                            let position = sink.get_pos();
                            if let Ok(mut shared) = shared.lock() {
                                shared.state = Playback::Paused {
                                    cid: current_cid.clone(),
                                };
                                shared.position = position;
                            }
                        } else if sink.empty() {
                            current_cid.clear();
                            if let Ok(mut shared) = shared.lock() {
                                shared.state = Playback::Stopped;
                                shared.position = Duration::ZERO;
                                shared.total = Duration::ZERO;
                            }
                        } else {
                            let position = sink.get_pos();
                            if let Ok(mut shared) = shared.lock() {
                                shared.position = position;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Extensions rodio can decode out of the box.
pub fn supported_extension(name: &str) -> bool {
    let ext = name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(ext.as_str(), "wav" | "mp3" | "ogg" | "oga" | "flac")
}

/// Media types produced by the web MediaRecorder; rodio cannot decode them.
pub fn is_webm(mime: &str, name: &str) -> bool {
    let mime = mime.to_ascii_lowercase();
    mime.starts_with("audio/webm") || name.to_ascii_lowercase().ends_with(".webm")
}

/// Begin (or restart) playback of `bytes` tagged with `cid`. The source keeps
/// playing in the background on the audio thread.
pub fn play(cid: &str, bytes: &[u8]) -> Result<(), PlayError> {
    // Cheap pre-check so garbage bytes fail synchronously and stay testable
    // without an output device.
    if Decoder::new(Cursor::new(bytes.to_vec())).is_err() {
        record_error(cid, PlayError::Unsupported);
        return Err(PlayError::Unsupported);
    }
    clear_error();

    let tx = {
        let mut slot = sender_slot().lock().map_err(|_| PlayError::Device)?;
        match slot.as_ref() {
            Some(tx) if tx.send(Cmd::Stop).is_ok() => tx.clone(),
            _ => {
                let tx = spawn_thread();
                *slot = Some(tx.clone());
                tx
            }
        }
    };
    tx.send(Cmd::Play(cid.to_string(), bytes.to_vec()))
        .map_err(|_| PlayError::Device)?;
    Ok(())
}

/// Pause the current source (no-op if another cid is playing).
pub fn pause(cid: &str) {
    if let Some(tx) = sender_slot().lock().ok().and_then(|slot| slot.clone()) {
        let _ = tx.send(Cmd::Pause(cid.to_string()));
    }
}

/// Resume a paused source (no-op if another cid is playing).
pub fn resume(cid: &str) {
    if let Some(tx) = sender_slot().lock().ok().and_then(|slot| slot.clone()) {
        let _ = tx.send(Cmd::Resume(cid.to_string()));
    }
}

/// Jump the source tagged with `cid` to an absolute offset. Clamped into the
/// track; a no-op when nothing (or something else) is playing.
pub fn seek(cid: &str, pos: Duration) {
    if let Some(tx) = sender_slot().lock().ok().and_then(|slot| slot.clone()) {
        let _ = tx.send(Cmd::Seek(cid.to_string(), pos));
    }
}

/// Session playback volume, 0.0..=1.0. Applies to the current source (if any)
/// and to every track started afterwards; survives device re-setup.
pub fn set_volume(value: f32) {
    let value = value.clamp(0.0, 1.0);
    if let Ok(mut shared) = shared().lock() {
        shared.volume = value;
    }
    if let Some(tx) = sender_slot().lock().ok().and_then(|slot| slot.clone()) {
        let _ = tx.send(Cmd::Volume(value));
    }
}

/// The session volume last set via [`set_volume`] (starts at 1.0).
pub fn volume() -> f32 {
    shared().lock().map(|shared| shared.volume).unwrap_or(1.0)
}

/// `(position, total)` of the current source in seconds; `(0.0, 0.0)` when
/// nothing is loaded. Mirrored from the audio thread on its poll interval.
pub fn progress() -> (f64, f64) {
    shared()
        .lock()
        .map(|shared| (shared.position.as_secs_f64(), shared.total.as_secs_f64()))
        .unwrap_or((0.0, 0.0))
}

/// Drop whatever is playing and forget the current cid.
pub fn stop() {
    if let Some(tx) = sender_slot().lock().ok().and_then(|slot| slot.clone()) {
        let _ = tx.send(Cmd::Stop);
    }
    set_shared(Playback::Stopped);
}

/// The failure of the most recent [`play`] call for `cid`, if any. Cleared by
/// the next successful play; never persisted to the vault.
pub fn error_for(cid: &str) -> Option<PlayError> {
    let slot = last_error_slot().lock().ok()?;
    match slot.as_ref() {
        Some((failed_cid, err)) if failed_cid == cid => Some(err.clone()),
        _ => None,
    }
}

/// Current playback state as seen by the UI.
pub fn state() -> Playback {
    shared()
        .lock()
        .map(|shared| shared.state.clone())
        .unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayError {
    /// No usable output device (headless CI, no sound server).
    Device,
    /// rodio has no decoder for these bytes (e.g. webm/opus).
    Unsupported,
}

impl std::fmt::Display for PlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlayError::Device => write!(f, "no audio output device"),
            PlayError::Unsupported => write!(f, "unsupported audio format"),
        }
    }
}

impl From<rodio::StreamError> for PlayError {
    fn from(_: rodio::StreamError) -> Self {
        PlayError::Device
    }
}

impl From<rodio::PlayError> for PlayError {
    fn from(_: rodio::PlayError) -> Self {
        PlayError::Device
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 16-bit mono PCM WAV with 64 silent samples, 8 kHz.
    fn tiny_wav() -> Vec<u8> {
        let samples = vec![0i16; 64];
        let data = samples
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect::<Vec<u8>>();
        let sample_rate = 8000u32;
        let bits = 16u16;
        let channels = 1u16;
        let byte_rate = sample_rate * channels as u32 * bits as u32 / 8;
        let block_align = channels * bits / 8;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&((36 + data.len()) as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&byte_rate.to_le_bytes());
        out.extend_from_slice(&block_align.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn format_gates() {
        assert!(supported_extension("voice-note.WAV"));
        assert!(supported_extension("song.mp3"));
        assert!(supported_extension("loop.OGG"));
        assert!(supported_extension("take.flac"));
        assert!(!supported_extension("record.webm"));
        assert!(!supported_extension("noext"));

        assert!(is_webm("audio/webm;codecs=opus", "rec.webm"));
        assert!(!is_webm("audio/mpeg", "song.mp3"));
    }

    #[test]
    fn wav_bytes_decode() {
        // The pure decode path must succeed without an output device.
        let bytes = tiny_wav();
        let decoded = Decoder::new(Cursor::new(bytes));
        assert!(decoded.is_ok(), "tiny WAV should decode");
    }

    #[test]
    fn garbage_is_rejected_synchronously() {
        // Random bytes fail in play() itself, before any device is opened —
        // deterministic even in headless CI.
        assert_eq!(play("cid-x", &[0u8; 32]), Err(PlayError::Unsupported));
        assert_eq!(error_for("cid-x"), Some(PlayError::Unsupported));
        assert_eq!(error_for("other"), None);
    }

    #[test]
    fn state_is_stopped_without_playback() {
        stop();
        assert_eq!(state(), Playback::Stopped);
    }

    #[test]
    fn volume_roundtrips_and_clamps() {
        set_volume(0.25);
        assert_eq!(volume(), 0.25);
        set_volume(7.0); // out of range clamps into 0..=1
        assert_eq!(volume(), 1.0);
        set_volume(-1.0);
        assert_eq!(volume(), 0.0);
        set_volume(1.0);
        assert_eq!(volume(), 1.0);
    }

    #[test]
    fn progress_is_zero_when_stopped() {
        stop();
        assert_eq!(progress(), (0.0, 0.0));
    }

    #[test]
    fn seek_without_playback_is_a_no_op() {
        // Must not panic nor spawn a thread just for a stray seek.
        seek("cid-x", Duration::from_secs(3));
        stop();
        assert_eq!(state(), Playback::Stopped);
    }

    #[test]
    fn tiny_wav_reports_total_duration() {
        // The decoder path feeding the progress bar must know the length.
        let source = Decoder::new(Cursor::new(tiny_wav())).expect("decodes");
        let total = source.total_duration();
        assert!(total.is_some(), "WAV decoder reports total_duration");
        let total = total.unwrap();
        let rate = 8000.0;
        assert!((total.as_secs_f64() - 64.0 / rate).abs() < 0.01);
    }
}
