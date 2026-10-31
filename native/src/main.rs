#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! CapsuleCraft native egui prototype.
//!
//! A separate binary, not a rewrite of the React app. It opens the same `.eml`
//! vault folder, edits a small honest subset of the block model and writes the
//! same MIME shape back. Unsupported blocks are preserved in
//! `canvas_state.json`; external HTML emails are read-only.
//!
//! The shell follows ArtCraft's native visual system: undecorated window, a
//! custom title bar, a flush sidebar, a centred document panel and intentional
//! typography. The product remains CapsuleCraft: local folder, one `.eml` per
//! page, no telemetry.

mod audio_player;
mod audio_recorder;
mod blocks;
mod brand;
mod chrome;
mod cli;
mod cothinker;
mod crypto;
mod export;
mod graph;
mod history;
mod i18n;
mod icons;
mod importers;
mod js_engine;
mod links;
mod mail;
mod mcp;
mod mime;
mod modals;
mod planning;
pub mod rich_text;
mod search;
mod snapshot;
mod sync_imap;
mod theme;
mod ui_state;
mod util;
mod vault;
mod views;
mod watch;
mod widgets;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, FontId, Frame, Margin, Stroke};
use i18n::{t, Key, Locale};
use icons::Icon;
use mime::Capsule;
use serde_json::{json, Value};
use ui_state::UiState;
pub(crate) use util::*;
use vault::{PageTree, Vault};
pub(crate) use views::document_view::{guess_mime, sanitize_cid};
use views::graph_view::GraphState;
use views::mail_view::MailState;
use views::palette::{Command as PaletteCommand, PaletteState};
use views::planning_view::PlanningState;
use views::threads_view::ThreadsState;
use views::{draw_document, EditorAction, LinkContext, View, ViewAction};
pub(crate) use widgets::*;

const CONTENT_MAX_WIDTH: f32 = 752.0;
const SIDEBAR_WIDTH: f32 = 262.0;
pub(crate) const GUTTER: f32 = 26.0;

/// Set once the `--snapshot` capture has been written, so the app can close.
static SNAPSHOT_WRITTEN: AtomicBool = AtomicBool::new(false);

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let launch = cli::parse(&args);
    if launch.print_config {
        println!("{launch:#?}");
        return Ok(());
    }
    if launch.print_mcp_config {
        mcp::print_config(&launch);
        return Ok(());
    }
    if launch.mcp {
        mcp::run_server(launch.vault);
        return Ok(());
    }
    if let Some(obsidian_path) = launch.import_obsidian {
        let vault_dir = launch.vault.unwrap_or_else(vault::get_active_vault_dir);
        match importers::obsidian::import_obsidian_vault(&obsidian_path, &vault_dir) {
            Ok(summary) => {
                println!(
                    "Successfully imported Obsidian vault into {}",
                    vault_dir.display()
                );
                println!("- Pages imported: {}", summary.imported_pages);
                println!("- Folders created: {}", summary.created_folders);
                println!("- Attachments imported: {}", summary.imported_attachments);
                if !summary.errors.is_empty() {
                    eprintln!("Warnings/errors ({}):", summary.errors.len());
                    for err in &summary.errors {
                        eprintln!("  • {err}");
                    }
                }
                return Ok(());
            }
            Err(err) => {
                eprintln!("Error importing Obsidian vault: {err}");
                std::process::exit(1);
            }
        }
    }
    let window = if launch.snapshot.is_some() {
        [1440.0, 900.0]
    } else {
        [1380.0, 880.0]
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size(window)
        .with_min_inner_size([980.0, 640.0])
        .with_decorations(false)
        .with_resizable(true)
        .with_title("CapsuleCraft");
    let initial_ui_state = UiState::load();
    if let Some(icon) = brand::window_icon(initial_ui_state.icon_style) {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    let result = eframe::run_native(
        "CapsuleCraft",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(NativeApp::new(launch)))
        }),
    );
    if let Err(err) = result {
        eprintln!("CapsuleCraft error: {err}");
        std::process::exit(1);
    }
    std::process::exit(0);
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

/// Everything an undo step restores for one page.
#[derive(Clone)]
struct PageSnapshot {
    subject: String,
    tags: Vec<String>,
    status: String,
    blocks: Vec<Value>,
}

impl PageSnapshot {
    fn of(capsule: &Capsule) -> Self {
        Self {
            subject: capsule.subject.clone(),
            tags: capsule.tags.clone(),
            status: capsule.status.clone(),
            blocks: capsule.blocks.clone(),
        }
    }

    fn restore(&self, capsule: &mut Capsule) {
        capsule.subject = self.subject.clone();
        capsule.tags = self.tags.clone();
        capsule.status = self.status.clone();
        capsule.blocks = self.blocks.clone();
    }
}

struct NativeApp {
    vault: Vault,
    selected: Option<usize>,
    collapsed: HashSet<String>,
    search: String,
    dirty: bool,
    last_edit: Instant,
    status: String,
    status_error: bool,
    locale: Locale,
    view: View,
    planning: PlanningState,
    graph: GraphState,
    mail: MailState,
    threads: ThreadsState,
    palette: PaletteState,
    history: history::HistoryStore,
    /// Snapshot of the page as it was before the latest edit.
    snapshot: Option<(String, PageSnapshot)>,
    toast: Option<(String, Instant)>,
    confirm_delete: bool,
    show_mcp_modal: bool,
    mcp_tab: usize,
    show_imap_modal: bool,
    imap_config: sync_imap::ImapConfig,
    imap_status: sync_imap::SyncStatus,
    show_cothink_modal: bool,
    show_faq_modal: bool,
    cothink_mode: cothinker::CoThinkMode,
    cothink_focus: String,
    encrypt: modals::EncryptState,
    /// `.eml.asc` picked for decryption, waiting for the passphrase modal.
    pending_decrypt_path: Option<PathBuf>,
    dragged_page: Option<usize>,
    /// `--snapshot`: render a few frames, write a PNG, then close.
    snapshot_job: Option<SnapshotJob>,
    /// Cached `[[link]]` resolution targets: (normalised subject, normalised
    /// Message-ID, index). Rebuilt when the vault or the selection changes, not
    /// every frame.
    link_index: Vec<(String, String, usize)>,
    /// Pages that link to the selected page.
    backlinks: Vec<(usize, String)>,
    /// Pages closest to this one by tf-idf cosine similarity.
    related: Vec<(usize, f32)>,
    /// Full-text index over the vault, rebuilt with the link index.
    search_index: search::Bm25Index,
    /// Lowercased-free page text per capsule, used for search snippets.
    page_texts: Vec<String>,
    /// Fixed document scroll offset (screenshots only).
    document_scroll: Option<f32>,
    /// Watches the vault folder so external edits show up without a restart.
    watcher: Option<watch::VaultWatch>,
    /// A watched change arrived while we had unsaved edits; reload when safe.
    external_pending: bool,
    /// When the app itself last wrote to the vault, to ignore its own events.
    last_write: Instant,
    /// Persistent UI state (zoom, sidebar geometry, canvas layout).
    ui_state: UiState,
    /// Zen focus mode (immersive document writing).
    zen_mode: bool,
}

