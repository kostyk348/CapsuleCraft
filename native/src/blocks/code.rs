//! Code snippet / REPL block renderer.
//!
//! Roadmap item: the native shell now executes code, not just stores it.
//! JavaScript runs through the embedded boa engine ([`crate::js_engine`]),
//! JSON is validated and pretty-printed; Python/Rust/SQL stay honest about
//! not having interpreters in the offline build.

use egui::{Align, FontId, Frame, Label, Layout, Margin, RichText, TextEdit, Ui};
use serde_json::{json, Value};

use crate::i18n::{t, Key, Locale};
use crate::icons::Icon;
use crate::theme;
use crate::{ghost, hud, js_engine, menu_header, menu_row, popover_frame, text_button};

/// Executable language labels for the honest "not supported" note.
fn language_label(language: &str) -> Option<&'static str> {
    match language {
        "typescript" => Some("TypeScript"),
        "python" => Some("Python"),
        "rust" => Some("Rust"),
        "sql" => Some("SQL"),
        _ => None,
    }
}

/// Execute the block. Only called for languages that have an interpreter;
/// everything else shows the honest offline note instead of a Run button.
fn run_code(block: &mut Value, code: &str) {
    let language = block
        .get("data")
        .and_then(|data| data.get("language"))
        .and_then(Value::as_str)
        .unwrap_or("javascript")
        .to_string();

    match language.as_str() {
        "javascript" => match js_engine::run_javascript(code) {
            Ok(output) => {
                block["data"]["output"] = json!(output);
                block["data"]
                    .as_object_mut()
                    .map(|data| data.remove("error"));
            }
            Err(err) => {
                block["data"]["error"] = json!(err);
                block["data"]
                    .as_object_mut()
                    .map(|data| data.remove("output"));
            }
        },
        "json" => match serde_json::from_str::<serde_json::Value>(code) {
            Ok(parsed) => {
                let pretty = serde_json::to_string_pretty(&parsed).unwrap_or_default();
                block["data"]["output"] = json!(pretty);
                block["data"]
                    .as_object_mut()
                    .map(|data| data.remove("error"));
            }
            Err(err) => {
                block["data"]["error"] = json!(err.to_string());
                block["data"]
                    .as_object_mut()
                    .map(|data| data.remove("output"));
            }
        },
        _ => {}
    }
}

pub fn draw_code_block(ui: &mut Ui, block: &mut Value, locale: Locale) -> bool {
    let mut changed = false;
    Frame::new()
        .fill(theme::SUNKEN)
        .corner_radius(theme::RADIUS)
        .stroke(theme::hairline())
        .inner_margin(Margin {
            left: 14,
            right: 14,
            top: 10,
            bottom: 12,
        })
        .show(ui, |ui| {
            let mut code = block
                .get("data")
                .and_then(|data| data.get("code"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let language = block
                .get("data")
                .and_then(|data| data.get("language"))
                .and_then(Value::as_str)
                .unwrap_or("javascript")
                .to_string();

            ui.horizontal(|ui| {
                let response = ghost(
                    ui,
                    Icon::Code,
                    Some(language.as_str()),
                    t(locale, Key::Language),
                    true,
                    24.0,
                );
                egui::Popup::menu(&response)
                    .frame(popover_frame())
                    .align(egui::RectAlign::BOTTOM_START)
                    .show(|ui| {
                        ui.set_min_width(150.0);
                        menu_header(ui, t(locale, Key::Language));
                        for candidate in
                            ["javascript", "typescript", "python", "rust", "sql", "json"]
                        {
                            if menu_row(
                                ui,
                                None,
                                candidate,
                                None,
                                language == candidate,
                                true,
                                false,
                            )
                            .clicked()
                            {
                                block["data"]["language"] = json!(candidate);
                                changed = true;
                            }
                        }
                    });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    match language.as_str() {
                        "javascript" | "json" => {
                            if text_button(ui, None, t(locale, Key::CodeRun), false).clicked() {
                                run_code(block, &code);
                                changed = true;
                            }
                        }
                        other => {
                            let note = match language_label(other) {
                                Some(label) => {
                                    format!("{label} — {}", t(locale, Key::CodeUnsupported))
                                }
                                _ => t(locale, Key::CodeUnsupported).to_string(),
                            };
                            let _ = hud(ui, &note, theme::FAINT);
                        }
                    }
                });
            });
            ui.add_space(8.0);

            let rows = code.lines().count().max(4);
            let response = ui.add(
                TextEdit::multiline(&mut code)
                    .font(FontId::new(12.5, theme::mono()))
                    .text_color(theme::INK)
                    .frame(Frame::NONE)
                    .margin(Margin::same(0))
                    .desired_width(f32::INFINITY)
                    .desired_rows(rows)
                    .code_editor()
                    .hint_text(t(locale, Key::PlaceholderCode)),
            );
            if response.changed() {
                block["data"]["code"] = json!(code);
                changed = true;
            }

            let output = block
                .get("data")
                .and_then(|data| data.get("output"))
                .and_then(Value::as_str)
                .filter(|output| !output.trim().is_empty());
            let error = block
                .get("data")
                .and_then(|data| data.get("error"))
                .and_then(Value::as_str)
                .filter(|error| !error.trim().is_empty());

            if let Some(output) = output.or(error) {
                let is_error = error.is_some();
                ui.add_space(10.0);
                let _ = hud(
                    ui,
                    if is_error {
                        t(locale, Key::CodeExecError)
                    } else {
                        t(locale, Key::CodeOutput)
                    },
                    if is_error { theme::BAD } else { theme::FAINT },
                );
                ui.add_space(3.0);
                ui.add(
                    Label::new(
                        RichText::new(output)
                            .family(theme::mono())
                            .size(11.5)
                            .color(if is_error { theme::BAD } else { theme::MUTED }),
                    )
                    .wrap(),
                );
            }
            ui.add_space(6.0);
        });
    changed
}
