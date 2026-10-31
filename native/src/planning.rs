//! Planning: tasks collected from every page's todo blocks.
//!
//! Direct port of `src/engine/planning.ts`. Pure and deterministic, so the same
//! filters, buckets, Gantt geometry and ICS output can be tested without a UI.
//! Dates are local calendar days (`YYYY-MM-DD`), never UTC instants.

use chrono::{Datelike, Local, NaiveDate};
use serde_json::{json, Value};

use crate::mime::block_type;

pub const DATE_FMT: &str = "%Y-%m-%d";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TodoItem {
    pub id: String,
    pub text: String,
    pub done: bool,
    /// Explicit board status; falls back to `done ? done : todo`.
    pub status: Option<String>,
    pub start: Option<String>,
    pub blocked_by: Vec<String>,
    pub due: Option<String>,
    pub priority: Option<String>,
}

impl TodoItem {
    pub fn status(&self) -> String {
        match &self.status {
            Some(status) if !status.is_empty() => status.clone(),
            _ => {
                if self.done {
                    "done".to_string()
                } else {
                    "todo".to_string()
                }
            }
        }
    }

    pub fn is_done(&self) -> bool {
        self.status() == "done"
    }

    pub fn priority(&self) -> &str {
        self.priority.as_deref().unwrap_or("normal")
    }

    /// First day of the task: explicit start, otherwise the due day.
    pub fn start_day(&self) -> Option<&str> {
        if let Some(start) = self.start.as_deref().filter(|v| is_date_key(v)) {
            return Some(start);
        }
        self.due.as_deref().filter(|v| is_date_key(v))
    }

    pub fn to_json(&self) -> Value {
        let mut value = json!({
            "id": self.id,
            "text": self.text,
            "done": self.done,
        });
        if let Some(status) = &self.status {
            value["status"] = json!(status);
        }
        if let Some(start) = &self.start {
            value["start"] = json!(start);
        }
        if !self.blocked_by.is_empty() {
            value["blockedBy"] = json!(self.blocked_by);
        }
        if let Some(due) = &self.due {
            value["due"] = json!(due);
        }
        if let Some(priority) = &self.priority {
            value["priority"] = json!(priority);
        }
        value
    }

