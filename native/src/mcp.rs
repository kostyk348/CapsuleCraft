//! Model Context Protocol (MCP) server over stdin/stdout for CapsuleCraft.
//!
//! Exposes the RFC 5322 .eml vault to AI agents (Antigravity, Claude Code,
//! Claude Desktop, Cursor, Windsurf) as a local Second Brain.
//!
//! Protocol: JSON-RPC 2.0 line-delimited over stdio.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::cli::Launch;
use crate::links;
use crate::mime::{new_block_id, Capsule};
use crate::planning::{self, TaskFilters};
use crate::search::{self, Bm25Index};
use crate::vault::Vault;

pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";
pub const SERVER_NAME: &str = "capsulecraft-mcp";
pub const SERVER_VERSION: &str = "1.5.0";

/// Generate the MCP client configuration snippet.
pub fn mcp_config_json(exe_path: &Path, _vault_dir: &Path) -> Value {
    let exe_str = exe_path.to_string_lossy().to_string();

    json!({
        "mcpServers": {
            "capsulecraft": {
                "command": exe_str,
                "args": ["--mcp"]
            }
        }
    })
}

/// Print MCP configuration snippets for various clients and exit.
pub fn print_config(launch: &Launch) {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("capsuleos-native.exe"));
    let vault = launch
        .vault
        .clone()
        .unwrap_or_else(crate::vault::get_active_vault_dir);
    let cfg = mcp_config_json(&exe, &vault);

    println!("=== CapsuleCraft MCP Server Configuration ===");
    println!();
    println!("Executable : {}", exe.display());
    println!("Active vault : {}", vault.display());
    println!();
    println!("--- JSON Configuration (Claude Desktop / Antigravity / Cursor) ---");
    println!("{}", serde_json::to_string_pretty(&cfg).unwrap_or_default());
    println!();
    println!("--- Claude Code CLI Command ---");
    println!("claude mcp add capsulecraft \"{}\" --mcp", exe.display());
    println!();
}

/// List of tools exposed by the MCP server.
pub fn tool_definitions() -> Value {
    json!([
        {
            "name": "capsule_list",
            "description": "List all capsules in the CapsuleCraft vault with metadata (Message-ID, subject, author, date, tags, filename, parent_id).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tag": {
                        "type": "string",
                        "description": "Optional tag filter (e.g. 'project', 'architecture', 'decision')"
                    }
                }
            }
        },
        {
            "name": "capsule_read",
            "description": "Read a full capsule by its Message-ID, file name, or page subject. Returns full plain text content, structured blocks, headers, and attachment information.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Message-ID (e.g. <alpha@vault.local>), file name (e.g. capsule_1.eml), or page title"
                    }
                },
                "required": ["id"]
            }
        },
        {
            "name": "capsule_search",
            "description": "Ranked Okapi BM25 search across all capsules in the vault, returning top matching pages with highlighted text snippets.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query terms (supports English and Russian stem/words)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: 10)"
                    }
                },
                "required": ["query"]
            }
        },
        {
            "name": "capsule_create",
            "description": "Create a new capsule (.eml) in the vault with structured blocks (paragraphs, callout boxes, todo checklists, and optional parent page relation).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "subject": {
                        "type": "string",
                        "description": "Title/subject of the new capsule"
                    },
                    "content": {
                        "type": "string",
                        "description": "Main body text (will be broken into paragraphs)"
                    },
                    "author": {
                        "type": "string",
                        "description": "Author name or identifier (defaults to 'AI Agent')"
                    },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of tags to assign (e.g. ['ai', 'research'])"
                    },
                    "parent_id": {
                        "type": "string",
                        "description": "Optional parent capsule Message-ID to place this under"
                    },
                    "callouts": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "text": { "type": "string" },
                                "tone": {
                                    "type": "string",
                                    "enum": ["emerald", "amber", "rose", "indigo"],
                                    "description": "Tone of callout box: emerald (success/highlight), amber (warning), rose (risk), indigo (info)"
                                }
                            },
                            "required": ["text"]
                        },
                        "description": "Optional highlighted callout boxes"
                    },
                    "todos": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional actionable todo/checklist items"
                    }
                },
                "required": ["subject"]
            }
        },
        {
            "name": "capsule_reply",
            "description": "Add a reply to an existing capsule thread, establishing RFC 5322 In-Reply-To and References relationships.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "parent_id": {
                        "type": "string",
                        "description": "Message-ID or file name of the capsule being replied to"
                    },
                    "content": {
                        "type": "string",
                        "description": "Body text of the reply"
                    },
                    "subject": {
                        "type": "string",
                        "description": "Reply subject (defaults to 'Re: <parent subject>')"
                    },
                    "author": {
                        "type": "string",
                        "description": "Author name (defaults to 'AI Opponent')"
                    }
                },
                "required": ["parent_id", "content"]
            }
        },
        {
            "name": "capsule_patch",
            "description": "Atomically patch an existing capsule: append text/notes, replace specific sections, toggle task checkboxes, or add tags without clobbering other content.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Message-ID, title, or filename of the capsule to patch"
                    },
                    "append_markdown": {
                        "type": "string",
                        "description": "Markdown text to append to the end of the note"
                    },
                    "toggle_task": {
                        "type": "string",
                        "description": "Task label or task ID to toggle between [ ] and [x]"
                    },
                    "add_tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Tags to add to the capsule"
                    },
                    "action": {
                        "type": "string",
                        "enum": ["append_text", "replace_text", "toggle_task", "add_tag"],
                        "description": "Specific patch action to perform"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content for append_text or replacement text for replace_text"
                    },
                    "target": {
                        "type": "string",
                        "description": "Target string to replace, or task text/id to toggle, or tag to add"
                    }
                },
                "required": ["id"]
            }
        },
        {
            "name": "capsule_backlinks",
            "description": "Retrieve all capsules that link to the specified capsule via [[Wiki links]] or RFC Message-IDs.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Message-ID, title, or filename of the target capsule"
                    }
                },
                "required": ["id"]
            }
        },
        {
            "name": "capsule_related",
            "description": "Find capsules semantically related by content using TF-IDF cosine similarity across the vault.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Message-ID, title, or filename of the reference capsule"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of related capsules to return (default: 5)"
                    },
                    "min_score": {
                        "type": "number",
                        "description": "Minimum similarity threshold (0.0 to 1.0, default: 0.05)"
                    }
                },
                "required": ["id"]
            }
        },
        {
            "name": "capsule_tasks",
            "description": "Scan and query tasks and checklist items across all capsules in the vault, with status and deadline filtering.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["all", "open", "todo", "doing", "done"],
                        "description": "Filter by task status (default: 'all')"
                    },
                    "due": {
                        "type": "string",
                        "enum": ["all", "overdue", "today", "week", "upcoming", "none"],
                        "description": "Filter by deadline bucket"
                    },
                    "priority": {
                        "type": "string",
                        "enum": ["all", "low", "normal", "high"],
                        "description": "Filter by priority level"
                    },
                    "capsule_id": {
                        "type": "string",
                        "description": "Filter tasks belonging to a specific capsule"
                    },
                    "tag": {
                        "type": "string",
                        "description": "Filter tasks from capsules with a specific tag"
                    },
                    "query": {
                        "type": "string",
                        "description": "Filter tasks matching search query string"
                    }
                }
            }
        }
    ])
}

