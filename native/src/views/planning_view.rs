//! Planning view: one task board over every page's todo blocks.
//!
//! Ported from `src/components/PlanningView.tsx` + `src/engine/planning.ts`:
//! agenda buckets, kanban columns, a Gantt timeline, a month calendar, the
//! cross-project filter bar, quick add, dependencies and ICS export.
//!
//! The view never mutates while it draws: it collects `Edit`s and applies them
//! to the vault afterwards, so egui borrows stay simple and every change goes
//! through one auditable path.

use chrono::Datelike;
use egui::{
    pos2, vec2, Align, Align2, Color32, FontId, Frame, Layout, Margin, Rect, RichText, ScrollArea,
    Sense, Stroke, TextEdit, Ui,
};

use crate::i18n::{t, Key, Locale};
use crate::icons::{self, Icon};
use crate::planning::{
    self, add_days, bucket_of, human_date, is_blocked, is_date_key, month_grid, month_label,
    task_bar, task_stats, tasks_on_day, today_key, weekday_labels, Bucket, GanttRange, PlannedTask,
    TaskFilters, TodoItem,
};
use crate::theme;
use crate::vault::Vault;
use crate::{ghost, hud, icon_button, menu_header, menu_row, menu_separator, popover_frame, status_pill, tag_chip, text_button, truncate_chars, checkbox};

use super::{centred, empty_state, header, section, stat, segments, ViewAction};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Agenda,
    Kanban,
    Gantt,
    Calendar,
}

pub struct PlanningState {
    pub mode: Mode,
    pub filters: TaskFilters,
    pub quick_text: String,
    pub quick_due: String,
    pub quick_priority: String,
    pub year: i32,
    pub month: u32,
    pub focus_day: Option<String>,
    pub show_filters: bool,
    /// Task key waiting for a blocker to be picked.
    pub linking: Option<String>,
}

impl Default for PlanningState {
    fn default() -> Self {
        let today = chrono::Local::now().date_naive();
        Self {
            mode: Mode::Agenda,
            filters: TaskFilters {
                status: "all".into(),
                priority: "all".into(),
                due: "all".into(),
                ..TaskFilters::default()
            },
            quick_text: String::new(),
            quick_due: today_key(),
            quick_priority: "normal".into(),
            year: chrono::Datelike::year(&today),
            month: chrono::Datelike::month(&today),
            focus_day: None,
            show_filters: false,
            linking: None,
        }
    }
}

enum Edit {
    Toggle(String, String, TodoItem),
    Patch(String, String, TodoItem),
    Delete(String, String, String),
    Add {
        text: String,
        due: Option<String>,
        priority: String,
    },
}

pub fn draw(
    ui: &mut Ui,
    vault: &mut Vault,
    state: &mut PlanningState,
    locale: Locale,
    selected: Option<usize>,
) -> ViewAction {
    let mut action = ViewAction::default();
    let today = today_key();
    let all_tasks = collect(vault);
    let visible = planning::filter_tasks(&all_tasks, &state.filters, &today);
    let stats = task_stats(&all_tasks, &today);
    let mut edits: Vec<Edit> = Vec::new();

    header(
        ui,
        t(locale, Key::PlanningTitle),
        &format!(
            "{} {} · {} {}",
            stats.total,
            t(locale, Key::TasksTotal),
            stats.open,
            t(locale, Key::TasksOpen)
        ),
    );

    // Toolbar: mode segments, stats, ICS export.
    let (_, bar_rect) = super::toolbar(ui);
    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar_rect.shrink2(vec2(super::VIEW_PAD, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    bar.spacing_mut().item_spacing.x = 6.0;
    let options = [
        (t(locale, Key::Agenda), state.mode == Mode::Agenda),
        (t(locale, Key::Kanban), state.mode == Mode::Kanban),
        (t(locale, Key::Gantt), state.mode == Mode::Gantt),
        (t(locale, Key::Calendar), state.mode == Mode::Calendar),
    ];
    if let Some(index) = segments(&mut bar, &options) {
        state.mode = [Mode::Agenda, Mode::Kanban, Mode::Gantt, Mode::Calendar][index];
    }
    bar.add_space(10.0);
    let filters_active = state.filters.is_active();
    if ghost(
        &mut bar,
        Icon::Filter,
        Some(t(locale, Key::Filters)),
        t(locale, Key::Filters),
        true,
        28.0,
    )
    .clicked()
    {
        state.show_filters = !state.show_filters;
    }
    if filters_active {
        bar.add_space(2.0);
        let _ = tag_chip(&mut bar, "filtered", theme::ACCENT_INK);
        if text_button(&mut bar, None, t(locale, Key::ClearFilters), false).clicked() {
            state.filters.clear();
        }
    }

    // The right group gets its own right-to-left child anchored to the toolbar
    // edge: sharing the left-to-right cursor made the stats collide with the
    // segmented control whenever a filter chip was visible.
    let mut right = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar_rect.shrink2(vec2(super::VIEW_PAD, 4.0)))
            .layout(Layout::right_to_left(Align::Center)),
    );
    right.spacing_mut().item_spacing.x = 0.0;
    if ghost(
        &mut right,
        Icon::Download,
        Some("ICS"),
        t(locale, Key::ExportIcs),
        !all_tasks.is_empty(),
        28.0,
    )
    .clicked()
    {
        export_ics(vault, locale);
    }
    right.add_space(12.0);
    stat(
        &mut right,
        &stats.overdue.to_string(),
        t(locale, Key::TasksOverdue),
        if stats.overdue > 0 { theme::BAD } else { theme::FAINT },
    );
    right.add_space(10.0);
    stat(
        &mut right,
        &format!("{}%", stats.completion_rate),
        t(locale, Key::TasksDone),
        theme::INK,
    );
    right.add_space(10.0);
    stat(&mut right, &stats.open.to_string(), t(locale, Key::TasksOpen), theme::INK);

    if state.show_filters {
        filter_bar(ui, state, &all_tasks, locale);
    }

    quick_add(ui, state, locale, &mut edits, !all_tasks.is_empty() || !vault.capsules.is_empty());

    let body = Rect::from_min_max(
        pos2(ui.max_rect().left(), ui.cursor().top()),
        ui.max_rect().right_bottom(),
    );
    let mut body_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(body)
            .layout(Layout::top_down(Align::Min)),
    );
    ScrollArea::vertical()
        .id_salt("planning-body")
        .auto_shrink([false, false])
        .show(&mut body_ui, |ui| {
            if all_tasks.is_empty() {
                empty_state(
                    ui,
                    Icon::Check,
                    t(locale, Key::PlanningTitle),
                    t(locale, Key::EmptyPlanning),
                );
                return;
            }
            if visible.is_empty() {
                empty_state(
                    ui,
                    Icon::Filter,
                    t(locale, Key::Filters),
                    t(locale, Key::EmptyFiltered),
                );
                return;
            }
            match state.mode {
                Mode::Agenda => agenda(ui, &visible, &all_tasks, &today, locale, &mut edits, &mut action),
                Mode::Kanban => kanban(ui, &visible, &all_tasks, &today, locale, &mut edits, &mut action),
                Mode::Gantt => gantt(ui, &visible, &today, locale, &mut edits, &mut action),
                Mode::Calendar => calendar(ui, &visible, &today, state, locale, &mut edits, &mut action),
            }
            ui.add_space(60.0);
        });

    let _ = selected;
    apply(vault, &edits, locale, &mut action);
    action
}

