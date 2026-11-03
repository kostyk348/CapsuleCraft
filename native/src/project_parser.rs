//! Codebase parser & knowledge capsule generator.
//!
//! Scans local projects respecting `.gitignore`, extracts architecture,
//! module relationships, public API signatures, documentation and TODO items,
//! and turns them into an interconnected RFC 5322 `.eml` knowledge vault.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::mime::{assemble_eml, new_block_id, parse_eml, Attachment, Capsule};

/// Detected project ecosystem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectKind {
    Rust,
    Node,
    Python,
    Go,
    Flutter,
    CSharp,
    Java,
    Generic,
}

impl ProjectKind {
    pub fn display_name(&self) -> &'static str {
        match self {
            ProjectKind::Rust => "Rust (Cargo)",
            ProjectKind::Node => "Node.js / TypeScript",
            ProjectKind::Python => "Python",
            ProjectKind::Go => "Go",
            ProjectKind::Flutter => "Flutter / Dart",
            ProjectKind::CSharp => "C# / .NET",
            ProjectKind::Java => "Java / Kotlin",
            ProjectKind::Generic => "Codebase",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            ProjectKind::Rust => "🦀",
            ProjectKind::Node => "📦",
            ProjectKind::Python => "🐍",
            ProjectKind::Go => "🐹",
            ProjectKind::Flutter => "🎯",
            ProjectKind::CSharp => "🔷",
            ProjectKind::Java => "☕",
            ProjectKind::Generic => "📁",
        }
    }
}

/// A TODO or FIXME item extracted from code comments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedTodo {
    pub file_path: String,
    pub line: usize,
    pub kind: String, // "TODO", "FIXME", "NOTE"
    pub text: String,
}

/// Persistent metadata for a single indexed file in `.capsule/.index_state.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIndexMeta {
    pub hash: String,
    pub mtime: i64,
    pub size: u64,
    pub capsule_file: String,
    pub subsystem: String,
    pub doc_preview: Option<String>,
    pub title: Option<String>,
    pub todos: Vec<ExtractedTodo>,
}

/// Root state serialized into `.capsule/.index_state.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexState {
    pub version: u32,
    pub project_name: String,
    pub files: HashMap<String, FileIndexMeta>,
}

/// Lightweight reference to a module inside a subsystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubsystemModuleRef {
    pub rel_path: String,
    pub doc_preview: Option<String>,
}

/// Compute fast deterministic FNV-1a 64-bit hash.
pub fn compute_fast_hash(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

/// A parsed source file module.
#[derive(Debug, Clone, Default)]
pub struct ParsedModule {
    pub rel_path: String,
    pub file_name: String,
    pub doc_comment: Option<String>,
    pub imports: Vec<String>,
    pub public_items: Vec<String>,
    pub todos: Vec<ExtractedTodo>,
    pub line_count: usize,
}

/// A parsed markdown documentation file (README, guides, architecture, etc.).
#[derive(Debug, Clone, Default)]
pub struct ParsedDoc {
    pub rel_path: String,
    pub title: String,
    pub blocks: Vec<Value>,
}

/// Summary of an ingestion run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestSummary {
    pub project_name: String,
    pub project_kind: ProjectKind,
    pub modules_count: usize,
    pub docs_count: usize,
    pub todos_count: usize,
    pub capsules_created: usize,
    pub capsules_updated: usize,
    pub capsules_removed: usize,
    pub vault_dir: PathBuf,
}

/// Inspect the directory root to detect project type and name.
pub fn detect_project_meta(root: &Path) -> (String, ProjectKind) {
    let folder_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project")
        .to_string();

    // 1. Rust Cargo.toml
    let cargo = root.join("Cargo.toml");
    if cargo.is_file() {
        if let Ok(content) = fs::read_to_string(&cargo) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("name =") || trimmed.starts_with("name=") {
                    let parts: Vec<&str> = trimmed.split('=').collect();
                    if parts.len() >= 2 {
                        let name = parts[1].trim().trim_matches('"').trim_matches('\'');
                        if !name.is_empty() {
                            return (name.to_string(), ProjectKind::Rust);
                        }
                    }
                }
            }
        }
        return (folder_name, ProjectKind::Rust);
    }

    // 2. Node package.json
    let pkg = root.join("package.json");
    if pkg.is_file() {
        if let Ok(content) = fs::read_to_string(&pkg) {
            if let Ok(val) = serde_json::from_str::<Value>(&content) {
                if let Some(name) = val.get("name").and_then(Value::as_str) {
                    return (name.to_string(), ProjectKind::Node);
                }
            }
        }
        return (folder_name, ProjectKind::Node);
    }

    // 3. Go go.mod
    let gomod = root.join("go.mod");
    if gomod.is_file() {
        if let Ok(content) = fs::read_to_string(&gomod) {
            if let Some(first_line) = content.lines().next() {
                if first_line.starts_with("module ") {
                    let mod_name = first_line.trim_start_matches("module ").trim();
                    let short_name = mod_name.split('/').last().unwrap_or(mod_name);
                    return (short_name.to_string(), ProjectKind::Go);
                }
            }
        }
        return (folder_name, ProjectKind::Go);
    }

    // 4. Flutter / Dart pubspec.yaml
    let pubspec = root.join("pubspec.yaml");
    if pubspec.is_file() {
        if let Ok(content) = fs::read_to_string(&pubspec) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("name:") {
                    let name = trimmed.trim_start_matches("name:").trim().trim_matches('"').trim_matches('\'');
                    if !name.is_empty() {
                        return (name.to_string(), ProjectKind::Flutter);
                    }
                }
            }
        }
        return (folder_name, ProjectKind::Flutter);
    }

    // 5. Python
    if root.join("pyproject.toml").is_file() || root.join("setup.py").is_file() {
        return (folder_name, ProjectKind::Python);
    }

    // 6. C#
    if root.join("*.csproj").is_file() || root.join("*.sln").is_file() {
        return (folder_name, ProjectKind::CSharp);
    }

    (folder_name, ProjectKind::Generic)
}

/// Check if a path corresponds to a source code file.
pub fn is_code_file(path: &Path) -> bool {
    let ext = match path.extension().and_then(|e| e.to_str()) {
        Some(e) => e.to_ascii_lowercase(),
        None => return false,
    };
    // Exclude binary, media, archives, and build artifacts
    let is_binary_or_media = matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" | "bmp" | "svg" | "tiff" |
        "mp3" | "wav" | "ogg" | "flac" | "aac" | "m4a" |
        "mp4" | "webm" | "mkv" | "avi" | "mov" |
        "zip" | "tar" | "gz" | "7z" | "rar" | "bz2" | "xz" |
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" |
        "exe" | "dll" | "so" | "dylib" | "bin" | "wasm" | "obj" | "o" | "a" | "lib" | "class" |
        "pyc" | "pyo" | "pyd" |
        "lock" | "sum" | "ttf" | "otf" | "woff" | "woff2" | "eot" | "db" | "sqlite" | "eml"
    );
    if is_binary_or_media {
        return false;
    }
    matches!(
        ext.as_str(),
        "dart" | "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" |
        "py" | "pyw" | "go" | "cs" | "java" | "kt" | "kts" |
        "c" | "cpp" | "cc" | "cxx" | "h" | "hpp" | "hh" | "hxx" |
        "swift" | "rb" | "php" | "lua" | "zig" | "nim" | "scala" | "groovy" |
        "sh" | "bash" | "zsh" | "ps1" | "bat" | "cmd" |
        "vue" | "svelte" | "html" | "css" | "scss" | "sass" | "less" |
        "sql" | "graphql" | "proto"
    )
}