/// Handler for the MCP JSON-RPC protocol.
pub struct McpHandler {
    pub vault: Vault,
    pub vault_override: Option<PathBuf>,
}

impl McpHandler {
    pub fn new(vault_override: Option<PathBuf>) -> Self {
        let dir = vault_override
            .clone()
            .unwrap_or_else(crate::vault::get_active_vault_dir);
        let vault = Vault::load(dir);
        Self {
            vault,
            vault_override,
        }
    }

    /// Reload the vault from disk to reflect any concurrent edits or active folder switches.
    pub fn reload(&mut self) {
        if let Some(fixed) = &self.vault_override {
            if self.vault.dir != *fixed {
                self.vault = Vault::load(fixed.clone());
            } else {
                self.vault.reload();
            }
        } else {
            let active = crate::vault::get_active_vault_dir();
            if self.vault.dir != active {
                eprintln!(
                    "[capsulecraft-mcp] Active vault changed to: {}",
                    active.display()
                );
                self.vault = Vault::load(active);
            } else {
                self.vault.reload();
            }
        }
    }

    /// Process a single JSON-RPC request and return an optional response JSON-RPC string.
    pub fn handle_message(&mut self, line: &str) -> Option<String> {
        let req: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(err) => {
                let err_resp = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {err}")
                    }
                });
                return Some(err_resp.to_string());
            }
        };

        let method = match req.get("method").and_then(Value::as_str) {
            Some(m) => m,
            None => return None,
        };

        let id = req.get("id").cloned();

        // Notifications have no id and expect no response
        if id.is_none() {
            if method == "notifications/initialized" {
                eprintln!("[capsulecraft-mcp] Client initialized notification received.");
            }
            return None;
        }

        let id = id.unwrap();
        let params = req.get("params").cloned().unwrap_or(Value::Null);

        let response = match method {
            "initialize" => {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": MCP_PROTOCOL_VERSION,
                        "capabilities": {
                            "tools": {},
                            "resources": {}
                        },
                        "serverInfo": {
                            "name": SERVER_NAME,
                            "version": SERVER_VERSION
                        }
                    }
                })
            }
            "ping" => {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {}
                })
            }
            "tools/list" => {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "tools": tool_definitions()
                    }
                })
            }
            "tools/call" => {
                let tool_name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
                let result = self.call_tool(tool_name, &arguments);
                match result {
                    Ok(val) => {
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": [
                                    {
                                        "type": "text",
                                        "text": serde_json::to_string_pretty(&val).unwrap_or_else(|_| val.to_string())
                                    }
                                ],
                                "isError": false
                            }
                        })
                    }
                    Err(err_msg) => {
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": [
                                    {
                                        "type": "text",
                                        "text": err_msg
                                    }
                                ],
                                "isError": true
                            }
                        })
                    }
                }
            }
            "resources/list" => {
                self.reload();
                let resources: Vec<Value> = self
                    .vault
                    .capsules
                    .iter()
                    .map(|c| {
                        json!({
                            "uri": format!("capsule://{}", c.file_name),
                            "name": c.subject.clone(),
                            "description": format!("Capsule '{}' by {}", c.subject, c.from),
                            "mimeType": "text/plain"
                        })
                    })
                    .collect();

                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "resources": resources
                    }
                })
            }
            "resources/read" => {
                self.reload();
                let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
                let identifier = uri.strip_prefix("capsule://").unwrap_or(uri);

                let found = self.vault.capsules.iter().find(|c| {
                    c.file_name == identifier
                        || c.message_id == identifier
                        || c.message_id.trim_matches(|ch| ch == '<' || ch == '>') == identifier
                });

                match found {
                    Some(c) => {
                        let text = links::page_text(c);
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "contents": [
                                    {
                                        "uri": uri,
                                        "mimeType": "text/plain",
                                        "text": text
                                    }
                                ]
                            }
                        })
                    }
                    None => {
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": {
                                "code": -32602,
                                "message": format!("Resource not found: {uri}")
                            }
                        })
                    }
                }
            }
            _ => {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("Method not found: {method}")
                    }
                })
            }
        };

        Some(response.to_string())
    }

    fn call_tool(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        self.reload();
        match name {
            "capsule_list" => self.tool_capsule_list(args),
            "capsule_read" => self.tool_capsule_read(args),
            "capsule_search" => self.tool_capsule_search(args),
            "capsule_create" => self.tool_capsule_create(args),
            "capsule_reply" => self.tool_capsule_reply(args),
            "capsule_patch" => self.tool_capsule_patch(args),
            "capsule_backlinks" => self.tool_capsule_backlinks(args),
            "capsule_related" => self.tool_capsule_related(args),
            "capsule_tasks" => self.tool_capsule_tasks(args),
            unknown => Err(format!("Unknown tool: {unknown}")),
        }
    }

    fn tool_capsule_list(&self, args: &Value) -> Result<Value, String> {
        let tag_filter = args.get("tag").and_then(Value::as_str).map(str::trim);

        let list: Vec<Value> = self
            .vault
            .capsules
            .iter()
            .filter(|c| {
                if let Some(tag) = tag_filter {
                    c.tags.iter().any(|t| t.eq_ignore_ascii_case(tag))
                } else {
                    true
                }
            })
            .map(|c| {
                json!({
                    "id": c.message_id,
                    "subject": c.subject,
                    "from": c.from,
                    "to": c.to,
                    "date": c.date,
                    "tags": c.tags,
                    "status": c.status,
                    "parent_id": c.parent_id,
                    "file_name": c.file_name,
                    "native": c.native,
                    "blocks_count": c.blocks.len(),
                    "attachments_count": c.attachments.len()
                })
            })
            .collect();

        Ok(json!({
            "count": list.len(),
            "capsules": list
        }))
    }

    fn find_capsule(&self, id: &str) -> Option<(usize, &Capsule)> {
        let clean = id.trim();
        self.vault.capsules.iter().enumerate().find(|(_, c)| {
            c.message_id == clean
                || c.message_id.trim_matches(|ch| ch == '<' || ch == '>') == clean
                || c.file_name.eq_ignore_ascii_case(clean)
                || c.subject.eq_ignore_ascii_case(clean)
        })
    }

    fn tool_capsule_read(&self, args: &Value) -> Result<Value, String> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing required argument 'id'".to_string())?;

        let (_, capsule) = self
            .find_capsule(id)
            .ok_or_else(|| format!("Capsule not found: '{id}'"))?;

        let full_text = links::page_text(capsule);
        let attachments: Vec<Value> = capsule
            .attachments
            .iter()
            .map(|a| {
                json!({
                    "name": a.name,
                    "cid": a.cid,
                    "mime_type": a.mime_type,
                    "size_bytes": a.size_bytes
                })
            })
            .collect();

        Ok(json!({
            "id": capsule.message_id,
            "subject": capsule.subject,
            "from": capsule.from,
            "to": capsule.to,
            "date": capsule.date,
            "tags": capsule.tags,
            "status": capsule.status,
            "parent_id": capsule.parent_id,
            "in_reply_to": capsule.in_reply_to,
            "references": capsule.references,
            "file_name": capsule.file_name,
            "content": full_text,
            "blocks": capsule.blocks,
            "attachments": attachments
        }))
    }

    fn tool_capsule_search(&self, args: &Value) -> Result<Value, String> {
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing required argument 'query'".to_string())?;

        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|l| l as usize)
            .unwrap_or(10);

        let mut bm25 = Bm25Index::default();
        let mut capsule_texts: HashMap<String, (String, &Capsule)> = HashMap::new();

        for c in &self.vault.capsules {
            let text = links::page_text(c);
            let tokens = search::tokenize(&text);
            bm25.add(&c.message_id, tokens);
            capsule_texts.insert(c.message_id.clone(), (text, c));
        }

        let hits = bm25.rank(query);

        let mut results: Vec<Value> = Vec::new();
        for hit in hits.into_iter().take(limit) {
            if let Some((text, capsule)) = capsule_texts.get(&hit.id) {
                let snippet = search::snippet(text, query, 160).unwrap_or_else(|| {
                    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
                    lines.first().copied().unwrap_or("").to_string()
                });

                results.push(json!({
                    "id": capsule.message_id,
                    "file_name": capsule.file_name,
                    "subject": capsule.subject,
                    "score": hit.score,
                    "snippet": snippet,
                    "tags": capsule.tags,
                    "date": capsule.date
                }));
            }
        }

        Ok(json!({
            "query": query,
            "total_matches": results.len(),
            "results": results
        }))
    }

    fn tool_capsule_create(&mut self, args: &Value) -> Result<Value, String> {
        let subject = args
            .get("subject")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "Missing required argument 'subject'".to_string())?;

        let parent_id = args
            .get("parent_id")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string());

        let author = args
            .get("author")
            .and_then(Value::as_str)
            .unwrap_or("AI Agent");

        let tags: Vec<String> = args
            .get("tags")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default();

        let index = self.vault.create_page(parent_id.clone())?;

        let mut blocks: Vec<Value> = Vec::new();

        // 1. Content paragraphs
        if let Some(content) = args.get("content").and_then(Value::as_str) {
            let paras: Vec<&str> = content
                .split("\n\n")
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .collect();

            if paras.is_empty() {
                blocks.push(json!({
                    "id": new_block_id("paragraph"),
                    "type": "paragraph",
                    "data": { "text": content }
                }));
            } else {
                for p in paras {
                    blocks.push(json!({
                        "id": new_block_id("paragraph"),
                        "type": "paragraph",
                        "data": { "text": p }
                    }));
                }
            }
        }

        // 2. Callouts
        if let Some(callouts) = args.get("callouts").and_then(Value::as_array) {
            for callout in callouts {
                if let Some(text) = callout.get("text").and_then(Value::as_str) {
                    let tone = callout
                        .get("tone")
                        .and_then(Value::as_str)
                        .unwrap_or("indigo");

                    blocks.push(json!({
                        "id": new_block_id("callout"),
                        "type": "callout",
                        "data": {
                            "title": "Agent Note",
                            "content": text,
                            "tone": tone,
                            "icon": "info"
                        }
                    }));
                }
            }
        }

        // 3. Todos
        if let Some(todos) = args.get("todos").and_then(Value::as_array) {
            let items: Vec<Value> = todos
                .iter()
                .filter_map(Value::as_str)
                .enumerate()
                .map(|(i, item_text)| {
                    json!({
                        "id": format!("todo_{i}"),
                        "text": item_text,
                        "done": false
                    })
                })
                .collect();

            if !items.is_empty() {
                blocks.push(json!({
                    "id": new_block_id("todo_list"),
                    "type": "todo_list",
                    "data": {
                        "title": "Action Items",
                        "items": items
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

        // Apply changes to the created capsule
        let capsule = &mut self.vault.capsules[index];
        capsule.subject = subject.to_string();
        capsule.from = format!("{author} <agent@capsuleos.local>");
        capsule.blocks = blocks;
        if !tags.is_empty() {
            capsule.tags = tags;
        }

        self.vault.save(index)?;

        let saved = &self.vault.capsules[index];
        Ok(json!({
            "success": true,
            "id": saved.message_id,
            "file_name": saved.file_name,
            "subject": saved.subject,
            "status": saved.status,
            "blocks_count": saved.blocks.len(),
            "parent_id": saved.parent_id
        }))
    }

    fn tool_capsule_reply(&mut self, args: &Value) -> Result<Value, String> {
        let parent_id = args
            .get("parent_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing required argument 'parent_id'".to_string())?;

        let content = args
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing required argument 'content'".to_string())?;

        let (_, parent) = self
            .find_capsule(parent_id)
            .ok_or_else(|| format!("Parent capsule not found: '{parent_id}'"))?;

        let parent_msg_id = parent.message_id.clone();
        let default_subject = if parent.subject.starts_with("Re: ") {
            parent.subject.clone()
        } else {
            format!("Re: {}", parent.subject)
        };

        let subject = args
            .get("subject")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&default_subject);

        let author = args
            .get("author")
            .and_then(Value::as_str)
            .unwrap_or("AI Opponent");

        let mut references = parent.references.clone();
        if !references.contains(&parent_msg_id) {
            references.push(parent_msg_id.clone());
        }

        let inherited_tags = parent.tags.clone();

        // Create the new child page
        let index = self.vault.create_page(Some(parent_msg_id.clone()))?;
        let capsule = &mut self.vault.capsules[index];

        capsule.subject = subject.to_string();
        capsule.from = format!("{author} <opponent@capsuleos.local>");
        capsule.in_reply_to = Some(parent_msg_id.clone());
        capsule.references = references;
        capsule.tags = inherited_tags;

        let mut blocks = Vec::new();
        for p in content
            .split("\n\n")
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            blocks.push(json!({
                "id": new_block_id("paragraph"),
                "type": "paragraph",
                "data": { "text": p }
            }));
        }
        if blocks.is_empty() {
            blocks.push(json!({
                "id": new_block_id("paragraph"),
                "type": "paragraph",
                "data": { "text": content }
            }));
        }
        capsule.blocks = blocks;

        self.vault.save(index)?;

        let saved = &self.vault.capsules[index];
        Ok(json!({
            "success": true,
            "id": saved.message_id,
            "file_name": saved.file_name,
            "subject": saved.subject,
            "parent_id": saved.parent_id,
            "in_reply_to": saved.in_reply_to,
            "blocks_count": saved.blocks.len()
        }))
    }

    fn tool_capsule_patch(&mut self, args: &Value) -> Result<Value, String> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing required argument 'id'".to_string())?;

        let (index, _) = self
            .find_capsule(id)
            .ok_or_else(|| format!("Capsule not found: '{id}'"))?;

        let action = args.get("action").and_then(Value::as_str);
        let content = args.get("content").and_then(Value::as_str);
        let target = args.get("target").and_then(Value::as_str);

        let append_markdown = args
            .get("append_markdown")
            .and_then(Value::as_str)
            .or_else(|| {
                if action == Some("append_text") {
                    content.or(target)
                } else {
                    None
                }
            });

        let toggle_task = args.get("toggle_task").and_then(Value::as_str).or_else(|| {
            if action == Some("toggle_task") {
                target.or(content)
            } else {
                None
            }
        });

        let mut tags_to_add: Vec<String> = args
            .get("add_tags")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default();

        if action == Some("add_tag") {
            if let Some(t) = target.or(content) {
                let clean = t.trim();
                if !clean.is_empty() && !tags_to_add.iter().any(|s| s.eq_ignore_ascii_case(clean)) {
                    tags_to_add.push(clean.to_string());
                }
            }
        }

        let replace_req = if action == Some("replace_text") {
            match (target, content) {
                (Some(tgt), Some(cnt)) => Some((tgt, cnt)),
                _ => {
                    return Err(
                        "Action 'replace_text' requires both 'target' and 'content' parameters"
                            .to_string(),
                    )
                }
            }
        } else {
            None
        };

        if append_markdown.is_none()
            && toggle_task.is_none()
            && tags_to_add.is_empty()
            && replace_req.is_none()
        {
            return Err("No patch operation specified. Provide 'append_markdown', 'toggle_task', 'add_tags', or 'action'".to_string());
        }

        let capsule = &mut self.vault.capsules[index];
        if !capsule.native {
            return Err(format!(
                "Capsule '{}' is read-only in the native runtime",
                capsule.message_id
            ));
        }

        let mut patch_summary = Vec::new();

        // 1. Append markdown
        if let Some(md) = append_markdown {
            let paras: Vec<&str> = md
                .split("\n\n")
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .collect();

            if paras.is_empty() {
                if !md.trim().is_empty() {
                    capsule.blocks.push(json!({
                        "id": new_block_id("paragraph"),
                        "type": "paragraph",
                        "data": { "text": md.trim() }
                    }));
                    patch_summary.push("Appended 1 paragraph block".to_string());
                }
            } else {
                let count = paras.len();
                for p in paras {
                    capsule.blocks.push(json!({
                        "id": new_block_id("paragraph"),
                        "type": "paragraph",
                        "data": { "text": p }
                    }));
                }
                patch_summary.push(format!("Appended {count} paragraph blocks"));
            }
        }

        // 2. Toggle task
        if let Some(task_target) = toggle_task {
            match toggle_task_in_blocks(&mut capsule.blocks, task_target) {
                Some((text, next_done)) => {
                    let status_str = if next_done {
                        "done ([x])"
                    } else {
                        "todo ([ ])"
                    };
                    patch_summary.push(format!("Toggled task '{text}' -> {status_str}"));
                }
                None => {
                    return Err(format!(
                        "Task matching '{task_target}' was not found in any todo block"
                    ));
                }
            }
        }

        // 3. Add tags
        if !tags_to_add.is_empty() {
            let mut added = Vec::new();
            for tag in tags_to_add {
                if !capsule.tags.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
                    capsule.tags.push(tag.clone());
                    added.push(tag);
                }
            }
            if !added.is_empty() {
                patch_summary.push(format!("Added tags: {}", added.join(", ")));
            }
        }

        // 4. Replace text
        if let Some((tgt, cnt)) = replace_req {
            let replacements = replace_in_blocks(&mut capsule.blocks, tgt, cnt);
            if replacements == 0 {
                return Err(format!(
                    "Target text '{tgt}' was not found in any content block"
                ));
            }
            patch_summary.push(format!("Replaced {replacements} occurrence(s) of '{tgt}'"));
        }

        self.vault.save(index)?;

        let saved = &self.vault.capsules[index];
        Ok(json!({
            "success": true,
            "id": saved.message_id,
            "file_name": saved.file_name,
            "subject": saved.subject,
            "blocks_count": saved.blocks.len(),
            "tags": saved.tags,
            "operations": patch_summary
        }))
    }

    fn tool_capsule_backlinks(&self, args: &Value) -> Result<Value, String> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing required argument 'id'".to_string())?;

        let (target_idx, capsule) = self
            .find_capsule(id)
            .ok_or_else(|| format!("Capsule not found: '{id}'"))?;

        let target_id = capsule.message_id.clone();
        let target_subject = capsule.subject.clone();

        let backlink_indices = links::backlinks(&self.vault.capsules, target_idx);
        let backlinks: Vec<Value> = backlink_indices
            .into_iter()
            .filter_map(|idx| self.vault.capsules.get(idx))
            .map(|c| {
                json!({
                    "id": c.message_id,
                    "subject": c.subject,
                    "file_name": c.file_name,
                    "date": c.date,
                    "tags": c.tags
                })
            })
            .collect();

        Ok(json!({
            "target_id": target_id,
            "target_subject": target_subject,
            "count": backlinks.len(),
            "backlinks": backlinks
        }))
    }

    fn tool_capsule_related(&self, args: &Value) -> Result<Value, String> {
        let id = args
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "Missing required argument 'id'".to_string())?;

        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|l| l as usize)
            .unwrap_or(5);

        let min_score = args
            .get("min_score")
            .and_then(Value::as_f64)
            .map(|s| s as f32)
            .unwrap_or(0.01);

        let (_, capsule) = self
            .find_capsule(id)
            .ok_or_else(|| format!("Capsule not found: '{id}'"))?;

        let target_id = capsule.message_id.clone();
        let target_subject = capsule.subject.clone();

        let mut bm25 = Bm25Index::default();
        for c in &self.vault.capsules {
            let text = links::page_text(c);
            let tokens = search::tokenize(&text);
            bm25.add(&c.message_id, tokens);
        }

        let ids: Vec<&str> = self
            .vault
            .capsules
            .iter()
            .map(|c| c.message_id.as_str())
            .collect();

        let matches = search::related(&bm25, &target_id, &ids, min_score, limit);

        let results: Vec<Value> = matches
            .into_iter()
            .filter_map(|(idx, score)| {
                self.vault.capsules.get(idx).map(|other| {
                    json!({
                        "id": other.message_id,
                        "subject": other.subject,
                        "similarity": (score * 1000.0).round() / 1000.0,
                        "file_name": other.file_name,
                        "tags": other.tags
                    })
                })
            })
            .collect();

        Ok(json!({
            "target_id": target_id,
            "target_subject": target_subject,
            "count": results.len(),
            "related": results
        }))
    }

    fn tool_capsule_tasks(&self, args: &Value) -> Result<Value, String> {
        let status = args
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("all")
            .to_string();
        let due = args.get("due").and_then(Value::as_str).unwrap_or("all");
        let priority = args
            .get("priority")
            .and_then(Value::as_str)
            .unwrap_or("all")
            .to_string();
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let capsule_id = args
            .get("capsule_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        let tag = args.get("tag").and_then(Value::as_str).map(str::to_string);

        let mut all_tasks = Vec::new();
        for c in &self.vault.capsules {
            all_tasks.extend(planning::tasks_in_capsule(
                &c.message_id,
                &c.subject,
                &c.tags,
                &c.blocks,
            ));
        }

        let today = planning::today_key();
        let (due_filter, upcoming_only) = if due == "upcoming" {
            ("all".to_string(), true)
        } else {
            (due.to_string(), false)
        };

        let filters = TaskFilters {
            text: query,
            status,
            priority,
            due: due_filter,
            capsule_id,
            tag,
        };

        let mut filtered = planning::filter_tasks(&all_tasks, &filters, &today);
        if upcoming_only {
            filtered.retain(|t| {
                if let Some(day) = t.item.due.as_deref().filter(|d| planning::is_date_key(d)) {
                    day >= today.as_str()
                } else {
                    false
                }
            });
        }

        let sorted = planning::sort_tasks(&filtered);

        let tasks_json: Vec<Value> = sorted
            .into_iter()
            .map(|t| {
                json!({
                    "id": t.item.id,
                    "text": t.item.text,
                    "done": t.item.done,
                    "status": t.item.status(),
                    "due": t.item.due,
                    "start": t.item.start,
                    "priority": t.item.priority(),
                    "blocked_by": t.item.blocked_by,
                    "capsule_id": t.capsule_id,
                    "capsule_subject": t.capsule_subject,
                    "capsule_tags": t.capsule_tags,
                    "block_id": t.block_id,
                    "block_title": t.block_title
                })
            })
            .collect();

        Ok(json!({
            "total": tasks_json.len(),
            "tasks": tasks_json
        }))
    }
}