fn collect(vault: &Vault) -> Vec<PlannedTask> {
    let mut tasks = Vec::new();
    for capsule in &vault.capsules {
        tasks.extend(planning::tasks_in_capsule(
            &capsule.message_id,
            &capsule.subject,
            &capsule.tags,
            &capsule.blocks,
        ));
    }
    tasks
}

fn apply(vault: &mut Vault, edits: &[Edit], locale: Locale, action: &mut ViewAction) {
    for edit in edits {
        let changed = match edit {
            Edit::Toggle(capsule_id, block_id, item) => {
                patch_item(vault, capsule_id, block_id, item)
            }
            Edit::Patch(capsule_id, block_id, item) => patch_item(vault, capsule_id, block_id, item),
            Edit::Delete(capsule_id, block_id, item_id) => {
                delete_item(vault, capsule_id, block_id, item_id)
            }
            Edit::Add { text, due, priority } => add_task(vault, text, due.clone(), priority),
        };
        if changed {
            action.changed = true;
            if matches!(edit, Edit::Add { .. }) {
                action.toast = Some(t(locale, Key::TaskAdded).to_string());
            }
        }
    }
}

fn patch_item(vault: &mut Vault, capsule_id: &str, block_id: &str, item: &TodoItem) -> bool {
    let Some(capsule) = vault
        .capsules
        .iter_mut()
        .find(|capsule| capsule.message_id == capsule_id)
    else {
        return false;
    };
    let Some(block) = capsule.blocks.iter_mut().find(|block| {
        block.get("id").and_then(|id| id.as_str()) == Some(block_id)
    }) else {
        return false;
    };
    planning::patch_item(block, &item.id, item)
}

fn delete_item(vault: &mut Vault, capsule_id: &str, block_id: &str, item_id: &str) -> bool {
    let Some(capsule) = vault
        .capsules
        .iter_mut()
        .find(|capsule| capsule.message_id == capsule_id)
    else {
        return false;
    };
    let Some(block) = capsule.blocks.iter_mut().find(|block| {
        block.get("id").and_then(|id| id.as_str()) == Some(block_id)
    }) else {
        return false;
    };
    let Some(items) = block
        .get_mut("data")
        .and_then(|data| data.get_mut("items"))
        .and_then(|items| items.as_array_mut())
    else {
        return false;
    };
    let before = items.len();
    items.retain(|item| item.get("id").and_then(|id| id.as_str()) != Some(item_id));
    items.len() != before
}