struct SnapshotJob {
    path: PathBuf,
    frames_left: u32,
    requested: bool,
}

impl NativeApp {
    fn new(launch: cli::Launch) -> Self {
        let dir = launch
            .vault
            .clone()
            .unwrap_or_else(vault::get_active_vault_dir);
        vault::set_active_vault_dir(&dir);
        let imap_config = sync_imap::ImapConfig::load_from_vault(&dir);
        let vault = Vault::load(dir);
        let locale = launch.locale.unwrap_or_else(Locale::detect);
        let selected = launch
            .page
            .filter(|index| *index < vault.capsules.len())
            .or_else(|| vault.selected_root());
        let snapshot_job = launch.snapshot.clone().map(|path| SnapshotJob {
            path,
            frames_left: launch.frames.max(1),
            requested: false,
        });
        let mut app = Self {
            vault,
            selected,
            collapsed: HashSet::new(),
            search: launch.search.clone().unwrap_or_default(),
            dirty: false,
            last_edit: Instant::now(),
            status: t(locale, Key::Saved).to_string(),
            status_error: false,
            locale,
            view: launch
                .view
                .filter(|value| !matches!(value, View::Faq))
                .unwrap_or(View::Page),
            planning: PlanningState::default(),
            graph: GraphState::new(),
            mail: MailState::default(),
            threads: ThreadsState::default(),
            palette: PaletteState::default(),
            history: history::HistoryStore::default(),
            snapshot: None,
            toast: None,
            confirm_delete: false,
            show_mcp_modal: false,
            mcp_tab: 0,
            show_imap_modal: false,
            imap_config,
            imap_status: sync_imap::SyncStatus::Idle,
            show_cothink_modal: false,
            show_faq_modal: false,
            cothink_mode: cothinker::CoThinkMode::Critique,
            cothink_focus: String::new(),
            encrypt: modals::EncryptState::default(),
            pending_decrypt_path: None,
            dragged_page: None,
            snapshot_job,
            link_index: Vec::new(),
            backlinks: Vec::new(),
            related: Vec::new(),
            search_index: search::Bm25Index::default(),
            page_texts: Vec::new(),
            document_scroll: launch.scroll,
            watcher: None,
            external_pending: false,
            last_write: Instant::now(),
            ui_state: UiState::load(),
            zen_mode: false,
        }
        .with_links();
        app.show_faq_modal = matches!(launch.view, Some(View::Faq));
        app.watcher = watch::VaultWatch::start(&app.vault.dir);
        app
    }

    fn with_links(mut self) -> Self {
        self.refresh_links();
        self
    }

    /// Reload when a watched `.eml` changed and there is nothing unsaved.
    ///
    /// Our own writes also produce events, so anything within the debounce
    /// window after a save is ignored — otherwise autosave would reload the
    /// page under the cursor.
    fn poll_watcher(&mut self) {
        if let Some(watcher) = self.watcher.as_ref() {
            if watcher.changed() {
                self.external_pending = true;
            }
        }
        if self.external_pending
            && !self.dirty
            && self.last_write.elapsed() > Duration::from_millis(700)
        {
            self.external_pending = false;
            self.reload();
        }
    }

    /// Recompute the link index and this page's backlinks. CPU-bound (it reads
    /// every page's text), so callers do it on navigation rather than per frame.
    fn refresh_links(&mut self) {
        self.page_texts = self.vault.capsules.iter().map(links::page_text).collect();
        self.search_index.clear();
        for (index, capsule) in self.vault.capsules.iter().enumerate() {
            let text = self.page_texts.get(index).map(String::as_str).unwrap_or("");
            let tokens = search::capsule_tokens("", &capsule.tags, text, "");
            self.search_index.add(&capsule.message_id, tokens);
        }
        self.link_index = self
            .vault
            .capsules
            .iter()
            .enumerate()
            .map(|(index, capsule)| {
                (
                    links::normalise(&capsule.subject),
                    links::normalise(&capsule.message_id),
                    index,
                )
            })
            .collect();
        self.backlinks = self
            .selected
            .map(|index| links::backlinks(&self.vault.capsules, index))
            .unwrap_or_default()
            .into_iter()
            .map(|other| {
                let subject = self
                    .vault
                    .capsules
                    .get(other)
                    .map(|capsule| capsule.subject.clone())
                    .unwrap_or_default();
                (other, subject)
            })
            .collect();
        // "Related by meaning": tf-idf cosine over the same index that powers
        // search, so a page can point at neighbours it never linked to.
        let ids: Vec<&str> = self
            .vault
            .capsules
            .iter()
            .map(|capsule| capsule.message_id.as_str())
            .collect();
        self.related = self
            .selected
            .and_then(|index| self.vault.capsules.get(index))
            .map(|capsule| search::related(&self.search_index, &capsule.message_id, &ids, 0.08, 5))
            .unwrap_or_default();
    }

    /// After a rename, rewrite every page that linked to the old title.
    ///
    /// Only pages CapsuleCraft owns are touched: an imported email is never
    /// rewritten, even if its text happens to look like a link.
    fn propagate_rename(&mut self, old: &str) {
        let Some(index) = self.selected else {
            return;
        };
        let Some(new_title) = self.vault.capsules.get(index).map(|c| c.subject.clone()) else {
            return;
        };
        if new_title.trim().is_empty() || links::normalise(old) == links::normalise(&new_title) {
            return;
        }
        let mut pages = 0usize;
        let mut total = 0usize;
        for other in 0..self.vault.capsules.len() {
            if !self.vault.capsules[other].native {
                continue;
            }
            let count =
                links::rewrite_targets(&mut self.vault.capsules[other].blocks, old, &new_title);
            if count == 0 {
                continue;
            }
            total += count;
            pages += 1;
            if other != index {
                let _ = self.vault.save(other);
            }
        }
        if total > 0 {
            self.mark_dirty();
            self.save_selected();
            self.refresh_links();
            self.toast(format!(
                "{} · {} · {}",
                t(self.locale, Key::LinksUpdated),
                total,
                pages
            ));
        }
    }

    /// Snapshot of this page's outgoing links and backlinks for the document.
    fn current_link_context(&self) -> LinkContext {
        let mut context = LinkContext {
            backlinks: self.backlinks.clone(),
            ..LinkContext::default()
        };
        if let Some(index) = self.selected {
            if let Some(capsule) = self.vault.capsules.get(index) {
                context.outgoing = links::page_targets(capsule)
                    .into_iter()
                    .map(|title| {
                        let resolved = self.resolve_link(&title);
                        (title, resolved)
                    })
                    .collect();
            }
        }
        context.related = self
            .related
            .iter()
            .filter_map(|(index, score)| {
                self.vault
                    .capsules
                    .get(*index)
                    .map(|capsule| (*index, capsule.subject.clone(), *score))
            })
            .collect();
        context
    }

    /// Resolve a link target against the cached index.
    fn resolve_link(&self, target: &str) -> Option<usize> {
        let wanted = links::normalise(target);
        if wanted.is_empty() {
            return None;
        }
        self.link_index
            .iter()
            .find(|(_, id, _)| id == &wanted)
            .or_else(|| {
                self.link_index
                    .iter()
                    .find(|(subject, _, _)| subject == &wanted)
            })
            .map(|(_, _, index)| *index)
    }

