//! Audio player / note block renderer.

use base64::Engine;
use egui::{
    vec2, Align, FontFamily, FontId, Frame, Layout, Margin, Rect, RichText, Sense, Stroke,
    TextEdit, Ui,
};
use serde_json::{json, Value};

use crate::i18n::{t, Key, Locale};
use crate::icons::{self, Icon};
use crate::mime::Attachment;
use crate::{audio_player, theme};
use crate::{guess_mime, hud, sanitize_cid, text_button};

pub fn draw_audio_block(
    ui: &mut Ui,
    block: &mut Value,
    attachments: &[Attachment],
    locale: Locale,
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
        .fill(theme::SUNKEN)
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
                    theme::ACCENT_INK,
                );
                ui.add_space(20.0);
                let name = file
                    .map(|attachment| attachment.name.clone())
                    .unwrap_or_else(|| t(locale, Key::AudioNoFile).to_string());
                ui.label(RichText::new(name).size(12.0).color(theme::INK));
                ui.add_space(6.0);
                let duration = block
                    .get("data")
                    .and_then(|data| data.get("duration"))
                    .and_then(Value::as_str)
                    .unwrap_or("0:00");
                let _ = hud(ui, duration, theme::FAINT);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if text_button(ui, Some(Icon::Plus), t(locale, Key::AttachAudio), false).clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new()
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
                                let encoded =
                                    base64::engine::general_purpose::STANDARD.encode(&bytes);
                                let size = bytes.len();
                                block["data"]["audioCid"] = json!(format!("cid:{cid}"));
                                block["data"]["audioMime"] = json!(mime.clone());
                                block["data"]["audioName"] = json!(name.clone());
                                block["data"]["__attach"] = json!({
                                    "name": name,
                                    "cid": cid,
                                    "mime": mime,
                                    "b64": encoded,
                                    "size": size,
                                });
                                changed = true;
                            }
                        }
                    }
                });
            });
            ui.add_space(10.0);

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
                    Stroke::new(1.0, theme::fade(theme::ACCENT_INK, 0.55)),
                );
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
                let _ = is_mine;

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
                    }
                });

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
                    let _ = hud(ui, &status, theme::ACCENT_INK);
                } else if !playable {
                    if decodable {
                        // Real bytes, but webm/opus from the MediaRecorder —
                        // say so instead of a dead button.
                        let _ = hud(ui, t(locale, Key::AudioUnsupportedFmt), theme::FAINT);
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
            let rows = transcript.lines().count().max(2);
            let response = ui.add(
                TextEdit::multiline(&mut transcript)
                    .font(FontId::new(13.0, FontFamily::Proportional))
                    .text_color(theme::MUTED)
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
                let _ = hud(ui, message, theme::FAINT);
            }
        });
    changed
}
