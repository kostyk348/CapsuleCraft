//! Todo / task checklist block renderer.

use egui::{vec2, FontFamily, FontId, Frame, Margin, RichText, TextEdit, Ui};
use serde_json::{json, Value};

use crate::i18n::{t, Key, Locale};
use crate::icons::Icon;
use crate::theme;
use crate::{
    checkbox, menu_header, menu_row, menu_separator, now_nanos, popover_frame, status_pill,
    text_button,
};

pub fn draw_todo_block(ui: &mut Ui, block: &mut Value, locale: Locale) -> bool {
    let mut changed = false;
    let mut title = block
        .get("data")
        .and_then(|data| data.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let title_response = ui.add(
        TextEdit::singleline(&mut title)
            .font(FontId::new(15.0, theme::semibold()))
            .text_color(theme::ink())
            .frame(Frame::NONE)
            .margin(Margin::same(0))
            .desired_width(f32::INFINITY)
            .hint_text(t(locale, Key::BlockTodo)),
    );
    if title_response.changed() {
        block["data"]["title"] = json!(title);
        changed = true;
    }
    ui.add_space(7.0);

    let mut items = block
        .get("data")
        .and_then(|data| data.get("items"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut items_changed = false;
    let mut remove_item: Option<usize> = None;

    if items.is_empty() {
        ui.label(
            RichText::new(t(locale, Key::NoTasks))
                .size(11.5)
                .color(theme::faint()),
        );
    }

    for (item_index, item) in items.iter_mut().enumerate() {
        let mut done = item
            .get("status")
            .and_then(Value::as_str)
            .map(|status| status == "done")
            .or_else(|| item.get("done").and_then(Value::as_bool))
            .unwrap_or(false);
        let due = item
            .get("due")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut next_status: Option<String> = None;
        let mut delete = false;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add_space(1.0);
            if checkbox(ui, &mut done).changed() {
                item["done"] = json!(done);
                item["status"] = json!(if done { "done" } else { "todo" });
                items_changed = true;
            }

            let trailing = 56.0 + if due.is_empty() { 0.0 } else { 66.0 } + 8.0;
            let text_w = (ui.available_width() - trailing).max(80.0);
            let mut text = item
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let text_response = ui.add_sized(
                vec2(text_w, 22.0),
                TextEdit::singleline(&mut text)
                    .frame(Frame::NONE)
                    .margin(Margin::same(0))
                    .font(FontId::new(13.5, FontFamily::Proportional))
                    .text_color(if done { theme::muted() } else { theme::ink() }),
            );
            if text_response.changed() {
                item["text"] = json!(text);
                items_changed = true;
            }

            if !due.is_empty() {
                ui.label(
                    RichText::new(due)
                        .family(theme::mono())
                        .size(9.0)
                        .color(theme::faint()),
                );
            }

            let current = item
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or(if done { "done" } else { "todo" })
                .to_string();
            let status = status_pill(ui, &current);
            egui::Popup::menu(&status)
                .frame(popover_frame())
                .align(egui::RectAlign::BOTTOM_END)
                .show(|ui| {
                    ui.set_min_width(170.0);
                    menu_header(ui, t(locale, Key::TaskStatus));
                    for candidate in ["todo", "doing", "done"] {
                        if menu_row(
                            ui,
                            None,
                            t(
                                locale,
                                match candidate {
                                    "todo" => Key::StatusTodo,
                                    "doing" => Key::StatusDoing,
                                    _ => Key::StatusDone,
                                },
                            ),
                            None,
                            current == candidate,
                            true,
                            false,
                        )
                        .clicked()
                        {
                            next_status = Some(candidate.to_string());
                        }
                    }
                    menu_separator(ui);
                    if menu_row(
                        ui,
                        Some(Icon::Trash),
                        t(locale, Key::DeleteTask),
                        None,
                        false,
                        true,
                        true,
                    )
                    .clicked()
                    {
                        delete = true;
                    }
                });
        });

        if let Some(status) = next_status {
            item["status"] = json!(status);
            item["done"] = json!(status == "done");
            items_changed = true;
        }
        if delete {
            remove_item = Some(item_index);
        }
    }

    if let Some(index) = remove_item {
        if index < items.len() {
            items.remove(index);
            items_changed = true;
        }
    }

    ui.add_space(6.0);
    if text_button(ui, Some(Icon::Plus), t(locale, Key::AddTaskShort), false).clicked() {
        items.push(json!({
            "id": format!("item_{}", now_nanos()),
            "text": "",
            "done": false,
            "status": "todo",
            "priority": "normal"
        }));
        items_changed = true;
    }

    if items_changed {
        block["data"]["items"] = Value::Array(items);
        changed = true;
    }
    changed
}