    fn toast(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    /// Dim the window while files are held over it, so the drop target is
    /// explicit instead of a surprise import.
    fn draw_drop_hint(&self, ctx: &egui::Context) {
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if !hovering {
            return;
        }
        let rect = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("drop-hint"),
        ));
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(180));
        painter.rect_stroke(
            rect.shrink(48.0),
            theme::RADIUS,
            Stroke::new(1.5, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            t(self.locale, Key::DropHint),
            FontId::new(17.0, theme::display()),
            theme::INK,
        );
        ctx.request_repaint();
    }

    /// `--snapshot`: let fonts and layout settle, capture the framebuffer, quit.
    fn drive_snapshot(&mut self, ctx: &egui::Context) {
        let Some(job) = self.snapshot_job.as_mut() else {
            return;
        };
        if !job.requested {
            if job.frames_left > 0 {
                job.frames_left -= 1;
                ctx.request_repaint();
                return;
            }
            job.requested = true;
            let path = job.path.clone();
            ctx.request_screenshot(move |image| {
                match snapshot::write_png(&path, &image) {
                    Ok(()) => println!("snapshot: {}", path.display()),
                    Err(err) => eprintln!("snapshot failed: {err}"),
                }
                SNAPSHOT_WRITTEN.store(true, Ordering::SeqCst);
            });
            ctx.request_repaint();
            return;
        }
        if SNAPSHOT_WRITTEN.load(Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            ctx.request_repaint();
        }
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.last_edit = Instant::now();
    }

    /// Returns `false` when a dirty page could not be saved. Callers must not
    /// navigate away in that case.
    fn save_selected(&mut self) -> bool {
        let Some(index) = self.selected else {
            return true;
        };
        if !self.dirty {
            return true;
        }
        match self.vault.save(index) {
            Ok(()) => {
                self.dirty = false;
                self.last_write = Instant::now();
                self.status = format!("Saved {}", chrono::Local::now().format("%H:%M:%S"));
                self.status_error = false;
                // Keep search and backlinks in sync with what is on disk.
                self.refresh_links();
                true
            }
            Err(err) => {
                self.status = err;
                self.status_error = true;
                false
            }
        }
    }

    fn select_page(&mut self, index: usize) {
        if self.selected == Some(index) || index >= self.vault.capsules.len() {
            return;
        }
        if !self.save_selected() {
            return;
        }
        self.selected = Some(index);
        self.dirty = false;
        self.refresh_links();
    }

    fn new_page(&mut self, parent: Option<String>) {
        if !self.save_selected() {
            return;
        }
        match self.vault.create_page(parent) {
            Ok(index) => {
                self.selected = Some(index);
                self.dirty = false;
                self.status = "New page".to_string();
                self.status_error = false;
                self.refresh_links();
            }
            Err(err) => {
                self.status = err;
                self.status_error = true;
            }
        }
    }

    /// Create a page whose subject is already known — used when clicking a
    /// `[[link]]` that points at a page which does not exist yet.
    fn create_page_titled(&mut self, title: &str) {
        if !self.save_selected() {
            return;
        }
        match self.vault.create_page(None) {
            Ok(index) => {
                if let Some(capsule) = self.vault.capsules.get_mut(index) {
                    capsule.subject = title.trim().to_string();
                }
                self.selected = Some(index);
                self.dirty = true;
                self.status = t(self.locale, Key::PageCreated).to_string();
                self.status_error = false;
                self.save_selected();
                self.refresh_links();
                self.toast(t(self.locale, Key::PageCreated));
            }
            Err(err) => {
                self.status = err;
                self.status_error = true;
            }
        }
    }

    fn reload(&mut self) {
        if !self.save_selected() {
            return;
        }
        let selected_id = self
            .selected
            .and_then(|index| self.vault.capsules.get(index))
            .map(|capsule| capsule.message_id.clone());
        self.vault.reload();
        self.selected = selected_id
            .and_then(|id| {
                self.vault
                    .capsules
                    .iter()
                    .position(|capsule| capsule.message_id == id)
            })
            .or_else(|| self.vault.selected_root());
        self.dirty = false;
        self.status = "Reloaded".to_string();
        self.status_error = false;
        self.last_write = Instant::now();
        self.refresh_links();
    }

    fn open_vault_dialog(&mut self) {
        if !self.save_selected() {
            return;
        }
        if let Some(dir) = rfd::FileDialog::new()
            .set_title("Choose a CapsuleCraft .eml vault folder")
            .set_directory(&self.vault.dir)
            .pick_folder()
        {
            vault::set_active_vault_dir(&dir);
            self.vault = Vault::load(dir);
            self.selected = self.vault.selected_root();
            self.collapsed.clear();
            self.search.clear();
            self.dirty = false;
            self.snapshot = None;
            self.history = history::HistoryStore::default();
            self.status = t(self.locale, Key::VaultOpened).to_string();
            self.status_error = false;
            self.last_write = Instant::now();
            self.watcher = watch::VaultWatch::start(&self.vault.dir);
            self.refresh_links();
        }
    }

    /// Keep the undo baseline in sync with whatever page is selected.
    fn sync_snapshot(&mut self) {
        let Some(index) = self.selected else {
            self.snapshot = None;
            return;
        };
        let Some(capsule) = self.vault.capsules.get(index) else {
            return;
        };
        let id = capsule.message_id.clone();
        if self
            .snapshot
            .as_ref()
            .map(|(snapshot_id, _)| snapshot_id != &id)
            .unwrap_or(true)
        {
            self.snapshot = Some((id, PageSnapshot::of(capsule)));
        }
    }

    /// Push the pre-edit state onto the page's undo stack, then re-baseline.
    fn record_change(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        let Some(capsule) = self.vault.capsules.get(index) else {
            return;
        };
        let id = capsule.message_id.clone();
        let current = PageSnapshot::of(capsule);
        if let Some((snapshot_id, previous)) = &self.snapshot {
            if snapshot_id == &id && !same_snapshot(previous, &current) {
                self.history
                    .maybe_push(&id, snapshot_to_string(previous), now_millis());
            }
        }
        self.snapshot = Some((id, current));
        self.mark_dirty();
    }

    fn undo(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        let Some(id) = self.vault.capsules.get(index).map(|c| c.message_id.clone()) else {
            return;
        };
        let current = match self.vault.capsules.get(index) {
            Some(capsule) => snapshot_to_string(&PageSnapshot::of(capsule)),
            None => return,
        };
        let previous = self.history.undo(&id, &current);
        let Some(previous) = previous else {
            self.toast(t(self.locale, Key::UndoEmpty));
            return;
        };
        let Some(snapshot) = snapshot_from_string(&previous) else {
            self.toast(t(self.locale, Key::UndoEmpty));
            return;
        };
        if let Some(capsule) = self.vault.capsules.get_mut(index) {
            snapshot.restore(capsule);
        } else {
            return;
        }
        self.snapshot = Some((id, snapshot));
        self.mark_dirty();
        self.toast(t(self.locale, Key::Undone));
    }

    fn redo(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        let Some(id) = self.vault.capsules.get(index).map(|c| c.message_id.clone()) else {
            return;
        };
        let current = match self.vault.capsules.get(index) {
            Some(capsule) => snapshot_to_string(&PageSnapshot::of(capsule)),
            None => return,
        };
        let next = self.history.redo(&id, &current);
        let Some(next) = next else {
            self.toast(t(self.locale, Key::RedoEmpty));
            return;
        };
        let Some(snapshot) = snapshot_from_string(&next) else {
            self.toast(t(self.locale, Key::RedoEmpty));
            return;
        };
        if let Some(capsule) = self.vault.capsules.get_mut(index) {
            snapshot.restore(capsule);
        } else {
            return;
        }
        self.snapshot = Some((id, snapshot));
        self.mark_dirty();
        self.toast(t(self.locale, Key::Redone));
    }

    /// Palette: encrypt the selected capsule into an armored `.eml.asc`
    /// (roadmap: OpenPGP capsule encryption, fully offline via rPGP).
    fn begin_encrypt_capsule(&mut self) {
        if !self.save_selected() {
            return;
        }
        let Some(index) = self.selected else {
            return;
        };
        let Some(capsule) = self.vault.capsules.get(index) else {
            return;
        };
        let file_name = capsule.file_name.clone();
        self.encrypt
            .open(modals::EncryptMode::Encrypt { file_name });
    }

    /// Called after the passphrase modal confirms in encrypt mode.
    fn finish_encrypt_capsule(&mut self, file_name: &str, passphrase: &str) {
        let Some(index) = self.selected else {
            return;
        };
        let Some(capsule) = self.vault.capsules.get(index) else {
            return;
        };
        let armored = match crypto::encrypt_to_armored(capsule.export_eml().as_bytes(), passphrase)
        {
            Ok(text) => text,
            Err(err) => {
                self.status = err.to_string();
                self.status_error = true;
                return;
            }
        };
        let stem = file_name.trim_end_matches(".eml").trim_end_matches(".EML");
        let default_name = format!("{stem}.eml.asc");
        let Some(path) = rfd::FileDialog::new()
            .set_title(t(self.locale, Key::EncryptCapsuleCmd))
            .set_file_name(&default_name)
            .add_filter("PGP armored message", &["asc"])
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, armored) {
            Ok(()) => {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("capsule.eml.asc");
                self.toast(format!("{}: {name}", t(self.locale, Key::EncryptedDone)));
            }
            Err(err) => {
                self.status = err.to_string();
                self.status_error = true;
            }
        }
    }

    /// Palette: pick an `.eml.asc`, then ask for the passphrase.
    fn begin_decrypt_asc(&mut self) {
        if !self.save_selected() {
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_title(t(self.locale, Key::DecryptEmlCmd))
            .add_filter("PGP armored message", &["asc"])
            .pick_file()
        else {
            return;
        };
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("capsule.eml.asc")
            .to_string();
        self.pending_decrypt_path = Some(path);
        self.encrypt
            .open(modals::EncryptMode::Decrypt { file_name });
    }

    /// Called after the passphrase modal confirms in decrypt mode. A wrong
    /// passphrase reopens the modal with an error instead of giving up.
    fn finish_decrypt_asc(&mut self, file_name: &str, passphrase: &str) {
        let Some(path) = self.pending_decrypt_path.clone() else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            self.status = t(self.locale, Key::CryptoFailed).to_string();
            self.status_error = true;
            return;
        };
        match crypto::decrypt_from_armored(&text, passphrase) {
            Ok(eml_bytes) => {
                let target_name = file_name.trim_end_matches(".asc").to_string();
                let target = unique_path(&self.vault.dir, &target_name);
                match std::fs::write(&target, &eml_bytes) {
                    Ok(()) => {
                        self.pending_decrypt_path = None;
                        self.reload();
                        self.toast(t(self.locale, Key::DecryptDone));
                    }
                    Err(err) => {
                        self.status = err.to_string();
                        self.status_error = true;
                    }
                }
            }
            Err(_) => {
                // Keep the picked file and reopen the modal for another try.
                self.encrypt.open(modals::EncryptMode::Decrypt {
                    file_name: file_name.to_string(),
                });
                self.encrypt.error = Some(t(self.locale, Key::CryptoFailed).to_string());
            }
        }
    }

    fn export_page(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        let Some(capsule) = self.vault.capsules.get(index) else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .set_title(t(self.locale, Key::ExportEml))
            .set_file_name(&capsule.file_name)
            .add_filter("email", &["eml"])
            .save_file()
        else {
            return;
        };
        match std::fs::write(path, capsule.export_eml()) {
            Ok(()) => self.toast(t(self.locale, Key::ExportEml)),
            Err(err) => {
                self.status = err.to_string();
                self.status_error = true;
            }
        }
    }

    /// Export the selected page as Markdown — for pasting into a README, a
    /// ticket or a chat without dragging the block model along.
    fn export_markdown(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        let Some(capsule) = self.vault.capsules.get(index) else {
            return;
        };
        let stem = capsule
            .file_name
            .trim_end_matches(".eml")
            .trim_end_matches(".EML");
        let default_name = format!("{stem}.md");
        let Some(path) = rfd::FileDialog::new()
            .set_title(t(self.locale, Key::ExportMarkdown))
            .set_file_name(&default_name)
            .add_filter("markdown", &["md"])
            .save_file()
        else {
            return;
        };
        match std::fs::write(path, export::page_markdown(capsule)) {
            Ok(()) => self.toast(t(self.locale, Key::ExportMarkdown)),
            Err(err) => {
                self.status = err.to_string();
                self.status_error = true;
            }
        }
    }

    fn export_vault(&mut self) {
        if !self.save_selected() {
            return;
        }
        let Some(dir) = rfd::FileDialog::new()
            .set_title(t(self.locale, Key::ExportVault))
            .set_directory(&self.vault.dir)
            .pick_folder()
        else {
            return;
        };
        let mut written = 0usize;
        for capsule in &self.vault.capsules {
            let path = dir.join(&capsule.file_name);
            if std::fs::write(path, capsule.export_eml()).is_ok() {
                written += 1;
            }
        }
        self.toast(format!(
            "{} · {} .eml",
            t(self.locale, Key::VaultExported),
            written
        ));
    }

    fn import_eml(&mut self) {
        if !self.save_selected() {
            return;
        }
        let Some(paths) = rfd::FileDialog::new()
            .set_title(t(self.locale, Key::ImportEml))
            .add_filter("email", &["eml"])
            .pick_files()
        else {
            return;
        };
        self.import_paths(paths);
    }

    fn import_obsidian_dialog(&mut self) {
        if !self.save_selected() {
            return;
        }
        let Some(folder) = rfd::FileDialog::new()
            .set_title(t(self.locale, Key::ImportObsidianTitle))
            .pick_folder()
        else {
            return;
        };
        match importers::obsidian::import_obsidian_vault(&folder, &self.vault.dir) {
            Ok(summary) => {
                self.reload();
                self.toast(format!(
                    "{} · {} .eml{}",
                    t(self.locale, Key::ObsidianImportSummary),
                    summary.imported_pages + summary.created_folders,
                    if summary.imported_attachments > 0 {
                        format!(
                            " · {} {}",
                            summary.imported_attachments,
                            t(self.locale, Key::Attachment)
                        )
                    } else {
                        String::new()
                    }
                ));
            }
            Err(err) => {
                self.toast(format!("Import error: {err}"));
            }
        }
    }

    /// Copy `.eml` files into the vault. Dropped files and the import dialog
    /// share this path, so both behave the same and both keep the original
    /// bytes (an imported mail is never re-encoded on the way in).
    fn import_paths(&mut self, paths: Vec<PathBuf>) {
        if !self.save_selected() {
            return;
        }
        let mut imported = 0usize;
        let mut skipped = 0usize;
        for path in paths {
            let is_eml = path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("eml"))
                .unwrap_or(false);
            if !is_eml {
                skipped += 1;
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                skipped += 1;
                continue;
            };
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("imported.eml");
            let target = unique_path(&self.vault.dir, name);
            if std::fs::write(&target, &bytes).is_ok() {
                imported += 1;
            } else {
                skipped += 1;
            }
        }
        if imported > 0 {
            self.reload();
            self.toast(format!(
                "{} · {} .eml{}",
                t(self.locale, Key::Imported),
                imported,
                if skipped > 0 {
                    format!(" · {} {}", skipped, t(self.locale, Key::SkippedFiles))
                } else {
                    String::new()
                }
            ));
        } else {
            self.toast(t(self.locale, Key::AttachFailed));
        }
    }

    fn delete_selected_page(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        let Some(capsule) = self.vault.capsules.get(index) else {
            return;
        };
        let path = self.vault.dir.join(&capsule.file_name);
        let removed = std::fs::remove_file(&path);
        if let Err(err) = removed {
            self.status = err.to_string();
            self.status_error = true;
            return;
        }
        self.dirty = false;
        self.snapshot = None;
        self.vault.reload();
        self.selected = self.vault.selected_root();
        self.confirm_delete = false;
        self.last_write = Instant::now();
        self.refresh_links();
        self.toast(t(self.locale, Key::PageDeleted));
    }

    fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
        locale.persist();
    }
}