/// Check if a path corresponds to a markdown documentation file.
pub fn is_doc_file(path: &Path) -> bool {
    let ext = match path.extension().and_then(|e| e.to_str()) {
        Some(e) => e.to_ascii_lowercase(),
        None => return false,
    };
    matches!(ext.as_str(), "md" | "markdown")
}

/// Universal subsystem classifier grouping modules into architectural layers.
pub fn classify_subsystem(rel_path: &str) -> String {
    let normalized = rel_path.replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').collect();
    if parts.len() <= 1 {
        return "root".to_string();
    }
    let top = parts[0];
    if (top == "crates" || top == "packages" || top == "modules" || top == "apps" || top == "services" || top == "plugins") && parts.len() >= 3 {
        format!("{}/{}", top, parts[1])
    } else if top == "lib" && parts.len() >= 4 && (parts[1] == "features" || parts[1] == "modules" || parts[1] == "src" || parts[1] == "screens") {
        format!("{}/{}/{}", top, parts[1], parts[2])
    } else if parts.len() >= 3 {
        format!("{}/{}", top, parts[1])
    } else {
        top.to_string()
    }
}

/// Parse a markdown documentation file into a `ParsedDoc`.
pub fn parse_markdown_file(rel_path: &str, content: &str, project_name: &str) -> ParsedDoc {
    let mut title = Path::new(rel_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(rel_path)
        .to_string();

    for line in content.lines().take(10) {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            title = trimmed.trim_start_matches('#').trim().to_string();
            break;
        }
    }

    let blocks = markdown_to_blocks(content, project_name);

    ParsedDoc {
        rel_path: rel_path.to_string(),
        title,
        blocks,
    }
}

/// Convert arbitrary Markdown text into CapsuleCraft native block models.
pub fn markdown_to_blocks(content: &str, _project_name: &str) -> Vec<Value> {
    let mut blocks = Vec::new();
    let mut current_p: Vec<String> = Vec::new();
    let mut in_code_block = false;
    let mut code_lines: Vec<String> = Vec::new();
    let mut code_lang = String::new();

    let flush_p = |p: &mut Vec<String>, bl: &mut Vec<Value>| {
        if !p.is_empty() {
            let text = p.join("\n").trim().to_string();
            if !text.is_empty() {
                bl.push(json!({
                    "id": new_block_id("paragraph"),
                    "type": "paragraph",
                    "managed_by": "project_parser",
                    "data": { "text": text }
                }));
            }
            p.clear();
        }
    };

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("```") {
            if in_code_block {
                blocks.push(json!({
                    "id": new_block_id("code"),
                    "type": "code",
                    "managed_by": "project_parser",
                    "data": {
                        "language": if code_lang.is_empty() { "text" } else { &code_lang },
                        "code": code_lines.join("\n")
                    }
                }));
                code_lines.clear();
                code_lang.clear();
                in_code_block = false;
            } else {
                flush_p(&mut current_p, &mut blocks);
                in_code_block = true;
                code_lang = trimmed.trim_start_matches('`').trim().to_string();
            }
            continue;
        }

        if in_code_block {
            code_lines.push(line.to_string());
            continue;
        }

        if trimmed.starts_with("# ") {
            flush_p(&mut current_p, &mut blocks);
            blocks.push(json!({
                "id": new_block_id("heading"),
                "type": "heading",
                "managed_by": "project_parser",
                "data": {
                    "level": 1,
                    "text": trimmed.trim_start_matches('#').trim().to_string()
                }
            }));
        } else if trimmed.starts_with("## ") {
            flush_p(&mut current_p, &mut blocks);
            blocks.push(json!({
                "id": new_block_id("heading"),
                "type": "heading",
                "managed_by": "project_parser",
                "data": {
                    "level": 2,
                    "text": trimmed.trim_start_matches('#').trim().to_string()
                }
            }));
        } else if trimmed.starts_with("### ") {
            flush_p(&mut current_p, &mut blocks);
            blocks.push(json!({
                "id": new_block_id("heading"),
                "type": "heading",
                "managed_by": "project_parser",
                "data": {
                    "level": 3,
                    "text": trimmed.trim_start_matches('#').trim().to_string()
                }
            }));
        } else if trimmed.starts_with("> ") {
            flush_p(&mut current_p, &mut blocks);
            blocks.push(json!({
                "id": new_block_id("callout"),
                "type": "callout",
                "managed_by": "project_parser",
                "data": {
                    "icon": "info",
                    "text": trimmed.trim_start_matches('>').trim().to_string()
                }
            }));
        } else if trimmed.is_empty() {
            flush_p(&mut current_p, &mut blocks);
        } else {
            current_p.push(line.to_string());
        }
    }

    if in_code_block {
        blocks.push(json!({
            "id": new_block_id("code"),
            "type": "code",
            "managed_by": "project_parser",
            "data": {
                "language": if code_lang.is_empty() { "text" } else { &code_lang },
                "code": code_lines.join("\n")
            }
        }));
    } else {
        flush_p(&mut current_p, &mut blocks);
    }

    blocks
}

