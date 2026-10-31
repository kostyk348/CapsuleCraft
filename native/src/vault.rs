//! Vault folder scanning, `.eml` persistence and the page tree.

use crate::mime::{assemble_eml, parse_eml, Capsule};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Vault {
    pub dir: PathBuf,
    pub capsules: Vec<Capsule>,
    pub load_errors: Vec<String>,
}

impl Vault {
    pub fn load(dir: PathBuf) -> Self {
        let mut vault = Self {
            dir,
            capsules: Vec::new(),
            load_errors: Vec::new(),
        };
        vault.reload();
        vault
    }

    pub fn reload(&mut self) {
        self.capsules.clear();
        self.load_errors.clear();

        if let Err(err) = fs::create_dir_all(&self.dir) {
            self.load_errors.push(format!(
                "Cannot create vault folder {}: {err}",
                self.dir.display()
            ));
            return;
        }

        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(err) => {
                self.load_errors.push(format!(
                    "Cannot read vault folder {}: {err}",
                    self.dir.display()
                ));
                return;
            }
        };

        let mut files: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("eml"))
                    .unwrap_or(false)
            })
            .collect();
        files.sort();

        for path in files {
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("capsule.eml")
                .to_string();
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(err) => {
                    self.load_errors.push(format!("{file_name}: {err}"));
                    continue;
                }
            };
            let raw = String::from_utf8_lossy(&bytes);
            let modified = fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as i64)
                .unwrap_or(0);
            let capsule = parse_eml(&raw, &file_name, modified);
            if let Some(note) = &capsule.parse_note {
                self.load_errors.push(format!("{file_name}: {note}"));
            }
            self.capsules.push(capsule);
        }
    }

    pub fn save(&self, index: usize) -> Result<(), String> {
        let capsule = self
            .capsules
            .get(index)
            .ok_or_else(|| "No page selected".to_string())?;
        if !capsule.native {
            return Err("This .eml is read-only in the native prototype".to_string());
        }
        let raw = assemble_eml(capsule);
        let path = self.dir.join(&capsule.file_name);
        // Small files: a direct write is acceptable for the prototype. The web
        // app remains the atomic-write path used in production.
        fs::write(&path, raw).map_err(|err| format!("Save failed: {err}"))
    }

    pub fn create_page(&mut self, parent: Option<String>) -> Result<usize, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|err| format!("Clock error: {err}"))?;
        let stamp = now.as_nanos();
        let suffix = format!("{:x}", stamp & 0xfffff);

        let mut file_name = format!("capsule_native_{suffix}.eml");
        let mut counter = 1;
        while self.dir.join(&file_name).exists()
            || self
                .capsules
                .iter()
                .any(|capsule| capsule.file_name == file_name)
        {
            file_name = format!("capsule_native_{suffix}_{counter}.eml");
            counter += 1;
        }
        let message_id = format!(
            "<capsule-native-{suffix}-{stamp}-{}@vault.local>",
            self.capsules.len()
        );

        let parent_capsule = parent
            .as_ref()
            .and_then(|id| self.capsules.iter().find(|c| &c.message_id == id));
        let mut capsule = Capsule::blank(message_id, file_name, "Untitled".to_string());
        capsule.parent_id = parent.clone();
        if let Some(parent_capsule) = parent_capsule {
            capsule.tags = parent_capsule.tags.clone();
        }

        self.capsules.push(capsule);
        let index = self.capsules.len() - 1;
        self.save(index)?;
        Ok(index)
    }

    pub fn reparent_page(
        &mut self,
        index: usize,
        new_parent_id: Option<String>,
    ) -> Result<(), String> {
        if index >= self.capsules.len() {
            return Err("Invalid page index".to_string());
        }

        // Cycle check: verify new_parent_id is not the page itself and not a descendant of the page
        if let Some(target_id) = new_parent_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let this_id = &self.capsules[index].message_id;
            if target_id == this_id {
                return Err("Cannot reparent page to itself".to_string());
            }

            let tree = PageTree::build(&self.capsules);
            if let Some(target_idx) = self.capsules.iter().position(|c| c.message_id == target_id) {
                if tree.is_descendant_of(target_idx, index) {
                    return Err(
                        "Cannot reparent page inside its own subtree (cycle detected)".to_string(),
                    );
                }
            }
        }

        let capsule = &mut self.capsules[index];
        capsule.parent_id = new_parent_id.filter(|s| !s.trim().is_empty());
        self.save(index)?;
        Ok(())
    }

    pub fn selected_root(&self) -> Option<usize> {
        PageTree::build(&self.capsules).roots().first().copied()
    }
}

