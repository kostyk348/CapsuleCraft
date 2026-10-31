//! Data grid / table block renderer and CSV helpers.
//!
//! Roadmap item: `=…` formula cells evaluated by the same offline boa engine
//! as code blocks — `=SUM(A1:A5)`, `=AVG(B2:B9)`, `=MIN`, `=MAX`, `=COUNT`,
//! plain arithmetic and nested cell references. The raw formula is what gets
//! stored; the computed value is what you normally see.

use std::sync::{Mutex, OnceLock};

use egui::{vec2, Align, Button, FontId, Frame, Layout, Margin, RichText, ScrollArea, Stroke, TextEdit, Ui};
use serde_json::{json, Value};

use crate::i18n::{t, Key, Locale};
use crate::mime::Attachment;
use crate::theme;
use crate::{hud, js_engine, text_button};

/// The formula cell currently being edited, across frames.
/// `just` marks the frame right after entering, to request focus once.
static EDIT_CELL: OnceLock<Mutex<Option<EditCell>>> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq)]
struct EditCell {
    block_id: String,
    row: usize,
    col: usize,
    just_entered: bool,
}

fn editing_cell() -> Option<EditCell> {
    EDIT_CELL
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()?
        .clone()
}

fn set_editing(cell: Option<EditCell>) {
    if let Ok(mut slot) = EDIT_CELL.get_or_init(|| Mutex::new(None)).lock() {
        *slot = cell;
    }
}

/// Split CSV into rows. Deliberately simple: no quoted fields, no embedded
/// commas — the same limitation the web grid has.
pub fn parse_csv(raw: &str) -> Vec<Vec<String>> {
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            line.split(',')
                .map(|cell| cell.trim().to_string())
                .collect::<Vec<_>>()
        })
        .collect()
}