fn same_snapshot(a: &PageSnapshot, b: &PageSnapshot) -> bool {
    a.subject == b.subject
        && a.tags == b.tags
        && a.status == b.status
        && Value::Array(a.blocks.clone()) == Value::Array(b.blocks.clone())
}

fn snapshot_to_string(snapshot: &PageSnapshot) -> String {
    json!({
        "subject": snapshot.subject,
        "tags": snapshot.tags,
        "status": snapshot.status,
        "blocks": snapshot.blocks,
    })
    .to_string()
}

fn snapshot_from_string(raw: &str) -> Option<PageSnapshot> {
    let value: Value = serde_json::from_str(raw).ok()?;
    Some(PageSnapshot {
        subject: value.get("subject")?.as_str()?.to_string(),
        tags: value
            .get("tags")?
            .as_array()?
            .iter()
            .filter_map(|tag| tag.as_str().map(str::to_string))
            .collect(),
        status: value.get("status")?.as_str()?.to_string(),
        blocks: value.get("blocks")?.as_array()?.clone(),
    })
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0)
}

/// Pick `name`, or `name-2.eml`, `name-3.eml`, … so an import never overwrites
/// an existing page.
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("imported");
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("eml");
    for suffix in 2..1000 {
        let candidate = dir.join(format!("{stem}-{suffix}.{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}-{}.{ext}", now_millis()))
}