fn add_task(vault: &mut Vault, text: &str, due: Option<String>, priority: &str) -> bool {
    let text = text.trim();
    if text.is_empty() {
        return false;
    }
    let item = serde_json::json!({
        "id": format!("item_{}", crate::now_nanos()),
        "text": text,
        "done": false,
        "status": "todo",
        "due": due,
        "priority": priority,
    });
    let inbox = vault
        .capsules
        .iter()
        .position(|capsule| capsule.tags.iter().any(|tag| tag == "#inbox"))
        .or(if vault.capsules.is_empty() { None } else { Some(0) });
    let Some(index) = inbox else {
        return false;
    };
    let capsule = &mut vault.capsules[index];
    let block = capsule
        .blocks
        .iter_mut()
        .find(|block| crate::mime::block_type(block) == "todo_list");
    match block {
        Some(block) => {
            if !block.get("data").is_some_and(|data| data.is_object()) {
                block["data"] = serde_json::json!({ "items": [] });
            }
            if !block["data"].get("items").is_some_and(|items| items.is_array()) {
                block["data"]["items"] = serde_json::json!([]);
            }
            block["data"]["items"]
                .as_array_mut()
                .expect("items is an array")
                .push(item);
        }
        None => capsule.blocks.push(serde_json::json!({
            "id": crate::mime::new_block_id("todo_list"),
            "type": "todo_list",
            "data": { "items": [item] }
        })),
    }
    true
}

fn export_ics(vault: &Vault, locale: Locale) {
    let tasks = collect(vault);
    if tasks.is_empty() {
        return;
    }
    let name = planning::ics_file_name(&today_key());
    let Some(path) = rfd::FileDialog::new()
        .set_title(t(locale, Key::ExportIcs))
        .set_file_name(&name)
        .add_filter("iCalendar", &["ics"])
        .save_file()
    else {
        return;
    };
    let ics = planning::build_ics(
        &tasks,
        &planning::IcsOptions {
            lang: locale,
            ..planning::IcsOptions::default()
        },
        chrono::Utc::now(),
    );
    let _ = std::fs::write(path, ics);
}

// ---------------------------------------------------------------------------
// Filters and quick add
// ---------------------------------------------------------------------------

fn filter_bar(ui: &mut Ui, state: &mut PlanningState, all: &[PlannedTask], locale: Locale) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::SUNKEN);
    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(super::VIEW_PAD, 9.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui = &mut bar;
    ui.spacing_mut().item_spacing.x = 6.0;

    // Text
    let width = 150.0;
    Frame::new()
        .fill(theme::CONTROLS)
        .corner_radius(theme::RADIUS)
        .stroke(theme::hairline())
        .inner_margin(Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.add(
                TextEdit::singleline(&mut state.filters.text)
                    .frame(Frame::NONE)
                    .margin(Margin::same(0))
                    .desired_width(width)
                    .font(FontId::new(12.0, egui::FontFamily::Proportional))
                    .hint_text(t(locale, Key::FilterText)),
            );
        });

    let status_options = [
        ("all", t(locale, Key::All)),
        ("open", t(locale, Key::Open)),
        ("todo", t(locale, Key::StatusTodo)),
        ("doing", t(locale, Key::StatusDoing)),
        ("done", t(locale, Key::Done)),
    ];
    if let Some(pick) = menu_choice(ui, &state.filters.status, &status_options) {
        state.filters.status = pick;
    }
    let priority_options = [
        ("all", t(locale, Key::All)),
        ("high", t(locale, Key::PriorityHigh)),
        ("normal", t(locale, Key::PriorityNormal)),
        ("low", t(locale, Key::PriorityLow)),
    ];
    if let Some(pick) = menu_choice(ui, &state.filters.priority, &priority_options) {
        state.filters.priority = pick;
    }
    let due_options = [
        ("all", t(locale, Key::All)),
        ("overdue", t(locale, Key::Overdue)),
        ("today", t(locale, Key::Today)),
        ("week", t(locale, Key::ThisWeek)),
        ("none", t(locale, Key::NoDate)),
    ];
    if let Some(pick) = menu_choice(ui, &state.filters.due, &due_options) {
        state.filters.due = pick;
    }

    let mut capsules: Vec<(String, String)> = {
        let mut seen: Vec<(String, String)> = Vec::new();
        for task in all {
            if !seen.iter().any(|(id, _)| id == &task.capsule_id) {
                seen.push((task.capsule_id.clone(), task.capsule_subject.clone()));
            }
        }
        seen.sort_by(|a, b| a.1.cmp(&b.1));
        seen
    };
    capsules.truncate(12);
    let capsule_current = state
        .filters
        .capsule_id
        .clone()
        .unwrap_or_default();
    let capsule_label = capsules
        .iter()
        .find(|(id, _)| id == &capsule_current)
        .map(|(_, subject)| truncate_chars(subject, 18))
        .unwrap_or_else(|| t(locale, Key::FilterCapsule).to_string());
    let response = ghost(ui, Icon::Doc, Some(&capsule_label), t(locale, Key::FilterCapsule), true, 26.0);
    egui::Popup::menu(&response)
        .frame(popover_frame())
        .align(egui::RectAlign::BOTTOM_START)
        .show(|ui| {
            ui.set_min_width(200.0);
            if menu_row(ui, None, t(locale, Key::All), None, capsule_current.is_empty(), true, false).clicked() {
                state.filters.capsule_id = None;
            }
            for (id, subject) in &capsules {
                if menu_row(ui, Some(Icon::Doc), &truncate_chars(subject, 34), None, &capsule_current == id, true, false).clicked() {
                    state.filters.capsule_id = Some(id.clone());
                }
            }
        });

    let mut tags = planning::task_tags(all);
    tags.truncate(12);
    let tag_label = state
        .filters
        .tag
        .clone()
        .unwrap_or_else(|| t(locale, Key::FilterTag).to_string());
    let response = ghost(ui, Icon::Tag, Some(&tag_label), t(locale, Key::FilterTag), true, 26.0);
    egui::Popup::menu(&response)
        .frame(popover_frame())
        .align(egui::RectAlign::BOTTOM_START)
        .show(|ui| {
            ui.set_min_width(180.0);
            if menu_row(ui, None, t(locale, Key::All), None, state.filters.tag.is_none(), true, false).clicked() {
                state.filters.tag = None;
            }
            for tag in &tags {
                if menu_row(ui, Some(Icon::Tag), tag, None, state.filters.tag.as_deref() == Some(tag.as_str()), true, false).clicked() {
                    state.filters.tag = Some(tag.clone());
                }
            }
        });
}

