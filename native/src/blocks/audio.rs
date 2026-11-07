//! Audio player / note block renderer.

use base64::Engine;
use chrono::Local;
use egui::{
    pos2, vec2, Align, Color32, FontFamily, FontId, Frame, Layout, Margin, Rect, RichText, Sense,
    Stroke, TextEdit, Ui,
};
use serde_json::{json, Value};
use std::time::Duration;

use super::BlockOp;
use crate::audio_recorder::{self, RecState};
use crate::i18n::{t, Key, Locale};
use crate::icons::{self, Icon};
use crate::mime::Attachment;
use crate::{audio_player, theme};
use crate::{guess_mime, hud, sanitize_cid, text_button};

/// The one non-palette color in the app: a live mic take is universally red.
const REC_RED: Color32 = Color32::from_rgb(0xe5, 0x48, 0x3d);

pub fn draw_audio_block(
    ui: &mut Ui,
    block: &mut Value,
    attachments: &[Attachment],
    locale: Locale,
    op: &mut Option<BlockOp>,
) -> bool {
    let mut changed = false;
    let audio_cid = block
        .get("data")
        .and_then(|data| data.get("audioCid"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim_start_matches("cid:")
        .to_string();
    let file = attachments
        .iter()
        .find(|attachment| attachment.cid == audio_cid);

    Frame::new()
        .fill(theme::sunken())
        .corner_radius(theme::RADIUS)
        .stroke(theme::hairline())
        .inner_margin(Margin {
            left: 14,
            right: 14,
            top: 12,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::paint(
                    ui.painter(),
                    Rect::from_min_size(ui.cursor().min + vec2(0.0, 2.0), vec2(14.0, 14.0)),
                    Icon::Audio,
                    theme::accent_ink(),
                );
                ui.add_space(20.0);
                let name = file
                    .map(|attachment| attachment.name.clone())
                    .unwrap_or_else(|| t(locale, Key::AudioNoFile).to_string());
                ui.label(RichText::new(name).size(12.0).color(theme::ink()));
                ui.add_space(6.0);
                let duration = block
                    .get("data")
                    .and_then(|data| data.get("duration"))
                    .and_then(Value::as_str)
                    .unwrap_or("0:00");
                let _ = hud(ui, duration, theme::faint());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if text_button(ui, Some(Icon::Plus), t(locale, Key::AttachAudio), false).clicked()
                    {
                        if let Some(path) = crate::util::dialog::FileDialog::new()
                            .set_title(t(locale, Key::AttachAudio))
                            .add_filter("audio", &["mp3", "wav", "ogg", "m4a", "flac", "webm"])
                            .pick_file()
                        {
                            if let Ok(bytes) = std::fs::read(&path) {
                                let name = path
                                    .file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or("audio")
                                    .to_string();
                                let cid = format!("audio-{}", sanitize_cid(&name));
                                let mime = guess_mime(&name);
                                let size = bytes.len();
                                let encoded =
                                    base64::engine::general_purpose::STANDARD.encode(&bytes);
                                block["data"]["audioCid"] = json!(format!("cid:{cid}"));
                                block["data"]["audioMime"] = json!(mime.clone());
                                block["data"]["audioName"] = json!(name.clone());
                                // The renderer only sees an immutable attachment
                                // slice — ask the document view to add it.
                                *op = Some(BlockOp::AttachAudio {
                                    name,
                                    cid,
                                    mime,
                                    b64: encoded,
                                    size,
                                });
                                changed = true;
                            }
                        }
                    }
                    let recording = matches!(audio_recorder::state(), RecState::Recording { .. });
                    if !recording
                        && text_button(
                            ui,
                            Some(Icon::Record),
                            t(locale, Key::AudioRecord),
                            false,
                        )
                        .clicked()
                    {
                        // Playback and capture share the device; keep them apart.
                        audio_player::stop();
                        let _ = audio_recorder::record();
                    }
                });
            });
            ui.add_space(10.0);

            if matches!(audio_recorder::state(), RecState::Recording { .. }) {
                // ── Live take: pulsing dot, honest clock, RMS level meter ──
                ui.horizontal(|ui| {
                    let now = ui.input(|input| input.time);
                    let pulse = (0.55 + 0.45 * (now * 4.0).sin()) as f32;
                    let (dot_rect, _) =
                        ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                    ui.painter().circle_filled(
                        dot_rect.center(),
                        4.0 * pulse,
                        theme::fade(REC_RED, pulse),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(t(locale, Key::AudioRecLive))
                            .size(12.0)
                            .color(REC_RED),
                    );
                    ui.add_space(6.0);
                    let _ = hud(
                        ui,
                        &audio_recorder::format_clock(audio_recorder::elapsed_secs()),
                        REC_RED,
                    );
                    let level = audio_recorder::level().clamp(0.0, 1.0);
                    let meter_width = (ui.available_width() - 150.0).max(40.0);
                    let (meter, _) = ui
                        .allocate_exact_size(vec2(meter_width, 8.0), Sense::hover());
                    ui.painter().rect_filled(
                        Rect::from_min_size(meter.min, vec2(meter.width(), 8.0)),
                        4.0,
                        theme::wash(),
                    );
                    if level > 0.001 {
                        ui.painter().rect_filled(
                            Rect::from_min_size(
                                meter.min,
                                vec2(meter.width() * level, 8.0),
                            ),
                            4.0,
                            REC_RED,
                        );
                    }
                    if text_button(ui, Some(Icon::Close), t(locale, Key::AudioRecStop), true)
                        .clicked()
                    {
                        if let Some(clip) = audio_recorder::stop() {
                            // A clicked-then-released empty take is not a note.
                            if clip.duration_secs >= 0.2 {
                                let name =
                                    format!("voice-{}.wav", Local::now().format("%Y%m%d-%H%M%S"));
                                let cid = format!(
                                    "voice-{}",
                                    chrono::Utc::now().timestamp_millis()
                                );
                                let mime = "audio/wav".to_string();
                                let size = clip.wav.len();
                                let b64 = base64::engine::general_purpose::STANDARD
                                    .encode(&clip.wav);
                                block["data"]["audioCid"] = json!(format!("cid:{cid}"));
                                block["data"]["audioMime"] = json!(mime.clone());
                                block["data"]["audioName"] = json!(name.clone());
                                block["data"]["duration"] =
                                    json!(audio_recorder::format_clock(clip.duration_secs as f64));
                                block["data"]["waveform"] = json!(clip.bars);
                                *op = Some(BlockOp::AttachAudio {
                                    name,
                                    cid,
                                    mime,
                                    b64,
                                    size,
                                });
                                changed = true;
                            }
                        }
                    }
                });
            } else {
                // Waveform: real amplitude values when the file carried them, bars otherwise.
                let waveform: Vec<f32> = block
                .get("data")
                .and_then(|data| data.get("waveform"))
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_f64)
                        .map(|value| value as f32)
                        .collect()
                })
                .unwrap_or_default();
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
            let bars = waveform.len().max(24);
            let peak = waveform.iter().cloned().fold(1.0_f32, f32::max);
            for index in 0..bars {
                let value = waveform
                    .get(index)
                    .copied()
                    .unwrap_or_else(|| 3.0 + ((index * 7) % 11) as f32);
                let height = (value / peak * 26.0).clamp(2.0, 26.0);
                let x = rect.left() + index as f32 * (rect.width() / bars as f32);
                ui.painter().vline(
                    x,
                    egui::Rangef::new(
                        rect.center().y - height / 2.0,
                        rect.center().y + height / 2.0,
                    ),
                    Stroke::new(1.0, theme::fade(theme::accent_ink(), 0.55)),
                );
            }
            }

            ui.add_space(8.0);

            // Playback controls (roadmap: native audio playback via rodio).
            if let Some(attachment) = file {
                let raw = attachment.raw_b64.as_deref().unwrap_or("");
                let decode = || {
                    base64::engine::general_purpose::STANDARD
                        .decode(raw)
                        .ok()
                        .filter(|bytes| !bytes.is_empty())
                };
                let decodable = decode().is_some();
                let playable = decodable
                    && audio_player::supported_extension(&attachment.name)
                    && !audio_player::is_webm(&attachment.mime_type, &attachment.name);
                let playback = audio_player::state();
                let is_mine = matches!(&playback,
                    audio_player::Playback::Playing { cid } | audio_player::Playback::Paused { cid }
                        if cid == &audio_cid);

                ui.horizontal(|ui| {
                    if playable {
                        let label = match &playback {
                            audio_player::Playback::Playing { cid } if cid == &audio_cid => {
                                t(locale, Key::AudioPause)
                            }
                            _ => t(locale, Key::AudioPlay),
                        };
                        let active = matches!(&playback,
                            audio_player::Playback::Playing { cid } | audio_player::Playback::Paused { cid }
                                if cid == &audio_cid);
                        if text_button(ui, Some(Icon::Audio), label, active).clicked() {
                            match &playback {
                                audio_player::Playback::Playing { cid } if cid == &audio_cid => {
                                    audio_player::pause(&audio_cid);
                                }
                                audio_player::Playback::Paused { cid } if cid == &audio_cid => {
                                    audio_player::resume(&audio_cid);
                                }
                                _ => match decode() {
                                    Some(bytes) => {
                                        // Errors are tracked inside the player;
                                        // nothing is written into the block.
                                        let _ = audio_player::play(&audio_cid, &bytes);
                                    }
                                    None => {}
                                },
                            }
                        }
                        if text_button(ui, Some(Icon::Close), t(locale, Key::AudioStop), false).clicked() {
                            audio_player::stop();
                        }
                        // Mini volume slider: track, fill, knob — on palette.
                        let volume = audio_player::volume();
                        let (vol_rect, vol_resp) =
                            ui.allocate_exact_size(vec2(64.0, 12.0), Sense::click_and_drag());
                        ui.painter().rect_filled(
                            vol_rect.shrink2(vec2(0.0, 4.0)),
                            2.0,
                            theme::wash(),
                        );
                        ui.painter().rect_filled(
                            Rect::from_min_size(
                                vol_rect.min + vec2(0.0, 4.0),
                                vec2(vol_rect.width() * volume, 4.0),
                            ),
                            2.0,
                            theme::accent_ink(),
                        );
                        ui.painter().circle_filled(
                            pos2(vol_rect.min.x + vol_rect.width() * volume, vol_rect.center().y),
                            4.0,
                            theme::ink(),
                        );
                        if let Some(pos) = vol_resp
                            .interact_pointer_pos()
                            .filter(|_| vol_resp.dragged() || vol_resp.clicked())
                        {
                            let frac = ((pos.x - vol_rect.min.x) / vol_rect.width()).clamp(0.0, 1.0);
                            audio_player::set_volume(frac);
                        }
                        vol_resp.on_hover_text(t(locale, Key::AudioVolume));
                    }
                });

                // Progress bar with scrubbing — only for the loaded track.
                if is_mine {
                    let (pos_secs, total_secs) = audio_player::progress();
                    if total_secs > 0.0 {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            let _ = hud(ui, &audio_recorder::format_clock(pos_secs), theme::faint());
                            let bar_width = (ui.available_width() - 88.0).max(60.0);
                            let (bar, resp) = ui
                                .allocate_exact_size(vec2(bar_width, 12.0), Sense::click_and_drag());
                            let frac = (pos_secs / total_secs).clamp(0.0, 1.0);
                            ui.painter().rect_filled(
                                bar.shrink2(vec2(0.0, 4.0)),
                                2.0,
                                theme::wash(),
                            );
                            ui.painter().rect_filled(
                                Rect::from_min_size(
                                    bar.min + vec2(0.0, 4.0),
                                    vec2(bar.width() * frac as f32, 4.0),
                                ),
                                2.0,
                                theme::accent_ink(),
                            );
                            ui.painter().circle_filled(
                                pos2(bar.min.x + bar.width() * frac as f32, bar.center().y),
                                4.0,
                                theme::ink(),
                            );
                            if let Some(pos) = resp
                                .interact_pointer_pos()
                                .filter(|_| resp.dragged() || resp.clicked())
                            {
                                let target =
                                    (((pos.x - bar.min.x) / bar.width()).clamp(0.0, 1.0)
                                        * total_secs as f32) as f64;
                                audio_player::seek(&audio_cid, Duration::from_secs_f64(target));
                            }
                            let _ =
                                hud(ui, &audio_recorder::format_clock(total_secs), theme::faint());
                        });
                    }
                }

                let status = match &playback {
                    audio_player::Playback::Playing { cid } if cid == &audio_cid => {
                        Some(t(locale, Key::AudioPlaying).to_string())
                    }
                    audio_player::Playback::Paused { cid } if cid == &audio_cid => {
                        Some(t(locale, Key::AudioPaused).to_string())
                    }
                    _ => None,
                };
                if let Some(status) = status {
                    let _ = hud(ui, &status, theme::accent_ink());
                } else if !playable {
                    if decodable {
                        // Real bytes, but webm/opus from the MediaRecorder —
                        // say so instead of a dead button.
                        let _ = hud(ui, t(locale, Key::AudioUnsupportedFmt), theme::faint());
                    }
                }
                ui.add_space(4.0);
            }

            let mut transcript = block
                .get("data")
                .and_then(|data| data.get("transcript"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            // Local STT: only for real decodable takes (webm imports cannot be
            // decoded by the in-process whisper WAV path).
            let has_decodable_audio = file
                .as_ref()
                .and_then(|attachment| attachment.raw_b64.as_deref())
                .map(|raw| {
                    base64::engine::general_purpose::STANDARD
                        .decode(raw)
                        .ok()
                        .is_some_and(|bytes| !bytes.is_empty())
                })
                .unwrap_or(false);
            if !audio_cid.is_empty() && has_decodable_audio {
                ui.horizontal(|ui| {
                    if text_button(
                        ui,
                        Some(Icon::Audio),
                        t(locale, Key::TranscribeAudio),
                        false,
                    )
                    .clicked()
                    {
                        *op = Some(BlockOp::RequestTranscribe(audio_cid.clone()));
                    }
                });
            }
            let rows = transcript.lines().count().max(2);
            let response = ui.add(
                TextEdit::multiline(&mut transcript)
                    .font(FontId::new(13.0, FontFamily::Proportional))
                    .text_color(theme::muted())
                    .frame(Frame::NONE)
                    .margin(Margin::same(0))
                    .desired_width(f32::INFINITY)
                    .desired_rows(rows)
                    .hint_text(t(locale, Key::Transcript)),
            );
            if response.changed() {
                block["data"]["transcript"] = json!(transcript);
                changed = true;
            }
            ui.add_space(6.0);
            if let Some(err) = audio_player::error_for(&audio_cid) {
                let message = match err {
                    audio_player::PlayError::Device => t(locale, Key::AudioDeviceFail),
                    audio_player::PlayError::Unsupported => t(locale, Key::AudioUnsupportedFmt),
                };
                let _ = hud(ui, message, theme::faint());
            }
            if audio_recorder::last_error().is_some() {
                let _ = hud(ui, t(locale, Key::AudioNoMic), theme::faint());
            }
        });
    changed
}
