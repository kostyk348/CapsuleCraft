//! Minimal, dependency-light MIME layer for the native prototype.
//!
//! The web app remains the source of truth for the `.eml` layout. This module
//! writes the same shape: RFC 5322 headers, a `multipart/mixed` root, a
//! `canvas_state.json` payload and optional attachment parts. Parsing is
//! intentionally focused on CapsuleCraft capsules; arbitrary HTML newsletters are
//! reported as non-native and opened read-only instead of being silently
//! rewritten with a lossy HTML-to-text conversion.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub struct Attachment {
    pub name: String,
    pub cid: String,
    pub mime_type: String,
    /// Decoded text. Binary parts are lossy here; `raw_b64` is what we write back.
    pub content: String,
    pub size_bytes: usize,
    pub raw_b64: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Capsule {
    pub message_id: String,
    pub subject: String,
    pub from: String,
    pub to: String,
    pub date: String,
    pub timestamp: i64,
    pub in_reply_to: Option<String>,
    pub parent_id: Option<String>,
    pub references: Vec<String>,
    pub tags: Vec<String>,
    pub status: String,
    pub file_name: String,
    pub blocks: Vec<Value>,
    pub attachments: Vec<Attachment>,
    /// True when this file carried a CapsuleCraft canvas payload and can be saved
    /// back without destroying unknown blocks.
    pub native: bool,
    pub parse_note: Option<String>,
    /// Decoded `text/plain` part of an imported email (read-only pages only).
    pub plain_body: Option<String>,
    /// Raw `text/html` part of an imported email (read-only pages only).
    pub html_body: Option<String>,
    /// The exact bytes this page was parsed from, so an imported email can be
    /// exported without a lossy re-assembly. Native pages regenerate instead.
    pub raw_eml: Option<String>,
}

impl Capsule {
    pub fn blank(message_id: String, file_name: String, subject: String) -> Self {
        let now = Utc::now();
        Self {
            message_id,
            subject,
            from: "Local user <user@capsuleos.local>".to_string(),
            to: "CapsuleCraft Vault <vault@capsuleos.local>".to_string(),
            date: now.to_rfc2822(),
            timestamp: now.timestamp_millis(),
            in_reply_to: None,
            parent_id: None,
            references: Vec::new(),
            tags: Vec::new(),
            status: "Active".to_string(),
            file_name,
            blocks: vec![json!({
                "id": new_block_id("paragraph"),
                "type": "paragraph",
                "data": { "text": "" }
            })],
            attachments: Vec::new(),
            native: true,
            parse_note: None,
            plain_body: None,
            html_body: None,
            raw_eml: None,
        }
    }
}

impl Capsule {
    /// Bytes to write when the user exports or opens this page as MIME.
    /// Native pages are regenerated so edits are never stale; imported emails
    /// are returned byte-for-byte because they are never rewritten.
    pub fn export_eml(&self) -> String {
        if self.native {
            assemble_eml(self)
        } else {
            self.raw_eml.clone().unwrap_or_else(|| assemble_eml(self))
        }
    }
}

pub fn new_block_id(kind: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("blk_native_{kind}_{nanos}")
}

pub fn block_type(block: &Value) -> &str {
    block
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
}