fn menu_choice(ui: &mut Ui, current: &str, options: &[(&str, &str)]) -> Option<String> {
    let label = options
        .iter()
        .find(|(id, _)| *id == current)
        .map(|(_, label)| *label)
        .unwrap_or(current);
    let response = ghost(ui, Icon::ChevronDown, Some(label), label, true, 26.0);
    let mut picked = None;
    egui::Popup::menu(&response)
        .frame(popover_frame())
        .align(egui::RectAlign::BOTTOM_START)
        .show(|ui| {
            ui.set_min_width(170.0);
            for (id, label) in options {
                if menu_row(ui, None, label, None, *id == current, true, false).clicked() {
                    picked = Some((*id).to_string());
                }
            }
        });
    picked
}

fn quick_add(ui: &mut Ui, state: &mut PlanningState, locale: Locale, edits: &mut Vec<Edit>, enabled: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::SUNKEN);
    ui.painter()
        .hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline());
    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(super::VIEW_PAD, 11.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui = &mut bar;
    ui.spacing_mut().item_spacing.x = 6.0;
    icons::paint(
        ui.painter(),
        Rect::from_min_size(ui.cursor().min + vec2(2.0, 5.0), vec2(13.0, 13.0)),
        Icon::Plus,
        theme::FAINT,
    );
    ui.add_space(20.0);

    let response = ui.add_sized(
        vec2((ui.available_width() - 300.0).max(160.0), 28.0),
        TextEdit::singleline(&mut state.quick_text)
            .frame(Frame::NONE)
            .margin(Margin::same(0))
            .font(FontId::new(13.5, egui::FontFamily::Proportional))
            .hint_text(t(locale, Key::QuickAdd)),
    );
    let submit = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

    let mut due = state.quick_due.clone();
    let due_invalid = !is_date_key(&due);
    let due_response = ui.add_sized(
        vec2(92.0, 24.0),
        TextEdit::singleline(&mut due)
            .frame(Frame::NONE)
            .margin(Margin::same(2))
            .font(FontId::new(10.5, theme::mono()))
            .text_color(if due_invalid { theme::BAD } else { theme::MUTED }),
    );
    if due_response.changed() {
        state.quick_due = due.clone();
    }

    let priority = match state.quick_priority.as_str() {
        "high" => t(locale, Key::PriorityHigh),
        "low" => t(locale, Key::PriorityLow),
        _ => t(locale, Key::PriorityNormal),
    };
    let priority_response = ghost(ui, Icon::ChevronDown, Some(priority), t(locale, Key::Priority), true, 26.0);
    egui::Popup::menu(&priority_response)
        .frame(popover_frame())
        .align(egui::RectAlign::BOTTOM_END)
        .show(|ui| {
            for (id, label) in [
                ("high", t(locale, Key::PriorityHigh)),
                ("normal", t(locale, Key::PriorityNormal)),
                ("low", t(locale, Key::PriorityLow)),
            ] {
                if menu_row(ui, None, label, None, state.quick_priority == id, true, false).clicked() {
                    state.quick_priority = id.to_string();
                }
            }
        });

    if ui
        .add_enabled(enabled, egui::Button::new(RichText::new(t(locale, Key::AddTask)).size(11.5)))
        .clicked()
        || submit
    {
        if !state.quick_text.trim().is_empty() {
            edits.push(Edit::Add {
                text: state.quick_text.trim().to_string(),
                due: Some(state.quick_due.clone()).filter(|due| is_date_key(due)),
                priority: state.quick_priority.clone(),
            });
            state.quick_text.clear();
        }
    }
}

// ---------------------------------------------------------------------------
// Agenda
// ---------------------------------------------------------------------------