fn replace_in_blocks(blocks: &mut [Value], target: &str, replacement: &str) -> usize {
    let mut count = 0;
    for block in blocks.iter_mut() {
        if let Some(data) = block.get_mut("data") {
            count += replace_in_value(data, target, replacement);
        }
    }
    count
}

fn replace_in_value(val: &mut Value, target: &str, replacement: &str) -> usize {
    let mut count = 0;
    match val {
        Value::String(s) => {
            if s.contains(target) {
                *s = s.replace(target, replacement);
                count += 1;
            }
        }
        Value::Array(arr) => {
            for item in arr {
                count += replace_in_value(item, target, replacement);
            }
        }
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if k != "id" && k != "type" {
                    count += replace_in_value(v, target, replacement);
                }
            }
        }
        _ => {}
    }
    count
}

fn toggle_task_in_blocks(blocks: &mut [Value], target: &str) -> Option<(String, bool)> {
    let target_clean = target.trim();
    for block in blocks.iter_mut() {
        if block.get("type").and_then(Value::as_str) == Some("todo_list") {
            if let Some(items) = block
                .get_mut("data")
                .and_then(|d| d.get_mut("items"))
                .and_then(Value::as_array_mut)
            {
                for item in items.iter_mut() {
                    let id = item
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let text = item
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let matches = id == target_clean
                        || text.eq_ignore_ascii_case(target_clean)
                        || text.to_lowercase().contains(&target_clean.to_lowercase());
                    if matches {
                        let current_done =
                            item.get("done").and_then(Value::as_bool).unwrap_or(false);
                        let next_done = !current_done;
                        item["done"] = json!(next_done);
                        item["status"] = json!(if next_done { "done" } else { "todo" });
                        return Some((text, next_done));
                    }
                }
            }
        }
    }
    None
}