pub fn app_data_dir() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata).join("com.capsuleos.app");
    }
    if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
        return PathBuf::from(data_home).join("com.capsuleos.app");
    }
    if let Ok(home) = std::env::var("HOME") {
        #[cfg(target_os = "macos")]
        {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("com.capsuleos.app");
        }
        #[cfg(not(target_os = "macos"))]
        {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("com.capsuleos.app");
        }
    }
    PathBuf::from("com.capsuleos.app")
}

pub fn active_vault_file() -> PathBuf {
    app_data_dir().join("active-vault.txt")
}

pub fn get_active_vault_dir() -> PathBuf {
    let file = active_vault_file();
    if let Ok(content) = fs::read_to_string(&file) {
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            let path = PathBuf::from(trimmed);
            if path.exists() {
                return path;
            }
        }
    }
    default_vault_dir()
}

pub fn set_active_vault_dir(dir: &std::path::Path) {
    let app_dir = app_data_dir();
    let _ = fs::create_dir_all(&app_dir);
    let _ = fs::write(active_vault_file(), format!("{}\n", dir.display()));
}

pub fn default_vault_dir() -> PathBuf {
    app_data_dir().join("vault")
}

/// Effective page tree. Dangling parents and cycle participants become roots;
/// nothing is hidden just because a hand-edited file is malformed.
pub struct PageTree {
    parents: Vec<Option<usize>>,
    children: BTreeMap<Option<usize>, Vec<usize>>,
}

impl PageTree {
    pub fn build(capsules: &[Capsule]) -> Self {
        let by_id: HashMap<&str, usize> = capsules
            .iter()
            .enumerate()
            .map(|(index, capsule)| (capsule.message_id.as_str(), index))
            .collect();

        let raw_parents: Vec<Option<usize>> = capsules
            .iter()
            .enumerate()
            .map(|(index, capsule)| {
                let id = capsule
                    .parent_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|id| !id.is_empty())?;
                let parent = *by_id.get(id)?;
                (parent != index).then_some(parent)
            })
            .collect();

        let parents: Vec<Option<usize>> = (0..capsules.len())
            .map(|index| effective_parent(index, &raw_parents))
            .collect();

        let mut children: BTreeMap<Option<usize>, Vec<usize>> = BTreeMap::new();
        for (index, parent) in parents.iter().enumerate() {
            children.entry(*parent).or_default().push(index);
        }
        for bucket in children.values_mut() {
            bucket.sort_by(|a, b| {
                let a = &capsules[*a];
                let b = &capsules[*b];
                a.subject
                    .to_lowercase()
                    .cmp(&b.subject.to_lowercase())
                    .then(a.timestamp.cmp(&b.timestamp))
            });
        }

        Self { parents, children }
    }

    pub fn roots(&self) -> &[usize] {
        self.children.get(&None).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn children_of(&self, parent: Option<usize>) -> &[usize] {
        self.children.get(&parent).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn parent_of(&self, index: usize) -> Option<usize> {
        self.parents.get(index).copied().flatten()
    }

    pub fn ancestors(&self, index: usize) -> Vec<usize> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        seen.insert(index);
        let mut current = self.parent_of(index);
        while let Some(parent) = current {
            if !seen.insert(parent) {
                break;
            }
            result.push(parent);
            current = self.parent_of(parent);
        }
        result.reverse();
        result
    }

    /// Returns `true` if `node` is a descendant of `ancestor`.
    pub fn is_descendant_of(&self, node: usize, ancestor: usize) -> bool {
        if node == ancestor {
            return false;
        }
        let mut current = self.parent_of(node);
        let mut seen = HashSet::new();
        seen.insert(node);
        while let Some(parent) = current {
            if parent == ancestor {
                return true;
            }
            if !seen.insert(parent) {
                break;
            }
            current = self.parent_of(parent);
        }
        false
    }
}