fn agenda(
    ui: &mut Ui,
    visible: &[PlannedTask],
    all: &[PlannedTask],
    today: &str,
    locale: Locale,
    edits: &mut Vec<Edit>,
    action: &mut ViewAction,
) {
    for (bucket, tasks) in planning::group_tasks(visible, today) {
        section(ui, bucket.title(locale));
        for task in &tasks {
            task_row(ui, task, all, today, locale, edits, action);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn task_row(
    ui: &mut Ui,
    task: &PlannedTask,
    all: &[PlannedTask],
    today: &str,
    locale: Locale,
    edits: &mut Vec<Edit>,
    action: &mut ViewAction,
) {
    let key = format!("{}|{}", task.capsule_id, task.key());
    let mut item = task.item.clone();
    let mut touched = false;
    let blocked = is_blocked(task, all);
    let overdue = bucket_of(task, today) == Bucket::Overdue;

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
    let hovered = ui.rect_contains_pointer(rect);
    if hovered {
        ui.painter().rect_filled(rect, theme::RADIUS, theme::WASH);
    }
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(8.0, 4.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui = &mut row;
    ui.spacing_mut().item_spacing.x = 8.0;

    let mut done = item.is_done();
    if checkbox(ui, &mut done).changed() {
        item.done = done;
        item.status = Some(if done { "done".into() } else { "todo".into() });
        edits.push(Edit::Toggle(
            task.capsule_id.clone(),
            task.block_id.clone(),
            item.clone(),
        ));
    }

    // Text: inline edit.
    let width = (ui.available_width() - 250.0).max(120.0);
    let response = ui.add_sized(
        vec2(width, 24.0),
        TextEdit::singleline(&mut item.text)
            .frame(Frame::NONE)
            .margin(Margin::same(0))
            .font(FontId::new(13.0, egui::FontFamily::Proportional))
            .text_color(if done { theme::FAINT } else { theme::INK }),
    );
    if response.changed() {
        touched = true;
    }

    if blocked {
        let _ = tag_chip(ui, t(locale, Key::BlockedShort), theme::BADGE_BETA);
    }

    // Due date edit.
    let mut due = item.due.clone().unwrap_or_default();
    let due_color = if due.is_empty() {
        theme::FAINT
    } else if is_date_key(&due) {
        if overdue {
            theme::BAD
        } else {
            theme::MUTED
        }
    } else {
        theme::BAD
    };
    let due_response = ui.add_sized(
        vec2(84.0, 22.0),
        TextEdit::singleline(&mut due)
            .frame(Frame::NONE)
            .margin(Margin::same(0))
            .font(FontId::new(9.5, theme::mono()))
            .text_color(due_color)
            .hint_text("YYYY-MM-DD"),
    );
    if due_response.changed() {
        item.due = if due.trim().is_empty() {
            None
        } else if is_date_key(due.trim()) {
            Some(due.trim().to_string())
        } else {
            item.due.clone()
        };
        if due.trim().is_empty() || is_date_key(due.trim()) {
            touched = true;
        }
    }

    let status = status_pill(ui, &item.status());
    egui::Popup::menu(&status)
        .frame(popover_frame())
        .align(egui::RectAlign::BOTTOM_END)
        .show(|ui| {
            ui.set_min_width(190.0);
            menu_header(ui, t(locale, Key::Status));
            for (id, label) in [
                ("todo", t(locale, Key::StatusTodo)),
                ("doing", t(locale, Key::StatusDoing)),
                ("done", t(locale, Key::StatusDone)),
            ] {
                if menu_row(ui, None, label, None, item.status() == id, true, false).clicked() {
                    item.status = Some(id.to_string());
                    item.done = id == "done";
                    touched = true;
                }
            }
            menu_separator(ui);
            menu_header(ui, t(locale, Key::Priority));
            for (id, label) in [
                ("high", t(locale, Key::PriorityHigh)),
                ("normal", t(locale, Key::PriorityNormal)),
                ("low", t(locale, Key::PriorityLow)),
            ] {
                if menu_row(ui, None, label, None, item.priority() == id, true, false).clicked() {
                    item.priority = Some(id.to_string());
                    touched = true;
                }
            }
            menu_separator(ui);
            menu_header(ui, t(locale, Key::BlockedBy));
            if menu_row(ui, None, t(locale, Key::NoDependency), None, item.blocked_by.is_empty(), true, false).clicked() {
                item.blocked_by.clear();
                touched = true;
            }
            for other in all {
                if other.key() == task.key() {
                    continue;
                }
                let label = truncate_chars(&other.item.text, 28);
                let checked = item.blocked_by.contains(&other.key());
                if menu_row(ui, None, &label, None, checked, true, false).clicked() {
                    let target = other.key();
                    if checked {
                        item.blocked_by.retain(|key| key != &target);
                    } else if !planning::would_create_cycle(all, &task.key(), &target) {
                        item.blocked_by.push(target);
                    }
                    touched = true;
                }
            }
            menu_separator(ui);
            if menu_row(ui, Some(Icon::Trash), t(locale, Key::Remove), None, false, true, true).clicked() {
                edits.push(Edit::Delete(
                    task.capsule_id.clone(),
                    task.block_id.clone(),
                    task.item.id.clone(),
                ));
            }
        });

    // Capsule link.
    let capsule = truncate_chars(&task.capsule_subject, 20);
    let link = text_button(ui, Some(Icon::Doc), &capsule, false);
    if link.clicked() {
        if let Some(index) = capsule_index(action, &task.capsule_id) {
            action.select = Some(index);
        }
    }
    link.on_hover_text(&task.capsule_subject);

    if touched {
        edits.push(Edit::Patch(
            task.capsule_id.clone(),
            task.block_id.clone(),
            item,
        ));
    }

    ui.painter()
        .hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, theme::fade(theme::LINE, 0.5)));
        let _ = key;
}

/// The shell resolves the message id to a page index after drawing; the view
/// never needs to know the vault ordering.
fn capsule_index(action: &mut ViewAction, id: &str) -> Option<usize> {
    action.select_id = Some(id.to_string());
    None
}

// ---------------------------------------------------------------------------
// Kanban
// ---------------------------------------------------------------------------

fn kanban(
    ui: &mut Ui,
    visible: &[PlannedTask],
    all: &[PlannedTask],
    today: &str,
    locale: Locale,
    edits: &mut Vec<Edit>,
    action: &mut ViewAction,
) {
    let columns = planning::group_by_status(visible);
    let labels = [
        t(locale, Key::StatusTodo),
        t(locale, Key::StatusDoing),
        t(locale, Key::StatusDone),
    ];
    let available = ui.available_width() - super::VIEW_PAD * 2.0 - 20.0;
    let width = (available / 3.0).max(180.0);
    ui.add_space(14.0);
    ui.horizontal_top(|ui| {
        ui.add_space(super::VIEW_PAD);
        ui.spacing_mut().item_spacing.x = 10.0;
        for (index, column) in columns.iter().enumerate() {
            ui.allocate_ui_with_layout(
                vec2(width, 0.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(width);
                    Frame::new()
                        .fill(theme::WASH)
                        .corner_radius(theme::RADIUS)
                        .stroke(theme::hairline())
                        .inner_margin(Margin::same(8))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let _ = hud(ui, labels[index], theme::MUTED);
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(column.len().to_string())
                                            .family(theme::mono())
                                            .size(9.5)
                                            .color(theme::FAINT),
                                    );
                                });
                            });
                            ui.add_space(6.0);
                            ScrollArea::vertical()
                                .id_salt(("kanban", index))
                                .max_height(ui.available_height() - 40.0)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    for task in column {
                                        kanban_card(ui, task, all, today, locale, edits, action, index);
                                    }
                                });
                        });
                },
            );
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn kanban_card(
    ui: &mut Ui,
    task: &PlannedTask,
    all: &[PlannedTask],
    today: &str,
    locale: Locale,
    edits: &mut Vec<Edit>,
    action: &mut ViewAction,
    column: usize,
) {
    let blocked = is_blocked(task, all);
    let overdue = bucket_of(task, today) == Bucket::Overdue;
    let card = Frame::new()
        .fill(theme::BG)
        .corner_radius(theme::RADIUS)
        .stroke(theme::hairline())
        .inner_margin(Margin {
            left: 9,
            right: 7,
            top: 7,
            bottom: 7,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut item = task.item.clone();
            let mut touched = false;
            let mut done = item.is_done();
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if checkbox(ui, &mut done).changed() {
                    item.done = done;
                    item.status = Some(if done { "done".into() } else { "todo".into() });
                    touched = true;
                }
                let response = ui.add_sized(
                    vec2(ui.available_width() - 8.0, 20.0),
                    TextEdit::multiline(&mut item.text)
                        .frame(Frame::NONE)
                        .margin(Margin::same(0))
                        .desired_rows(1)
                        .font(FontId::new(12.5, egui::FontFamily::Proportional))
                        .text_color(if done { theme::FAINT } else { theme::INK }),
                );
                if response.changed() {
                    touched = true;
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if let Some(due) = &item.due {
                    ui.label(
                        RichText::new(if is_date_key(due) {
                            human_date(due, locale)
                        } else {
                            due.clone()
                        })
                        .family(theme::mono())
                        .size(9.0)
                        .color(if overdue { theme::BAD } else { theme::FAINT }),
                    );
                }
                if blocked {
                    let _ = tag_chip(ui, t(locale, Key::BlockedShort), theme::BADGE_BETA);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if column < 2 {
                        if icon_button(ui, Icon::ChevronRight, "→", true, false).clicked() {
                            item.status = Some(if column == 0 { "doing" } else { "done" }.into());
                            item.done = column == 1;
                            touched = true;
                        }
                    }
                    if column > 0 {
                        if icon_button(ui, Icon::ChevronLeft, "←", true, false).clicked() {
                            item.status = Some(if column == 2 { "doing" } else { "todo" }.into());
                            item.done = false;
                            touched = true;
                        }
                    }
                });
            });
            if text_button(ui, Some(Icon::Doc), &truncate_chars(&task.capsule_subject, 24), false).clicked() {
                action.select = capsule_index(action, &task.capsule_id);
            }
            if touched {
                edits.push(Edit::Patch(
                    task.capsule_id.clone(),
                    task.block_id.clone(),
                    item,
                ));
            }
        });
    let _ = card;
    ui.add_space(6.0);
}

