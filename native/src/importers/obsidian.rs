//! Obsidian & Markdown vault importer for CapsuleCraft.
//!
//! Converts Obsidian Markdown notes with YAML frontmatter, [[wikilinks]],
//! embedded attachments, and nested folder hierarchies into native RFC 5322
//! `.eml` capsules.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::mime::{assemble_eml, new_block_id, Attachment, Capsule};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frontmatter {
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    pub date: Option<String>,
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ImportSummary {
    pub imported_pages: usize,
    pub created_folders: usize,
    pub imported_attachments: usize,
    pub errors: Vec<String>,
}

/// Parse YAML frontmatter between `---` boundaries at the start of a markdown file.
pub fn parse_frontmatter(content: &str) -> (Frontmatter, &str) {
    let s = content.trim_start_matches('\u{feff}');
    let trimmed = s.trim_start();
    if !trimmed.starts_with("---") {
        return (Frontmatter::default(), s);
    }

    let rest_after_first = &trimmed[3..];
    let Some(first_newline) = rest_after_first.find('\n') else {
        return (Frontmatter::default(), s);
    };

    let body_start_candidate = &rest_after_first[first_newline + 1..];
    let mut end_idx = None;
    let mut current_offset = 0;

    for line in body_start_candidate.lines() {
        let t = line.trim();
        if t == "---" || t == "..." {
            end_idx = Some(current_offset);
            break;
        }
        current_offset += line.len() + 1; // approx line len + newline
    }

    let Some(end_offset) = end_idx else {
        return (Frontmatter::default(), s);
    };

    let yaml_slice = &body_start_candidate[..end_offset];
    let after_yaml = &body_start_candidate[end_offset..];
    let md_body = if let Some(nl) = after_yaml.find('\n') {
        &after_yaml[nl + 1..]
    } else {
        ""
    };

    let mut fm = Frontmatter::default();
    let mut current_key = String::new();

    for raw_line in yaml_slice.lines() {
        let line = raw_line.trim_end();
        if line.trim().is_empty() || line.trim().starts_with('#') {
            continue;
        }

        if let Some(list_item) = line.trim().strip_prefix('-') {
            let val = list_item.trim().trim_matches(|c| c == '\'' || c == '"');
            if !val.is_empty() {
                match current_key.as_str() {
                    "tags" | "tag" => {
                        let clean_tag = val.trim_start_matches('#');
                        if !clean_tag.is_empty() {
                            fm.tags.push(clean_tag.to_string());
                        }
                    }
                    "aliases" | "alias" => {
                        fm.aliases.push(val.to_string());
                    }
                    _ => {}
                }
            }
            continue;
        }

        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim();
            current_key = key.clone();

            match key.as_str() {
                "title" => {
                    let clean = val.trim_matches(|c| c == '\'' || c == '"').trim();
                    if !clean.is_empty() {
                        fm.title = Some(clean.to_string());
                    }
                }
                "tags" | "tag" => {
                    if !val.is_empty() {
                        parse_inline_list(val, &mut fm.tags, true);
                    }
                }
                "aliases" | "alias" => {
                    if !val.is_empty() {
                        parse_inline_list(val, &mut fm.aliases, false);
                    }
                }
                "date" | "created" | "created_at" | "date created" => {
                    let clean = val.trim_matches(|c| c == '\'' || c == '"').trim();
                    if !clean.is_empty() {
                        fm.date = Some(clean.to_string());
                    }
                }
                "parent" | "up" => {
                    let clean = val.trim_matches(|c| c == '\'' || c == '"').trim();
                    if !clean.is_empty() {
                        fm.parent = Some(clean.to_string());
                    }
                }
                _ => {}
            }
        }
    }

    (fm, md_body)
}

fn parse_inline_list(val: &str, target: &mut Vec<String>, is_tag: bool) {
    let clean = val.trim_matches(|c| c == '[' || c == ']' || c == '\'' || c == '"');
    for part in clean.split(',') {
        let item = part.trim().trim_matches(|c| c == '\'' || c == '"');
        let item = if is_tag { item.trim_start_matches('#') } else { item };
        if !item.is_empty() {
            target.push(item.to_string());
        }
    }
}