#[derive(Default)]
pub(crate) struct ShellAction {
    pub(crate) select: Option<usize>,
    pub(crate) open_vault: bool,
    pub(crate) new_page: bool,
    pub(crate) reload: bool,
    pub(crate) save: bool,
    pub(crate) view: Option<View>,
    pub(crate) toggle_locale: bool,
    pub(crate) palette: bool,
    pub(crate) connect_mcp: bool,
    pub(crate) faq: bool,
    pub(crate) open_imap: bool,
    pub(crate) sync_imap: bool,
    pub(crate) import: bool,
    pub(crate) export_vault: bool,
    pub(crate) export_page: bool,
    pub(crate) delete_page: bool,
    pub(crate) reparent: Option<(usize, Option<String>)>,
    pub(crate) toggle_icon_style: bool,
    pub(crate) toggle_sidebar: bool,
    pub(crate) toggle_zen: bool,
}

impl ShellAction {
    pub(crate) fn absorb(&mut self, other: ShellAction) {
        self.select = other.select.or(self.select);
        self.open_vault |= other.open_vault;
        self.new_page |= other.new_page;
        self.reload |= other.reload;
        self.save |= other.save;
        self.view = other.view.or(self.view);
        self.toggle_locale |= other.toggle_locale;
        self.palette |= other.palette;
        self.connect_mcp |= other.connect_mcp;
        self.faq |= other.faq;
        self.open_imap |= other.open_imap;
        self.sync_imap |= other.sync_imap;
        self.import |= other.import;
        self.export_vault |= other.export_vault;
        self.export_page |= other.export_page;
        self.delete_page |= other.delete_page;
        self.reparent = other.reparent.or_else(|| self.reparent.take());
        self.toggle_icon_style |= other.toggle_icon_style;
        self.toggle_sidebar |= other.toggle_sidebar;
        self.toggle_zen |= other.toggle_zen;
    }
}