// ---------------------------------------------------------------------------
// Gantt
// ---------------------------------------------------------------------------

fn gantt(
    ui: &mut Ui,
    visible: &[PlannedTask],
    today: &str,
    locale: Locale,
    edits: &mut Vec<Edit>,
    action: &mut ViewAction,
) {
    let range = planning::gantt_range(visible, today);
    let days = planning::days_between(&range.start, &range.end).max(1) + 1;
    let label_w = 230.0;
    let cell_w = ((ui.available_width() - super::VIEW_PAD * 2.0 - label_w) / days as f32).clamp(14.0, 40.0);
    let graph_w = cell_w * days as f32;

    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.add_space(super::VIEW_PAD);
        ui.vertical(|ui| {
            ui.set_width(label_w + graph_w);
            // Header: day numbers.
            ui.horizontal(|ui| {
                ui.add_space(label_w);
                for i in 0..days {
                    let day = add_days(&range.start, i);
                    let is_today = day == today;
                    ui.allocate_ui_with_layout(vec2(cell_w, 20.0), Layout::top_down(Align::Center), |ui| {
                        ui.label(
                            RichText::new(&day[8..10])
                                .family(theme::mono())
                                .size(8.5)
                                .color(if is_today { theme::ACCENT_INK } else { theme::FAINT }),
                        );
                    });
                }
            });
            ui.painter()
                .hline(ui.cursor().min.x..=(ui.cursor().min.x + label_w + graph_w), ui.cursor().min.y, theme::hairline());
            ui.add_space(6.0);

            for task in visible {
                let (rect, _) = ui.allocate_exact_size(vec2(label_w + graph_w, 32.0), Sense::hover());
                let hovered = ui.rect_contains_pointer(rect);
                if hovered {
                    ui.painter().rect_filled(rect, theme::RADIUS, theme::WASH);
                }
                // Today column wash.
                let today_offset = planning::days_between(&range.start, today);
                if today_offset >= 0 && today_offset < days {
                    let x = rect.left() + label_w + today_offset as f32 * cell_w;
                    ui.painter().rect_filled(
                        Rect::from_min_max(pos2(x, rect.top()), pos2(x + cell_w, rect.bottom())),
                        0.0,
                        theme::fade(theme::ACCENT, 0.08),
                    );
                }
                let mut label = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(Rect::from_min_size(rect.min, vec2(label_w, rect.height())))
                        .layout(Layout::left_to_right(Align::Center)),
                );
                let mut done = task.item.is_done();
                if checkbox(&mut label, &mut done).clicked() {
                    // Toggling from the Gantt uses the same patch path as agenda.
                    let mut item = task.item.clone();
                    item.done = !task.item.is_done();
                    item.status = Some(if item.done { "done" } else { "todo" }.into());
                    edits.push(Edit::Patch(task.capsule_id.clone(), task.block_id.clone(), item));
                }
                let text = truncate_chars(&task.item.text, 24);
                let response = label.add(
                    egui::Label::new(
                        RichText::new(text)
                            .size(11.5)
                            .color(if task.item.is_done() { theme::FAINT } else { theme::INK }),
                    )
                    .truncate()
                    .sense(Sense::click()),
                );
                if response.clicked() {
                    action.select = capsule_index(action, &task.capsule_id);
                }
                if let Some((left, span)) = task_bar(task, &GanttRange {
                    start: range.start.clone(),
                    end: range.end.clone(),
                }) {
                    let x = rect.left() + label_w + left as f32 * cell_w + 1.0;
                    let w = (span as f32 * cell_w - 2.0).max(4.0);
                    let bar = Rect::from_min_size(pos2(x, rect.center().y - 6.0), vec2(w, 12.0));
                    let color = if task.item.is_done() {
                        theme::fade(theme::MUTED, 0.5)
                    } else {
                        match task.item.priority() {
                            "high" => theme::BAD,
                            "low" => theme::ACCENT_INK,
                            _ => theme::ACCENT,
                        }
                    };
                    ui.painter()
                        .rect_filled(bar, theme::RADIUS, theme::fade(color, 0.35));
                    ui.painter().rect_stroke(
                        bar,
                        theme::RADIUS,
                        Stroke::new(1.0, theme::fade(color, 0.7)),
                        egui::StrokeKind::Inside,
                    );
                    if hovered {
                        ui.painter().text(
                            pos2(bar.right() + 6.0, bar.center().y),
                            Align2::LEFT_CENTER,
                            format!("{} {}{}", task.item.text, span, t(locale, Key::SpanDays)),
                            FontId::new(9.0, theme::mono()),
                            theme::MUTED,
                        );
                    }
                }
                ui.painter()
                    .hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, theme::fade(theme::LINE, 0.5)));
            }
        });
    });
}