/// Parse a single source file to extract docs, exports, imports, and TODOs with multi-line signature and docstring support.
pub fn parse_source_file(rel_path: &str, content: &str) -> ParsedModule {
    let mut module = ParsedModule {
        rel_path: rel_path.to_string(),
        file_name: Path::new(rel_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(rel_path)
            .to_string(),
        doc_comment: None,
        imports: Vec::new(),
        public_items: Vec::new(),
        todos: Vec::new(),
        line_count: 0,
    };

    let lang = detect_lang(&module.file_name);
    let lines: Vec<&str> = content.lines().collect();
    module.line_count = lines.len();

    let mut doc_lines = Vec::new();
    let mut in_header_doc = true;
    let mut in_block_comment = false;
    let mut in_python_doc = false;
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        let line_num = i + 1;

        // 1. Python multiline triple quotes docstrings
        if lang == "python" && in_header_doc {
            if !in_python_doc && (trimmed.starts_with("\"\"\"") || trimmed.starts_with("'''")) {
                let quote = if trimmed.starts_with("\"\"\"") { "\"\"\"" } else { "'''" };
                let rest = trimmed.trim_start_matches(quote);
                if rest.ends_with(quote) && rest.len() >= quote.len() {
                    let inside = rest.trim_end_matches(quote).trim();
                    if !inside.is_empty() {
                        doc_lines.push(inside.to_string());
                    }
                    in_header_doc = false;
                } else {
                    in_python_doc = true;
                    if !rest.is_empty() {
                        doc_lines.push(rest.to_string());
                    }
                }
                i += 1;
                continue;
            } else if in_python_doc {
                if trimmed.ends_with("\"\"\"") || trimmed.ends_with("'''") {
                    let inside = trimmed.trim_end_matches("\"\"\"").trim_end_matches("'''").trim();
                    if !inside.is_empty() {
                        doc_lines.push(inside.to_string());
                    }
                    in_python_doc = false;
                    in_header_doc = false;
                } else {
                    doc_lines.push(line.trim_end().to_string());
                }
                i += 1;
                continue;
            }
        }

        // 2. Multiline block comments /** ... */
        if in_header_doc {
            if !in_block_comment && (trimmed.starts_with("/**") || trimmed.starts_with("/*!")) {
                in_block_comment = true;
                let rest = trimmed.trim_start_matches("/**").trim_start_matches("/*!").trim_end_matches("*/").trim();
                if !rest.is_empty() {
                    doc_lines.push(rest.to_string());
                }
                if trimmed.ends_with("*/") && trimmed.len() > 3 {
                    in_block_comment = false;
                }
                i += 1;
                continue;
            } else if in_block_comment {
                if trimmed.ends_with("*/") {
                    let rest = trimmed.trim_end_matches("*/").trim_start_matches('*').trim();
                    if !rest.is_empty() {
                        doc_lines.push(rest.to_string());
                    }
                    in_block_comment = false;
                } else {
                    let cleaned = trimmed.trim_start_matches('*').trim();
                    doc_lines.push(cleaned.to_string());
                }
                i += 1;
                continue;
            } else if trimmed.starts_with("///") || trimmed.starts_with("//!") {
                let doc = trimmed.trim_start_matches("///").trim_start_matches("//!").trim();
                doc_lines.push(doc.to_string());
                i += 1;
                continue;
            } else if trimmed.starts_with("//") && doc_lines.is_empty() && trimmed.to_lowercase().contains("copyright") {
                // skip license lines
                i += 1;
                continue;
            } else if !trimmed.is_empty() && !trimmed.starts_with("//") && !trimmed.starts_with('#') {
                in_header_doc = false;
            }
        }

        // 3. Extract TODO / FIXME comments
        if let Some(todo) = extract_todo(rel_path, line_num, trimmed) {
            module.todos.push(todo);
        }

        // 4. Extract imports
        if trimmed.starts_with("use crate::") || trimmed.starts_with("use super::") {
            let item = trimmed.trim_start_matches("use ").trim_end_matches(';').trim();
            module.imports.push(item.to_string());
        } else if trimmed.starts_with("import ") {
            let clean = trimmed.trim_start_matches("import ").trim_end_matches(';').trim().trim_matches('\'').trim_matches('"');
            if !clean.is_empty() {
                module.imports.push(clean.to_string());
            }
        } else if trimmed.starts_with("from .") {
            module.imports.push(trimmed.to_string());
        }

        // 5. Extract multi-line declarations
        if is_declaration_start(trimmed, lang) && module.public_items.len() < 30 {
            let (accumulated, consumed) = accumulate_declaration(&lines, i, lang);
            if !accumulated.is_empty() {
                module.public_items.push(accumulated);
            }
            i += consumed.max(1);
            continue;
        }

        i += 1;
    }

    if !doc_lines.is_empty() {
        let joined = doc_lines.join("\n").trim().to_string();
        if !joined.is_empty() {
            module.doc_comment = Some(joined);
        }
    }

    module
}

/// Helper to detect and extract a TODO, FIXME, or BUG comment.
fn extract_todo(file: &str, line: usize, trimmed: &str) -> Option<ExtractedTodo> {
    let lower = trimmed.to_ascii_uppercase();
    let (kind, pos) = if let Some(p) = lower.find("TODO") {
        ("TODO", p)
    } else if let Some(p) = lower.find("FIXME") {
        ("FIXME", p)
    } else if let Some(p) = lower.find("BUG:") {
        ("BUG", p)
    } else {
        return None;
    };

    let is_comment = trimmed.starts_with("//")
        || trimmed.starts_with('#')
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*');

    if !is_comment {
        return None;
    }

    let remainder = &trimmed[pos..];
    let text = remainder
        .trim_start_matches("TODO:")
        .trim_start_matches("TODO")
        .trim_start_matches("FIXME:")
        .trim_start_matches("FIXME")
        .trim_start_matches("BUG:")
        .trim_start_matches('-')
        .trim_end_matches("*/")
        .trim();

    if text.is_empty() {
        return None;
    }

    Some(ExtractedTodo {
        file_path: file.to_string(),
        line,
        kind: kind.to_string(),
        text: text.to_string(),
    })
}

/// Check if line marks the start of an exported/public declaration across supported languages.
fn is_declaration_start(trimmed: &str, lang: &str) -> bool {
    match lang {
        "rust" => {
            trimmed.starts_with("pub struct ")
                || trimmed.starts_with("pub enum ")
                || trimmed.starts_with("pub fn ")
                || trimmed.starts_with("pub async fn ")
                || trimmed.starts_with("pub trait ")
                || trimmed.starts_with("pub type ")
                || trimmed.starts_with("pub const ")
        }
        "typescript" | "javascript" => {
            trimmed.starts_with("export function ")
                || trimmed.starts_with("export async function ")
                || trimmed.starts_with("export const ")
                || trimmed.starts_with("export class ")
                || trimmed.starts_with("export interface ")
                || trimmed.starts_with("export type ")
                || trimmed.starts_with("export default ")
        }
        "python" => {
            (trimmed.starts_with("def ") && !trimmed.starts_with("def _"))
                || (trimmed.starts_with("async def ") && !trimmed.starts_with("async def _"))
                || (trimmed.starts_with("class ") && !trimmed.starts_with("class _"))
        }
        "go" => {
            if trimmed.starts_with("func ") {
                trimmed.chars().nth(5).map_or(false, |c| c.is_ascii_uppercase())
            } else if trimmed.starts_with("type ") {
                trimmed.chars().nth(5).map_or(false, |c| c.is_ascii_uppercase())
            } else {
                false
            }
        }
        "dart" => {
            (trimmed.starts_with("class ") && !trimmed.starts_with("class _"))
                || (trimmed.starts_with("abstract class ") && !trimmed.starts_with("abstract class _"))
                || (trimmed.starts_with("enum ") && !trimmed.starts_with("enum _"))
                || (trimmed.starts_with("mixin ") && !trimmed.starts_with("mixin _"))
                || (trimmed.starts_with("extension ") && !trimmed.starts_with("extension _"))
        }
        "csharp" | "java" => {
            trimmed.starts_with("public ")
        }
        _ => {
            trimmed.starts_with("pub ") || trimmed.starts_with("export ")
        }
    }
}

