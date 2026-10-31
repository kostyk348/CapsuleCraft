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
}

struct Shared {
    state: Playback,
}

fn shared() -> &'static Arc<Mutex<Shared>> {
    static SHARED: OnceLock<Arc<Mutex<Shared>>> = OnceLock::new();
    SHARED.get_or_init(|| {
        Arc::new(Mutex::new(Shared {
            state: Playback::Stopped,
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
                        Ok(source) => {
                            sink.clear();
                            sink.append(source.buffered());
                            sink.play();
                            current_cid = cid.clone();
                            if let Ok(mut shared) = shared.lock() {
                                shared.state = Playback::Playing { cid };
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
                        if let Ok(mut shared) = shared.lock() {
                            shared.state = Playback::Paused { cid };
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
            Some(Cmd::Stop) => {
                if let Ok((_stream, sink)) = &device {
                    sink.stop();
                    sink.clear();
                }
                current_cid.clear();
                if let Ok(mut shared) = shared.lock() {
                    shared.state = Playback::Stopped;
                }
            }
            None => {
                // Timeout: detect natural end of the current source.
                if !current_cid.is_empty() {
                    if let Ok((_stream, sink)) = &device {
                        if sink.is_paused() {
                            if let Ok(mut shared) = shared.lock() {
                                shared.state = Playback::Paused {
                                    cid: current_cid.clone(),
                                };
                            }
                        } else if sink.empty() {
                            current_cid.clear();
                            if let Ok(mut shared) = shared.lock() {
                                shared.state = Playback::Stopped;
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
}