/// Convert Markdown content into CapsuleCraft blocks (`heading`, `todo_list`, `callout`, `code_repl`, `paragraph`).
pub fn parse_markdown_to_blocks(md: &str) -> Vec<Value> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = md.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        // 1. Code blocks (```)
        if trimmed.starts_with("```") {
            let lang = trimmed.trim_start_matches('`').trim().to_string();
            let mut code_lines = Vec::new();
            i += 1;
            while i < lines.len() {
                if lines[i].trim().starts_with("```") {
                    i += 1;
                    break;
                }
                code_lines.push(lines[i]);
                i += 1;
            }
            blocks.push(json!({
                "id": new_block_id("code_repl"),
                "type": "code_repl",
                "data": {
                    "language": if lang.is_empty() { "text" } else { &lang },
                    "code": code_lines.join("\n")
                }
            }));
            continue;
        }

        // 2. Headings (#)
        if trimmed.starts_with('#') {
            let mut count = 0;
            for ch in trimmed.chars() {
                if ch == '#' {
                    count += 1;
                } else {
                    break;
                }
            }
            if count > 0 && trimmed[count..].starts_with(' ') {
                let text = trimmed[count..].trim();
                let level = count.clamp(1, 3);
                blocks.push(json!({
                    "id": new_block_id("heading"),
                    "type": "heading",
                    "data": {
                        "level": level,
                        "text": text
                    }
                }));
                i += 1;
                continue;
            }
        }

        // 3. Task / Checkbox lists (- [ ] or - [x])
        if is_task_line(trimmed) {
            let mut items = Vec::new();
            let mut item_counter = 0;
            while i < lines.len() {
                let cur_trimmed = lines[i].trim();
                if let Some((done, text)) = parse_task_line(cur_trimmed) {
                    items.push(json!({
                        "id": format!("task_{item_counter}"),
                        "text": text,
                        "done": done,
                        "status": if done { "done" } else { "todo" },
                        "priority": "normal"
                    }));
                    item_counter += 1;
                    i += 1;
                } else {
                    break;
                }
            }
            blocks.push(json!({
                "id": new_block_id("todo_list"),
                "type": "todo_list",
                "data": {
                    "title": "",
                    "items": items
                }
            }));
            continue;
        }

        // 4. Callouts & Blockquotes (> )
        if trimmed.starts_with('>') {
            let mut quote_lines = Vec::new();
            let first_clean = strip_quote_prefix(trimmed);
            let (is_callout, callout_type, callout_title) = parse_callout_header(first_clean);

            if !is_callout {
                quote_lines.push(first_clean);
            }

            i += 1;
            while i < lines.len() {
                let cur = lines[i].trim();
                if cur.starts_with('>') {
                    quote_lines.push(strip_quote_prefix(cur));
                    i += 1;
                } else {
                    break;
                }
            }

            let (icon, tone) = match callout_type.to_ascii_lowercase().as_str() {
                "note" | "info" => ("info", "blue"),
                "tip" => ("lightbulb", "green"),
                "warning" | "caution" | "danger" => ("alert", "amber"),
                _ => ("quote", "gray"),
            };

            blocks.push(json!({
                "id": new_block_id("callout"),
                "type": "callout",
                "data": {
                    "title": callout_title,
                    "content": quote_lines.join("\n").trim().to_string(),
                    "icon": icon,
                    "tone": tone
                }
            }));
            continue;
        }

        // 5. Standard paragraph
        let mut para_lines = Vec::new();
        while i < lines.len() {
            let cur = lines[i];
            let cur_t = cur.trim();
            if cur_t.is_empty()
                || cur_t.starts_with("```")
                || (cur_t.starts_with('#') && cur_t.contains(' '))
                || is_task_line(cur_t)
                || cur_t.starts_with('>')
            {
                break;
            }
            para_lines.push(cur);
            i += 1;
        }

        if !para_lines.is_empty() {
            blocks.push(json!({
                "id": new_block_id("paragraph"),
                "type": "paragraph",
                "data": {
                    "text": para_lines.join("\n").trim().to_string()
                }
            }));
        }
    }

    if blocks.is_empty() {
        blocks.push(json!({
            "id": new_block_id("paragraph"),
            "type": "paragraph",
            "data": { "text": "" }
        }));
    }

    blocks
}

fn is_task_line(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with("- [ ]")
        || t.starts_with("- [x]")
        || t.starts_with("- [X]")
        || t.starts_with("* [ ]")
        || t.starts_with("* [x]")
        || t.starts_with("* [X]")
}