/// Accumulate multi-line signature until `{`, `;`, or `:` (Python), up to 8 lines.
/// Returns `(cleaned_declaration, lines_consumed)`.
fn accumulate_declaration(lines: &[&str], start_idx: usize, lang: &str) -> (String, usize) {
    let mut collected = Vec::new();
    let mut consumed = 0;
    let max_lines = 8.min(lines.len() - start_idx);

    for offset in 0..max_lines {
        let current_line = lines[start_idx + offset].trim();
        consumed += 1;
        collected.push(current_line);

        if lang == "python" {
            if current_line.ends_with(':') {
                break;
            }
        } else {
            if current_line.contains('{') || current_line.ends_with(';') {
                break;
            }
        }
    }

    let joined = collected.join(" ");
    let mut clean = if lang == "python" {
        joined.trim_end_matches(':').trim().to_string()
    } else {
        joined.split('{').next().unwrap_or(&joined).trim_end_matches(';').trim().to_string()
    };

    // Compact multiple internal spaces into single space
    while clean.contains("  ") {
        clean = clean.replace("  ", " ");
    }

    (clean, consumed)
}

/// Run full project ingestion into a target `.capsule` directory with incremental caching
/// and non-destructive user content preservation.
pub fn ingest_project(project_root: &Path, vault_dir: &Path) -> Result<IngestSummary, String> {
    if !project_root.is_dir() {
        return Err(format!("Project root does not exist: {}", project_root.display()));
    }

    fs::create_dir_all(vault_dir)
        .map_err(|e| format!("Cannot create vault dir {}: {e}", vault_dir.display()))?;

    let (project_name, project_kind) = detect_project_meta(project_root);

    // 1. Load existing incremental index cache if present
    let index_file = vault_dir.join(".index_state.json");
    let mut index_state: IndexState = if index_file.is_file() {
        fs::read_to_string(&index_file)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_else(|| IndexState {
                version: 1,
                project_name: project_name.clone(),
                files: HashMap::new(),
            })
    } else {
        IndexState {
            version: 1,
            project_name: project_name.clone(),
            files: HashMap::new(),
        }
    };
    index_state.project_name = project_name.clone();

    // 2. Walk directory respecting .gitignore
    let walker = WalkBuilder::new(project_root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .build();

    let root_msg_id = format!("<project-index-{}@capsuleos.local>", slugify(&project_name));

    let mut current_scanned_files = HashSet::new();
    let mut all_todos: Vec<ExtractedTodo> = Vec::new();
    let mut subsystems: BTreeMap<String, Vec<SubsystemModuleRef>> = BTreeMap::new();
    let mut parsed_docs: Vec<ParsedDoc> = Vec::new();

    let mut capsules_created = 0;
    let mut capsules_updated = 0;
    let mut capsules_removed = 0;
    let mut total_modules = 0;

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let rel_path = match path.strip_prefix(project_root) {
            Ok(p) => p.to_string_lossy().replace('\\', "/"),
            Err(_) => path.to_string_lossy().replace('\\', "/"),
        };

        if rel_path.starts_with("target/")
            || rel_path.starts_with("node_modules/")
            || rel_path.starts_with("dist/")
            || rel_path.starts_with(".capsule/")
            || rel_path.starts_with(".dart_tool/")
            || rel_path.starts_with("build/")
        {
            continue;
        }

        let metadata = match fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let size = metadata.len();

        current_scanned_files.insert(rel_path.clone());

        if is_code_file(path) {
            total_modules += 1;
            let sub = classify_subsystem(&rel_path);
            let safe_slug = slugify(&rel_path);
            let rel_capsule_file = format!("modules/module_{safe_slug}.eml");
            let target_eml = vault_dir.join(&rel_capsule_file);

            let sub_msg_id = format!(
                "<subsystem-{}-{}@capsuleos.local>",
                slugify(&project_name),
                slugify(&sub)
            );

            let cached = index_state.files.get(&rel_path);
            let is_up_to_date = match cached {
                Some(meta) => meta.mtime == mtime && meta.size == size && target_eml.is_file(),
                None => false,
            };

            if is_up_to_date {
                let meta = cached.unwrap();
                all_todos.extend(meta.todos.clone());
                subsystems.entry(sub).or_default().push(SubsystemModuleRef {
                    rel_path: rel_path.clone(),
                    doc_preview: meta.doc_preview.clone(),
                });
            } else if let Ok(content) = fs::read_to_string(path) {
                let hash = compute_fast_hash(content.as_bytes());
                let module = parse_source_file(&rel_path, &content);
                all_todos.extend(module.todos.clone());

                let doc_preview = module
                    .doc_comment
                    .as_deref()
                    .and_then(|d| d.lines().next())
                    .map(|s| s.to_string());

                subsystems.entry(sub.clone()).or_default().push(SubsystemModuleRef {
                    rel_path: rel_path.clone(),
                    doc_preview: doc_preview.clone(),
                });

                let module_capsule = build_module_capsule(&project_name, &sub_msg_id, &root_msg_id, &module);
                let was_new = write_capsule_preserving(vault_dir, &rel_capsule_file, module_capsule, &project_name)?;
                if was_new {
                    capsules_created += 1;
                } else {
                    capsules_updated += 1;
                }

                index_state.files.insert(
                    rel_path.clone(),
                    FileIndexMeta {
                        hash,
                        mtime,
                        size,
                        capsule_file: rel_capsule_file,
                        subsystem: sub,
                        doc_preview,
                        title: None,
                        todos: module.todos,
                    },
                );
            }
        } else if is_doc_file(path) {
            let safe_slug = slugify(&rel_path);
            let rel_capsule_file = format!("docs/doc_{safe_slug}.eml");
            let target_eml = vault_dir.join(&rel_capsule_file);

            let cached = index_state.files.get(&rel_path);
            let is_up_to_date = match cached {
                Some(meta) => meta.mtime == mtime && meta.size == size && target_eml.is_file(),
                None => false,
            };

            if is_up_to_date {
                let meta = cached.unwrap();
                let title = meta.title.clone().unwrap_or_else(|| rel_path.clone());
                parsed_docs.push(ParsedDoc {
                    rel_path: rel_path.clone(),
                    title,
                    blocks: Vec::new(),
                });
            } else if let Ok(content) = fs::read_to_string(path) {
                let hash = compute_fast_hash(content.as_bytes());
                let doc = parse_markdown_file(&rel_path, &content, &project_name);
                let doc_capsule = build_doc_capsule(&project_name, &root_msg_id, &doc);
                let was_new = write_capsule_preserving(vault_dir, &rel_capsule_file, doc_capsule, &project_name)?;
                if was_new {
                    capsules_created += 1;
                } else {
                    capsules_updated += 1;
                }

                index_state.files.insert(
                    rel_path.clone(),
                    FileIndexMeta {
                        hash,
                        mtime,
                        size,
                        capsule_file: rel_capsule_file,
                        subsystem: "docs".to_string(),
                        doc_preview: None,
                        title: Some(doc.title.clone()),
                        todos: Vec::new(),
                    },
                );

                parsed_docs.push(doc);
            }
        }
    }

    // 3. Delete obsolete capsules for removed files
    let mut removed_paths = Vec::new();
    for (tracked_file, meta) in &index_state.files {
        if !current_scanned_files.contains(tracked_file) {
            let eml_path = vault_dir.join(&meta.capsule_file);
            let _ = fs::remove_file(&eml_path);
            let old_flat = vault_dir.join(
                Path::new(&meta.capsule_file)
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or(&meta.capsule_file),
            );
            let _ = fs::remove_file(&old_flat);
            removed_paths.push(tracked_file.clone());
            capsules_removed += 1;
        }
    }
    for p in removed_paths {
        index_state.files.remove(&p);
    }

    // 4. Update Subsystems Capsules
    for (sub_name, mods) in &subsystems {
        let sub_msg_id = format!(
            "<subsystem-{}-{}@capsuleos.local>",
            slugify(&project_name),
            slugify(sub_name)
        );
        let sub_capsule = build_subsystem_capsule(&project_name, &root_msg_id, sub_name, &sub_msg_id, mods);
        let sub_file = format!("subsystems/subsystem_{}.eml", slugify(sub_name));
        let was_new = write_capsule_preserving(vault_dir, &sub_file, sub_capsule, &project_name)?;
        if was_new {
            capsules_created += 1;
        } else {
            capsules_updated += 1;
        }
    }

    // 5. Update Documentation overview & Root Project Capsule
    parsed_docs.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    let root_capsule = build_root_capsule(
        &project_name,
        &project_kind,
        project_root,
        &root_msg_id,
        &subsystems,
        &parsed_docs,
        all_todos.len(),
    );
    let was_new = write_capsule_preserving(vault_dir, "index.eml", root_capsule, &project_name)?;
    if was_new {
        capsules_created += 1;
    } else {
        capsules_updated += 1;
    }

    // 6. Update Tasks & Debt Capsule
    if !all_todos.is_empty() {
        let tasks_capsule = build_tasks_capsule(&project_name, &root_msg_id, &all_todos);
        let was_new = write_capsule_preserving(vault_dir, "tasks_todo.eml", tasks_capsule, &project_name)?;
        if was_new {
            capsules_created += 1;
        } else {
            capsules_updated += 1;
        }
    } else {
        let tasks_path = vault_dir.join("tasks_todo.eml");
        if tasks_path.is_file() {
            let _ = fs::remove_file(&tasks_path);
            capsules_removed += 1;
        }
    }

    // 7. Persist incremental index cache
    if let Ok(serialized) = serde_json::to_string_pretty(&index_state) {
        let _ = fs::write(&index_file, serialized);
    }

    // 8. Scaffold Autonomous Agent Skill
    let _ = crate::project_skill::scaffold_agent_skill(project_root, &project_name);

    Ok(IngestSummary {
        project_name,
        project_kind,
        modules_count: total_modules,
        docs_count: parsed_docs.len(),
        todos_count: all_todos.len(),
        capsules_created,
        capsules_updated,
        capsules_removed,
        vault_dir: vault_dir.to_path_buf(),
    })
}

