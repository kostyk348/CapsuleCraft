//! Markdown export for one page.
//!
//! The web app exports a standalone HTML dashboard; the native shell exports
//! plain Markdown so a capsule can be pasted into a README, a ticket or a
//! message without dragging the block model along. Unknown block types are
//! preserved as HTML comments instead of being dropped.

use serde_json::Value;

use crate::mime::{block_type, Capsule};

/// Render one page as Markdown.
pub fn page_markdown(capsule: &Capsule) -> String {
    let mut out = String::new();
    let title = if capsule.subject.trim().is_empty() {
        "Untitled"
    } else {
        capsule.subject.trim()
    };
    out.push_str("# ");
    out.push_str(title);
    out.push_str("\n\n");

    out.push_str(&format!("_{} · {}_", capsule.from, capsule.date));
    out.push_str("\n\n");

    if !capsule.tags.is_empty() {
        let tags: Vec<String> = capsule.tags.iter().map(|tag| format!("#{tag}")).collect();
        out.push_str(&tags.join(" "));
        out.push_str("\n\n");
    }

    for block in &capsule.blocks {
        render_block(block, &mut out);
    }

    if !capsule.attachments.is_empty() {
        out.push_str("## Attachments\n\n");
        for attachment in &capsule.attachments {
            out.push_str(&format!(
                "- `{}` — {} · {} bytes\n",
                attachment.name, attachment.mime_type, attachment.size_bytes
            ));
        }
        out.push('\n');
    }

    while out.ends_with("\n\n\n") {
        out.pop();
    }
    out
}