impl eframe::App for NativeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.sync_snapshot();
        self.drive_snapshot(&ctx);
        self.poll_watcher();

        // Apply persistent zoom factor
        ctx.set_zoom_factor(self.ui_state.zoom_factor);

        // Drag and drop: `.eml` files land in the vault folder untouched.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.import_paths(dropped);
        }
        self.draw_drop_hint(&ctx);

        let (cmd, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
        if ctx.input(|i| i.key_pressed(egui::Key::F11))
            || (cmd && shift && ctx.input(|i| i.key_pressed(egui::Key::F)))
        {
            self.toggle_zen_mode();
        }
        if cmd && ctx.input(|i| i.key_pressed(egui::Key::Backslash)) {
            self.toggle_sidebar();
        }
        if cmd && shift && ctx.input(|i| i.key_pressed(egui::Key::W)) {
            self.toggle_wide_canvas();
        }
        if cmd
            && (ctx.input(|i| i.key_pressed(egui::Key::Plus))
                || ctx.input(|i| i.key_pressed(egui::Key::Equals)))
        {
            self.zoom_in(&ctx);
        }
        if cmd && ctx.input(|i| i.key_pressed(egui::Key::Minus)) {
            self.zoom_out(&ctx);
        }
        if cmd && ctx.input(|i| i.key_pressed(egui::Key::Num0)) {
            self.reset_zoom(&ctx);
        }

        let modals_open = self.palette.open
            || self.confirm_delete
            || self.show_mcp_modal
            || self.show_imap_modal
            || self.show_cothink_modal
            || self.show_faq_modal
            || self.encrypt.open;
        if self.zen_mode && !modals_open && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.toggle_zen_mode();
        }

        if cmd && shift && ctx.input(|i| i.key_pressed(egui::Key::A)) {
            self.show_cothink_modal = true;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
            self.show_faq_modal = true;
        }
        if cmd && shift && ctx.input(|i| i.key_pressed(egui::Key::S)) {
            self.trigger_imap_sync();
        }
        if cmd && ctx.input(|i| i.key_pressed(egui::Key::S)) {
            self.save_selected();
        }
        if cmd && ctx.input(|i| i.key_pressed(egui::Key::N)) {
            self.new_page(None);
        }
        if cmd && ctx.input(|i| i.key_pressed(egui::Key::K)) {
            self.palette.open = !self.palette.open;
            self.palette.just_opened = self.palette.open;
            self.palette.query.clear();
            self.palette.cursor = 0;
        }
        if !self.palette.open {
            if cmd && ctx.input(|i| i.key_pressed(egui::Key::Z)) {
                if shift {
                    self.redo();
                } else {
                    self.undo();
                }
            }
            if cmd && ctx.input(|i| i.key_pressed(egui::Key::Y)) {
                self.redo();
            }
            if cmd && ctx.input(|i| i.key_pressed(egui::Key::E)) {
                self.export_page();
            }
            if ctx.input(|i| i.modifiers.alt) {
                for (index, key) in [
                    egui::Key::Num1,
                    egui::Key::Num2,
                    egui::Key::Num3,
                    egui::Key::Num4,
                    egui::Key::Num5,
                    egui::Key::Num6,
                ]
                .into_iter()
                .enumerate()
                {
                    if ctx.input(|i| i.key_pressed(key)) {
                        self.view = View::all()[index];
                    }
                }
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            self.ui_state.save();
            if !self.save_selected() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            } else {
                std::process::exit(0);
            }
        }

        let tree = PageTree::build(&self.vault.capsules);
        let mut shell = ShellAction::default();
        // Links are resolved before the document borrows its page mutably.
        let link_context = self.current_link_context();

        let hover_top = ctx
            .input(|i| i.pointer.hover_pos())
            .map(|p| p.y < chrome::TITLEBAR_HEIGHT)
            .unwrap_or(false);
        let show_titlebar = !self.zen_mode || hover_top;

        if show_titlebar {
            egui::Panel::top("titlebar")
                .exact_size(chrome::TITLEBAR_HEIGHT)
                .frame(
                    Frame::new()
                        .fill(theme::CHROME)
                        .inner_margin(Margin::same(0)),
                )
                .show(ui, |ui| {
                    shell.absorb(draw_titlebar(
                        ui,
                        &self.vault,
                        self.dirty,
                        &self.status,
                        self.status_error,
                        &self.imap_config,
                        &self.imap_status,
                        self.locale,
                        self.ui_state.sidebar_collapsed,
                        self.zen_mode,
                        self.ui_state.icon_style,
                    ));
                });
        }

        let mut editor = EditorAction::default();

        if !self.zen_mode {
            egui::Panel::top("workspace_strip")
                .exact_size(chrome::WORKSPACE_STRIP_HEIGHT)
                .frame(
                    Frame::new()
                        .fill(theme::CHROME)
                        .inner_margin(Margin::same(0)),
                )
                .show(ui, |ui| {
                    let (chosen, strip_editor) = draw_workspace_strip(
                        ui,
                        self.view,
                        self.locale,
                        self.selected,
                        &self.vault.capsules,
                        self.ui_state.wide_canvas,
                    );
                    if let Some(v) = chosen {
                        shell.view = Some(v);
                    }
                    editor.absorb(strip_editor);
                });
        }

        let sidebar_action = if !self.zen_mode && !self.ui_state.sidebar_collapsed {
            let sidebar_resp = egui::Panel::left("sidebar")
                .default_size(self.ui_state.sidebar_width)
                .size_range(220.0..=480.0)
                .resizable(true)
                .frame(
                    Frame::new()
                        .fill(theme::CHROME)
                        .inner_margin(Margin::same(0)),
                )
                .show(ui, |ui| {
                    views::draw_sidebar(
                        ui,
                        &self.vault,
                        &tree,
                        self.selected,
                        &mut self.dragged_page,
                        &mut self.search,
                        &mut self.collapsed,
                        &self.search_index,
                        &self.page_texts,
                        self.watcher.is_some(),
                        self.locale,
                    )
                });
            let new_w = sidebar_resp.response.rect.width();
            if (new_w - self.ui_state.sidebar_width).abs() > 0.5 {
                self.ui_state.sidebar_width = new_w.clamp(220.0, 480.0);
                self.ui_state.save();
            }
            sidebar_resp.inner
        } else {
            ShellAction::default()
        };
        shell.absorb(sidebar_action);

        let mut view_action = ViewAction::default();
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(theme::CHROME)
                    .inner_margin(Margin::same(8)),
            )
            .show(ui, |ui| {
                Frame::new()
                    .fill(theme::BG)
                    .corner_radius(8)
                    .stroke(theme::hairline())
                    .inner_margin(Margin::same(0))
                    .show(ui, |ui| match self.view {
                        View::Page => match self.selected {
                            Some(index) if index < self.vault.capsules.len() => {
                                let ancestors: Vec<(usize, String)> = tree
                                    .ancestors(index)
                                    .into_iter()
                                    .filter_map(|idx| {
                                        self.vault
                                            .capsules
                                            .get(idx)
                                            .map(|c| (idx, c.subject.clone()))
                                    })
                                    .collect();
                                editor.absorb(draw_document(
                                    ui,
                                    &mut self.vault.capsules[index],
                                    &ancestors,
                                    self.locale,
                                    &link_context,
                                    self.document_scroll,
                                    self.ui_state.wide_canvas,
                                ));
                            }
                            _ => views::draw_empty_state(ui, &mut editor, self.locale),
                        },
                        View::Planning => view_action.absorb(views::planning_view::draw(
                            ui,
                            &mut self.vault,
                            &mut self.planning,
                            self.locale,
                            self.selected,
                        )),
                        View::Graph => view_action.absorb(views::graph_view::draw(
                            ui,
                            &self.vault.capsules,
                            &mut self.graph,
                            self.locale,
                            self.selected,
                        )),
                        View::Threads => view_action.absorb(views::threads_view::draw(
                            ui,
                            &self.vault.capsules,
                            &mut self.threads,
                            self.locale,
                            self.selected,
                        )),
                        View::Mime => {
                            let selected = self
                                .selected
                                .and_then(|index| self.vault.capsules.get(index));
                            match selected {
                                Some(capsule) => view_action.absorb(views::mime_view::draw(
                                    ui,
                                    capsule,
                                    self.locale,
                                )),
                                None => views::empty_state(
                                    ui,
                                    Icon::Code,
                                    t(self.locale, Key::MimeTitle),
                                    t(self.locale, Key::NoPageSelected),
                                ),
                            }
                        }
                        View::Mail => {
                            let selected = self
                                .selected
                                .and_then(|index| self.vault.capsules.get(index));
                            match selected {
                                Some(capsule) => view_action.absorb(views::mail_view::draw(
                                    ui,
                                    capsule,
                                    &mut self.mail,
                                    self.locale,
                                )),
                                None => views::empty_state(
                                    ui,
                                    Icon::Mail,
                                    t(self.locale, Key::ExternalPreview),
                                    t(self.locale, Key::NoPageSelected),
                                ),
                            }
                        }
                        View::Faq => {
                            // The content area stays empty; the FAQ overlay carries the view.
                        }
                    });
            });

        if editor.changed {
            self.record_change();
        }
        if let Some(old_title) = editor.rename_from.clone() {
            self.propagate_rename(&old_title);
        }
        if view_action.changed {
            self.record_change();
        }
        if let Some(toast) = view_action.toast.clone() {
            self.toast(toast);
        }
        if let Some(id) = view_action.select_id.clone() {
            self.select_by_id(&id);
        }
        if let Some(title) = editor.create_page.clone() {
            // Clicking a link to a page that does not exist yet creates it with
            // that title, so a wiki can grow from the text instead of a menu.
            self.create_page_titled(&title);
        } else if let Some(index) = view_action.select.or(editor.select).or(shell.select) {
            self.select_page(index);
        }
        if let Some((dragged_idx, new_parent_id)) = shell.reparent {
            if let Err(err) = self.vault.reparent_page(dragged_idx, new_parent_id) {
                self.toast(err);
            } else {
                self.toast(t(self.locale, Key::Reparented));
                self.refresh_links();
            }
        }
        if let Some(view) = view_action.view.or(shell.view) {
            self.view = view;
        }
        if view_action.request_palette || shell.palette {
            self.palette.open = true;
            self.palette.just_opened = true;
            self.palette.query.clear();
            self.palette.cursor = 0;
        }
        if editor.new_page || shell.new_page {
            self.new_page(None);
        }
        if editor.open_vault || shell.open_vault {
            self.open_vault_dialog();
        }
        if editor.new_subpage {
            let parent = self
                .selected
                .and_then(|index| self.vault.capsules.get(index))
                .map(|capsule| capsule.message_id.clone());
            self.new_page(parent);
        }
        if editor.cothink {
            self.show_cothink_modal = true;
        }
        if editor.faq {
            self.show_faq_modal = true;
        }
        if shell.save || view_action.request_save {
            self.save_selected();
        }
        if shell.reload {
            self.reload();
        }
        if shell.toggle_locale {
            let locale = self.locale.other();
            self.set_locale(locale);
        }
        if shell.import {
            self.import_eml();
        }
        if shell.export_vault {
            self.export_vault();
        }
        if shell.export_page {
            self.export_page();
        }
        if shell.delete_page {
            self.confirm_delete = true;
        }
        if shell.connect_mcp {
            self.show_mcp_modal = true;
        }
        if shell.faq {
            self.show_faq_modal = true;
        }
        if shell.open_imap {
            self.show_imap_modal = true;
        }
        if shell.sync_imap {
            self.trigger_imap_sync();
        }
        if shell.toggle_sidebar {
            self.toggle_sidebar();
        }
        if shell.toggle_icon_style {
            self.toggle_icon_style();
        }
        if shell.toggle_zen {
            self.toggle_zen_mode();
        }
        if editor.toggle_wide {
            self.toggle_wide_canvas();
        }

        // Command palette: a pure overlay; the shell performs whatever it picks.
        if let Some(command) =
            views::palette::draw(&ctx, &self.vault.capsules, self.locale, &mut self.palette)
        {
            self.run_command(&ctx, command);
        }

        match modals::draw_confirm_delete(self.confirm_delete, self.locale, &ctx) {
            modals::ConfirmDeleteAction::Confirm => {
                self.delete_selected_page();
                self.confirm_delete = false;
            }
            modals::ConfirmDeleteAction::Cancel => {
                self.confirm_delete = false;
            }
            modals::ConfirmDeleteAction::None => {}
        }

        match modals::draw_mcp_modal(
            self.show_mcp_modal,
            &mut self.mcp_tab,
            &self.vault.dir,
            self.locale,
            &ctx,
        ) {
            modals::McpModalAction::Close => self.show_mcp_modal = false,
            modals::McpModalAction::CopyConfig(text) => {
                ctx.copy_text(text);
                self.toast(t(self.locale, Key::ConfigCopied));
            }
            modals::McpModalAction::None => {}
        }

        match modals::draw_imap_modal(
            self.show_imap_modal,
            &mut self.imap_config,
            &self.imap_status,
            self.locale,
            &ctx,
        ) {
            modals::ImapModalAction::Close => self.show_imap_modal = false,
            modals::ImapModalAction::SaveSettings => {
                let _ = self.imap_config.save_to_vault(&self.vault.dir);
                self.toast(t(self.locale, Key::Saved));
            }
            modals::ImapModalAction::TestConnection => {
                let engine = sync_imap::ImapSyncEngine::new(
                    self.imap_config.clone(),
                    self.vault.dir.clone(),
                );
                match engine.test_connection() {
                    Ok(msg) => self.imap_status = sync_imap::SyncStatus::Success(msg),
                    Err(err) => self.imap_status = sync_imap::SyncStatus::Error(err),
                }
            }
            modals::ImapModalAction::SyncNow => {
                let _ = self.imap_config.save_to_vault(&self.vault.dir);
                self.trigger_imap_sync();
            }
            modals::ImapModalAction::None => {}
        }

        match modals::draw_cothink_modal(
            self.show_cothink_modal,
            &mut self.cothink_mode,
            &mut self.cothink_focus,
            self.locale,
            &ctx,
        ) {
            modals::CoThinkModalAction::Close => self.show_cothink_modal = false,
            modals::CoThinkModalAction::Run => {
                self.execute_cothink();
                self.show_cothink_modal = false;
            }
            modals::CoThinkModalAction::None => {}
        }

        if modals::draw_faq_modal(self.show_faq_modal, self.locale, &ctx) {
            self.show_faq_modal = false;
        }

        let encrypt_action = modals::draw_encrypt_modal(&mut self.encrypt, self.locale, &ctx);
        match encrypt_action {
            modals::EncryptAction::Confirm(passphrase) => {
                if let Some(mode) = self.encrypt.mode.take() {
                    match mode {
                        modals::EncryptMode::Encrypt { file_name } => {
                            self.finish_encrypt_capsule(&file_name, &passphrase);
                        }
                        modals::EncryptMode::Decrypt { file_name } => {
                            self.finish_decrypt_asc(&file_name, &passphrase);
                        }
                    }
                }
                self.encrypt.close();
            }
            modals::EncryptAction::Cancel | modals::EncryptAction::None => {}
        }

        modals::draw_toast(&mut self.toast, &ctx);

        if self.dirty && self.last_edit.elapsed() > Duration::from_millis(750) {
            self.save_selected();
        }

        chrome::window_edges(&ctx);
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