/// Check if block was generated by the automated project parser.
pub fn is_system_block(block: &Value) -> bool {
    block.get("managed_by").and_then(|v| v.as_str()) == Some("project_parser")
}

/// Extract user-added content from an existing capsule to preserve during re-indexing.
pub fn extract_user_state(
    existing: &Capsule,
    project_name: &str,
) -> (Vec<Value>, Vec<Attachment>, Vec<String>, Option<String>, Option<String>) {
    let user_blocks: Vec<Value> = existing
        .blocks
        .iter()
        .filter(|b| !is_system_block(b))
        .cloned()
        .collect();

    let user_attachments = existing.attachments.clone();

    let default_tags: HashSet<String> = [
        "project".to_string(),
        slugify(project_name),
        "module".to_string(),
        "subsystem".to_string(),
        "overview".to_string(),
        "tasks".to_string(),
        "docs".to_string(),
    ]
    .into_iter()
    .collect();

    let user_tags: Vec<String> = existing
        .tags
        .iter()
        .filter(|t| !default_tags.contains(*t))
        .cloned()
        .collect();

    let custom_status = if existing.status != "Active" {
        Some(existing.status.clone())
    } else {
        None
    };

    let custom_parent = existing.parent_id.clone();

    (user_blocks, user_attachments, user_tags, custom_status, custom_parent)
}

/// Write a generated capsule to vault, non-destructively preserving existing user blocks,
/// attachments (e.g. whiteboard canvas state), custom tags, and custom page status.
/// Handles migration from legacy flat file names to organized subdirectories.
pub fn write_capsule_preserving(
    vault_dir: &Path,
    rel_file: &str,
    mut new_capsule: Capsule,
    project_name: &str,
) -> Result<bool, String> {
    let target = vault_dir.join(rel_file);
    let old_flat_file = Path::new(rel_file)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(rel_file);
    let old_flat_path = vault_dir.join(old_flat_file);

    let mut is_new = true;

    // Check if capsule already exists either at structured path or old flat path
    let existing_source = if target.is_file() {
        Some(target.clone())
    } else if old_flat_path.is_file() {
        Some(old_flat_path.clone())
    } else {
        None
    };

    if let Some(src) = existing_source {
        if let Ok(raw) = fs::read_to_string(&src) {
            let existing_capsule = parse_eml(&raw, rel_file, 0);
            let (user_blocks, user_attachments, user_tags, custom_status, _custom_parent) =
                extract_user_state(&existing_capsule, project_name);

            if !user_blocks.is_empty() {
                new_capsule.blocks.extend(user_blocks);
            }
            if !user_attachments.is_empty() {
                for att in user_attachments {
                    if !new_capsule.attachments.iter().any(|a| a.name == att.name) {
                        new_capsule.attachments.push(att);
                    }
                }
            }
            for tag in user_tags {
                if !new_capsule.tags.contains(&tag) {
                    new_capsule.tags.push(tag);
                }
            }
            if let Some(status) = custom_status {
                new_capsule.status = status;
            }
            is_new = false;
        }

        // If old flat file exists and is in a different location, clean it up
        if src == old_flat_path && old_flat_path != target {
            let _ = fs::remove_file(&old_flat_path);
        }
    }

    if let Some(parent) = target.parent() {
        let _ = fs::create_dir_all(parent);
    }

    new_capsule.file_name = rel_file.to_string();
    let raw_eml = assemble_eml(&new_capsule);
    fs::write(&target, raw_eml)
        .map_err(|e| format!("Failed to write {}: {e}", target.display()))?;

    Ok(is_new)
}