fn render_block(block: &Value, out: &mut String) {
    let kind = block_type(block).to_string();
    let data = block.get("data").cloned().unwrap_or(Value::Null);
    let text = |key: &str| {
        data.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };

    match kind.as_str() {
        "heading" => {
            let level = data
                .get("level")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 3);
            out.push_str(&"#".repeat(level as usize + 1));
            out.push(' ');
            out.push_str(&text("text"));
            out.push_str("\n\n");
        }
        "paragraph" => {
            let body = text("text");
            if !body.is_empty() {
                out.push_str(&body);
                out.push_str("\n\n");
            }
        }
        "todo_list" => {
            let title = text("title");
            if !title.is_empty() {
                out.push_str(&format!("**{title}**\n\n"));
            }
            if let Some(items) = data.get("items").and_then(Value::as_array) {
                for item in items {
                    let checked = item
                        .get("status")
                        .and_then(Value::as_str)
                        .map(|status| status == "done")
                        .or_else(|| item.get("done").and_then(Value::as_bool))
                        .unwrap_or(false);
                    let label = item.get("text").and_then(Value::as_str).unwrap_or("");
                    let mut line = format!("- [{}] {}", if checked { "x" } else { " " }, label);
                    let mut meta = Vec::new();
                    if let Some(due) = item.get("due").and_then(Value::as_str) {
                        meta.push(format!("due {due}"));
                    }
                    if let Some(priority) = item.get("priority").and_then(Value::as_str) {
                        if !priority.is_empty() && priority != "normal" {
                            meta.push(priority.to_string());
                        }
                    }
                    if !meta.is_empty() {
                        line.push_str(&format!(" _({})_", meta.join(", ")));
                    }
                    out.push_str(&line);
                    out.push('\n');
                }
                out.push('\n');
            }
        }
        "callout" => {
            let title = text("title");
            if !title.is_empty() {
                out.push_str(&format!("> **{title}**\n>\n"));
            }
            for line in text("content").lines() {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
            out.push('\n');
        }
        "code_repl" => {
            let language = text("language");
            out.push_str("```");
            out.push_str(if language.is_empty() {
                "text"
            } else {
                &language
            });
            out.push('\n');
            out.push_str(&text("code"));
            out.push_str("\n```\n\n");
        }
        "data_grid_view" => {
            let source = text("source_cid");
            out.push_str(&format!(
                "> 📊 **Data table** — source attachment: `{}`\n\n",
                if source.is_empty() { "none" } else { &source }
            ));
        }
        "audio_note" => {
            let duration = text("duration");
            let transcript = text("transcript");
            out.push_str(&format!(
                "> 🎧 **Audio note** ({duration})\n>\n> {transcript}\n\n"
            ));
        }
        "interactive_canvas" => {
            out.push_str("> 🧩 **Whiteboard**\n\n");
            if let Some(elements) = data.get("elements").and_then(Value::as_array) {
                for element in elements {
                    let kind = element
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("element");
                    let label = element.get("text").and_then(Value::as_str).unwrap_or("");
                    out.push_str(&format!("- {kind}: {label}\n"));
                }
                out.push('\n');
            }
        }
        "ai_opponent_block" => {
            let claim = text("sourceClaim");
            if !claim.is_empty() {
                out.push_str(&format!("**Claim:** {claim}\n\n"));
            }
            for line in text("criticism").lines() {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
            out.push('\n');
        }
        other => {
            out.push_str(&format!("<!-- preserved block: {other} -->\n\n"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn capsule(blocks: Vec<Value>) -> Capsule {
        let mut capsule = Capsule::blank("<a@v>".into(), "a.eml".into(), "Notes".into());
        capsule.from = "you@vault.local".into();
        capsule.date = "Fri, 23 Oct 2026 12:00:00 +0300".into();
        capsule.tags = vec!["architecture".into()];
        capsule.blocks = blocks;
        capsule
    }

    #[test]
    fn starts_with_the_title_and_metadata() {
        let markdown = page_markdown(&capsule(vec![]));
        assert!(markdown.starts_with("# Notes\n\n"));
        assert!(markdown.contains("_you@vault.local"));
        assert!(markdown.contains("#architecture"));
    }

    #[test]
    fn headings_map_to_markdown_levels() {
        let markdown = page_markdown(&capsule(vec![
            json!({"id": "h1", "type": "heading", "data": {"level": 1, "text": "One"}}),
            json!({"id": "h3", "type": "heading", "data": {"level": 3, "text": "Three"}}),
        ]));
        assert!(markdown.contains("## One"));
        assert!(markdown.contains("#### Three"));
    }

    #[test]
    fn tasks_render_checkboxes_and_metadata() {
        let markdown = page_markdown(&capsule(vec![json!({
            "id": "t",
            "type": "todo_list",
            "data": {
                "title": "Work",
                "items": [
                    {"id": "1", "text": "Ship", "status": "done", "due": "2026-10-23"},
                    {"id": "2", "text": "Review", "status": "todo", "priority": "high"}
                ]
            }
        })]));
        assert!(markdown.contains("**Work**"));
        assert!(markdown.contains("- [x] Ship _(due 2026-10-23)_"));
        assert!(markdown.contains("- [ ] Review _(high)_"));
    }

    #[test]
    fn code_and_callouts_keep_their_shape() {
        let markdown = page_markdown(&capsule(vec![
            json!({"id": "c", "type": "code_repl", "data": {"language": "rust", "code": "fn main() {}"}}),
            json!({"id": "q", "type": "callout", "data": {"title": "Careful", "content": "Line one\nLine two"}}),
        ]));
        assert!(markdown.contains("```rust\nfn main() {}\n```"));
        assert!(markdown.contains("> **Careful**"));
        assert!(markdown.contains("> Line one\n> Line two"));
    }

    #[test]
    fn unknown_blocks_are_preserved_as_comments() {
        let markdown = page_markdown(&capsule(vec![
            json!({"id": "x", "type": "mystery_block", "data": {"text": "keep"}}),
        ]));
        assert!(markdown.contains("<!-- preserved block: mystery_block -->"));
    }

    #[test]
    fn attachments_are_listed() {
        let mut capsule = capsule(vec![]);
        capsule.attachments = vec![crate::mime::Attachment {
            name: "data.csv".into(),
            mime_type: "text/csv".into(),
            cid: "data.csv".into(),
            content: "a,b\n1,2".into(),
            raw_b64: None,
            size_bytes: 7,
        }];
        let markdown = page_markdown(&capsule);
        assert!(markdown.contains("## Attachments"));
        assert!(markdown.contains("`data.csv` — text/csv · 7 bytes"));
    }
}
