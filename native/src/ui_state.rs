//! Persistent UI state (zoom, sidebar geometry, canvas layout).
//!
//! Stored in `%APPDATA%\com.capsuleos.app\ui_state.json` (Windows) or
//! `~/.local/share/com.capsuleos.app/ui_state.json` (Linux) outside the vault
//! so personal display ergonomics do not pollute the git/vault repo.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::brand::BrandIconStyle;
use crate::mime::Capsule;
use crate::vault;

/// Sidebar page ordering. `Manual` keeps the vault's own order (the default,
/// so the tree view behaves exactly like it always did); the rest are flat
/// sorts over all pages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageSort {
    #[default]
    Manual,
    TitleAsc,
    TitleDesc,
    Newest,
    Oldest,
}

impl PageSort {
    pub const ALL: [PageSort; 5] = [
        PageSort::Manual,
        PageSort::TitleAsc,
        PageSort::TitleDesc,
        PageSort::Newest,
        PageSort::Oldest,
    ];

    pub fn key(self) -> crate::i18n::Key {
        match self {
            PageSort::Manual => crate::i18n::Key::PageSortManual,
            PageSort::TitleAsc => crate::i18n::Key::PageSortTitleAsc,
            PageSort::TitleDesc => crate::i18n::Key::PageSortTitleDesc,
            PageSort::Newest => crate::i18n::Key::PageSortNewest,
            PageSort::Oldest => crate::i18n::Key::PageSortOldest,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiState {
    /// Global UI scale factor (e.g. 1.1 = 110%).
    pub zoom_factor: f32,
    /// Sidebar width in pixels (bounded 220.0..=480.0).
    pub sidebar_width: f32,
    /// Whether the sidebar is currently hidden.
    pub sidebar_collapsed: bool,
    /// Wide canvas mode for documents (1120 px max vs 780 px standard).
    pub wide_canvas: bool,
    /// Active brand icon style (mascot or minimal document).
    #[serde(default)]
    pub icon_style: BrandIconStyle,
    /// Folder the vault syncs with across devices (Dropbox, Nextcloud,
    /// Syncthing); plain sidebar click syncs, Shift re-picks.
    #[serde(default)]
    pub sync_dir: Option<String>,
    /// User-supplied model folders (outside the components catalog).
    #[serde(default)]
    pub custom_models: CustomModels,
    /// Message-ID of the page open when the app last exited, so a restart
    /// reopens the same page (falls back to the vault root when the page was
    /// deleted or renamed on disk meanwhile).
    #[serde(default)]
    pub last_page_id: Option<String>,
    /// Key of the view (`views::View::key`) that was active last time.
    #[serde(default)]
    pub last_view: Option<String>,
    /// Sidebar ordering, persisted so the list stops reshuffling itself.
    #[serde(default)]
    pub sort_mode: PageSort,
    /// Pages pinned to the top of the sidebar (Message-IDs, display order).
    #[serde(default)]
    pub pinned_ids: Vec<String>,
    /// Message-IDs of recently visited pages, newest first (session aid).
    #[serde(default)]
    pub recent_page_ids: Vec<String>,
    /// Keys of recently executed palette commands (`Command::key`), newest
    /// first: the palette boosts them instead of making the user re-type.
    #[serde(default)]
    pub palette_recents: Vec<String>,
    /// Paragraph text wraps inside the document view. Turning it off gives
    /// every paragraph a horizontal scroll strip (prose-code style).
    #[serde(default = "default_true")]
    pub wrap_body: bool,
    /// Desktop window size from the previous run, applied on startup.
    #[serde(default)]
    pub window_size: Option<[f32; 2]>,
}

fn default_true() -> bool {
    true
}

/// User-supplied model directories: "I already have this model on disk".
///
/// Paths are strings because this struct lives in JSON settings; every use
/// validates the directory first, so a moved folder degrades to the
/// component catalog instead of breaking the feature.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CustomModels {
    #[serde(default)]
    pub stt: Option<String>,
    #[serde(default)]
    pub tts: Option<String>,
    #[serde(default)]
    pub ocr: Option<String>,
}

impl CustomModels {
    pub fn slot(&self, kind: CustomModelKind) -> Option<&str> {
        match kind {
            CustomModelKind::Stt => self.stt.as_deref(),
            CustomModelKind::Tts => self.tts.as_deref(),
            CustomModelKind::Ocr => self.ocr.as_deref(),
        }
    }

    pub fn set(&mut self, kind: CustomModelKind, path: Option<String>) {
        match kind {
            CustomModelKind::Stt => self.stt = path,
            CustomModelKind::Tts => self.tts = path,
            CustomModelKind::Ocr => self.ocr = path,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomModelKind {
    Stt,
    Tts,
    Ocr,
}

impl CustomModelKind {
    pub const ALL: [CustomModelKind; 3] = [
        CustomModelKind::Stt,
        CustomModelKind::Tts,
        CustomModelKind::Ocr,
    ];

    pub fn key(self) -> crate::i18n::Key {
        match self {
            CustomModelKind::Stt => crate::i18n::Key::CustomModelStt,
            CustomModelKind::Tts => crate::i18n::Key::CustomModelTts,
            CustomModelKind::Ocr => crate::i18n::Key::CustomModelOcr,
        }
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            zoom_factor: 1.1,
            sidebar_width: 280.0,
            sidebar_collapsed: false,
            wide_canvas: false,
            icon_style: BrandIconStyle::default(),
            custom_models: CustomModels::default(),
            sync_dir: None,
            last_page_id: None,
            last_view: None,
            sort_mode: PageSort::Manual,
            pinned_ids: Vec::new(),
            recent_page_ids: Vec::new(),
            palette_recents: Vec::new(),
            wrap_body: true,
            window_size: None,
        }
    }
}

impl UiState {
    pub fn file_path() -> PathBuf {
        vault::app_data_dir().join("ui_state.json")
    }

    pub fn load() -> Self {
        let path = Self::file_path();
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(mut state) = serde_json::from_str::<UiState>(&content) {
                if state.icon_style.is_legacy_mascot() {
                    state.icon_style = BrandIconStyle::Nova;
                }
                return state;
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        let path = Self::file_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(&path, json);
        }
    }

    pub fn zoom_in(&mut self) -> f32 {
        self.zoom_factor = ((self.zoom_factor + 0.1) * 10.0).round() / 10.0;
        self.zoom_factor = self.zoom_factor.min(2.5);
        self.zoom_factor
    }

    pub fn zoom_out(&mut self) -> f32 {
        self.zoom_factor = ((self.zoom_factor - 0.1) * 10.0).round() / 10.0;
        self.zoom_factor = self.zoom_factor.max(0.75);
        self.zoom_factor
    }

    pub fn reset_zoom(&mut self) -> f32 {
        self.zoom_factor = 1.1;
        self.zoom_factor
    }

    pub fn toggle_sidebar(&mut self) -> bool {
        self.sidebar_collapsed = !self.sidebar_collapsed;
        self.sidebar_collapsed
    }

    pub fn toggle_wide_canvas(&mut self) -> bool {
        self.wide_canvas = !self.wide_canvas;
        self.wide_canvas
    }

    pub fn toggle_icon_style(&mut self) -> BrandIconStyle {
        self.icon_style = self.icon_style.cycle();
        self.icon_style
    }

    /// Record a page visit in the recents ring (dedup, newest first, cap 8).
    pub fn note_page_visit(&mut self, id: &str) {
        if id.trim().is_empty() {
            return;
        }
        self.recent_page_ids.retain(|existing| existing != id);
        self.recent_page_ids.insert(0, id.to_string());
        self.recent_page_ids.truncate(8);
    }

    /// Pin/unpin a page by Message-ID; returns the new pinned state. Pinned
    /// pages keep first-pinned-first display order.
    pub fn toggle_pin(&mut self, id: &str) -> bool {
        if let Some(position) = self.pinned_ids.iter().position(|existing| existing == id) {
            self.pinned_ids.remove(position);
            return false;
        }
        self.pinned_ids.push(id.to_string());
        true
    }

    /// Record an executed palette command key (dedup, newest first, cap 8).
    pub fn note_palette_command(&mut self, key: &str) {
        if key.trim().is_empty() {
            return;
        }
        self.palette_recents.retain(|existing| existing != key);
        self.palette_recents.insert(0, key.to_string());
        self.palette_recents.truncate(8);
    }

    /// Sidebar display order: pinned pages first (in pin order), then the
    /// rest under the chosen sort. Returns indices into `capsules`.
    pub fn order_pages(&self, capsules: &[Capsule]) -> Vec<usize> {
        let mut pinned = Vec::new();
        let mut rest: Vec<usize> = Vec::new();
        for (index, capsule) in capsules.iter().enumerate() {
            if self.pinned_ids.contains(&capsule.message_id) {
                pinned.push(index);
            } else {
                rest.push(index);
            }
        }
        pinned.sort_by_key(|index| {
            self.pinned_ids
                .iter()
                .position(|id| capsules[*index].message_id == *id)
                .unwrap_or(usize::MAX)
        });
        match self.sort_mode {
            PageSort::Manual => {}
            PageSort::TitleAsc => rest.sort_by(|a, b| {
                capsules[*a]
                    .subject
                    .to_lowercase()
                    .cmp(&capsules[*b].subject.to_lowercase())
            }),
            PageSort::TitleDesc => rest.sort_by(|a, b| {
                capsules[*b]
                    .subject
                    .to_lowercase()
                    .cmp(&capsules[*a].subject.to_lowercase())
            }),
            PageSort::Newest => {
                rest.sort_by_key(|index| std::cmp::Reverse(capsules[*index].timestamp))
            }
            PageSort::Oldest => rest.sort_by_key(|index| capsules[*index].timestamp),
        }
        pinned.extend(rest);
        pinned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_ui_state() {
        let state = UiState::default();
        assert!((state.zoom_factor - 1.1).abs() < 1e-4);
        assert_eq!(state.sidebar_width, 280.0);
        assert!(!state.sidebar_collapsed);
        assert!(!state.wide_canvas);
        assert_eq!(state.icon_style, BrandIconStyle::Nova);
    }

    #[test]
    fn test_zoom_clamps() {
        let mut state = UiState::default();
        for _ in 0..30 {
            state.zoom_in();
        }
        assert_eq!(state.zoom_factor, 2.5);

        for _ in 0..40 {
            state.zoom_out();
        }
        assert_eq!(state.zoom_factor, 0.75);

        state.reset_zoom();
        assert!((state.zoom_factor - 1.1).abs() < 1e-4);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let state = UiState {
            zoom_factor: 1.3,
            sidebar_width: 320.0,
            sidebar_collapsed: true,
            wide_canvas: true,
            icon_style: BrandIconStyle::Capsule,
            custom_models: CustomModels {
                stt: Some("/models/whisper".to_string()),
                tts: None,
                ocr: None,
            },
            sync_dir: Some("/sync/vault".to_string()),
            last_page_id: Some("<pke@capsuleos.local>".to_string()),
            last_view: Some("planning".to_string()),
            sort_mode: PageSort::TitleAsc,
            pinned_ids: vec!["<pin@capsuleos.local>".to_string()],
            recent_page_ids: Vec::new(),
            palette_recents: Vec::new(),
            wrap_body: true,
            window_size: None,
        };
        let json = serde_json::to_string(&state).unwrap();
        let decoded: UiState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, decoded);
    }

    #[test]
    fn session_fields_default_when_absent() {
        // Settings written before session restore have neither field.
        let json = r#"{"zoom_factor":1.0,"sidebar_width":280.0,"sidebar_collapsed":false,"wide_canvas":false}"#;
        let decoded: UiState = serde_json::from_str(json).unwrap();
        assert_eq!(decoded.last_page_id, None);
        assert_eq!(decoded.last_view, None);
    }

    #[test]
    fn custom_models_default_to_empty_when_absent() {
        // Older settings files have no `custom_models`: loading must not fail.
        let json = r#"{"zoom_factor":1.0,"sidebar_width":280.0,"sidebar_collapsed":false,"wide_canvas":false}"#;
        let decoded: UiState = serde_json::from_str(json).unwrap();
        assert_eq!(decoded.custom_models, CustomModels::default());
    }

    #[test]
    fn custom_model_slots_are_addressable() {
        let mut models = CustomModels::default();
        assert_eq!(models.slot(CustomModelKind::Stt), None);
        models.set(CustomModelKind::Ocr, Some("/m/ocr".to_string()));
        assert_eq!(models.slot(CustomModelKind::Ocr), Some("/m/ocr"));
        models.set(CustomModelKind::Ocr, None);
        assert_eq!(models.slot(CustomModelKind::Ocr), None);
    }

    fn capsule(id: &str, subject: &str, timestamp: i64) -> Capsule {
        let mut capsule = Capsule::blank(id.to_string(), format!("{id}.eml"), subject.to_string());
        capsule.timestamp = timestamp;
        capsule
    }

    #[test]
    fn page_visit_ring_dedups_and_caps() {
        let mut state = UiState::default();
        for i in 0..12 {
            state.note_page_visit(&format!("<p{i}@x>"));
        }
        assert_eq!(state.recent_page_ids.len(), 8);
        assert_eq!(state.recent_page_ids[0], "<p11@x>");
        state.note_page_visit("<p5@x>");
        assert_eq!(state.recent_page_ids[0], "<p5@x>");
        assert_eq!(state.recent_page_ids.len(), 8);
        assert_eq!(
            state
                .recent_page_ids
                .iter()
                .filter(|id| *id == "<p5@x>")
                .count(),
            1
        );
        state.note_page_visit("   ");
        assert_eq!(state.recent_page_ids.len(), 8);
    }

    #[test]
    fn toggle_pin_round_trips() {
        let mut state = UiState::default();
        assert!(state.toggle_pin("<a@x>"));
        assert!(state.toggle_pin("<b@x>"));
        assert!(!state.toggle_pin("<a@x>"));
        assert_eq!(state.pinned_ids, vec!["<b@x>".to_string()]);
    }

    #[test]
    fn palette_recents_dedup_and_cap() {
        let mut state = UiState::default();
        for i in 0..10 {
            state.note_palette_command(&format!("cmd{i}"));
        }
        assert_eq!(state.palette_recents.len(), 8);
        state.note_palette_command("cmd3");
        assert_eq!(state.palette_recents[0], "cmd3");
        assert_eq!(
            state
                .palette_recents
                .iter()
                .filter(|k| *k == "cmd3")
                .count(),
            1
        );
    }

    #[test]
    fn order_pages_pinned_first_then_sort() {
        let capsules = vec![
            capsule("<a@x>", "Beta", 100),
            capsule("<b@x>", "alpha", 300),
            capsule("<c@x>", "Gamma", 200),
            capsule("<d@x>", "delta", 400),
        ];
        // Manual keeps vault order, minus pinned reshuffling.
        let mut state = UiState::default();
        state.toggle_pin("<c@x>");
        assert_eq!(state.order_pages(&capsules), vec![2, 0, 1, 3]);

        state.sort_mode = PageSort::TitleAsc;
        let order = state.order_pages(&capsules);
        let subjects: Vec<&str> = order
            .iter()
            .map(|i| capsules[*i].subject.as_str())
            .collect();
        assert_eq!(subjects, vec!["Gamma", "alpha", "Beta", "delta"]);

        state.sort_mode = PageSort::Newest;
        let order = state.order_pages(&capsules);
        assert_eq!(order[0], 2, "pinned page stays first");
        assert_eq!(order[1], 3, "newest unpinned next");

        state.sort_mode = PageSort::Oldest;
        let order = state.order_pages(&capsules);
        assert_eq!(order[0], 2);
        assert_eq!(order[1], 0);
    }

    #[test]
    fn new_fields_default_and_round_trip() {
        let json = r#"{"zoom_factor":1.0,"sidebar_width":280.0,"sidebar_collapsed":false,"wide_canvas":false}"#;
        let decoded: UiState = serde_json::from_str(json).unwrap();
        assert_eq!(decoded.sort_mode, PageSort::Manual);
        assert!(decoded.pinned_ids.is_empty());
        assert!(decoded.palette_recents.is_empty());
        assert!(decoded.wrap_body, "wrap defaults to on for older settings");
        assert_eq!(decoded.window_size, None);

        let mut state = UiState::default();
        state.wrap_body = false;
        state.window_size = Some([1280.0, 800.0]);
        state.sort_mode = PageSort::Newest;
        let json = serde_json::to_string(&state).unwrap();
        let decoded: UiState = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, state);
    }
}