fn slugify(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

fn root_subject(project_name: &str) -> String {
    format!("📦 {} (Overview)", project_name)
}

fn subsystem_subject(project_name: &str, subsystem: &str) -> String {
    format!("📁 {} / {}", project_name, subsystem)
}

fn doc_subject(project_name: &str, doc_title: &str) -> String {
    format!("📚 {} / {}", project_name, doc_title)
}

fn module_subject(project_name: &str, rel_path: &str) -> String {
    format!("📄 {} / {}", project_name, rel_path)
}

fn tasks_subject(project_name: &str) -> String {
    format!("📋 {} / Tasks", project_name)
}

/// Construct root project overview capsule.
fn build_root_capsule(
    project_name: &str,
    project_kind: &ProjectKind,
    root_path: &Path,
    message_id: &str,
    subsystems: &std::collections::BTreeMap<String, Vec<SubsystemModuleRef>>,
    docs: &[ParsedDoc],
    todos_count: usize,
) -> Capsule {
    let now = Utc::now();
    let mut blocks = Vec::new();

    // 1. Title Heading
    blocks.push(json!({
        "id": new_block_id("heading"),
        "type": "heading",
        "managed_by": "project_parser",
        "data": {
            "level": 1,
            "text": format!("{} Project: {}", project_kind.icon(), project_name)
        }
    }));

    // 2. Callout Info
    let total_modules: usize = subsystems.values().map(|v| v.len()).sum();
    let info_text = format!(
        "Ecosystem: {}\nRoot path: {}\nSubsystems: {}\nModules indexed: {}\nDocumentation guides: {}\nPending code tasks: {}",
        project_kind.display_name(),
        root_path.display(),
        subsystems.len(),
        total_modules,
        docs.len(),
        todos_count
    );
    blocks.push(json!({
        "id": new_block_id("callout"),
        "type": "callout",
        "managed_by": "project_parser",
        "data": {
            "icon": "info",
            "text": info_text
        }
    }));

    // 3. Subsystem map
    blocks.push(json!({
        "id": new_block_id("heading"),
        "type": "heading",
        "managed_by": "project_parser",
        "data": {
            "level": 2,
            "text": "Subsystems Architecture".to_string()
        }
    }));

    let mut map_md = String::from("Architectural layers & subsystems in this project:\n\n");
    for (sub_name, mods) in subsystems {
        map_md.push_str(&format!(
            "- [[{}]] ({} module{})\n",
            subsystem_subject(project_name, sub_name),
            mods.len(),
            if mods.len() == 1 { "" } else { "s" }
        ));
    }
    blocks.push(json!({
        "id": new_block_id("paragraph"),
        "type": "paragraph",
        "managed_by": "project_parser",
        "data": { "text": map_md }
    }));

    // 4. Documentation guides if any
    if !docs.is_empty() {
        blocks.push(json!({
            "id": new_block_id("heading"),
            "type": "heading",
            "managed_by": "project_parser",
            "data": {
                "level": 2,
                "text": "Project Documentation".to_string()
            }
        }));

        let mut docs_md = String::from("Indexed project documentation & guides:\n\n");
        for d in docs {
            docs_md.push_str(&format!("- [[{}]] (`{}`)\n", doc_subject(project_name, &d.title), d.rel_path));
        }
        blocks.push(json!({
            "id": new_block_id("paragraph"),
            "type": "paragraph",
            "managed_by": "project_parser",
            "data": { "text": docs_md }
        }));
    }

    // 5. Tasks link if any
    if todos_count > 0 {
        blocks.push(json!({
            "id": new_block_id("paragraph"),
            "type": "paragraph",
            "managed_by": "project_parser",
            "data": {
                "text": format!("📋 Found **{}** tasks in code comments. See: [[{} | Codebase Tasks & TODOs]]", todos_count, tasks_subject(project_name))
            }
        }));
    }

    Capsule {
        message_id: message_id.to_string(),
        subject: root_subject(project_name),
        from: "CapsuleCraft Code Ingest <system@capsulecraft.local>".to_string(),
        to: "Vault <vault@capsuleos.local>".to_string(),
        date: now.to_rfc2822(),
        timestamp: now.timestamp_millis(),
        in_reply_to: None,
        parent_id: None,
        references: Vec::new(),
        tags: vec!["project".to_string(), slugify(project_name), "overview".to_string()],
        status: "Active".to_string(),
        file_name: "index.eml".to_string(),
        blocks,
        attachments: Vec::new(),
        native: true,
        parse_note: None,
        plain_body: None,
        html_body: None,
        raw_eml: None,
    }
}

/// Construct subsystem capsule.
fn build_subsystem_capsule(
    project_name: &str,
    root_id: &str,
    subsystem_name: &str,
    subsystem_msg_id: &str,
    modules: &[SubsystemModuleRef],
) -> Capsule {
    let now = Utc::now();
    let mut blocks = Vec::new();

    blocks.push(json!({
        "id": new_block_id("heading"),
        "type": "heading",
        "managed_by": "project_parser",
        "data": {
            "level": 1,
            "text": format!("📁 Subsystem: {}", subsystem_name)
        }
    }));

    let info_text = format!(
        "Part of [[{} | 📦 {} (Overview)]]\nContains {} source module(s).",
        root_subject(project_name),
        project_name,
        modules.len()
    );
    blocks.push(json!({
        "id": new_block_id("callout"),
        "type": "callout",
        "managed_by": "project_parser",
        "data": {
            "icon": "folder",
            "text": info_text
        }
    }));

    blocks.push(json!({
        "id": new_block_id("heading"),
        "type": "heading",
        "managed_by": "project_parser",
        "data": {
            "level": 2,
            "text": "Modules in this Subsystem".to_string()
        }
    }));

    let mut list_md = String::new();
    for m in modules {
        let desc = if let Some(peek) = &m.doc_preview {
            if !peek.is_empty() { format!(" — *{peek}*") } else { String::new() }
        } else {
            String::new()
        };
        list_md.push_str(&format!("- [[{}]] (`{}`){desc}\n", module_subject(project_name, &m.rel_path), m.rel_path));
    }
    blocks.push(json!({
        "id": new_block_id("paragraph"),
        "type": "paragraph",
        "managed_by": "project_parser",
        "data": { "text": list_md }
    }));

    let safe_slug = slugify(subsystem_name);
    Capsule {
        message_id: subsystem_msg_id.to_string(),
        subject: subsystem_subject(project_name, subsystem_name),
        from: "CapsuleCraft Code Ingest <system@capsulecraft.local>".to_string(),
        to: "Vault <vault@capsuleos.local>".to_string(),
        date: now.to_rfc2822(),
        timestamp: now.timestamp_millis(),
        in_reply_to: Some(root_id.to_string()),
        parent_id: Some(root_id.to_string()),
        references: vec![root_id.to_string()],
        tags: vec!["project".to_string(), slugify(project_name), "subsystem".to_string()],
        status: "Active".to_string(),
        file_name: format!("subsystems/subsystem_{safe_slug}.eml"),
        blocks,
        attachments: Vec::new(),
        native: true,
        parse_note: None,
        plain_body: None,
        html_body: None,
        raw_eml: None,
    }
}

/// Construct documentation capsule.
fn build_doc_capsule(
    project_name: &str,
    root_id: &str,
    doc: &ParsedDoc,
) -> Capsule {
    let now = Utc::now();
    let mut blocks = Vec::new();

    let meta = format!(
        "Documentation file: `{}`\nBelongs to project: [[{} | 📦 {} (Overview)]]",
        doc.rel_path,
        root_subject(project_name),
        project_name
    );
    blocks.push(json!({
        "id": new_block_id("callout"),
        "type": "callout",
        "managed_by": "project_parser",
        "data": {
            "icon": "book",
            "text": meta
        }
    }));

    blocks.extend(doc.blocks.clone());

    let safe_slug = slugify(&doc.rel_path);
    Capsule {
        message_id: format!("<doc-{}-{}@capsuleos.local>", slugify(project_name), safe_slug),
        subject: doc_subject(project_name, &doc.title),
        from: "CapsuleCraft Code Ingest <system@capsulecraft.local>".to_string(),
        to: "Vault <vault@capsuleos.local>".to_string(),
        date: now.to_rfc2822(),
        timestamp: now.timestamp_millis(),
        in_reply_to: Some(root_id.to_string()),
        parent_id: Some(root_id.to_string()),
        references: vec![root_id.to_string()],
        tags: vec!["project".to_string(), slugify(project_name), "docs".to_string()],
        status: "Active".to_string(),
        file_name: format!("docs/doc_{safe_slug}.eml"),
        blocks,
        attachments: Vec::new(),
        native: true,
        parse_note: None,
        plain_body: None,
        html_body: None,
        raw_eml: None,
    }
}

/// Construct aggregated tasks checklist capsule.
fn build_tasks_capsule(project_name: &str, parent_id: &str, todos: &[ExtractedTodo]) -> Capsule {
    let now = Utc::now();
    let mut blocks = Vec::new();

    blocks.push(json!({
        "id": new_block_id("heading"),
        "type": "heading",
        "managed_by": "project_parser",
        "data": {
            "level": 1,
            "text": format!("📋 Codebase Tasks & TODOs — {}", project_name)
        }
    }));

    blocks.push(json!({
        "id": new_block_id("callout"),
        "type": "callout",
        "managed_by": "project_parser",
        "data": {
            "icon": "tasks",
            "text": format!("Discovered {} action items extracted from code comments.", todos.len())
        }
    }));

    // Build tasks block
    let task_items: Vec<Value> = todos
        .iter()
        .map(|t| {
            json!({
                "text": format!("{}:{} [{}] {}", t.file_path, t.line, t.kind, t.text),
                "done": false
            })
        })
        .collect();

    blocks.push(json!({
        "id": new_block_id("tasks"),
        "type": "tasks",
        "managed_by": "project_parser",
        "data": { "items": task_items }
    }));

    Capsule {
        message_id: format!("<project-tasks-{}@capsuleos.local>", slugify(project_name)),
        subject: tasks_subject(project_name),
        from: "CapsuleCraft Code Ingest <system@capsulecraft.local>".to_string(),
        to: "Vault <vault@capsuleos.local>".to_string(),
        date: now.to_rfc2822(),
        timestamp: now.timestamp_millis(),
        in_reply_to: Some(parent_id.to_string()),
        parent_id: Some(parent_id.to_string()),
        references: vec![parent_id.to_string()],
        tags: vec!["project".to_string(), slugify(project_name), "tasks".to_string()],
        status: "Active".to_string(),
        file_name: "tasks_todo.eml".to_string(),
        blocks,
        attachments: Vec::new(),
        native: true,
        parse_note: None,
        plain_body: None,
        html_body: None,
        raw_eml: None,
    }
}

/// Construct module capsule.
fn build_module_capsule(
    project_name: &str,
    parent_id: &str,
    root_id: &str,
    module: &ParsedModule,
) -> Capsule {
    let now = Utc::now();
    let mut blocks = Vec::new();

    blocks.push(json!({
        "id": new_block_id("heading"),
        "type": "heading",
        "managed_by": "project_parser",
        "data": {
            "level": 1,
            "text": format!("📄 {}", module.file_name)
        }
    }));

    let meta = format!(
        "Path: `{}` · {} lines of code\nBelongs to project: [[{} | 📦 {} (Overview)]]",
        module.rel_path,
        module.line_count,
        root_subject(project_name),
        project_name
    );
    blocks.push(json!({
        "id": new_block_id("paragraph"),
        "type": "paragraph",
        "managed_by": "project_parser",
        "data": { "text": meta }
    }));

    // Header docs
    if let Some(doc) = &module.doc_comment {
        blocks.push(json!({
            "id": new_block_id("callout"),
            "type": "callout",
            "managed_by": "project_parser",
            "data": {
                "icon": "info",
                "text": doc.clone()
            }
        }));
    }

    // Public items contract
    if !module.public_items.is_empty() {
        blocks.push(json!({
            "id": new_block_id("heading"),
            "type": "heading",
            "managed_by": "project_parser",
            "data": {
                "level": 2,
                "text": "Public Interface".to_string()
            }
        }));

        let code_snippet = module.public_items.join("\n");
        blocks.push(json!({
            "id": new_block_id("code"),
            "type": "code",
            "managed_by": "project_parser",
            "data": {
                "language": detect_lang(&module.file_name),
                "code": code_snippet
            }
        }));
    }

    // Dependencies & Imports
    if !module.imports.is_empty() {
        blocks.push(json!({
            "id": new_block_id("heading"),
            "type": "heading",
            "managed_by": "project_parser",
            "data": {
                "level": 2,
                "text": "Internal Dependencies".to_string()
            }
        }));

        let mut dep_text = String::new();
        for imp in module.imports.iter().take(15) {
            dep_text.push_str(&format!("- `{imp}`\n"));
        }
        blocks.push(json!({
            "id": new_block_id("paragraph"),
            "type": "paragraph",
            "managed_by": "project_parser",
            "data": { "text": dep_text }
        }));
    }

    // Module tasks if any
    if !module.todos.is_empty() {
        blocks.push(json!({
            "id": new_block_id("heading"),
            "type": "heading",
            "managed_by": "project_parser",
            "data": {
                "level": 2,
                "text": "Module Tasks".to_string()
            }
        }));

        let items: Vec<Value> = module
            .todos
            .iter()
            .map(|t| {
                json!({
                    "text": format!("Line {}: [{}] {}", t.line, t.kind, t.text),
                    "done": false
                })
            })
            .collect();

        blocks.push(json!({
            "id": new_block_id("tasks"),
            "type": "tasks",
            "managed_by": "project_parser",
            "data": { "items": items }
        }));
    }

    let safe_slug = slugify(&module.rel_path);
    Capsule {
        message_id: format!("<module-{}@capsuleos.local>", safe_slug),
        subject: module_subject(project_name, &module.rel_path),
        from: "CapsuleCraft Code Ingest <system@capsulecraft.local>".to_string(),
        to: "Vault <vault@capsuleos.local>".to_string(),
        date: now.to_rfc2822(),
        timestamp: now.timestamp_millis(),
        in_reply_to: Some(parent_id.to_string()),
        parent_id: Some(parent_id.to_string()),
        references: vec![root_id.to_string(), parent_id.to_string()],
        tags: vec!["project".to_string(), slugify(project_name), "module".to_string()],
        status: "Active".to_string(),
        file_name: format!("modules/module_{safe_slug}.eml"),
        blocks,
        attachments: Vec::new(),
        native: true,
        parse_note: None,
        plain_body: None,
        html_body: None,
        raw_eml: None,
    }
}

fn detect_lang(file_name: &str) -> &'static str {
    let ext = file_name.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "dart" => "dart",
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        "go" => "go",
        "cs" => "csharp",
        "java" => "java",
        _ => "text",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("capsule-test-{prefix}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_detect_project_meta_rust() {
        let dir = temp_dir("rust-meta");
        let cargo_toml = r#"
[package]
name = "awesome-engine"
version = "0.1.0"
edition = "2021"
"#;
        fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();

        let (name, kind) = detect_project_meta(&dir);
        assert_eq!(name, "awesome-engine");
        assert_eq!(kind, ProjectKind::Rust);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_detect_project_meta_node() {
        let dir = temp_dir("node-meta");
        let pkg_json = r#"{"name": "frontend-app", "version": "1.0.0"}"#;
        fs::write(dir.join("package.json"), pkg_json).unwrap();

        let (name, kind) = detect_project_meta(&dir);
        assert_eq!(name, "frontend-app");
        assert_eq!(kind, ProjectKind::Node);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_detect_project_meta_flutter() {
        let dir = temp_dir("flutter-meta");
        let pubspec = r#"
name: survival_calc
description: "A survival calculator."
version: 1.0.0
"#;
        fs::write(dir.join("pubspec.yaml"), pubspec).unwrap();

        let (name, kind) = detect_project_meta(&dir);
        assert_eq!(name, "survival_calc");
        assert_eq!(kind, ProjectKind::Flutter);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_source_file_rust() {
        let code = r#"//! Audio engine thread controller.
//! Dedicated thread for cpal output streams.

use crate::theme;
use crate::i18n;

pub struct AudioPlayer {
    pub volume: f32,
}

impl AudioPlayer {
    pub fn play(&self) {}
}

// TODO: add crossfade support
// FIXME: handle device disconnection gracefully
fn private_helper() {}
"#;

        let module = parse_source_file("src/audio.rs", code);
        assert_eq!(module.file_name, "audio.rs");
        assert_eq!(module.rel_path, "src/audio.rs");
        assert!(module.doc_comment.is_some());
        let doc = module.doc_comment.unwrap();
        assert!(doc.contains("Audio engine thread controller"));
        assert!(doc.contains("Dedicated thread"));

        assert_eq!(module.imports.len(), 2);
        assert_eq!(module.todos.len(), 2);
        assert_eq!(module.todos[0].kind, "TODO");
        assert_eq!(module.todos[0].text, "add crossfade support");
        assert_eq!(module.todos[1].kind, "FIXME");
        assert_eq!(module.todos[1].text, "handle device disconnection gracefully");

        assert!(module
            .public_items
            .iter()
            .any(|i| i.contains("pub struct AudioPlayer")));
        assert!(module
            .public_items
            .iter()
            .any(|i| i.contains("pub fn play")));
    }

    #[test]
    fn test_ingest_project_creates_capsules() {
        let project = temp_dir("ingest-proj");
        fs::write(
            project.join("Cargo.toml"),
            r#"[package]
name = "micro-service"
version = "0.2.0"
"#,
        )
        .unwrap();

        let src = project.join("src");
        fs::create_dir_all(&src).unwrap();

        fs::write(
            src.join("main.rs"),
            r#"//! Main server entry point.
pub fn start_server() {}
// TODO: add graceful shutdown
"#,
        )
        .unwrap();

        let vault_dir = project.join(".capsule");
        let summary = ingest_project(&project, &vault_dir).unwrap();

        assert_eq!(summary.project_name, "micro-service");
        assert_eq!(summary.project_kind, ProjectKind::Rust);
        assert_eq!(summary.modules_count, 1);
        assert_eq!(summary.docs_count, 0);
        assert_eq!(summary.todos_count, 1);
        assert!(summary.capsules_created >= 4); // index + tasks + subsystem + module

        assert!(vault_dir.join("index.eml").is_file());
        assert!(vault_dir.join("tasks_todo.eml").is_file());
        assert!(vault_dir.join("subsystems/subsystem_src.eml").is_file());
        assert!(vault_dir.join("modules/module_src_main_rs.eml").is_file());
        assert!(vault_dir.join(".index_state.json").is_file());

        // Agent skills created
        assert!(project.join(".gemini/skills/capsulecraft/SKILL.md").is_file());
        assert!(project.join(".agents/skills/capsulecraft/SKILL.md").is_file());

        let index_eml = fs::read_to_string(vault_dir.join("index.eml")).unwrap();
        assert!(index_eml.contains("micro-service"));
        assert!(index_eml.contains("[[📁 micro-service / src]]"));

        // Stage 2 & Stage 1 test: add user note to module_src_main_rs.eml and re-ingest
        let mod_file = vault_dir.join("modules/module_src_main_rs.eml");
        let mut raw_mod = fs::read_to_string(&mod_file).unwrap();
        let mut parsed = parse_eml(&raw_mod, "modules/module_src_main_rs.eml", 0);
        parsed.blocks.push(json!({
            "id": new_block_id("paragraph"),
            "type": "paragraph",
            "data": { "text": "User annotation: critical path." }
        }));
        parsed.tags.push("custom-tag".to_string());
        fs::write(&mod_file, assemble_eml(&parsed)).unwrap();

        // Re-ingest
        let summary2 = ingest_project(&project, &vault_dir).unwrap();
        assert_eq!(summary2.capsules_removed, 0);

        // Verify user note preserved
        let re_read = fs::read_to_string(&mod_file).unwrap();
        assert!(re_read.contains("User annotation: critical path."));
        let re_parsed = parse_eml(&re_read, "modules/module_src_main_rs.eml", 0);
        assert!(re_parsed.tags.contains(&"custom-tag".to_string()));

        let _ = fs::remove_dir_all(&project);
    }

    #[test]
    fn test_multiline_signature_parsing() {
        let code = r#"
pub async fn calculate_metrics(
    context: &Context,
    threshold: f64,
) -> Result<Summary, Error> {
    Ok(Summary::default())
}
"#;
        let module = parse_source_file("src/calc.rs", code);
        assert_eq!(module.public_items.len(), 1);
        assert!(module.public_items[0].contains("pub async fn calculate_metrics"));
        assert!(module.public_items[0].contains("threshold: f64"));
        assert!(module.public_items[0].contains("Result<Summary, Error>"));
    }

    #[test]
    fn test_classify_subsystem() {
        assert_eq!(classify_subsystem("src/main.rs"), "src");
        assert_eq!(classify_subsystem("crates/companion_app/src/main.rs"), "crates/companion_app");
        assert_eq!(classify_subsystem("packages/ui/src/index.ts"), "packages/ui");
        assert_eq!(classify_subsystem("lib/features/auth/login.dart"), "lib/features/auth");
        assert_eq!(classify_subsystem("root_file.rs"), "root");
    }

    #[test]
    fn test_parse_markdown_file_and_blocks() {
        let md = r#"# Architecture Overview

This is an introductory paragraph explaining the system.

> Note: this is a vital callout.

```rust
fn example() -> bool { true }
```

## Section 2
Further details here.
"#;
        let doc = parse_markdown_file("docs/architecture.md", md, "my-app");
        assert_eq!(doc.rel_path, "docs/architecture.md");
        assert_eq!(doc.title, "Architecture Overview");
        assert!(doc.blocks.len() >= 4);
        assert_eq!(doc.blocks[0]["type"], "heading");
        assert_eq!(doc.blocks[0]["data"]["text"], "Architecture Overview");
    }
}
