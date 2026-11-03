//! Autonomous agent skill scaffolding and instruction marker injection.
//!
//! Generates `.agents/skills/capsulecraft/SKILL.md` (cross-platform open standard
//! for Claude, Cursor, Codex, OpenCode) and `.gemini/skills/capsulecraft/SKILL.md`
//! (Antigravity). Also idempotently attaches reference markers to existing
//! `AGENTS.md`, `CLAUDE.md`, or `GEMINI.md` files without overwriting user rules.

use std::fs;
use std::path::Path;

const MARKER_START: &str = "<!-- capsulecraft-start -->";
const MARKER_END: &str = "<!-- capsulecraft-end -->";

/// Content template for the standard Agent Skill.
fn skill_template(project_name: &str) -> String {
    format!(
r#"---
name: capsulecraft
description: Read, query, and update the CapsuleCraft project knowledge base and tasks for {project_name}.
---

# CapsuleCraft Knowledge Base Skill for {project_name}

This project is indexed into a local CapsuleCraft knowledge base located in `.capsule/`.
Every module, subsystem, architecture decision, task, and design document is stored
as an honest, self-contained RFC 5322 (`.eml`) capsule with full provenance.

## When to use this skill
- When exploring the architecture, public APIs, or subsystems of {project_name}.
- When checking technical debt, open tasks, and TODOs (`tasks_todo.eml`).
- When planning new features or cross-module refactorings.
- When saving architectural decisions or agent run artifacts.

## Available MCP Tools
If the CapsuleCraft MCP server is running (`capsulecraft`), use these tools:
- `project_overview`: Retrieve the structural overview of modules, subsystems, and metrics.
- `capsule_search`: Full-text BM25 search across capsules and code comments.
- `capsule_read`: Read a capsule by its Message-ID or file name.
- `capsule_create`: Create a new capsule documenting a feature, RFC, or bug analysis.
- `capsule_tasks`: List and filter codebase tasks and TODOs.

## Direct Capsule Access (Without MCP)
If MCP is not active, you can read the knowledge base directly from the filesystem:
- Root overview: `.capsule/index.eml`
- Subsystems: `.capsule/subsystem_*.eml`
- Module capsules: `.capsule/module_*.eml`
- Tasks: `.capsule/tasks_todo.eml`
"#
    )
}

/// Marker block injected into AGENTS.md / CLAUDE.md / GEMINI.md.
fn agent_marker_block(project_name: &str) -> String {
    format!(
r#"{MARKER_START}
## CapsuleCraft Knowledge Base & MCP Integration
This project `{project_name}` has an active CapsuleCraft knowledge base in `.capsule/`.
- Project Overview: `.capsule/index.eml`
- Tasks & Technical Debt: `.capsule/tasks_todo.eml`
- Subsystems & Architecture: `.capsule/subsystem_*.eml`
- Agent Skill definition: `.agents/skills/capsulecraft/SKILL.md`

Agents can query project architecture and tasks using CapsuleCraft MCP tools (`project_overview`, `capsule_search`, `capsule_read`, `capsule_tasks`).
{MARKER_END}"#
    )
}

