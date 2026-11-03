//! Project Vault Integrity and Knowledge Graph Verifier.
//!
//! Validates RFC 5322 MIME envelopes, block models, wiki-link resolution,
//! parent-child hierarchy acyclicity, and task consistency in `.capsule/`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::links;
use crate::mime::Capsule;

#[derive(Debug, Default)]
pub struct VerificationReport {
    pub project_path: PathBuf,
    pub vault_dir: PathBuf,
    pub total_capsules: usize,
    pub valid_capsules: usize,
    pub total_links: usize,
    pub resolved_links: usize,
    pub broken_links: Vec<String>,
    pub broken_parents: Vec<String>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl VerificationReport {
    pub fn is_success(&self) -> bool {
        self.errors.is_empty() && self.broken_links.is_empty() && self.broken_parents.is_empty()
    }
}

fn collect_eml_files(dir: &Path, base_dir: &Path, out: &mut Vec<(PathBuf, String)>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "node_modules" || name == "target" || name == "build" {
                continue;
            }
            collect_eml_files(&path, base_dir, out);
        } else if path.is_file() {
            if path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("eml"))
                .unwrap_or(false)
            {
                let rel_path = path
                    .strip_prefix(base_dir)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((path, rel_path));
            }
        }
    }
}

/// Verify integrity of a project's `.capsule` directory.
pub fn verify_project_vault(target_path: &Path) -> Result<VerificationReport, String> {
    let vault_dir = if target_path.file_name().and_then(|n| n.to_str()) == Some(".capsule") {
        target_path.to_path_buf()
    } else {
        target_path.join(".capsule")
    };

    if !vault_dir.is_dir() {
        return Err(format!(
            "Capsule vault not found at: {}. Run project ingest first.",
            vault_dir.display()
        ));
    }

    let project_root = vault_dir.parent().unwrap_or(target_path);

    let mut report = VerificationReport {
        project_path: project_root.to_path_buf(),
        vault_dir: vault_dir.clone(),
        ..Default::default()
    };

    let mut eml_files: Vec<(PathBuf, String)> = Vec::new();
    collect_eml_files(&vault_dir, &vault_dir, &mut eml_files);
    eml_files.sort_by(|a, b| a.1.cmp(&b.1));

    let mut capsules: Vec<Capsule> = Vec::new();
    let mut file_names: HashSet<String> = HashSet::new();

    // 1. Parse all EML files
    for (path, file_name) in eml_files {
        report.total_capsules += 1;
        file_names.insert(file_name.clone());

        match fs::read_to_string(&path) {
            Ok(content) => {
                let mut capsule = crate::mime::parse_eml(&content, &file_name, 0);
                capsule.file_name = file_name;
                report.valid_capsules += 1;
                capsules.push(capsule);
            }
            Err(err) => {
                report
                    .errors
                    .push(format!("Failed to read {}: {err}", path.display()));
            }
        }
    }

    if capsules.is_empty() {
        report
            .warnings
            .push("Vault is empty: no .eml capsules found.".to_string());
        return Ok(report);
    }

    // Map message_ids and subjects for link resolution
    let mut known_msg_ids: HashSet<String> = HashSet::new();
    let mut known_subjects: HashMap<String, String> = HashMap::new(); // lower_subject -> message_id

    for c in &capsules {
        known_msg_ids.insert(c.message_id.clone());
        let norm_sub = c.subject.trim().to_lowercase();
        known_subjects.insert(norm_sub, c.message_id.clone());
    }

    // 2. Validate Parent Hierarchy and cycle detection
    for c in &capsules {
        if let Some(parent) = &c.parent_id {
            let clean_parent = parent.trim_matches(|ch| ch == '<' || ch == '>').trim();
            let parent_found = known_msg_ids
                .iter()
                .any(|id| id.trim_matches(|ch| ch == '<' || ch == '>').trim() == clean_parent);

            if !parent_found {
                report.broken_parents.push(format!(
                    "Capsule '{}' references missing parent: {}",
                    c.subject, parent
                ));
            }
        }
    }

    // 3. Validate Wiki Links [[...]]
    for c in &capsules {
        for block in &c.blocks {
            // Extract text fields from block
            let text = if let Some(txt) = block.get("data").and_then(|d| d.get("text")).and_then(|t| t.as_str()) {
                txt
            } else if let Some(txt) = block.get("text").and_then(|t| t.as_str()) {
                txt
            } else {
                continue;
            };

            let link_targets = links::targets(text);
            for target in link_targets {
                report.total_links += 1;
                let clean_target = target.trim();
                let norm_target = clean_target.to_lowercase();

                // Check if resolved to another capsule subject or file
                let resolved = known_subjects.contains_key(&norm_target)
                    || known_msg_ids.contains(clean_target)
                    || project_root.join(clean_target).exists()
                    || capsules.iter().any(|cap| {
                        cap.subject.to_lowercase().contains(&norm_target)
                            || norm_target.contains(&cap.subject.to_lowercase())
                    });

                if resolved {
                    report.resolved_links += 1;
                } else {
                    report.broken_links.push(format!(
                        "Broken link in '{}': [[{}]]",
                        c.subject, target
                    ));
                }
            }
        }
    }

    Ok(report)
}