fn parse_task_line(s: &str) -> Option<(bool, String)> {
    let t = s.trim();
    let marker = if t.starts_with("- [ ]") || t.starts_with("* [ ]") {
        Some((false, 5))
    } else if t.starts_with("- [x]")
        || t.starts_with("- [X]")
        || t.starts_with("* [x]")
        || t.starts_with("* [X]")
    {
        Some((true, 5))
    } else {
        None
    }?;

    let text = t[marker.1..].trim().to_string();
    Some((marker.0, text))
}

fn strip_quote_prefix(s: &str) -> &str {
    let after_gt = s.strip_prefix('>').unwrap_or(s);
    after_gt.strip_prefix(' ').unwrap_or(after_gt)
}

fn parse_callout_header(s: &str) -> (bool, String, String) {
    if let Some(rest) = s.strip_prefix("[!") {
        if let Some(bracket_end) = rest.find(']') {
            let ctype = rest[..bracket_end].trim().to_string();
            let title = rest[bracket_end + 1..].trim().to_string();
            let final_title = if title.is_empty() {
                ctype.clone()
            } else {
                title
            };
            return (true, ctype, final_title);
        }
    }
    (false, String::new(), String::new())
}

/// Extract referenced filenames from `![[filename.ext]]` and `![alt](filename.ext)`.
pub fn extract_attachment_references(md: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = HashSet::new();

    // 1. Wikilink style: ![[image.png]] or ![[image.png|100]]
    let mut rest = md;
    while let Some(start) = rest.find("![[") {
        let after_start = &rest[start + 3..];
        if let Some(end) = after_start.find("]]") {
            let inner = &after_start[..end];
            let filename = inner.split('|').next().unwrap_or("").trim();
            if !filename.is_empty() && seen.insert(filename.to_ascii_lowercase()) {
                names.push(filename.to_string());
            }
            rest = &after_start[end + 2..];
        } else {
            break;
        }
    }

    // 2. Standard markdown style: ![alt](path/image.png)
    let mut rest_md = md;
    while let Some(start) = rest_md.find("![") {
        let after_bracket = &rest_md[start + 2..];
        if let Some(bracket_end) = after_bracket.find("](") {
            let after_paren = &after_bracket[bracket_end + 2..];
            if let Some(paren_end) = after_paren.find(')') {
                let path_str = &after_paren[..paren_end].trim();
                let file_name = Path::new(path_str)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .trim();
                if !file_name.is_empty() && seen.insert(file_name.to_ascii_lowercase()) {
                    names.push(file_name.to_string());
                }
                rest_md = &after_paren[paren_end + 1..];
            } else {
                break;
            }
        } else {
            break;
        }
    }

    names
}

pub fn guess_mime(name: &str) -> String {
    let ext = name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "csv" => "text/csv",
        "json" => "application/json",
        "txt" => "text/plain",
        "md" => "text/markdown",
        _ => "application/octet-stream",
    }
    .to_string()
}