// ---------------------------------------------------------------------------
// Calendar
// ---------------------------------------------------------------------------

fn calendar(
    ui: &mut Ui,
    visible: &[PlannedTask],
    today: &str,
    state: &mut PlanningState,
    locale: Locale,
    edits: &mut Vec<Edit>,
    action: &mut ViewAction,
) {
    ui.add_space(12.0);
    centred(ui, 880.0, |ui| {
        ui.horizontal(|ui| {
            if icon_button(ui, Icon::ChevronLeft, "←", true, false).clicked() {
                if state.month == 1 {
                    state.month = 12;
                    state.year -= 1;
                } else {
                    state.month -= 1;
                }
            }
            if icon_button(ui, Icon::ChevronRight, "→", true, false).clicked() {
                if state.month == 12 {
                    state.month = 1;
                    state.year += 1;
                } else {
                    state.month += 1;
                }
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new(month_label(state.year, state.month, locale))
                    .family(theme::display())
                    .size(16.0)
                    .color(theme::INK),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if text_button(ui, None, t(locale, Key::Today), false).clicked() {
                    let today_date = chrono::Local::now().date_naive();
                    state.year = chrono::Datelike::year(&today_date);
                    state.month = chrono::Datelike::month(&today_date);
                    state.focus_day = Some(today_key());
                }
            });
        });
        ui.add_space(8.0);

        let grid = month_grid(state.year, state.month);
        ui.horizontal(|ui| {
            let cell = (ui.available_width() - 6.0 * 6.0) / 7.0;
            for label in weekday_labels(locale) {
                ui.allocate_ui_with_layout(vec2(cell, 18.0), Layout::top_down(Align::Center), |ui| {
                    let _ = hud(ui, label, theme::FAINT);
                });
            }
        });
        ui.add_space(4.0);

        for week in 0..6 {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let cell_w = (ui.available_width() - 6.0 * 6.0) / 7.0;
                for day_index in 0..7 {
                    let key = &grid[week * 7 + day_index];
                    let date = planning::parse_key(key);
                    let in_month = date.map(|d| d.month() == state.month).unwrap_or(false);
                    let tasks = tasks_on_day(visible, key, true);
                    let (rect, response) = ui.allocate_exact_size(vec2(cell_w, 74.0), Sense::click());
                    let is_today = key == today;
                    let focused = state.focus_day.as_deref() == Some(key.as_str());
                    let fill = if focused {
                        theme::fade(theme::ACCENT, 0.12)
                    } else if is_today {
                        theme::WASH
                    } else {
                        Color32::TRANSPARENT
                    };
                    ui.painter().rect(
                        rect,
                        theme::RADIUS,
                        fill,
                        if is_today {
                            Stroke::new(1.0, theme::fade(theme::ACCENT, 0.5))
                        } else {
                            Stroke::new(1.0, theme::fade(theme::LINE, 0.6))
                        },
                        egui::StrokeKind::Inside,
                    );
                    if response.hovered() {
                        ui.painter().rect_filled(rect, theme::RADIUS, theme::WASH);
                    }
                    let day_number = date.map(|d| d.day()).unwrap_or(0);
                    ui.painter().text(
                        rect.min + vec2(7.0, 6.0),
                        Align2::LEFT_TOP,
                        day_number.to_string(),
                        FontId::new(10.0, theme::mono_bold()),
                        if in_month { theme::MUTED } else { theme::fade(theme::FAINT, 0.5) },
                    );
                    let mut y = rect.top() + 22.0;
                    for task in tasks.iter().take(3) {
                        if y + 12.0 > rect.bottom() - 2.0 {
                            break;
                        }
                        let color = match task.item.priority() {
                            "high" => theme::BAD,
                            _ => theme::ACCENT_INK,
                        };
                        let dot = pos2(rect.left() + 8.0, y + 4.0);
                        ui.painter().circle_filled(dot, 2.0, color);
                        ui.painter().text(
                            pos2(dot.x + 6.0, dot.y),
                            Align2::LEFT_CENTER,
                            truncate_chars(&task.item.text, 14),
                            FontId::new(9.0, egui::FontFamily::Proportional),
                            if task.item.is_done() { theme::FAINT } else { theme::MUTED },
                        );
                        y += 14.0;
                    }
                    if tasks.len() > 3 {
                        ui.painter().text(
                            pos2(rect.left() + 8.0, y + 4.0),
                            Align2::LEFT_CENTER,
                            format!("+{}", tasks.len() - 3),
                            FontId::new(8.5, theme::mono()),
                            theme::FAINT,
                        );
                    }
                    if response.clicked() {
                        state.focus_day = Some(key.clone());
                    }
                }
            });
            ui.add_space(6.0);
        }

        let focus = state.focus_day.clone().unwrap_or_else(today_key);
        section(ui, &format!("{focus}"));
        let day_tasks = tasks_on_day(visible, &focus, true);
        if day_tasks.is_empty() {
            ui.label(
                RichText::new("—")
                    .size(12.0)
                    .color(theme::FAINT),
            );
        }
        let all = visible.to_vec();
        for task in &day_tasks {
            task_row(ui, task, &all, today, locale, edits, action);
        }
    });
}