pub fn block_data_text(block: &Value) -> String {
    block
        .get("data")
        .and_then(|d| d.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

pub fn set_block_text(block: &mut Value, text: String) {
    if !block.get("data").is_some_and(Value::is_object) {
        block["data"] = json!({});
    }
    block["data"]["text"] = Value::String(text);
}

pub fn parse_eml(raw: &str, file_name: &str, fallback_timestamp: i64) -> Capsule {
    let (raw_headers, raw_body) = split_headers_body(raw);
    let headers = parse_headers(raw_headers);

    let message_id = headers
        .get("message-id")
        .cloned()
        .unwrap_or_else(|| format!("<{}@vault.local>", file_name.replace('.', "-")));
    let subject = decode_rfc2047(
        headers
            .get("subject")
            .map(String::as_str)
            .unwrap_or("Untitled Capsule"),
    );
    let from = decode_rfc2047(
        headers
            .get("from")
            .map(String::as_str)
            .unwrap_or("Unknown <unknown@local>"),
    );
    let to = decode_rfc2047(
        headers
            .get("to")
            .map(String::as_str)
            .unwrap_or("CapsuleCraft Vault <vault@local>"),
    );
    let date = headers
        .get("date")
        .cloned()
        .unwrap_or_else(|| Utc::now().to_rfc2822());
    let timestamp = DateTime::parse_from_rfc2822(&date)
        .map(|d| d.timestamp_millis())
        .unwrap_or(fallback_timestamp);

    let in_reply_to = headers.get("in-reply-to").cloned();
    let parent_id = headers
        .get("x-capsule-page-parent")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let references = headers
        .get("references")
        .map(|s| s.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();
    let tags = headers
        .get("x-capsule-tags")
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let status = headers
        .get("x-capsule-status")
        .cloned()
        .unwrap_or_else(|| "Active".to_string());

    let boundary = headers
        .get("content-type")
        .and_then(|ct| header_param(ct, "boundary"));

    let mut blocks: Vec<Value> = Vec::new();
    let mut native = false;
    let mut parse_note = None;
    if let Some(canvas) = extract_canvas_json(raw_body) {
        match serde_json::from_str::<Value>(&canvas) {
            Ok(value) => match value.get("blocks").and_then(Value::as_array) {
                Some(list) => {
                    blocks = list.clone();
                    native = true;
                }
                None => {
                    parse_note =
                        Some("canvas_state.json has no blocks array; opened read-only".to_string());
                }
            },
            Err(err) => {
                parse_note = Some(format!("canvas_state.json is not valid JSON: {err}"));
            }
        }
    } else {
        parse_note = Some(
            "No CapsuleCraft canvas payload. This imported email is rendered as text and never rewritten."
                .to_string(),
        );
    }

    let attachments = if native {
        parse_attachments(boundary.as_deref(), raw_body)
    } else {
        // Imported emails keep their parts visible, but the page itself stays
        // read-only: nothing here is ever written back.
        parse_attachments(boundary.as_deref(), raw_body)
    };

    let (plain_body, html_body) = if native {
        (None, None)
    } else {
        let body = external_body(raw);
        (body.plain, body.html)
    };

    Capsule {
        message_id,
        subject,
        from,
        to,
        date,
        timestamp,
        in_reply_to,
        parent_id,
        references,
        tags,
        status,
        file_name: file_name.to_string(),
        blocks,
        attachments,
        native,
        parse_note,
        plain_body,
        html_body,
        raw_eml: Some(raw.to_string()),
    }
}

/// The readable parts of an imported email.
#[derive(Clone, Debug, Default)]
pub struct ExternalBody {
    pub plain: Option<String>,
    pub html: Option<String>,
}

/// Walk `multipart/*` up to a sane depth and return the first `text/plain` and
/// `text/html` parts. Quoted-printable and base64 are decoded; charset is
/// respected when it is one of the encodings `encoding_rs`-free decoding can
/// handle (UTF-8, Latin-1, Windows-1251 subset).
pub fn external_body(raw: &str) -> ExternalBody {
    let (raw_headers, raw_body) = split_headers_body(raw);
    let headers = parse_headers(raw_headers);
    let content_type = headers
        .get("content-type")
        .cloned()
        .unwrap_or_else(|| "text/plain".to_string());
    let boundary = header_param(&content_type, "boundary");
    let mut body = ExternalBody::default();
    collect_text_parts(
        boundary.as_deref(),
        raw_body,
        0,
        headers.get("content-transfer-encoding"),
        Some(content_type.as_str()),
        &mut body,
    );
    body
}

fn collect_text_parts(
    boundary: Option<&str>,
    body: &str,
    depth: usize,
    top_encoding: Option<&String>,
    top_type: Option<&str>,
    out: &mut ExternalBody,
) {
    if depth > 4 {
        return;
    }
    let Some(boundary) = boundary else {
        let mime = top_type
            .unwrap_or("text/plain")
            .split(';')
            .next()
            .unwrap_or("text/plain")
            .trim()
            .to_ascii_lowercase();
        if !mime.starts_with("text/") {
            return;
        }
        let decoded = decode_part(
            body,
            top_encoding.map(String::as_str).unwrap_or(""),
            top_type.unwrap_or("text/plain"),
        );
        if mime == "text/html" {
            if out.html.is_none() {
                out.html = Some(decoded);
            }
        } else if out.plain.is_none() {
            out.plain = Some(decoded);
        }
        return;
    };

    let delimiter = format!("--{boundary}");
    for raw_part in body.split(&delimiter) {
        let part = raw_part.trim_start_matches("\r\n").trim_start_matches('\n');
        if part.is_empty() || part.starts_with("--") {
            continue;
        }
        let (raw_headers, raw_body) = split_headers_body(part);
        let headers = parse_headers(raw_headers);
        let content_type = headers
            .get("content-type")
            .cloned()
            .unwrap_or_else(|| "text/plain".to_string());
        let mime = content_type
            .split(';')
            .next()
            .unwrap_or("text/plain")
            .trim()
            .to_ascii_lowercase();
        let nested = header_param(&content_type, "boundary");
        if mime.starts_with("multipart/") {
            collect_text_parts(nested.as_deref(), raw_body, depth + 1, None, None, out);
            continue;
        }
        let is_attachment = headers
            .get("content-disposition")
            .map(|value| value.to_ascii_lowercase().contains("attachment"))
            .unwrap_or(false);
        if is_attachment {
            continue;
        }
        let raw_body = raw_body
            .strip_suffix("\r\n")
            .or_else(|| raw_body.strip_suffix('\n'))
            .unwrap_or(raw_body);
        let decoded = decode_part(
            raw_body,
            headers
                .get("content-transfer-encoding")
                .map(String::as_str)
                .unwrap_or(""),
            &content_type,
        );
        if mime == "text/html" && out.html.is_none() {
            out.html = Some(decoded);
        } else if mime == "text/plain" && out.plain.is_none() {
            out.plain = Some(decoded);
        }
    }
}

fn decode_part(body: &str, encoding: &str, content_type: &str) -> String {
    let charset = header_param(content_type, "charset").unwrap_or_else(|| "utf-8".to_string());
    let encoding = encoding.trim().to_ascii_lowercase();
    let bytes = match encoding.as_str() {
        "base64" => {
            let compact: String = body.chars().filter(|c| !c.is_whitespace()).collect();
            BASE64
                .decode(compact)
                .unwrap_or_else(|_| body.as_bytes().to_vec())
        }
        "quoted-printable" => decode_q_encoding(body),
        _ => body.as_bytes().to_vec(),
    };
    decode_charset(&bytes, &charset)
}

#[derive(Serialize)]
struct CanvasState<'a> {
    version: &'static str,
    generator: &'static str,
    #[serde(rename = "messageId")]
    message_id: &'a str,
    subject: &'a str,
    tags: &'a [String],
    blocks: &'a [Value],
}

pub fn assemble_eml(capsule: &Capsule) -> String {
    let seed = format!(
        "{}|{}|{}|{}",
        capsule.message_id,
        capsule.subject,
        capsule.blocks.len(),
        capsule.attachments.len()
    );
    let root_boundary = format!("BOUNDARY_ROOT_{}", stable_hash(&seed));
    let payload_boundary = format!(
        "BOUNDARY_PAYLOAD_{}",
        stable_hash(&format!("{seed}|payload"))
    );

    let mut headers: Vec<String> = vec![
        format!("From: {}", clean_header(&capsule.from)),
        format!("To: {}", clean_header(&capsule.to)),
        format!("Subject: {}", clean_header(&capsule.subject)),
        format!("Date: {}", clean_header(&capsule.date)),
        format!("Message-ID: {}", clean_header(&capsule.message_id)),
    ];

    if let Some(parent) = &capsule.in_reply_to {
        headers.push(format!("In-Reply-To: {}", clean_header(parent)));
    }
    if let Some(parent) = &capsule.parent_id {
        headers.push(format!("X-Capsule-Page-Parent: {}", clean_header(parent)));
    }
    if !capsule.references.is_empty() {
        let refs = capsule
            .references
            .iter()
            .map(|s| clean_header(s))
            .collect::<Vec<_>>()
            .join(" ");
        headers.push(format!("References: {refs}"));
    }
    if !capsule.tags.is_empty() {
        let tags = capsule
            .tags
            .iter()
            .map(|s| clean_header(s))
            .collect::<Vec<_>>()
            .join(", ");
        headers.push(format!("X-Capsule-Tags: {tags}"));
    }
    headers.push(format!(
        "X-Capsule-Status: {}",
        clean_header(&capsule.status)
    ));
    headers.push("MIME-Version: 1.0".to_string());
    headers.push(format!(
        "Content-Type: multipart/mixed; boundary=\"{root_boundary}\""
    ));

    let canvas = CanvasState {
        version: "1.0",
        generator: "CapsuleCraft Native egui prototype (Zero-DB Pure-MIME)",
        message_id: &capsule.message_id,
        subject: &capsule.subject,
        tags: &capsule.tags,
        blocks: &capsule.blocks,
    };
    let canvas_json = serde_json::to_string_pretty(&canvas).unwrap_or_else(|_| "{}".to_string());
    let html = generate_html(capsule);

    let alternative = [
        format!("--{root_boundary}"),
        format!("Content-Type: multipart/alternative; boundary=\"{payload_boundary}\""),
        String::new(),
        format!("--{payload_boundary}"),
        "Content-Type: application/json; name=\"canvas_state.json\"".to_string(),
        "Content-Disposition: inline; filename=\"canvas_state.json\"".to_string(),
        String::new(),
        canvas_json,
        String::new(),
        format!("--{payload_boundary}"),
        "Content-Type: text/html; charset=\"utf-8\"".to_string(),
        "Content-Disposition: inline".to_string(),
        String::new(),
        html,
        String::new(),
        format!("--{payload_boundary}--"),
    ]
    .join("\r\n");

    let mut parts = vec![alternative];

    for attachment in &capsule.attachments {
        let cid = attachment
            .cid
            .trim_start_matches('<')
            .trim_end_matches('>')
            .trim_start_matches("cid:");
        let (encoding, payload) = match &attachment.raw_b64 {
            Some(raw) => ("base64".to_string(), raw.trim().to_string()),
            None if needs_base64(&attachment.content) => (
                "base64".to_string(),
                BASE64.encode(attachment.content.as_bytes()),
            ),
            None => ("8bit".to_string(), attachment.content.clone()),
        };
        parts.push(
            [
                format!("--{root_boundary}"),
                format!(
                    "Content-Type: {}; name=\"{}\"",
                    clean_header(&attachment.mime_type),
                    clean_header(&attachment.name)
                ),
                format!("Content-ID: <{}>", clean_header(cid)),
                format!(
                    "Content-Disposition: attachment; filename=\"{}\"",
                    clean_header(&attachment.name)
                ),
                format!("Content-Transfer-Encoding: {encoding}"),
                String::new(),
                payload,
            ]
            .join("\r\n"),
        );
    }

    parts.push(format!("--{root_boundary}--"));
    format!("{}\r\n\r\n{}", headers.join("\r\n"), parts.join("\r\n"))
}

fn generate_html(capsule: &Capsule) -> String {
    let mut body = String::new();
    for block in &capsule.blocks {
        match block_type(block) {
            "heading" => {
                let level = block
                    .get("data")
                    .and_then(|d| d.get("level"))
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    .clamp(1, 3);
                body.push_str(&format!(
                    "<h{level}>{}</h{level}>",
                    escape_html(&block_data_text(block))
                ));
            }
            "paragraph" => {
                body.push_str(&format!(
                    "<p>{}</p>",
                    escape_html(&block_data_text(block)).replace('\n', "<br>")
                ));
            }
            "todo_list" => {
                let title = block
                    .get("data")
                    .and_then(|d| d.get("title"))
                    .and_then(Value::as_str)
                    .unwrap_or("Tasks");
                body.push_str(&format!("<h3>{}</h3><ul>", escape_html(title)));
                if let Some(items) = block
                    .get("data")
                    .and_then(|d| d.get("items"))
                    .and_then(Value::as_array)
                {
                    for item in items {
                        let text = item.get("text").and_then(Value::as_str).unwrap_or("");
                        let done = item
                            .get("status")
                            .and_then(Value::as_str)
                            .map(|s| s == "done")
                            .or_else(|| item.get("done").and_then(Value::as_bool))
                            .unwrap_or(false);
                        body.push_str(&format!(
                            "<li>{} {}</li>",
                            if done { "☑" } else { "☐" },
                            escape_html(text)
                        ));
                    }
                }
                body.push_str("</ul>");
            }
            other => {
                body.push_str(&format!(
                    "<p><em>[{other} block — open in CapsuleCraft web for its canvas]</em></p>"
                ));
            }
        }
    }

    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"generator\" content=\"CapsuleCraft Native\"></head><body style=\"font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;max-width:720px;margin:32px auto;line-height:1.55;color:#23201c;background:#faf8f4;\"><h1>{}</h1>{}<hr><p style=\"color:#8a8175;font-size:12px\">Exported from CapsuleCraft Native · {}</p></body></html>",
        escape_html(&capsule.subject),
        body,
        escape_html(&capsule.message_id)
    )
}

// ── parsing helpers ──────────────────────────────────────────────────────────

fn split_headers_body(raw: &str) -> (&str, &str) {
    if let Some(pos) = raw.find("\r\n\r\n") {
        (&raw[..pos], &raw[pos + 4..])
    } else if let Some(pos) = raw.find("\n\n") {
        (&raw[..pos], &raw[pos + 2..])
    } else {
        (raw, "")
    }
}

fn parse_headers(raw: &str) -> BTreeMap<String, String> {
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    let mut last_key: Option<String> = None;

    for line in raw.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(key) = &last_key {
                if let Some(value) = map.get_mut(key) {
                    value.push(' ');
                    value.push_str(line.trim());
                }
            }
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_ascii_lowercase();
            map.insert(key.clone(), value.trim().to_string());
            last_key = Some(key);
        }
    }

    map
}