pub fn sanitize_cid(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

pub fn slugify(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == '-' || ch == '_' || ch == ' ' || ch == '/' || ch == '\\' {
            if !out.ends_with('-') && !out.is_empty() {
                out.push('-');
            }
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "note".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Import an Obsidian vault into a CapsuleCraft vault folder.
pub fn import_obsidian_vault(
    obsidian_dir: &Path,
    target_vault_dir: &Path,
) -> Result<ImportSummary, String> {
    if !obsidian_dir.exists() || !obsidian_dir.is_dir() {
        return Err(format!(
            "Obsidian path is not a valid directory: {}",
            obsidian_dir.display()
        ));
    }
    fs::create_dir_all(target_vault_dir)
        .map_err(|e| format!("Cannot create target vault folder: {e}"))?;

    let mut summary = ImportSummary::default();

    // 1. Index all files in Obsidian vault
    let mut md_files = Vec::new();
    let mut asset_map: HashMap<String, PathBuf> = HashMap::new();

    let mut stack = vec![obsidian_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(err) => {
                summary.errors.push(format!("{}: {err}", dir.display()));
                continue;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            if name.starts_with('.') || name == ".trash" {
                continue;
            }

            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();

                if ext == "md" {
                    md_files.push(path);
                } else {
                    asset_map.insert(name.to_ascii_lowercase(), path);
                }
            }
        }
    }

    // 2. Identify folders and assign parent Message-IDs
    let mut folder_parent_ids: HashMap<PathBuf, String> = HashMap::new();
    let mut folders_set = HashSet::new();

    for md_path in &md_files {
        if let Ok(rel) = md_path.strip_prefix(obsidian_dir) {
            if let Some(parent) = rel.parent() {
                let mut cur = PathBuf::new();
                for comp in parent.iter() {
                    cur.push(comp);
                    folders_set.insert(cur.clone());
                }
            }
        }
    }

    let mut sorted_folders: Vec<PathBuf> = folders_set.into_iter().collect();
    sorted_folders.sort_by_key(|f| f.components().count());

    for folder_rel in sorted_folders {
        let folder_name = folder_rel
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("folder");
        let slug = slugify(&folder_rel.to_string_lossy());
        let message_id = format!("<obsidian-folder-{slug}@vault.local>");

        let parent_id = folder_rel
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .and_then(|p| folder_parent_ids.get(p).cloned());

        folder_parent_ids.insert(folder_rel.clone(), message_id.clone());

        // Check if an existing note covers this folder (e.g. folder_name.md)
        let exact_note = obsidian_dir.join(&folder_rel).with_extension("md");
        let index_note = obsidian_dir.join(&folder_rel).join("index.md");
        let readme_note = obsidian_dir.join(&folder_rel).join("README.md");

        if !exact_note.exists() && !index_note.exists() && !readme_note.exists() {
            let mut file_name = format!("{slug}.eml");
            let mut counter = 1;
            while target_vault_dir.join(&file_name).exists() {
                file_name = format!("{slug}-{counter}.eml");
                counter += 1;
            }

            let mut capsule = Capsule::blank(
                message_id.clone(),
                file_name.clone(),
                folder_name.to_string(),
            );
            capsule.parent_id = parent_id;
            capsule.blocks = vec![
                json!({
                    "id": new_block_id("heading"),
                    "type": "heading",
                    "data": { "level": 1, "text": folder_name }
                }),
                json!({
                    "id": new_block_id("paragraph"),
                    "type": "paragraph",
                    "data": { "text": "Folder imported from Obsidian." }
                }),
            ];

            let raw = assemble_eml(&capsule);
            if fs::write(target_vault_dir.join(&file_name), raw).is_ok() {
                summary.created_folders += 1;
            }
        }
    }

    // 3. Import each markdown note
    for md_path in md_files {
        let Ok(rel) = md_path.strip_prefix(obsidian_dir) else {
            continue;
        };
        let stem = md_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("note");

        let content = match fs::read_to_string(&md_path) {
            Ok(c) => c,
            Err(err) => {
                summary.errors.push(format!("{}: {err}", rel.display()));
                continue;
            }
        };

        let (fm, body) = parse_frontmatter(&content);

        // Determine title
        let mut title = fm.title.clone().unwrap_or_default();
        let mut blocks = parse_markdown_to_blocks(body);

        if title.is_empty() {
            if let Some(first) = blocks.first() {
                if first.get("type").and_then(Value::as_str) == Some("heading") {
                    let text = first
                        .get("data")
                        .and_then(|d| d.get("text"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if !text.is_empty() {
                        title = text.to_string();
                        blocks.remove(0);
                    }
                }
            }
        }
        if title.is_empty() {
            title = stem.to_string();
        }

        // Determine parent ID
        let parent_id = if let Some(p) = fm.parent {
            Some(format!("<obsidian-{}@vault.local>", slugify(&p)))
        } else if let Some(parent_dir) = rel.parent().filter(|p| !p.as_os_str().is_empty()) {
            folder_parent_ids.get(parent_dir).cloned()
        } else {
            None
        };

        let slug = slugify(&rel.with_extension("").to_string_lossy());
        let message_id = format!("<obsidian-{slug}@vault.local>");

        let mut file_name = format!("{slug}.eml");
        let mut counter = 1;
        while target_vault_dir.join(&file_name).exists() {
            file_name = format!("{slug}-{counter}.eml");
            counter += 1;
        }

        let mut capsule = Capsule::blank(message_id, file_name.clone(), title);
        capsule.parent_id = parent_id;
        capsule.tags = fm.tags;
        if !blocks.is_empty() {
            capsule.blocks = blocks;
        }

        // Parse date
        if let Some(d) = fm.date {
            capsule.date = d;
        } else if let Ok(meta) = fs::metadata(&md_path) {
            if let Ok(mod_time) = meta.modified() {
                let dt: DateTime<Utc> = mod_time.into();
                capsule.date = dt.to_rfc2822();
                capsule.timestamp = dt.timestamp_millis();
            }
        }

        // Process image attachments
        let referenced = extract_attachment_references(body);
        for att_name in referenced {
            if let Some(att_path) = asset_map.get(&att_name.to_ascii_lowercase()) {
                if let Ok(bytes) = fs::read(att_path) {
                    let b64 = BASE64.encode(&bytes);
                    let cid = sanitize_cid(&att_name);
                    capsule.attachments.push(Attachment {
                        name: att_name.clone(),
                        cid: format!("{cid}@vault.local"),
                        mime_type: guess_mime(&att_name),
                        content: String::new(),
                        size_bytes: bytes.len(),
                        raw_b64: Some(b64),
                    });
                    summary.imported_attachments += 1;
                }
            }
        }

        let raw = assemble_eml(&capsule);
        if let Err(err) = fs::write(target_vault_dir.join(&file_name), raw) {
            summary.errors.push(format!("{file_name}: {err}"));
        } else {
            summary.imported_pages += 1;
        }
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter() {
        let raw = r#"---
title: "Project Alpha"
tags:
  - rust
  - obsidian
aliases: [alpha, p-alpha]
date: 2026-10-24
parent: Projects
---
# Main Header
Content here
"#;
        let (fm, body) = parse_frontmatter(raw);
        assert_eq!(fm.title.as_deref(), Some("Project Alpha"));
        assert_eq!(fm.tags, vec!["rust", "obsidian"]);
        assert_eq!(fm.aliases, vec!["alpha", "p-alpha"]);
        assert_eq!(fm.date.as_deref(), Some("2026-10-24"));
        assert_eq!(fm.parent.as_deref(), Some("Projects"));
        assert!(body.starts_with("# Main Header"));
    }

    #[test]
    fn test_markdown_to_blocks() {
        let md = r#"# Architecture

Here is a paragraph with [[wikilink]].

- [ ] Task 1
- [x] Task 2

> [!NOTE] Design Decision
> We use RFC 5322 .eml format.

```rust
fn main() {
    println!("Hello");
}
```
"#;
        let blocks = parse_markdown_to_blocks(md);
        assert_eq!(blocks.len(), 5);
        assert_eq!(blocks[0]["type"], "heading");
        assert_eq!(blocks[1]["type"], "paragraph");
        assert_eq!(blocks[2]["type"], "todo_list");
        assert_eq!(blocks[2]["data"]["items"].as_array().unwrap().len(), 2);
        assert_eq!(blocks[3]["type"], "callout");
        assert_eq!(blocks[3]["data"]["title"], "Design Decision");
        assert_eq!(blocks[4]["type"], "code_repl");
        assert_eq!(blocks[4]["data"]["language"], "rust");
    }

    #[test]
    fn test_extract_attachments() {
        let md = "Check ![[diagram.png]] and ![[doc.pdf|preview]] and ![photo](assets/photo.jpg)";
        let atts = extract_attachment_references(md);
        assert_eq!(atts.len(), 3);
        assert!(atts.contains(&"diagram.png".to_string()));
        assert!(atts.contains(&"doc.pdf".to_string()));
        assert!(atts.contains(&"photo.jpg".to_string()));
    }

    #[test]
    fn test_import_vault_flow() {
        let temp_src = std::env::temp_dir().join("test_obsidian_src");
        let temp_dst = std::env::temp_dir().join("test_obsidian_dst");
        let _ = fs::remove_dir_all(&temp_src);
        let _ = fs::remove_dir_all(&temp_dst);

        fs::create_dir_all(temp_src.join("Projects/Sub")).unwrap();
        fs::write(
            temp_src.join("Projects/Sub/Note.md"),
            "---\ntags: [test]\n---\n# Sub Note\nHello world\n- [ ] Todo item\n",
        )
        .unwrap();

        let summary = import_obsidian_vault(&temp_src, &temp_dst).expect("import ok");
        assert_eq!(summary.imported_pages, 1);
        assert!(summary.created_folders >= 1);

        let _ = fs::remove_dir_all(&temp_src);
        let _ = fs::remove_dir_all(&temp_dst);
    }
}
