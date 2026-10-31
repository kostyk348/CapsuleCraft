use std::path::Path;

use egui::{FontId, Ui};
use serde_json::{json, Value};

use crate::mime::{self, block_type, Capsule};
use crate::theme;

pub fn folder_name(dir: &Path) -> String {
    dir.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "Vault".to_string())
}

pub fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let mut out: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

pub fn truncate_to_width(ui: &Ui, text: &str, font: FontId, max_width: f32) -> String {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font.clone(), theme::INK);
    if galley.size().x <= max_width {
        return text.to_string();
    }
    let ellipsis = "…";
    let ellipsis_w = ui
        .painter()
        .layout_no_wrap(ellipsis.to_string(), font.clone(), theme::INK)
        .size()
        .x;
    let target_w = (max_width - ellipsis_w).max(0.0);

    let mut result = String::new();
    for ch in text.chars() {
        let mut test = result.clone();
        test.push(ch);
        let test_g = ui.painter().layout_no_wrap(test, font.clone(), theme::INK);
        if test_g.size().x > target_w {
            break;
        }
        result.push(ch);
    }
    result.push('…');
    result
}

pub fn format_date(capsule: &Capsule) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(capsule.timestamp)
        .map(|date| {
            date.with_timezone(&chrono::Local)
                .format("%b %e, %Y · %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| capsule.date.clone())
}

pub fn format_date_long(capsule: &Capsule) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(capsule.timestamp)
        .map(|date| {
            date.with_timezone(&chrono::Local)
                .format("%A, %b %e, %Y at %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| capsule.date.clone())
}

pub fn human_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

pub fn status_label(status: &str) -> &'static str {
    match status {
        "doing" => "Doing",
        "done" => "Done",
        _ => "To do",
    }
}

pub fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

pub fn block_data_text(block: &Value) -> String {
    mime::block_data_text(block)
}

pub fn set_block_text(block: &mut Value, text: String) {
    mime::set_block_text(block, text)
}

pub fn is_duplicate_heading(block: &Value, subject: &str) -> bool {
    let kind = block_type(block);
    if kind != "heading" {
        return false;
    }
    let level = block
        .get("data")
        .and_then(|data| data.get("level"))
        .and_then(Value::as_u64)
        .unwrap_or(1);
    let text = mime::block_data_text(block);
    let text_norm = text.trim();
    let subj_norm = subject.trim();
    level == 1 && (text_norm == subj_norm || text_norm.to_lowercase() == subj_norm.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duplicate_heading_detection() {
        let h1_matching = json!({
            "type": "heading",
            "data": { "level": 1, "text": "Моя первая заметка" }
        });
        assert!(is_duplicate_heading(&h1_matching, "Моя первая заметка"));
        assert!(is_duplicate_heading(&h1_matching, "  моя первая заметка  "));

        let h2_same_text = json!({
            "type": "heading",
            "data": { "level": 2, "text": "Моя первая заметка" }
        });
        assert!(!is_duplicate_heading(&h2_same_text, "Моя первая заметка"));

        let h1_different_text = json!({
            "type": "heading",
            "data": { "level": 1, "text": "Другой заголовок" }
        });
        assert!(!is_duplicate_heading(
            &h1_different_text,
            "Моя первая заметка"
        ));

        let text_block = json!({
            "type": "paragraph",
            "data": { "text": "Моя первая заметка" }
        });
        assert!(!is_duplicate_heading(&text_block, "Моя первая заметка"));
    }
}