fn extract_canvas_json(body: &str) -> Option<String> {
    let lower = body.to_ascii_lowercase();
    let marker = "canvas_state.json";
    let marker_pos = lower.find(marker)?;
    let after_marker = &body[marker_pos + marker.len()..];

    let content_start = after_marker
        .find("\r\n\r\n")
        .map(|p| p + 4)
        .or_else(|| after_marker.find("\n\n").map(|p| p + 2))?;
    let content = &after_marker[content_start..];

    let end = ["\r\n--", "\n--"]
        .iter()
        .filter_map(|needle| content.find(needle))
        .min()?;
    let json = content[..end].trim();
    if json.is_empty() {
        None
    } else {
        Some(json.to_string())
    }
}

fn parse_attachments(boundary: Option<&str>, body: &str) -> Vec<Attachment> {
    let Some(boundary) = boundary else {
        return Vec::new();
    };
    let delimiter = format!("--{boundary}");
    let mut result = Vec::new();

    for raw_part in body.split(&delimiter) {
        let part = raw_part.trim_start_matches("\r\n").trim_start_matches('\n');
        if part.is_empty() || part.starts_with("--") {
            continue;
        }
        let (raw_headers, raw_body) = split_headers_body(part);
        let headers = parse_headers(raw_headers);
        let content_type = headers
            .get("content-type")
            .cloned()
            .unwrap_or_else(|| "text/plain".to_string());
        if content_type.to_ascii_lowercase().contains("multipart/") {
            continue;
        }

        let name = header_param(&content_type, "name")
            .or_else(|| {
                headers
                    .get("content-disposition")
                    .and_then(|value| header_param(value, "filename"))
            })
            .or_else(|| {
                headers
                    .get("content-disposition")
                    .and_then(|value| header_param(value, "name"))
            });
        let Some(name) = name else { continue };
        if name.eq_ignore_ascii_case("canvas_state.json") {
            continue;
        }

        let cid = headers
            .get("content-id")
            .map(|s| {
                s.trim()
                    .trim_start_matches('<')
                    .trim_end_matches('>')
                    .to_string()
            })
            .unwrap_or_else(|| name.replace(|c: char| !c.is_ascii_alphanumeric(), "_"));
        let mime_type = content_type
            .split(';')
            .next()
            .unwrap_or("text/plain")
            .trim()
            .to_string();
        let encoding = headers
            .get("content-transfer-encoding")
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_default();

        // The CRLF immediately before the next boundary is transport framing,
        // not necessarily part of the attachment. Strip exactly one.
        let raw = raw_body
            .strip_suffix("\r\n")
            .or_else(|| raw_body.strip_suffix('\n'))
            .unwrap_or(raw_body);
        let (content, size_bytes, raw_b64) = if encoding == "base64" {
            let compact: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
            match BASE64.decode(&compact) {
                Ok(bytes) => (
                    String::from_utf8_lossy(&bytes).to_string(),
                    bytes.len(),
                    Some(compact),
                ),
                Err(_) => (raw.to_string(), raw.len(), None),
            }
        } else {
            (raw.to_string(), raw.len(), None)
        };

        result.push(Attachment {
            name,
            cid,
            mime_type,
            content,
            size_bytes,
            raw_b64,
        });
    }

    result
}