impl NativeApp {
    fn toggle_zen_mode(&mut self) {
        self.zen_mode = !self.zen_mode;
        if self.zen_mode {
            self.toast(t(self.locale, Key::ZenModeEntered));
        } else {
            self.toast(t(self.locale, Key::ZenModeExited));
        }
    }

    fn toggle_sidebar(&mut self) {
        self.ui_state.toggle_sidebar();
        self.ui_state.save();
    }

    fn toggle_icon_style(&mut self) {
        self.ui_state.toggle_icon_style();
        self.ui_state.save();
        self.toast(t(self.locale, Key::IconStyleToast));
    }

    fn zoom_in(&mut self, ctx: &egui::Context) {
        let zoom = self.ui_state.zoom_in();
        ctx.set_zoom_factor(zoom);
        self.ui_state.save();
        let pct = (zoom * 100.0).round() as u32;
        self.toast(format!("{}: {}%", t(self.locale, Key::ZoomLevel), pct));
    }

    fn zoom_out(&mut self, ctx: &egui::Context) {
        let zoom = self.ui_state.zoom_out();
        ctx.set_zoom_factor(zoom);
        self.ui_state.save();
        let pct = (zoom * 100.0).round() as u32;
        self.toast(format!("{}: {}%", t(self.locale, Key::ZoomLevel), pct));
    }