    pub fn from_json(value: &Value) -> Option<Self> {
        let id = value.get("id")?.as_str()?.to_string();
        Some(Self {
            id,
            text: value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            done: value.get("done").and_then(Value::as_bool).unwrap_or(false),
            status: value
                .get("status")
                .and_then(Value::as_str)
                .map(str::to_string),
            start: value
                .get("start")
                .and_then(Value::as_str)
                .filter(|v| is_date_key(v))
                .map(str::to_string),
            blocked_by: value
                .get("blockedBy")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|entry| entry.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            due: value
                .get("due")
                .and_then(Value::as_str)
                .filter(|v| is_date_key(v))
                .map(str::to_string),
            priority: value
                .get("priority")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }
}

#[derive(Clone, Debug)]
pub struct PlannedTask {
    pub capsule_id: String,
    pub capsule_subject: String,
    pub capsule_tags: Vec<String>,
    pub block_id: String,
    pub block_title: Option<String>,
    pub item: TodoItem,
}

impl PlannedTask {
    /// Stable identity across the vault: `blockId:itemId`.
    pub fn key(&self) -> String {
        format!("{}:{}", self.block_id, self.item.id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bucket {
    Overdue,
    Today,
    Tomorrow,
    Week,
    Later,
    None,
    Done,
}

impl Bucket {
    pub fn id(self) -> &'static str {
        match self {
            Bucket::Overdue => "overdue",
            Bucket::Today => "today",
            Bucket::Tomorrow => "tomorrow",
            Bucket::Week => "week",
            Bucket::Later => "later",
            Bucket::None => "none",
            Bucket::Done => "done",
        }
    }

    pub fn title(self, locale: crate::i18n::Locale) -> &'static str {
        use crate::i18n::{t, Key};
        match self {
            Bucket::Overdue => t(locale, Key::Overdue),
            Bucket::Today => t(locale, Key::Today),
            Bucket::Tomorrow => t(locale, Key::Tomorrow),
            Bucket::Week => t(locale, Key::ThisWeek),
            Bucket::Later => t(locale, Key::Later),
            Bucket::None => t(locale, Key::NoDate),
            Bucket::Done => t(locale, Key::Done),
        }
    }
}

pub const BUCKET_ORDER: [Bucket; 7] = [
    Bucket::Overdue,
    Bucket::Today,
    Bucket::Tomorrow,
    Bucket::Week,
    Bucket::Later,
    Bucket::None,
    Bucket::Done,
];

#[derive(Clone, Debug, Default)]
pub struct TaskFilters {
    pub text: String,
    /// all | open | todo | doing | done
    pub status: String,
    /// all | low | normal | high
    pub priority: String,
    /// all | overdue | today | week | none
    pub due: String,
    pub capsule_id: Option<String>,
    pub tag: Option<String>,
}

impl TaskFilters {
    pub fn is_active(&self) -> bool {
        !self.text.trim().is_empty()
            || (!self.status.is_empty() && self.status != "all")
            || (!self.priority.is_empty() && self.priority != "all")
            || (!self.due.is_empty() && self.due != "all")
            || self.capsule_id.is_some()
            || self.tag.is_some()
    }

    pub fn clear(&mut self) {
        *self = Self {
            status: "all".into(),
            priority: "all".into(),
            due: "all".into(),
            ..Self::default()
        };
    }
}

#[derive(Clone, Debug, Default)]
pub struct TaskStats {
    pub total: usize,
    pub done: usize,
    pub open: usize,
    pub overdue: usize,
    pub today: usize,
    pub completion_rate: u32,
}

// ── Dates ────────────────────────────────────────────────────────────────────

pub fn today_key() -> String {
    to_key(Local::now().date_naive())
}

pub fn to_key(date: NaiveDate) -> String {
    date.format(DATE_FMT).to_string()
}

pub fn parse_key(value: &str) -> Option<NaiveDate> {
    if value.len() != 10 {
        return None;
    }
    NaiveDate::parse_from_str(value, DATE_FMT).ok()
}

pub fn is_date_key(value: &str) -> bool {
    parse_key(value).is_some()
}

pub fn add_days(key: &str, days: i64) -> String {
    match parse_key(key) {
        Some(date) => match date.checked_add_signed(chrono::Duration::days(days)) {
            Some(shifted) => to_key(shifted),
            None => key.to_string(),
        },
        None => key.to_string(),
    }
}

pub fn days_between(from: &str, to: &str) -> i64 {
    match (parse_key(from), parse_key(to)) {
        (Some(a), Some(b)) => (b - a).num_days(),
        _ => 0,
    }
}

/// Six-week (42-cell) month grid starting on Monday.
pub fn month_grid(year: i32, month: u32) -> Vec<String> {
    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap_or_else(|| {
        NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch is a valid date")
    });
    let offset = first.weekday().num_days_from_monday() as i64;
    let start = first - chrono::Duration::days(offset);
    (0..42)
        .map(|i| to_key(start + chrono::Duration::days(i)))
        .collect()
}

pub fn month_label(year: i32, month: u32, locale: crate::i18n::Locale) -> String {
    const RU: [&str; 12] = [
        "январь", "февраль", "март", "апрель", "май", "июнь", "июль", "август",
        "сентябрь", "октябрь", "ноябрь", "декабрь",
    ];
    const EN: [&str; 12] = [
        "January", "February", "March", "April", "May", "June", "July", "August",
        "September", "October", "November", "December",
    ];
    let names = match locale {
        crate::i18n::Locale::Ru => RU,
        crate::i18n::Locale::En => EN,
    };
    let name = names[(month.clamp(1, 12) - 1) as usize];
    match locale {
        crate::i18n::Locale::Ru => format!("{name} {year}"),
        crate::i18n::Locale::En => format!("{name} {year}"),
    }
}

pub fn weekday_labels(locale: crate::i18n::Locale) -> [&'static str; 7] {
    match locale {
        crate::i18n::Locale::Ru => ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"],
        crate::i18n::Locale::En => ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"],
    }
}

/// "12 окт" / "Oct 12".
pub fn human_date(key: &str, locale: crate::i18n::Locale) -> String {
    let Some(date) = parse_key(key) else {
        return key.to_string();
    };
    const RU: [&str; 12] = [
        "янв", "фев", "мар", "апр", "май", "июн", "июл", "авг", "сен", "окт", "ноя", "дек",
    ];
    const EN: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let index = (date.month().clamp(1, 12) - 1) as usize;
    match locale {
        crate::i18n::Locale::Ru => format!("{} {}", date.day(), RU[index]),
        crate::i18n::Locale::En => format!("{} {}", EN[index], date.day()),
    }
}

// ── Collection ───────────────────────────────────────────────────────────────

/// Every task in one capsule, in block order.
pub fn tasks_in_capsule(capsule_id: &str, subject: &str, tags: &[String], blocks: &[Value]) -> Vec<PlannedTask> {
    let mut tasks = Vec::new();
    for block in blocks {
        if block_type(block) != "todo_list" {
            continue;
        }
        let block_id = block
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("block")
            .to_string();
        let block_title = block
            .get("data")
            .and_then(|data| data.get("title"))
            .and_then(Value::as_str)
            .filter(|title| !title.trim().is_empty())
            .map(str::to_string);
        let Some(items) = block
            .get("data")
            .and_then(|data| data.get("items"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        for item in items {
            let Some(item) = TodoItem::from_json(item) else {
                continue;
            };
            tasks.push(PlannedTask {
                capsule_id: capsule_id.to_string(),
                capsule_subject: subject.to_string(),
                capsule_tags: tags.to_vec(),
                block_id: block_id.clone(),
                block_title: block_title.clone(),
                item,
            });
        }
    }
    tasks
}

/// Distinct tags present on task-owning capsules, alphabetically.
pub fn task_tags(tasks: &[PlannedTask]) -> Vec<String> {
    let mut tags: Vec<String> = tasks
        .iter()
        .flat_map(|task| task.capsule_tags.iter().cloned())
        .collect();
    tags.sort();
    tags.dedup();
    tags
}

// ── Filtering ────────────────────────────────────────────────────────────────

pub fn filter_tasks(tasks: &[PlannedTask], filters: &TaskFilters, today: &str) -> Vec<PlannedTask> {
    let needle = filters.text.trim().to_lowercase();
    let status = if filters.status.is_empty() {
        "all"
    } else {
        filters.status.as_str()
    };
    let priority = if filters.priority.is_empty() {
        "all"
    } else {
        filters.priority.as_str()
    };
    let due = if filters.due.is_empty() {
        "all"
    } else {
        filters.due.as_str()
    };
    let week_end = add_days(today, 7);

    tasks
        .iter()
        .filter(|task| {
            if !needle.is_empty() {
                let haystack = format!(
                    "{}\n{}\n{}\n{}",
                    task.item.text,
                    task.capsule_subject,
                    task.block_title.clone().unwrap_or_default(),
                    task.capsule_tags.join(" ")
                )
                .to_lowercase();
                if !haystack.contains(&needle) {
                    return false;
                }
            }

            let current = task.item.status();
            if status == "open" && current == "done" {
                return false;
            }
            if status != "all" && status != "open" && current != status {
                return false;
            }

            if priority != "all" && task.item.priority() != priority {
                return false;
            }

            if let Some(capsule_id) = &filters.capsule_id {
                if &task.capsule_id != capsule_id {
                    return false;
                }
            }
            if let Some(tag) = &filters.tag {
                if !task.capsule_tags.contains(tag) {
                    return false;
                }
            }

            if due != "all" {
                let day = task.item.due.as_deref().filter(|d| is_date_key(d));
                match due {
                    "none" if day.is_some() => return false,
                    "overdue" => match day {
                        Some(day) if day < today && current != "done" => {}
                        _ => return false,
                    },
                    "today" if day != Some(today) => return false,
                    "week" => match day {
                        Some(day) if day >= today && day <= week_end.as_str() => {}
                        _ => return false,
                    },
                    _ => {}
                }
            }
            true
        })
        .cloned()
        .collect()
}

pub fn bucket_of(task: &PlannedTask, today: &str) -> Bucket {
    if task.item.is_done() {
        return Bucket::Done;
    }
    let Some(due) = task.item.due.as_deref().filter(|d| is_date_key(d)) else {
        return Bucket::None;
    };
    let delta = days_between(today, due);
    match delta {
        d if d < 0 => Bucket::Overdue,
        0 => Bucket::Today,
        1 => Bucket::Tomorrow,
        2..=7 => Bucket::Week,
        _ => Bucket::Later,
    }
}

pub fn group_by_status(tasks: &[PlannedTask]) -> [Vec<PlannedTask>; 3] {
    let mut columns: [Vec<PlannedTask>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for task in tasks {
        match task.item.status().as_str() {
            "doing" => columns[1].push(task.clone()),
            "done" => columns[2].push(task.clone()),
            _ => columns[0].push(task.clone()),
        }
    }
    for column in &mut columns {
        *column = sort_tasks(column);
    }
    columns
}

pub fn compare_tasks(a: &PlannedTask, b: &PlannedTask) -> std::cmp::Ordering {
    let due_a = a
        .item
        .due
        .as_deref()
        .filter(|d| is_date_key(d))
        .unwrap_or("9999-12-31");
    let due_b = b
        .item
        .due
        .as_deref()
        .filter(|d| is_date_key(d))
        .unwrap_or("9999-12-31");
    if due_a != due_b {
        return due_a.cmp(due_b);
    }
    let weight = |priority: &str| match priority {
        "high" => 0,
        "low" => 2,
        _ => 1,
    };
    let pa = weight(a.item.priority());
    let pb = weight(b.item.priority());
    if pa != pb {
        return pa.cmp(&pb);
    }
    a.item
        .text
        .to_lowercase()
        .cmp(&b.item.text.to_lowercase())
}

pub fn sort_tasks(tasks: &[PlannedTask]) -> Vec<PlannedTask> {
    let mut sorted = tasks.to_vec();
    sorted.sort_by(compare_tasks);
    sorted
}

/// Fixed buckets, empty sections skipped.
pub fn group_tasks(tasks: &[PlannedTask], today: &str) -> Vec<(Bucket, Vec<PlannedTask>)> {
    BUCKET_ORDER
        .iter()
        .map(|bucket| {
            let items: Vec<PlannedTask> = tasks
                .iter()
                .filter(|task| bucket_of(task, today) == *bucket)
                .cloned()
                .collect();
            (*bucket, sort_tasks(&items))
        })
        .filter(|(_, items)| !items.is_empty())
        .collect()
}

pub fn task_stats(tasks: &[PlannedTask], today: &str) -> TaskStats {
    let mut stats = TaskStats::default();
    for task in tasks {
        if task.item.is_done() {
            stats.done += 1;
            continue;
        }
        match bucket_of(task, today) {
            Bucket::Overdue => stats.overdue += 1,
            Bucket::Today => stats.today += 1,
            _ => {}
        }
    }
    stats.total = tasks.len();
    stats.open = stats.total - stats.done;
    stats.completion_rate = if stats.total == 0 {
        0
    } else {
        ((stats.done * 100) as f32 / stats.total as f32).round() as u32
    };
    stats
}

pub fn tasks_on_day(tasks: &[PlannedTask], day: &str, include_done: bool) -> Vec<PlannedTask> {
    let matching: Vec<PlannedTask> = tasks
        .iter()
        .filter(|task| task.item.due.as_deref() == Some(day))
        .filter(|task| include_done || !task.item.is_done())
        .cloned()
        .collect();
    sort_tasks(&matching)
}

// ── Gantt ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GanttRange {
    pub start: String,
    pub end: String,
}

/// One day of padding around every dated task.
pub fn gantt_range(tasks: &[PlannedTask], today: &str) -> GanttRange {
    let mut min: Option<String> = None;
    let mut max: Option<String> = None;
    for task in tasks {
        let Some(start) = task.item.start_day().map(str::to_string) else {
            continue;
        };
        let end = task
            .item
            .due
            .as_deref()
            .filter(|d| is_date_key(d))
            .unwrap_or(&start)
            .to_string();
        let (lo, hi) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        if min.as_deref().map(|m| lo.as_str() < m).unwrap_or(true) {
            min = Some(lo);
        }
        if max.as_deref().map(|m| hi.as_str() > m).unwrap_or(true) {
            max = Some(hi);
        }
    }
    match (min, max) {
        (Some(min), Some(max)) => GanttRange {
            start: add_days(&min, -1),
            end: add_days(&max, 1),
        },
        _ => GanttRange {
            start: add_days(today, -1),
            end: add_days(today, 13),
        },
    }
}

/// Bar geometry in whole days relative to `range.start`.
pub fn task_bar(task: &PlannedTask, range: &GanttRange) -> Option<(i64, i64)> {
    let start = task.item.start_day()?.to_string();
    let due = task
        .item
        .due
        .as_deref()
        .filter(|d| is_date_key(d))
        .unwrap_or(&start)
        .to_string();
    let (lo, hi) = if start <= due {
        (start, due)
    } else {
        (due, start)
    };
    Some((
        days_between(&range.start, &lo),
        days_between(&lo, &hi).max(0) + 1,
    ))
}

// ── Dependencies ─────────────────────────────────────────────────────────────

pub fn would_create_cycle(tasks: &[PlannedTask], target: &str, dependency: &str) -> bool {
    if target == dependency {
        return true;
    }
    let by_key: std::collections::HashMap<String, &PlannedTask> =
        tasks.iter().map(|task| (task.key(), task)).collect();
    let mut seen = std::collections::HashSet::new();
    let mut stack = vec![dependency.to_string()];
    while let Some(current) = stack.pop() {
        if current == target {
            return true;
        }
        if !seen.insert(current.clone()) {
            continue;
        }
        if let Some(task) = by_key.get(&current) {
            for blocker in &task.item.blocked_by {
                stack.push(blocker.clone());
            }
        }
    }
    false
}

pub fn is_blocked(task: &PlannedTask, tasks: &[PlannedTask]) -> bool {
    if task.item.is_done() {
        return false;
    }
    let by_key: std::collections::HashMap<String, &PlannedTask> =
        tasks.iter().map(|candidate| (candidate.key(), candidate)).collect();
    task.item.blocked_by.iter().any(|key| {
        by_key
            .get(key)
            .map(|blocker| !blocker.item.is_done())
            .unwrap_or(false)
    })
}

// ── ICS export (RFC 5545) ────────────────────────────────────────────────────

pub fn escape_ics_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace("\r\n", "\\n")
        .replace(['\r', '\n'], "\\n")
}

/// Fold at 75 octets (excluding CRLF); never split a multi-byte character.
pub fn fold_ics_line(line: &str) -> Vec<String> {
    if line.len() <= 75 {
        return vec![line.to_string()];
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut bytes = 0;
    let mut limit = 75;
    for ch in line.chars() {
        let size = ch.len_utf8();
        if bytes + size > limit {
            chunks.push(std::mem::take(&mut current));
            current.push(ch);
            bytes = size;
            limit = 74; // continuation lines carry a leading space
        } else {
            current.push(ch);
            bytes += size;
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| {
            if index == 0 {
                chunk
            } else {
                format!(" {chunk}")
            }
        })
        .collect()
}

fn sanitize_uid(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn ics_priority(priority: &str) -> u8 {
    match priority {
        "high" => 1,
        "low" => 9,
        _ => 5,
    }
}

pub struct IcsOptions {
    pub calendar_name: String,
    pub include_completed: bool,
    /// Language of generated VTODO labels (SUMMARY fallback, DESCRIPTION lines,
    /// PRODID). Matches the web `buildIcs` behavior.
    pub lang: crate::i18n::Locale,
}

impl Default for IcsOptions {
    fn default() -> Self {
        Self {
            calendar_name: "CapsuleCraft Planning".to_string(),
            include_completed: false,
            lang: crate::i18n::Locale::En,
        }
    }
}

/// Localized labels for one VTODO description.
struct IcsLabels {
    untitled: &'static str,
    capsule: &'static str,
    list: &'static str,
    tags: &'static str,
}

impl IcsLabels {
    fn for_lang(lang: crate::i18n::Locale) -> Self {
        match lang {
            crate::i18n::Locale::Ru => Self {
                untitled: "(без названия)",
                capsule: "Капсула",
                list: "Список",
                tags: "Теги",
            },
            crate::i18n::Locale::En => Self {
                untitled: "(untitled)",
                capsule: "Capsule",
                list: "List",
                tags: "Tags",
            },
        }
    }
}

/// `VCALENDAR` with one `VTODO` per task, CRLF-terminated.
pub fn build_ics(tasks: &[PlannedTask], options: &IcsOptions, now: chrono::DateTime<chrono::Utc>) -> String {
    let stamp = now.format("%Y%m%dT%H%M%SZ").to_string();
    let labels = IcsLabels::for_lang(options.lang);
    let prodid_lang = match options.lang {
        crate::i18n::Locale::Ru => "RU",
        crate::i18n::Locale::En => "EN",
    };
    let mut lines: Vec<String> = vec![
        "BEGIN:VCALENDAR".into(),
        "VERSION:2.0".into(),
        format!("PRODID:-//CapsuleCraft//Planning//{prodid_lang}"),
        "CALSCALE:GREGORIAN".into(),
        "METHOD:PUBLISH".into(),
        format!("X-WR-CALNAME:{}", escape_ics_text(&options.calendar_name)),
    ];

    for task in tasks {
        if !options.include_completed && task.item.is_done() {
            continue;
        }
        let uid = format!(
            "{}.{}@capsuleos.local",
            sanitize_uid(&task.block_id),
            sanitize_uid(&task.item.id)
        );
        let summary = if task.item.text.trim().is_empty() {
            labels.untitled.to_string()
        } else {
            task.item.text.trim().to_string()
        };
        let mut description = vec![format!("{}: {}", labels.capsule, task.capsule_subject)];
        if let Some(title) = task.block_title.as_deref().filter(|t| !t.is_empty()) {
            description.push(format!("{}: {title}", labels.list));
        }
        if !task.capsule_tags.is_empty() {
            description.push(format!("{}: {}", labels.tags, task.capsule_tags.join(" ")));
        }

        lines.push("BEGIN:VTODO".into());
        lines.push(format!("UID:{uid}"));
        lines.push(format!("DTSTAMP:{stamp}"));
        if let Some(due) = task.item.due.as_deref().filter(|d| is_date_key(d)) {
            lines.push(format!("DUE;VALUE=DATE:{}", due.replace('-', "")));
        }
        lines.push(format!("SUMMARY:{}", escape_ics_text(&summary)));
        lines.push(format!(
            "DESCRIPTION:{}",
            escape_ics_text(&description.join("\n"))
        ));
        let status = task.item.status();
        lines.push(format!(
            "STATUS:{}",
            match status.as_str() {
                "done" => "COMPLETED",
                "doing" => "IN-PROCESS",
                _ => "NEEDS-ACTION",
            }
        ));
        lines.push(format!("PRIORITY:{}", ics_priority(task.item.priority())));
        if !task.capsule_tags.is_empty() {
            lines.push(format!(
                "CATEGORIES:{}",
                task.capsule_tags
                    .iter()
                    .map(|tag| escape_ics_text(tag))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        if status == "done" {
            lines.push("PERCENT-COMPLETE:100".into());
        }
        lines.push("END:VTODO".into());
    }

    lines.push("END:VCALENDAR".into());
    lines
        .iter()
        .flat_map(|line| fold_ics_line(line))
        .collect::<Vec<_>>()
        .join("\r\n")
        + "\r\n"
}

pub fn ics_file_name(today: &str) -> String {
    format!("capsuleos-planning-{today}.ics")
}

// ── JSON mutation helpers ────────────────────────────────────────────────────

/// Apply a patch to one todo item inside a todo block; returns true when the
/// block changed. Unknown items and non-todo blocks are untouched.
pub fn patch_item(block: &mut Value, item_id: &str, patch: &TodoItem) -> bool {
    if block_type(block) != "todo_list" {
        return false;
    }
    let Some(items) = block
        .get_mut("data")
        .and_then(|data| data.get_mut("items"))
        .and_then(Value::as_array_mut)
    else {
        return false;
    };
    for item in items.iter_mut() {
        if item.get("id").and_then(Value::as_str) == Some(item_id) {
            *item = patch.to_json();
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, text: &str, due: Option<&str>, priority: Option<&str>) -> PlannedTask {
        PlannedTask {
            capsule_id: "msg-1".into(),
            capsule_subject: "Spec".into(),
            capsule_tags: vec!["#plan".into()],
            block_id: "blk-1".into(),
            block_title: None,
            item: TodoItem {
                id: id.into(),
                text: text.into(),
                done: false,
                status: None,
                start: None,
                blocked_by: vec![],
                due: due.map(str::to_string),
                priority: priority.map(str::to_string),
            },
        }
    }

    #[test]
    fn date_math_survives_month_boundaries() {
        assert_eq!(add_days("2026-01-31", 1), "2026-02-01");
        assert_eq!(add_days("2026-03-01", -1), "2026-02-28");
        assert_eq!(days_between("2026-10-01", "2026-10-23"), 22);
        assert!(!is_date_key("2026-02-30"));
    }

    #[test]
    fn month_grid_starts_on_monday() {
        // 2026-10-01 is a Thursday → grid starts 2026-09-28.
        let grid = month_grid(2026, 10);
        assert_eq!(grid.len(), 42);
        assert_eq!(grid[0], "2026-09-28");
    }

    #[test]
    fn buckets_and_stats() {
        let today = "2026-10-23";
        let tasks = vec![
            task("a", "overdue", Some("2026-10-20"), None),
            task("b", "today", Some(today), Some("high")),
            task("c", "nodate", None, None),
        ];
        assert_eq!(bucket_of(&tasks[0], today), Bucket::Overdue);
        assert_eq!(bucket_of(&tasks[1], today), Bucket::Today);
        assert_eq!(bucket_of(&tasks[2], today), Bucket::None);
        let stats = task_stats(&tasks, today);
        assert_eq!((stats.total, stats.open, stats.overdue, stats.today), (3, 3, 1, 1));
        assert_eq!(stats.completion_rate, 0);
    }

    #[test]
    fn filters_compose() {
        let today = "2026-10-23";
        let tasks = vec![
            task("a", "write spec", Some("2026-10-20"), Some("high")),
            task("b", "review spec", Some("2026-10-24"), Some("low")),
            task("c", "buy coffee", None, None),
        ];
        let filters = TaskFilters {
            text: "spec".into(),
            status: "open".into(),
            priority: "high".into(),
            due: "all".into(),
            ..TaskFilters::default()
        };
        let hits = filter_tasks(&tasks, &filters, today);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].item.id, "a");

        let overdue = TaskFilters {
            due: "overdue".into(),
            ..TaskFilters::default()
        };
        assert_eq!(filter_tasks(&tasks, &overdue, today).len(), 1);

        let none = TaskFilters {
            due: "none".into(),
            ..TaskFilters::default()
        };
        assert_eq!(filter_tasks(&tasks, &none, today)[0].item.id, "c");
    }

    #[test]
    fn gantt_bars_are_at_least_one_day() {
        let task = task("a", "x", Some("2026-10-25"), None);
        let range = GanttRange {
            start: "2026-10-23".into(),
            end: "2026-10-30".into(),
        };
        assert_eq!(task_bar(&task, &range), Some((2, 1)));
    }

    #[test]
    fn dependencies_detect_cycles() {
        let mut a = task("a", "a", None, None);
        let mut b = task("b", "b", None, None);
        a.item.blocked_by = vec!["blk-1:b".into()];
        b.item.blocked_by = vec!["blk-1:a".into()];
        let tasks = vec![a, b];
        assert!(would_create_cycle(&tasks, "blk-1:a", "blk-1:b"));
        assert!(is_blocked(&tasks[0], &tasks));
    }

    #[test]
    fn ics_folds_at_octet_boundaries() {
        let line = "SUMMARY:".to_string() + &"я".repeat(60);
        let folded = fold_ics_line(&line);
        assert!(folded.len() > 1);
        for chunk in &folded {
            assert!(chunk.len() <= 75);
        }
        assert!(folded[1].starts_with(' '));
    }

    #[test]
    fn ics_contains_one_vtodo_per_open_task() {
        let mut done = task("b", "done", Some("2026-10-24"), None);
        done.item.done = true;
        let tasks = vec![task("a", "open", Some("2026-10-25"), Some("high")), done];
        let ics = build_ics(&tasks, &IcsOptions::default(), chrono::Utc::now());
        assert_eq!(ics.matches("BEGIN:VTODO").count(), 1);
        assert!(ics.contains("DUE;VALUE=DATE:20261025"));
        assert!(ics.contains("PRIORITY:1"));
        assert!(ics.ends_with("\r\n"));
    }

    #[test]
    fn ics_labels_follow_locale() {
        let tasks = vec![task("a", "", Some("2026-10-25"), None)];
        let mk = |lang| IcsOptions {
            lang,
            ..IcsOptions::default()
        };

        let en = build_ics(&tasks, &mk(crate::i18n::Locale::En), chrono::Utc::now());
        assert!(en.contains("PRODID:-//CapsuleCraft//Planning//EN"));
        assert!(en.contains("SUMMARY:(untitled)"));
        assert!(en.contains("Capsule: Spec"));
        assert!(en.contains("Tags: #plan"));
        assert!(!en.contains("(без названия)"));

        let ru = build_ics(&tasks, &mk(crate::i18n::Locale::Ru), chrono::Utc::now());
        assert!(ru.contains("PRODID:-//CapsuleCraft//Planning//RU"));
        assert!(ru.contains("SUMMARY:(без названия)"));
        assert!(ru.contains("Капсула: Spec"));
        assert!(ru.contains("Теги: #plan"));
    }

    #[test]
    fn patching_an_item_preserves_unknown_fields() {
        let mut block = json!({
            "id": "blk-1",
            "type": "todo_list",
            "data": {
                "title": "List",
                "items": [{ "id": "a", "text": "old", "done": false, "custom": 7 }],
                "unknown": { "keep": true }
            }
        });
        let mut item = TodoItem::from_json(&block["data"]["items"][0]).unwrap();
        item.text = "new".into();
        item.due = Some("2026-10-25".into());
        assert!(patch_item(&mut block, "a", &item));
        assert_eq!(block["data"]["items"][0]["text"], "new");
        assert_eq!(block["data"]["items"][0]["due"], "2026-10-25");
        assert_eq!(block["data"]["unknown"]["keep"], true);
    }
}