fn header_param(header: &str, param: &str) -> Option<String> {
    let lower = header.to_ascii_lowercase();
    let needle = format!("{param}=");
    let mut search_from = 0;
    while let Some(rel) = lower[search_from..].find(&needle) {
        let idx = search_from + rel;
        let before_ok = idx == 0
            || !lower.as_bytes()[idx - 1].is_ascii_alphanumeric()
                && lower.as_bytes()[idx - 1] != b'-';
        if !before_ok {
            search_from = idx + needle.len();
            continue;
        }
        let rest = header[idx + needle.len()..].trim_start();
        if let Some(stripped) = rest.strip_prefix('"') {
            if let Some(end) = stripped.find('"') {
                return Some(stripped[..end].to_string());
            }
            return None;
        }
        let value = rest
            .split([';', ' ', '\r', '\n'])
            .next()
            .unwrap_or("")
            .trim();
        if value.is_empty() {
            return None;
        }
        return Some(value.to_string());
    }
    None
}

fn decode_rfc2047(input: &str) -> String {
    let mut out = String::new();
    let mut rest = input;

    while let Some(start) = rest.find("=?") {
        out.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("?=") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let token = &after_start[..end];
        let token_end = start + 2 + end + 2;
        let fields: Vec<&str> = token.splitn(3, '?').collect();

        let decoded = if fields.len() == 3 {
            let charset = fields[0];
            let encoding = fields[1].to_ascii_uppercase();
            let text = fields[2];
            let bytes = match encoding.as_str() {
                "B" => BASE64.decode(text).ok(),
                "Q" => Some(decode_q_encoding(text)),
                _ => None,
            };
            bytes.map(|bytes| decode_charset(&bytes, charset))
        } else {
            None
        };

        match decoded {
            Some(value) => {
                out.push_str(&value);
                rest = &rest[token_end..];
                // RFC 2047 §6.2: whitespace between adjacent encoded words is ignored.
                if rest.starts_with(' ') && rest[1..].starts_with("=?") {
                    rest = &rest[1..];
                }
            }
            None => {
                out.push_str(&rest[start..start + 2]);
                rest = &rest[start + 2..];
            }
        }
    }

    out.push_str(rest);
    out
}