/// Scaffold agent skill and inject markers into existing agent instructions.
pub fn scaffold_agent_skill(project_root: &Path, project_name: &str) -> Result<(), String> {
    let skill_content = skill_template(project_name);

    // 1. Write open-standard skill: .agents/skills/capsulecraft/SKILL.md
    let agents_skill_dir = project_root.join(".agents").join("skills").join("capsulecraft");
    fs::create_dir_all(&agents_skill_dir)
        .map_err(|e| format!("Cannot create .agents skill dir: {e}"))?;
    fs::write(agents_skill_dir.join("SKILL.md"), &skill_content)
        .map_err(|e| format!("Cannot write .agents/skills/capsulecraft/SKILL.md: {e}"))?;

    // 2. Write Antigravity skill: .gemini/skills/capsulecraft/SKILL.md
    let gemini_skill_dir = project_root.join(".gemini").join("skills").join("capsulecraft");
    fs::create_dir_all(&gemini_skill_dir)
        .map_err(|e| format!("Cannot create .gemini skill dir: {e}"))?;
    fs::write(gemini_skill_dir.join("SKILL.md"), &skill_content)
        .map_err(|e| format!("Cannot write .gemini/skills/capsulecraft/SKILL.md: {e}"))?;

    // 3. Inject markers idempotently into existing agent instruction files
    let candidate_files = [
        project_root.join("AGENTS.md"),
        project_root.join("GEMINI.md"),
        project_root.join("CLAUDE.md"),
    ];

    let marker_text = agent_marker_block(project_name);
    let mut updated_any = false;

    for file_path in &candidate_files {
        if file_path.is_file() {
            if let Ok(existing) = fs::read_to_string(file_path) {
                let updated = inject_or_replace_marker(&existing, &marker_text);
                let _ = fs::write(file_path, updated);
                updated_any = true;
            }
        }
    }

    // If none existed, create a clean AGENTS.md
    if !updated_any {
        let agents_md = project_root.join("AGENTS.md");
        let initial_content = format!(
            "# Agent Guidelines for {project_name}\n\n{}\n",
            marker_text
        );
        let _ = fs::write(agents_md, initial_content);
    }

    Ok(())
}

/// Replace text between markers if already present, or append to end.
fn inject_or_replace_marker(content: &str, marker_block: &str) -> String {
    if let (Some(start_pos), Some(end_pos)) = (content.find(MARKER_START), content.find(MARKER_END)) {
        if start_pos < end_pos {
            let full_end = end_pos + MARKER_END.len();
            let mut res = String::new();
            res.push_str(&content[..start_pos]);
            res.push_str(marker_block);
            res.push_str(&content[full_end..]);
            return res;
        }
    }

    let mut res = content.trim_end().to_string();
    res.push_str("\n\n");
    res.push_str(marker_block);
    res.push('\n');
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_inject_or_replace_marker() {
        let initial = "# My Project\n\nSome custom agent rules.\n";
        let marker = "<!-- capsulecraft-start -->\nCapsuleCraft integration\n<!-- capsulecraft-end -->";

        // First injection: appends
        let res1 = inject_or_replace_marker(initial, marker);
        assert!(res1.contains("Some custom agent rules."));
        assert!(res1.contains("CapsuleCraft integration"));

        // Second injection: updates existing marker without duplicating
        let updated_marker = "<!-- capsulecraft-start -->\nUpdated info\n<!-- capsulecraft-end -->";
        let res2 = inject_or_replace_marker(&res1, updated_marker);
        assert!(res2.contains("Some custom agent rules."));
        assert!(res2.contains("Updated info"));
        assert!(!res2.contains("CapsuleCraft integration"));
        assert_eq!(res2.matches(MARKER_START).count(), 1);
        assert_eq!(res2.matches(MARKER_END).count(), 1);
    }

    #[test]
    fn test_scaffold_agent_skill() {
        let temp_dir = env::temp_dir().join(format!("cc_skill_test_{}", rand::random::<u32>()));
        fs::create_dir_all(&temp_dir).unwrap();

        // Put an existing CLAUDE.md
        fs::write(temp_dir.join("CLAUDE.md"), "# Claude instructions\n").unwrap();

        let res = scaffold_agent_skill(&temp_dir, "test-app");
        assert!(res.is_ok());

        assert!(temp_dir.join(".agents/skills/capsulecraft/SKILL.md").is_file());
        assert!(temp_dir.join(".gemini/skills/capsulecraft/SKILL.md").is_file());

        let claude_md = fs::read_to_string(temp_dir.join("CLAUDE.md")).unwrap();
        assert!(claude_md.contains("# Claude instructions"));
        assert!(claude_md.contains("CapsuleCraft Knowledge Base & MCP Integration"));

        let _ = fs::remove_dir_all(temp_dir);
    }
}