/// Run the stdio JSON-RPC loop.
pub fn run_server(vault_override: Option<PathBuf>) {
    let mut handler = McpHandler::new(vault_override);
    eprintln!(
        "[capsulecraft-mcp] Starting server on stdio. Active vault: {}",
        handler.vault.dir.display()
    );
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(err) => {
                eprintln!("[capsulecraft-mcp] Stdin error: {err}");
                break;
            }
        };

        let trimmed = line.trim().trim_start_matches('\u{feff}');
        if trimmed.is_empty() {
            continue;
        }

        if let Some(response) = handler.handle_message(trimmed) {
            let _ = stdout_lock.write_all(response.as_bytes());
            let _ = stdout_lock.write_all(b"\n");
            let _ = stdout_lock.flush();
        }
    }

    eprintln!("[capsulecraft-mcp] Stdio stream closed. Shutting down.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_vault() -> PathBuf {
        let unique = format!(
            "capsulecraft_test_mcp_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn initialize_and_ping_flow() {
        let dir = temp_vault();
        let mut handler = McpHandler::new(Some(dir.clone()));

        let init_req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05"
            }
        })
        .to_string();

        let resp = handler.handle_message(&init_req).expect("response");
        let resp_val: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(resp_val["id"], 1);
        assert_eq!(resp_val["result"]["serverInfo"]["name"], SERVER_NAME);

        let ping_req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "ping"
        })
        .to_string();

        let resp_ping = handler.handle_message(&ping_req).expect("ping response");
        let ping_val: Value = serde_json::from_str(&resp_ping).unwrap();
        assert_eq!(ping_val["id"], 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tools_list_and_create_capsule() {
        let dir = temp_vault();
        let mut handler = McpHandler::new(Some(dir.clone()));

        // 1. List tools
        let list_req = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/list"
        })
        .to_string();
        let resp = handler
            .handle_message(&list_req)
            .expect("tools list response");
        let val: Value = serde_json::from_str(&resp).unwrap();
        let tools = val["result"]["tools"].as_array().expect("tools array");
        assert!(tools.iter().any(|t| t["name"] == "capsule_create"));
        assert!(tools.iter().any(|t| t["name"] == "capsule_search"));

        // 2. Create a capsule
        let create_req = json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "capsule_create",
                "arguments": {
                    "subject": "AI Sharding Strategy",
                    "content": "Paragraph one.\n\nParagraph two with details.",
                    "author": "Antigravity",
                    "tags": ["architecture", "scaling"],
                    "callouts": [
                        { "text": "Important invariant: zero-data loss", "tone": "emerald" }
                    ],
                    "todos": ["Benchmarking", "Rollout plan"]
                }
            }
        })
        .to_string();

        let resp = handler
            .handle_message(&create_req)
            .expect("create response");
        let val: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(val["result"]["isError"], false);

        // 3. Search for it
        let search_req = json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "capsule_search",
                "arguments": {
                    "query": "Sharding"
                }
            }
        })
        .to_string();

        let resp = handler
            .handle_message(&search_req)
            .expect("search response");
        let val: Value = serde_json::from_str(&resp).unwrap();
        let text_content = val["result"]["content"][0]["text"].as_str().unwrap();
        let search_result: Value = serde_json::from_str(text_content).unwrap();
        assert_eq!(search_result["total_matches"], 1);
        assert_eq!(
            search_result["results"][0]["subject"],
            "AI Sharding Strategy"
        );

        // 4. Read it
        let read_req = json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": {
                "name": "capsule_read",
                "arguments": {
                    "id": "AI Sharding Strategy"
                }
            }
        })
        .to_string();

        let resp = handler.handle_message(&read_req).expect("read response");
        let val: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(val["result"]["isError"], false);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn capsule_patch_flow() {
        let dir = temp_vault();
        let mut handler = McpHandler::new(Some(dir.clone()));

        // 1. Create a capsule with a todo
        let create_req = json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "tools/call",
            "params": {
                "name": "capsule_create",
                "arguments": {
                    "subject": "Patch Test Note",
                    "content": "Original paragraph here.",
                    "tags": ["testing"],
                    "todos": ["Write unit test", "Deploy to prod"]
                }
            }
        })
        .to_string();
        let resp = handler.handle_message(&create_req).expect("create");
        let val: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(val["result"]["isError"], false);

        // 2. Patch: append markdown, toggle task, and add tags
        let patch_req = json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "tools/call",
            "params": {
                "name": "capsule_patch",
                "arguments": {
                    "id": "Patch Test Note",
                    "append_markdown": "Appended second paragraph.\n\nAppended third paragraph.",
                    "toggle_task": "Write unit test",
                    "add_tags": ["mcp", "verified"]
                }
            }
        })
        .to_string();
        let resp = handler.handle_message(&patch_req).expect("patch");
        let val: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(val["result"]["isError"], false);

        // 3. Patch: replace_text
        let replace_req = json!({
            "jsonrpc": "2.0",
            "id": 12,
            "method": "tools/call",
            "params": {
                "name": "capsule_patch",
                "arguments": {
                    "id": "Patch Test Note",
                    "action": "replace_text",
                    "target": "Original paragraph",
                    "content": "Modified paragraph"
                }
            }
        })
        .to_string();
        let resp = handler.handle_message(&replace_req).expect("replace");
        let val: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(val["result"]["isError"], false);

        // 4. Verify by reading back
        let read_req = json!({
            "jsonrpc": "2.0",
            "id": 13,
            "method": "tools/call",
            "params": {
                "name": "capsule_read",
                "arguments": {
                    "id": "Patch Test Note"
                }
            }
        })
        .to_string();
        let resp = handler.handle_message(&read_req).expect("read");
        let val: Value = serde_json::from_str(&resp).unwrap();
        let content_text = val["result"]["content"][0]["text"].as_str().unwrap();
        let read_val: Value = serde_json::from_str(content_text).unwrap();

        // Check tags
        let tags = read_val["tags"].as_array().unwrap();
        assert!(tags.iter().any(|t| t == "testing"));
        assert!(tags.iter().any(|t| t == "mcp"));
        assert!(tags.iter().any(|t| t == "verified"));

        // Check text modification
        let body = read_val["content"].as_str().unwrap();
        assert!(body.contains("Modified paragraph"));
        assert!(body.contains("Appended second paragraph"));

        // Check task toggled
        let blocks = read_val["blocks"].as_array().unwrap();
        let todo_block = blocks
            .iter()
            .find(|b| b["type"] == "todo_list")
            .expect("todo block exists");
        let items = todo_block["data"]["items"].as_array().unwrap();
        let toggled = items
            .iter()
            .find(|it| it["text"] == "Write unit test")
            .unwrap();
        assert_eq!(toggled["done"], true);
        assert_eq!(toggled["status"], "done");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn capsule_backlinks_and_related_flow() {
        let dir = temp_vault();
        let mut handler = McpHandler::new(Some(dir.clone()));

        // 1. Create target note A
        let note_a_req = json!({
            "jsonrpc": "2.0",
            "id": 20,
            "method": "tools/call",
            "params": {
                "name": "capsule_create",
                "arguments": {
                    "subject": "Distributed Consensus",
                    "content": "Raft protocol and leader election mechanics in distributed databases."
                }
            }
        })
        .to_string();
        handler.handle_message(&note_a_req);

        // 2. Create note B linking to A
        let note_b_req = json!({
            "jsonrpc": "2.0",
            "id": 21,
            "method": "tools/call",
            "params": {
                "name": "capsule_create",
                "arguments": {
                    "subject": "Replication Architecture",
                    "content": "Building resilient replication on top of [[Distributed Consensus]] leader election."
                }
            }
        })
        .to_string();
        handler.handle_message(&note_b_req);

        // 3. Query backlinks for Note A
        let backlinks_req = json!({
            "jsonrpc": "2.0",
            "id": 22,
            "method": "tools/call",
            "params": {
                "name": "capsule_backlinks",
                "arguments": {
                    "id": "Distributed Consensus"
                }
            }
        })
        .to_string();
        let resp = handler
            .handle_message(&backlinks_req)
            .expect("backlinks response");
        let val: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(val["result"]["isError"], false);
        let text_content = val["result"]["content"][0]["text"].as_str().unwrap();
        let backlinks_val: Value = serde_json::from_str(text_content).unwrap();
        assert_eq!(backlinks_val["count"], 1);
        assert_eq!(
            backlinks_val["backlinks"][0]["subject"],
            "Replication Architecture"
        );

        // 4. Query related for Note A
        let related_req = json!({
            "jsonrpc": "2.0",
            "id": 23,
            "method": "tools/call",
            "params": {
                "name": "capsule_related",
                "arguments": {
                    "id": "Distributed Consensus"
                }
            }
        })
        .to_string();
        let resp = handler
            .handle_message(&related_req)
            .expect("related response");
        let val: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(val["result"]["isError"], false);
        let text_content = val["result"]["content"][0]["text"].as_str().unwrap();
        let related_val: Value = serde_json::from_str(text_content).unwrap();
        assert_eq!(related_val["count"], 1);
        assert_eq!(
            related_val["related"][0]["subject"],
            "Replication Architecture"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn capsule_tasks_flow() {
        let dir = temp_vault();
        let mut handler = McpHandler::new(Some(dir.clone()));

        // Create a note with tasks
        let create_req = json!({
            "jsonrpc": "2.0",
            "id": 30,
            "method": "tools/call",
            "params": {
                "name": "capsule_create",
                "arguments": {
                    "subject": "Sprint Tasks",
                    "content": "Sprint planning overview",
                    "todos": ["Task Alpha", "Task Beta", "Task Gamma"]
                }
            }
        })
        .to_string();
        handler.handle_message(&create_req);

        // Toggle Task Beta to done
        let toggle_req = json!({
            "jsonrpc": "2.0",
            "id": 31,
            "method": "tools/call",
            "params": {
                "name": "capsule_patch",
                "arguments": {
                    "id": "Sprint Tasks",
                    "toggle_task": "Task Beta"
                }
            }
        })
        .to_string();
        handler.handle_message(&toggle_req);

        // Query all tasks
        let tasks_all_req = json!({
            "jsonrpc": "2.0",
            "id": 32,
            "method": "tools/call",
            "params": {
                "name": "capsule_tasks",
                "arguments": {
                    "status": "all"
                }
            }
        })
        .to_string();
        let resp = handler
            .handle_message(&tasks_all_req)
            .expect("tasks all response");
        let val: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(val["result"]["isError"], false);
        let text_content = val["result"]["content"][0]["text"].as_str().unwrap();
        let tasks_val: Value = serde_json::from_str(text_content).unwrap();
        assert_eq!(tasks_val["total"], 3);

        // Query only open tasks
        let tasks_open_req = json!({
            "jsonrpc": "2.0",
            "id": 33,
            "method": "tools/call",
            "params": {
                "name": "capsule_tasks",
                "arguments": {
                    "status": "open"
                }
            }
        })
        .to_string();
        let resp = handler
            .handle_message(&tasks_open_req)
            .expect("tasks open response");
        let val: Value = serde_json::from_str(&resp).unwrap();
        let text_content = val["result"]["content"][0]["text"].as_str().unwrap();
        let tasks_val: Value = serde_json::from_str(text_content).unwrap();
        assert_eq!(tasks_val["total"], 2);

        // Query only done tasks
        let tasks_done_req = json!({
            "jsonrpc": "2.0",
            "id": 34,
            "method": "tools/call",
            "params": {
                "name": "capsule_tasks",
                "arguments": {
                    "status": "done"
                }
            }
        })
        .to_string();
        let resp = handler
            .handle_message(&tasks_done_req)
            .expect("tasks done response");
        let val: Value = serde_json::from_str(&resp).unwrap();
        let text_content = val["result"]["content"][0]["text"].as_str().unwrap();
        let tasks_val: Value = serde_json::from_str(text_content).unwrap();
        assert_eq!(tasks_val["total"], 1);
        assert_eq!(tasks_val["tasks"][0]["text"], "Task Beta");

        let _ = fs::remove_dir_all(&dir);
    }
}