fn decode_q_encoding(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'_' => {
                out.push(b' ');
                i += 1;
            }
            b'=' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    out.push(value);
                    i += 3;
                } else {
                    out.push(b'=');
                    i += 1;
                }
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    out
}

fn decode_charset(bytes: &[u8], charset: &str) -> String {
    match charset.to_ascii_lowercase().as_str() {
        "iso-8859-1" | "latin1" | "windows-1252" => bytes.iter().map(|b| *b as char).collect(),
        _ => String::from_utf8_lossy(bytes).to_string(),
    }
}

// ── assembly helpers ─────────────────────────────────────────────────────────

fn clean_header(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

fn needs_base64(content: &str) -> bool {
    content.chars().any(|c| (c as u32) > 0x7f)
        || content
            .lines()
            .any(|line| line.starts_with("--") && line.len() > 2)
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn stable_hash(input: &str) -> String {
    let mut h1: u32 = 0x811c_9dc5;
    let mut h2: u32 = 0x0100_0193;
    for (i, code) in input.encode_utf16().enumerate() {
        let c = code as u32;
        h1 = (h1 ^ c).wrapping_mul(16_777_619);
        h2 = h2.wrapping_add(c.wrapping_mul(i as u32 + 7));
    }
    let raw = format!("{}{}", base36(h1), base36(h2)).to_ascii_uppercase();
    let mut value: String = raw.chars().take(10).collect();
    while value.len() < 8 {
        value.push('0');
    }
    value
}

fn base36(mut value: u32) -> String {
    if value == 0 {
        return "0".to_string();
    }
    let mut out = Vec::new();
    while value > 0 {
        let digit = (value % 36) as u8;
        out.push(match digit {
            0..=9 => (b'0' + digit) as char,
            _ => (b'a' + digit - 10) as char,
        });
        value /= 36;
    }
    out.iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Capsule {
        let mut capsule = Capsule::blank(
            "<capsule-native-test@vault.local>".to_string(),
            "test.eml".to_string(),
            "Проект Alpha / design".to_string(),
        );
        capsule.tags = vec!["#work".to_string(), "#проект".to_string()];
        capsule.parent_id = Some("<parent@vault.local>".to_string());
        capsule.blocks = vec![
            json!({"id":"h1","type":"heading","data":{"level":1,"text":"Заголовок"}}),
            json!({"id":"p1","type":"paragraph","data":{"text":"Привет, мир!"}}),
            json!({"id":"t1","type":"todo_list","data":{"title":"Tasks","items":[
                {"id":"i1","text":"Write spec","done":false,"status":"todo","priority":"high"}
            ]}}),
        ];
        capsule.attachments = vec![Attachment {
            name: "data.csv".to_string(),
            cid: "data_csv".to_string(),
            mime_type: "text/csv".to_string(),
            content: "Region,Sales\nNorth,120\n".to_string(),
            size_bytes: 24,
            raw_b64: None,
        }];
        capsule
    }

    #[test]
    fn round_trips_native_capsule() {
        let capsule = sample();
        let raw = assemble_eml(&capsule);
        let parsed = parse_eml(&raw, "test.eml", 0);

        assert!(parsed.native, "{:?}", parsed.parse_note);
        assert_eq!(parsed.message_id, capsule.message_id);
        assert_eq!(parsed.subject, capsule.subject);
        assert_eq!(parsed.tags, capsule.tags);
        assert_eq!(parsed.parent_id, capsule.parent_id);
        assert_eq!(parsed.status, capsule.status);
        assert_eq!(parsed.blocks, capsule.blocks);
        assert_eq!(parsed.attachments.len(), 1);
        assert_eq!(parsed.attachments[0].name, "data.csv");
        assert_eq!(
            parsed.attachments[0].content,
            capsule.attachments[0].content
        );
    }

    #[test]
    fn keeps_independent_page_parent_header() {
        let raw = assemble_eml(&sample());
        assert!(raw.contains("X-Capsule-Page-Parent: <parent@vault.local>"));
        assert!(!raw.contains("In-Reply-To: <parent@vault.local>"));
    }

    #[test]
    fn external_email_is_read_only() {
        let raw = "From: x@y\r\nSubject: =?UTF-8?B?0J/RgNC40LLQtdGC?=\r\n\r\n<html><body>Hello</body></html>";
        let parsed = parse_eml(raw, "external.eml", 1_700_000_000_000);
        assert!(!parsed.native);
        assert_eq!(parsed.subject, "Привет");
        assert!(parsed.parse_note.is_some());
        assert!(parsed.blocks.is_empty());
    }

    #[test]
    fn attachment_round_trip_keeps_bytes() {
        let mut capsule = sample();
        capsule.attachments[0].content = "Привет".to_string();
        capsule.attachments[0].raw_b64 = None;
        let parsed = parse_eml(&assemble_eml(&capsule), "test.eml", 0);
        assert_eq!(parsed.attachments[0].content, "Привет");
        assert!(parsed.attachments[0].raw_b64.is_some());
    }
}