    fn reset_zoom(&mut self, ctx: &egui::Context) {
        let zoom = self.ui_state.reset_zoom();
        ctx.set_zoom_factor(zoom);
        self.ui_state.save();
        let pct = (zoom * 100.0).round() as u32;
        self.toast(format!("{}: {}%", t(self.locale, Key::ZoomLevel), pct));
    }

    fn toggle_wide_canvas(&mut self) {
        let wide = self.ui_state.toggle_wide_canvas();
        self.ui_state.save();
        let label = if wide { "Wide" } else { "Standard (960px)" };
        self.toast(format!(
            "{}: {}",
            t(self.locale, Key::CommandWideCanvas),
            label
        ));
    }

    fn run_command(&mut self, ctx: &egui::Context, command: PaletteCommand) {
        match command {
            PaletteCommand::NewPage => self.new_page(None),
            PaletteCommand::SavePage => {
                self.save_selected();
            }
            PaletteCommand::Undo => self.undo(),
            PaletteCommand::Redo => self.redo(),
            PaletteCommand::ExportPage => self.export_page(),
            PaletteCommand::ExportMarkdown => self.export_markdown(),
            PaletteCommand::ExportVault => self.export_vault(),
            PaletteCommand::ImportEml => self.import_eml(),
            PaletteCommand::ImportObsidian => self.import_obsidian_dialog(),
            PaletteCommand::ToggleLocale => {
                let locale = self.locale.other();
                self.set_locale(locale);
            }
            PaletteCommand::SetView(view) => self.view = view,
            PaletteCommand::OpenPage(index) => {
                self.select_page(index);
                self.view = View::Page;
            }
            PaletteCommand::DeletePage => self.confirm_delete = true,
            PaletteCommand::ConnectMcp => self.show_mcp_modal = true,
            PaletteCommand::Faq => self.show_faq_modal = true,
            PaletteCommand::CoThink => self.show_cothink_modal = true,
            PaletteCommand::CoThinkMode(mode) => {
                self.cothink_mode = mode;
                self.show_cothink_modal = true;
            }
            PaletteCommand::OpenImapSync => self.show_imap_modal = true,
            PaletteCommand::SyncImapNow => self.trigger_imap_sync(),
            PaletteCommand::ExportEncrypted => self.begin_encrypt_capsule(),
            PaletteCommand::ImportEncrypted => self.begin_decrypt_asc(),
            PaletteCommand::ToggleZenMode => self.toggle_zen_mode(),
            PaletteCommand::ToggleSidebar => self.toggle_sidebar(),
            PaletteCommand::ZoomIn => self.zoom_in(ctx),
            PaletteCommand::ZoomOut => self.zoom_out(ctx),
            PaletteCommand::ResetZoom => self.reset_zoom(ctx),
            PaletteCommand::ToggleWideCanvas => self.toggle_wide_canvas(),
            PaletteCommand::ToggleIconStyle => self.toggle_icon_style(),
        }
    }

    fn select_by_id(&mut self, id: &str) {
        if let Some(index) = self
            .vault
            .capsules
            .iter()
            .position(|capsule| capsule.message_id == id)
        {
            self.select_page(index);
            self.view = View::Page;
        }
    }

    fn trigger_imap_sync(&mut self) {
        if !self.imap_config.enabled {
            self.show_imap_modal = true;
            return;
        }

        self.imap_status = sync_imap::SyncStatus::Syncing;
        let engine =
            sync_imap::ImapSyncEngine::new(self.imap_config.clone(), self.vault.dir.clone());
        match engine.run_sync() {
            Ok(msg) => {
                self.imap_status = sync_imap::SyncStatus::Success(msg.clone());
                self.toast(msg);
                self.reload();
            }
            Err(err) => {
                let err_msg = err.clone();
                self.imap_status = sync_imap::SyncStatus::Error(err);
                self.toast(format!(
                    "{}: {err_msg}",
                    t(self.locale, Key::ImapErrorToast)
                ));
            }
        }
    }

    fn execute_cothink(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        let Some(capsule) = self.vault.capsules.get(index) else {
            return;
        };

        let mut csv_data = Vec::new();
        for att in &capsule.attachments {
            if att.name.ends_with(".csv") || att.mime_type.contains("csv") {
                csv_data.push((att.name.as_str(), att.content.as_str()));
            }
        }

        let outcome = cothinker::generate_cothink(
            self.cothink_mode,
            &capsule.subject,
            &capsule.blocks,
            &csv_data,
            &self.cothink_focus,
        );

        match outcome {
            cothinker::CoThinkOutcome::OpponentBlock(b) => {
                let capsule = &mut self.vault.capsules[index];
                if let Some(pos) = capsule
                    .blocks
                    .iter()
                    .position(|blk| blk["type"] == "ai_opponent_block")
                {
                    capsule.blocks[pos] = b;
                } else {
                    capsule.blocks.insert(0, b);
                }
                self.dirty = true;
                self.last_edit = Instant::now();
                self.toast(t(self.locale, Key::CoThinkWorked));
            }
            cothinker::CoThinkOutcome::CalloutBlock(b) => {
                let capsule = &mut self.vault.capsules[index];
                capsule.blocks.insert(0, b);
                self.dirty = true;
                self.last_edit = Instant::now();
                self.toast(t(self.locale, Key::CoThinkWorked));
            }
            cothinker::CoThinkOutcome::ReplyCapsule {
                subject, blocks, ..
            } => {
                let parent_id = Some(capsule.message_id.clone());
                self.create_reply_capsule(parent_id, &subject, blocks);
            }
        }
    }

    fn create_reply_capsule(
        &mut self,
        parent_id: Option<String>,
        subject: &str,
        blocks: Vec<Value>,
    ) {
        if !self.save_selected() {
            return;
        }
        match self.vault.create_page(parent_id) {
            Ok(index) => {
                if let Some(capsule) = self.vault.capsules.get_mut(index) {
                    capsule.subject = subject.trim().to_string();
                    capsule.blocks = blocks;
                }
                self.selected = Some(index);
                self.dirty = true;
                self.status = t(self.locale, Key::ReplyCapsuleCreated).to_string();
                self.status_error = false;
                self.save_selected();
                self.refresh_links();
                self.toast(t(self.locale, Key::ReplyCapsuleCreated));
            }
            Err(err) => {
                self.status = err;
                self.status_error = true;
            }
        }
    }
}