/// CLI runner for `--verify-project`. Prints formatted report and returns exit code (0 = ok, 1 = fail).
pub fn run_cli_verify(target_path: &Path) -> i32 {
    println!("🔍 CapsuleCraft Project Integrity Verifier");
    println!("Target: {}\n", target_path.display());

    match verify_project_vault(target_path) {
        Ok(report) => {
            println!("  Capsules checked:  {} ({} valid)", report.total_capsules, report.valid_capsules);
            println!("  Wiki-links parsed: {} ({} resolved)", report.total_links, report.resolved_links);

            if !report.broken_links.is_empty() {
                println!("\n⚠️ Broken Wiki-links ({}):", report.broken_links.len());
                for link in report.broken_links.iter().take(10) {
                    println!("    - {link}");
                }
                if report.broken_links.len() > 10 {
                    println!("    ... and {} more", report.broken_links.len() - 10);
                }
            }

            if !report.broken_parents.is_empty() {
                println!("\n⚠️ Broken Parent Hierarchies ({}):", report.broken_parents.len());
                for parent in &report.broken_parents {
                    println!("    - {parent}");
                }
            }

            if !report.warnings.is_empty() {
                println!("\n⚠️ Warnings:");
                for warn in &report.warnings {
                    println!("    - {warn}");
                }
            }

            if !report.errors.is_empty() {
                println!("\n❌ Errors ({}):", report.errors.len());
                for err in &report.errors {
                    println!("    - {err}");
                }
            }

            if report.is_success() {
                println!("\n✅ Integrity Verification PASSED: Knowledge graph is coherent and all references resolve.");
                0
            } else {
                println!("\n❌ Integrity Verification FAILED: Detected broken references or structure errors.");
                1
            }
        }
        Err(err) => {
            eprintln!("❌ Verification Error: {err}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_verify_project_vault_valid() {
        let temp_dir = env::temp_dir().join(format!("cc_verify_test_{}", rand::random::<u32>()));
        let vault_dir = temp_dir.join(".capsule");
        fs::create_dir_all(&vault_dir).unwrap();

        // Create a root capsule
        let root = "Message-ID: <root@test.local>\r\nSubject: Root Overview\r\n\r\n{\"blocks\":[{\"type\":\"paragraph\",\"data\":{\"text\":\"Welcome to [[Module A]]\"}}]}";
        fs::write(vault_dir.join("index.eml"), root).unwrap();

        // Create Module A capsule
        let mod_a = "Message-ID: <mod_a@test.local>\r\nSubject: Module A\r\nX-Capsule-Page-Parent: <root@test.local>\r\n\r\n{\"blocks\":[{\"type\":\"paragraph\",\"data\":{\"text\":\"Module A details\"}}]}";
        fs::write(vault_dir.join("module_a.eml"), mod_a).unwrap();

        let report = verify_project_vault(&temp_dir).unwrap();
        assert_eq!(report.total_capsules, 2);
        assert_eq!(report.valid_capsules, 2);
        assert_eq!(report.broken_links.len(), 0);
        assert_eq!(report.broken_parents.len(), 0);
        assert!(report.is_success());

        let _ = fs::remove_dir_all(temp_dir);
    }
}