fn effective_parent(index: usize, raw_parents: &[Option<usize>]) -> Option<usize> {
    let first = raw_parents.get(index).copied().flatten()?;
    let mut current = Some(first);
    let mut seen = HashSet::new();
    while let Some(parent) = current {
        if parent == index {
            return None;
        }
        if !seen.insert(parent) {
            break;
        }
        current = raw_parents.get(parent).copied().flatten();
    }
    Some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capsule(id: &str, subject: &str, parent: Option<&str>) -> Capsule {
        let mut capsule = Capsule::blank(id.to_string(), format!("{id}.eml"), subject.to_string());
        capsule.parent_id = parent.map(str::to_string);
        capsule
    }

    #[test]
    fn builds_nested_tree_and_promotes_orphans() {
        let capsules = vec![
            capsule("<root>", "Root", None),
            capsule("<a>", "Alpha", Some("<root>")),
            capsule("<b>", "Beta", Some("<root>")),
            capsule("<a1>", "Alpha one", Some("<a>")),
            capsule("<orphan>", "Orphan", Some("<missing>")),
        ];
        let tree = PageTree::build(&capsules);

        assert_eq!(tree.roots(), &[4, 0]);
        assert_eq!(tree.children_of(Some(0)), &[1, 2]);
        assert_eq!(tree.children_of(Some(1)), &[3]);
        assert_eq!(tree.ancestors(3), vec![0, 1]);
    }

    #[test]
    fn breaks_parent_cycles_without_hiding_pages() {
        let capsules = vec![
            capsule("<a>", "A", Some("<b>")),
            capsule("<b>", "B", Some("<a>")),
            capsule("<child>", "Child", Some("<a>")),
        ];
        let tree = PageTree::build(&capsules);

        assert_eq!(tree.roots(), &[0, 1]);
        assert_eq!(tree.children_of(Some(0)), &[2]);
        assert!(tree.parent_of(2).is_some());
    }

    #[test]
    fn test_is_descendant_of() {
        let capsules = vec![
            capsule("<root>", "Root", None),
            capsule("<a>", "Alpha", Some("<root>")),
            capsule("<a1>", "Alpha one", Some("<a>")),
            capsule("<a2>", "Alpha two", Some("<a1>")),
            capsule("<b>", "Beta", Some("<root>")),
        ];
        let tree = PageTree::build(&capsules);

        // a1 is descendant of a (1) and root (0)
        assert!(tree.is_descendant_of(2, 1));
        assert!(tree.is_descendant_of(2, 0));
        assert!(tree.is_descendant_of(3, 1));
        assert!(tree.is_descendant_of(3, 0));

        // root is not descendant of a1
        assert!(!tree.is_descendant_of(0, 2));
        // beta is not descendant of alpha
        assert!(!tree.is_descendant_of(4, 1));
        // node is not descendant of itself
        assert!(!tree.is_descendant_of(1, 1));
    }

    #[test]
    fn test_reparent_page() {
        let dir = std::env::temp_dir().join(format!(
            "capsuleos-reparent-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut vault = Vault::load(dir.clone());

        let p1 = vault.create_page(None).unwrap();
        let p2 = vault.create_page(None).unwrap();
        let p3 = vault.create_page(None).unwrap();

        let id1 = vault.capsules[p1].message_id.clone();
        let id2 = vault.capsules[p2].message_id.clone();

        // Make p2 child of p1
        vault.reparent_page(p2, Some(id1.clone())).unwrap();
        assert_eq!(vault.capsules[p2].parent_id, Some(id1.clone()));

        // Make p3 child of p2
        vault.reparent_page(p3, Some(id2.clone())).unwrap();
        assert_eq!(vault.capsules[p3].parent_id, Some(id2.clone()));

        // Cycle check: attempt to make p1 child of p3 (which is descendant of p1) must fail
        let id3 = vault.capsules[p3].message_id.clone();
        let err = vault.reparent_page(p1, Some(id3)).unwrap_err();
        assert!(err.contains("cycle detected"));

        // Reparent p2 to root (None)
        vault.reparent_page(p2, None).unwrap();
        assert_eq!(vault.capsules[p2].parent_id, None);

        let _ = fs::remove_dir_all(&dir);
    }
}