pub fn to_csv(table: &[Vec<String>]) -> String {
    table
        .iter()
        .map(|row| row.join(","))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn draw_grid_block(
    ui: &mut Ui,
    block: &mut Value,
    attachments: &mut [Attachment],
    locale: Locale,
) -> bool {
    let source = block
        .get("data")
        .and_then(|data| data.get("source_cid"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let clean = source
        .trim_start_matches("cid:")
        .trim_matches(|ch| ch == '<' || ch == '>')
        .to_string();
    let found = attachments.iter().position(|attachment| {
        attachment.cid == clean
            || format!("cid:{}", attachment.cid) == source
            || attachment.name == clean
    });

    let mut changed = false;
    Frame::new()
        .fill(theme::SUNKEN)
        .corner_radius(theme::RADIUS)
        .stroke(theme::hairline())
        .inner_margin(Margin {
            left: 12,
            right: 12,
            top: 10,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let name = found
                    .map(|index| attachments[index].name.clone())
                    .unwrap_or_else(|| t(locale, Key::BlockGrid).to_string());
                let _ = hud(ui, &name, theme::FAINT);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if found.is_some() {
                        if text_button(ui, None, t(locale, Key::AddRow), false).clicked() {
                            block["data"]["__add_row"] = json!(true);
                        }
                        if text_button(ui, None, t(locale, Key::AddColumn), false).clicked() {
                            block["data"]["__add_col"] = json!(true);
                        }
                    }
                });
            });
            ui.add_space(8.0);

            let Some(index) = found else {
                let _ = hud(ui, t(locale, Key::DataGridMissing), theme::MUTED);
                return;
            };

            let mut table = parse_csv(&attachments[index].content);
            if table.is_empty() {
                table.push(vec![t(locale, Key::RenameField).to_string()]);
                table.push(vec![String::new()]);
            }
            let add_row = block["data"]
                .get("__add_row")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let add_col = block["data"]
                .get("__add_col")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if let Some(data) = block.get_mut("data").and_then(Value::as_object_mut) {
                data.remove("__add_row");
                data.remove("__add_col");
            }
            if add_row {
                let width = table[0].len();
                table.push(vec![String::new(); width]);
            }
            if add_col {
                for row in table.iter_mut() {
                    row.push(String::new());
                }
            }

            let columns = table.iter().map(Vec::len).max().unwrap_or(0).max(1);
            let cell_width = ((ui.available_width() - 24.0) / columns as f32 - 8.0).max(70.0);
            let mut table_changed = false;
            let block_id = block
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            // Immutable copy for formula resolution: a formula may reference
            // cells edited in this very frame; they resolve next frame.
            let snapshot = table.clone();

            ScrollArea::horizontal()
                .id_salt((
                    "grid-scroll",
                    block.get("id").and_then(Value::as_str).unwrap_or(""),
                ))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for (row_index, row) in table.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            for column in 0..columns {
                                if row.len() <= column {
                                    row.push(String::new());
                                }
                                let raw = row[column].clone();
                                let is_formula_cell =
                                    row_index > 0 && raw.trim_start().starts_with('=');
                                let editing = editing_cell()
                                    .is_some_and(|edit| {
                                        edit.block_id == block_id
                                            && edit.row == row_index
                                            && edit.col == column
                                    });

                                if is_formula_cell && !editing {
                                    // Computed display; click to edit the raw formula.
                                    let expr = raw.trim_start()[1..].trim().to_string();
                                    let computed = if expr.is_empty() {
                                        None
                                    } else {
                                        js_engine::eval_formula(&expr, &snapshot, 0)
                                    };
                                    let text = computed.clone().unwrap_or_else(|| "#ERR".into());
                                    let failed = computed.is_none();
                                    let font = FontId::new(11.5, theme::mono());
                                    let color = if failed {
                                        theme::BAD
                                    } else {
                                        theme::ACCENT_INK
                                    };
                                    let response = ui.add_sized(
                                        vec2(cell_width, 22.0),
                                        Button::new(RichText::new(text).font(font).color(color))
                                            .fill(theme::WASH)
                                            .corner_radius(theme::RADIUS),
                                    );
                                    let response = if failed {
                                        response.on_hover_text(t(locale, Key::FormulaHint))
                                    } else {
                                        response.on_hover_text(&raw)
                                    };
                                    if response.clicked() {
                                        set_editing(Some(EditCell {
                                            block_id: block_id.clone(),
                                            row: row_index,
                                            col: column,
                                            just_entered: true,
                                        }));
                                    }
                                    continue;
                                }

                                let mut cell = raw;
                                let font = if row_index == 0 {
                                    FontId::new(11.0, theme::mono_bold())
                                } else {
                                    FontId::new(11.5, theme::mono())
                                };
                                let response = ui.add_sized(
                                    vec2(cell_width, 22.0),
                                    TextEdit::singleline(&mut cell)
                                        .font(font)
                                        .text_color(if row_index == 0 {
                                            theme::MUTED
                                        } else {
                                            theme::INK
                                        })
                                        .frame(Frame::NONE)
                                        .margin(Margin::symmetric(4, 2))
                                        .desired_width(cell_width),
                                );
                                if editing {
                                    let mut edit = editing_cell().expect("checked above");
                                    if edit.just_entered {
                                        response.request_focus();
                                        edit.just_entered = false;
                                        set_editing(Some(edit));
                                    }
                                    if response.lost_focus() {
                                        set_editing(None);
                                    }
                                }
                                if response.changed() {
                                    row[column] = cell;
                                    table_changed = true;
                                }
                            }
                        });
                        let line_y = ui.cursor().min.y;
                        let line_x = ui.cursor().min.x;
                        ui.painter().hline(
                            egui::Rangef::new(line_x, line_x + cell_width * columns as f32),
                            line_y,
                            Stroke::new(1.0, theme::fade(theme::LINE, 0.55)),
                        );
                        ui.add_space(2.0);
                    }
                });

            ui.add_space(8.0);
            let _ = hud(ui, t(locale, Key::FormulaHint), theme::FAINT);

            if table_changed || add_row || add_col {
                attachments[index].content = to_csv(&table);
                attachments[index].size_bytes = attachments[index].content.len();
                changed = true;
            }
        });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csv_parse_and_serialize() {
        let raw = "a, b, c\n1, 2, 3";
        let table = parse_csv(raw);
        assert_eq!(table.len(), 2);
        assert_eq!(table[0], vec!["a", "b", "c"]);
        assert_eq!(table[1], vec!["1", "2", "3"]);
        let serialized = to_csv(&table);
        assert_eq!(serialized, "a,b,c\n1,2,3");
    }

    #[test]
    fn formulas_resolve_over_csv_table() {
        let table = parse_csv("Price,Qty\n10,2\n30,4");
        assert_eq!(
            js_engine::eval_formula("SUM(A2:A3)", &table, 0),
            Some("40".into())
        );
        assert_eq!(
            js_engine::eval_formula("A2*B2 + A3*B3", &table, 0),
            Some("140".into())
        );
        assert_eq!(
            js_engine::eval_formula("COUNT(A2:A3)", &table, 0),
            Some("2".into())
        );
    }
}
