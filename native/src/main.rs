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

mod brand;
mod chrome;
mod cli;
mod cothinker;
mod export;
mod graph;
mod history;
mod i18n;
mod icons;
mod importers;
mod links;
mod mail;
mod mcp;
mod mime;
mod planning;
mod search;
mod snapshot;
mod theme;
mod vault;
mod views;
mod watch;
mod sync_imap;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, FontFamily, FontId, Frame, Galley, Label, Layout,
    Margin, Pos2, Rect, Response, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui, UiBuilder,
    Vec2,
};
use i18n::{t, Key, Locale};
use icons::Icon;
use mime::{block_type, new_block_id, Attachment, Capsule};
use serde_json::{json, Value};
use vault::{default_vault_dir, PageTree, Vault};
use views::graph_view::GraphState;
use views::mail_view::MailState;
use views::palette::{Command as PaletteCommand, PaletteState};
use views::planning_view::PlanningState;
use views::threads_view::ThreadsState;
use views::{View, ViewAction};

const CONTENT_MAX_WIDTH: f32 = 752.0;
const SIDEBAR_WIDTH: f32 = 262.0;
const GUTTER: f32 = 26.0;

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
                println!(
                    "- Attachments imported: {}",
                    summary.imported_attachments
                );
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
    if let Some(icon) = brand::window_icon() {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "CapsuleCraft",
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(NativeApp::new(launch)))
        }),
    )
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
    cothink_mode: cothinker::CoThinkMode,
    cothink_focus: String,
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
}

struct SnapshotJob {
    path: PathBuf,
    frames_left: u32,
    requested: bool,
}

impl NativeApp {
    fn new(launch: cli::Launch) -> Self {
        let dir = launch.vault.clone().unwrap_or_else(vault::get_active_vault_dir);
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
            view: launch.view.unwrap_or(View::Page),
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
            cothink_mode: cothinker::CoThinkMode::Critique,
            cothink_focus: String::new(),
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
        }
        .with_links();
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
            .map(|capsule| {
                search::related(&self.search_index, &capsule.message_id, &ids, 0.08, 5)
            })
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
            .or_else(|| self.link_index.iter().find(|(subject, _, _)| subject == &wanted))
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
        let stem = capsule.file_name.trim_end_matches(".eml").trim_end_matches(".EML");
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
    let ext = path.extension().and_then(|ext| ext.to_str()).unwrap_or("eml");
    for suffix in 2..1000 {
        let candidate = dir.join(format!("{stem}-{suffix}.{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}-{}.{ext}", now_millis()))
}

#[derive(Default)]
struct ShellAction {
    select: Option<usize>,
    open_vault: bool,
    new_page: bool,
    reload: bool,
    save: bool,
    view: Option<View>,
    toggle_locale: bool,
    palette: bool,
    connect_mcp: bool,
    open_imap: bool,
    sync_imap: bool,
    import: bool,
    export_vault: bool,
    export_page: bool,
    delete_page: bool,
    reparent: Option<(usize, Option<String>)>,
}

impl ShellAction {
    fn absorb(&mut self, other: ShellAction) {
        self.select = other.select.or(self.select);
        self.open_vault |= other.open_vault;
        self.new_page |= other.new_page;
        self.reload |= other.reload;
        self.save |= other.save;
        self.view = other.view.or(self.view);
        self.toggle_locale |= other.toggle_locale;
        self.palette |= other.palette;
        self.connect_mcp |= other.connect_mcp;
        self.open_imap |= other.open_imap;
        self.sync_imap |= other.sync_imap;
        self.import |= other.import;
        self.export_vault |= other.export_vault;
        self.export_page |= other.export_page;
        self.delete_page |= other.delete_page;
        self.reparent = other.reparent.or_else(|| self.reparent.take());
    }
}

#[derive(Default)]
struct EditorAction {
    changed: bool,
    select: Option<usize>,
    new_subpage: bool,
    new_page: bool,
    open_vault: bool,
    cothink: bool,
    view: Option<View>,
    /// Title of a page that a `[[link]]` points at but which does not exist.
    create_page: Option<String>,
    /// Previous subject of the page being renamed, so links can follow it.
    rename_from: Option<String>,
}

impl EditorAction {
    fn absorb(&mut self, other: EditorAction) {
        self.changed |= other.changed;
        self.select = other.select.or(self.select);
        self.new_subpage |= other.new_subpage;
        self.new_page |= other.new_page;
        self.open_vault |= other.open_vault;
        self.cothink |= other.cothink;
        self.view = other.view.or(self.view);
        self.create_page = other.create_page.or_else(|| self.create_page.take());
        self.rename_from = other.rename_from.or_else(|| self.rename_from.take());
    }
}

/// What the document knows about links before it borrows the page mutably.
#[derive(Default)]
struct LinkContext {
    /// `(title as written, resolved page index)` for every outgoing link.
    outgoing: Vec<(String, Option<usize>)>,
    /// Pages that link here: `(index, subject)`.
    backlinks: Vec<(usize, String)>,
    /// Closest pages by meaning: `(index, subject, similarity 0..1)`.
    related: Vec<(usize, String, f32)>,
}

impl eframe::App for NativeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.sync_snapshot();
        self.drive_snapshot(&ctx);
        self.poll_watcher();

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
        if cmd && shift && ctx.input(|i| i.key_pressed(egui::Key::A)) {
            self.show_cothink_modal = true;
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
        if ctx.input(|i| i.viewport().close_requested()) && !self.save_selected() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }

        let tree = PageTree::build(&self.vault.capsules);
        let mut shell = ShellAction::default();
        // Links are resolved before the document borrows its page mutably.
        let link_context = self.current_link_context();

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
                    self.view,
                    self.locale,
                ));
            });

        let sidebar_action = {
            egui::Panel::left("sidebar")
                .exact_size(SIDEBAR_WIDTH)
                .resizable(false)
                .frame(
                    Frame::new()
                        .fill(theme::CHROME)
                        .inner_margin(Margin::same(0)),
                )
                .show(ui, |ui| {
                    draw_sidebar(
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
                })
                .inner
        };
        shell.absorb(sidebar_action);

        let mut editor = EditorAction::default();
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
                        View::Page => {
                            editor.absorb(draw_content_header(
                                ui,
                                self.selected,
                                &self.vault.capsules,
                                &tree,
                                self.locale,
                            ));
                            match self.selected {
                                Some(index) if index < self.vault.capsules.len() => {
                                    editor.absorb(draw_document(
                                        ui,
                                        &mut self.vault.capsules[index],
                                        self.locale,
                                        &link_context,
                                        self.document_scroll,
                                    ));
                                }
                                _ => draw_empty_state(ui, &mut editor, self.locale),
                            }
                        }
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
                                Some(capsule) => view_action.absorb(
                                    views::mime_view::draw(ui, capsule, self.locale),
                                ),
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
        if shell.open_imap {
            self.show_imap_modal = true;
        }
        if shell.sync_imap {
            self.trigger_imap_sync();
        }

        // Command palette: a pure overlay; the shell performs whatever it picks.
        if let Some(command) =
            views::palette::draw(&ctx, &self.vault.capsules, self.locale, &mut self.palette)
        {
            self.run_command(command);
        }

        self.draw_confirm_delete(&ctx);
        self.draw_mcp_modal(&ctx);
        self.draw_imap_modal(&ctx);
        self.draw_cothink_modal(&ctx);
        self.draw_toast(&ctx);

        if self.dirty && self.last_edit.elapsed() > Duration::from_millis(750) {
            self.save_selected();
        }

        chrome::window_edges(&ctx);
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

impl NativeApp {
    fn run_command(&mut self, command: PaletteCommand) {
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
            PaletteCommand::CoThink => self.show_cothink_modal = true,
            PaletteCommand::CoThinkMode(mode) => {
                self.cothink_mode = mode;
                self.show_cothink_modal = true;
            }
            PaletteCommand::OpenImapSync => self.show_imap_modal = true,
            PaletteCommand::SyncImapNow => self.trigger_imap_sync(),
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

    fn draw_confirm_delete(&mut self, ctx: &egui::Context) {
        if !self.confirm_delete {
            return;
        }
        let mut close = false;
        let mut confirm = false;
        egui::Area::new(egui::Id::new("confirm-delete"))
            .order(egui::Order::Foreground)
            .fixed_pos(ctx.content_rect().min)
            .show(ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(ctx.content_rect().size(), Sense::click());
                ui.painter()
                    .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(150));
                let panel = Rect::from_center_size(rect.center(), vec2(380.0, 150.0));
                ui.painter().rect(
                    panel,
                    theme::RADIUS,
                    theme::PANEL,
                    Stroke::new(1.0, theme::LINE_STRONG),
                    egui::StrokeKind::Inside,
                );
                let mut body = ui.new_child(
                    UiBuilder::new()
                        .max_rect(panel.shrink(20.0))
                        .layout(Layout::top_down(Align::Min)),
                );
                body.label(
                    RichText::new(t(self.locale, Key::DeletePage))
                        .family(theme::medium())
                        .size(14.0)
                        .color(theme::INK),
                );
                body.add_space(6.0);
                body.add(
                    Label::new(
                        RichText::new(t(self.locale, Key::DeletePageConfirm))
                            .size(12.0)
                            .color(theme::MUTED),
                    )
                    .wrap(),
                );
                body.add_space(14.0);
                body.horizontal(|ui| {
                    if primary_button(ui, t(self.locale, Key::Delete), 28.0).clicked() {
                        confirm = true;
                    }
                    if secondary_button(ui, None, t(self.locale, Key::Cancel), 28.0).clicked() {
                        close = true;
                    }
                });
            });
        if confirm {
            self.delete_selected_page();
        }
        if close {
            self.confirm_delete = false;
        }
    }

    fn draw_mcp_modal(&mut self, ctx: &egui::Context) {
        if !self.show_mcp_modal {
            return;
        }

        let exe = std::env::current_exe()
            .unwrap_or_else(|_| PathBuf::from("capsuleos-native.exe"));
        let vault = self.vault.dir.clone();
        let cfg = mcp::mcp_config_json(&exe, &vault);
        let cfg_str = serde_json::to_string_pretty(&cfg).unwrap_or_default();
        let claude_cmd = format!(
            "claude mcp add capsulecraft \"{}\" --mcp \"{}\"",
            exe.display(),
            vault.display()
        );

        let mut close = false;
        let mut copy_target: Option<String> = None;

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }

        egui::Area::new(egui::Id::new("mcp-connect-modal"))
            .order(egui::Order::Foreground)
            .fixed_pos(ctx.content_rect().min)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(ctx.content_rect().size(), Sense::click());
                if resp.clicked() {
                    close = true;
                }
                ui.painter()
                    .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(150));

                let panel = Rect::from_center_size(rect.center(), vec2(580.0, 420.0));
                ui.painter().rect(
                    panel,
                    theme::RADIUS,
                    theme::PANEL,
                    Stroke::new(1.0, theme::LINE_STRONG),
                    egui::StrokeKind::Inside,
                );

                let mut body = ui.new_child(
                    UiBuilder::new()
                        .max_rect(panel.shrink(20.0))
                        .layout(Layout::top_down(Align::Min)),
                );

                body.label(
                    RichText::new(t(self.locale, Key::McpTitle))
                        .family(theme::medium())
                        .size(15.0)
                        .color(theme::INK),
                );
                body.add_space(4.0);
                body.add(
                    Label::new(
                        RichText::new(t(self.locale, Key::McpSubtitle))
                            .size(12.0)
                            .color(theme::MUTED),
                    )
                    .wrap(),
                );
                body.add_space(12.0);

                // Tabs for clients
                body.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let tabs = [
                        (0, t(self.locale, Key::McpTabAntigravity)),
                        (1, t(self.locale, Key::McpTabClaudeDesktop)),
                        (2, t(self.locale, Key::McpTabClaudeCode)),
                        (3, t(self.locale, Key::McpTabCursor)),
                    ];
                    for (idx, label) in tabs {
                        let active = self.mcp_tab == idx;
                        let bg = if active { theme::CONTROLS } else { Color32::TRANSPARENT };
                        let frame = Frame::new()
                            .fill(bg)
                            .corner_radius(theme::RADIUS)
                            .inner_margin(Margin::symmetric(10, 5));
                        let resp = frame.show(ui, |ui| {
                            let text_color = if active { theme::INK } else { theme::MUTED };
                            ui.label(RichText::new(label).family(theme::medium()).size(12.0).color(text_color))
                        }).response;
                        if resp.interact(Sense::click()).clicked() {
                            self.mcp_tab = idx;
                        }
                    }
                });

                body.add_space(10.0);

                let (desc, content_to_copy) = match self.mcp_tab {
                    0 => (
                        "Config location: ~/.gemini/antigravity/mcp/ or AGY workspace mcp.json",
                        cfg_str.clone(),
                    ),
                    1 => (
                        "Config location: %APPDATA%\\Claude\\claude_desktop_config.json",
                        cfg_str.clone(),
                    ),
                    2 => (
                        "Terminal command to register the MCP server with Claude CLI:",
                        claude_cmd.clone(),
                    ),
                    _ => (
                        "Config location: .cursor/mcp.json",
                        cfg_str.clone(),
                    ),
                };

                body.label(RichText::new(desc).size(11.0).color(theme::FAINT));
                body.add_space(6.0);

                let text_frame = Frame::new()
                    .fill(theme::SUNKEN)
                    .corner_radius(theme::RADIUS)
                    .stroke(Stroke::new(1.0, theme::LINE))
                    .inner_margin(Margin::same(10));

                text_frame.show(&mut body, |ui| {
                    ScrollArea::vertical()
                        .max_height(160.0)
                        .show(ui, |ui| {
                            ui.add(
                                Label::new(
                                    RichText::new(&content_to_copy)
                                        .family(FontFamily::Monospace)
                                        .size(11.0)
                                        .color(theme::INK),
                                )
                                .wrap(),
                            );
                        });
                });

                body.add_space(14.0);

                body.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if secondary_button(ui, Some(Icon::Close), t(self.locale, Key::Close), 28.0).clicked() {
                        close = true;
                    }
                    ui.add_space(8.0);
                    if secondary_button(ui, Some(Icon::Copy), t(self.locale, Key::CopyConfig), 28.0).clicked() {
                        copy_target = Some(content_to_copy.clone());
                    }
                });
            });

        if let Some(to_copy) = copy_target {
            ctx.copy_text(to_copy);
            self.toast(t(self.locale, Key::ConfigCopied));
        }

        if close {
            self.show_mcp_modal = false;
        }
    }

    fn draw_imap_modal(&mut self, ctx: &egui::Context) {
        if !self.show_imap_modal {
            return;
        }

        let mut close = false;
        let mut test_conn = false;
        let mut save_cfg = false;
        let mut sync_now = false;

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }

        egui::Area::new(egui::Id::new("imap-sync-modal"))
            .order(egui::Order::Foreground)
            .fixed_pos(ctx.content_rect().min)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(ctx.content_rect().size(), Sense::click());
                if resp.clicked() {
                    close = true;
                }
                ui.painter()
                    .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(150));

                let panel = Rect::from_center_size(rect.center(), vec2(520.0, 480.0));
                ui.painter().rect(
                    panel,
                    theme::RADIUS,
                    theme::PANEL,
                    Stroke::new(1.0, theme::LINE_STRONG),
                    egui::StrokeKind::Inside,
                );

                let mut body = ui.new_child(
                    UiBuilder::new()
                        .max_rect(panel.shrink(22.0))
                        .layout(Layout::top_down(Align::Min)),
                );

                body.label(
                    RichText::new(t(self.locale, Key::ImapSyncTitle))
                        .family(theme::medium())
                        .size(15.0)
                        .color(theme::INK),
                );
                body.add_space(4.0);
                body.add(
                    Label::new(
                        RichText::new(t(self.locale, Key::ImapSyncDesc))
                            .size(12.0)
                            .color(theme::MUTED),
                    )
                    .wrap(),
                );
                body.add_space(14.0);

                // Enable checkbox
                body.checkbox(&mut self.imap_config.enabled, t(self.locale, Key::ImapEnable));
                body.add_space(8.0);

                // Fields
                body.label(RichText::new(t(self.locale, Key::ImapHost)).size(11.0).color(theme::MUTED));
                body.add(TextEdit::singleline(&mut self.imap_config.host).hint_text("imap.gmail.com").desired_width(f32::INFINITY));
                body.add_space(6.0);

                body.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(t(self.locale, Key::ImapPort)).size(11.0).color(theme::MUTED));
                        let mut port_str = self.imap_config.port.to_string();
                        if ui.add(TextEdit::singleline(&mut port_str).desired_width(100.0)).changed() {
                            if let Ok(p) = port_str.parse::<u16>() {
                                self.imap_config.port = p;
                            }
                        }
                    });
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(t(self.locale, Key::ImapFolder)).size(11.0).color(theme::MUTED));
                        ui.add(TextEdit::singleline(&mut self.imap_config.folder).hint_text("CapsuleCraft").desired_width(f32::INFINITY));
                    });
                });
                body.add_space(6.0);

                body.label(RichText::new(t(self.locale, Key::ImapUsername)).size(11.0).color(theme::MUTED));
                body.add(TextEdit::singleline(&mut self.imap_config.username).hint_text("user@example.com").desired_width(f32::INFINITY));
                body.add_space(6.0);

                body.label(RichText::new(t(self.locale, Key::ImapPassword)).size(11.0).color(theme::MUTED));
                body.add(TextEdit::singleline(&mut self.imap_config.password).password(true).desired_width(f32::INFINITY));
                body.add_space(16.0);

                // Status info
                let status_text = match &self.imap_status {
                    sync_imap::SyncStatus::Idle => String::new(),
                    sync_imap::SyncStatus::Syncing => t(self.locale, Key::ImapSyncing).to_string(),
                    sync_imap::SyncStatus::Success(s) => s.clone(),
                    sync_imap::SyncStatus::Error(e) => format!("Error: {e}"),
                };
                if !status_text.is_empty() {
                    let color = match &self.imap_status {
                        sync_imap::SyncStatus::Error(_) => theme::BAD,
                        sync_imap::SyncStatus::Syncing => theme::BADGE_BETA,
                        _ => theme::ACCENT,
                    };
                    body.label(RichText::new(status_text).size(11.5).color(color));
                    body.add_space(8.0);
                }

                // Buttons row
                body.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if secondary_button(ui, Some(Icon::Close), t(self.locale, Key::Close), 28.0).clicked() {
                        close = true;
                    }
                    ui.add_space(8.0);
                    if primary_button(ui, t(self.locale, Key::ImapSaveSettings), 28.0).clicked() {
                        save_cfg = true;
                    }
                    ui.add_space(8.0);
                    if secondary_button(ui, None, t(self.locale, Key::ImapTestConnection), 28.0).clicked() {
                        test_conn = true;
                    }
                    if self.imap_config.enabled {
                        ui.add_space(8.0);
                        if secondary_button(ui, Some(Icon::Reload), t(self.locale, Key::ImapSyncNow), 28.0).clicked() {
                            sync_now = true;
                        }
                    }
                });
            });

        if save_cfg {
            let _ = self.imap_config.save_to_vault(&self.vault.dir);
            self.toast(t(self.locale, Key::Saved));
        }

        if test_conn {
            let engine = sync_imap::ImapSyncEngine::new(self.imap_config.clone(), self.vault.dir.clone());
            match engine.test_connection() {
                Ok(msg) => self.imap_status = sync_imap::SyncStatus::Success(msg),
                Err(err) => self.imap_status = sync_imap::SyncStatus::Error(err),
            }
        }

        if sync_now {
            let _ = self.imap_config.save_to_vault(&self.vault.dir);
            self.trigger_imap_sync();
        }

        if close {
            self.show_imap_modal = false;
        }
    }

    fn trigger_imap_sync(&mut self) {
        if !self.imap_config.enabled {
            self.show_imap_modal = true;
            return;
        }

        self.imap_status = sync_imap::SyncStatus::Syncing;
        let engine = sync_imap::ImapSyncEngine::new(self.imap_config.clone(), self.vault.dir.clone());
        match engine.run_sync() {
            Ok(msg) => {
                self.imap_status = sync_imap::SyncStatus::Success(msg.clone());
                self.toast(msg);
                self.reload();
            }
            Err(err) => {
                let err_msg = err.clone();
                self.imap_status = sync_imap::SyncStatus::Error(err);
                self.toast(format!("{}: {err_msg}", t(self.locale, Key::ImapErrorToast)));
            }
        }
    }

    fn draw_cothink_modal(&mut self, ctx: &egui::Context) {
        if !self.show_cothink_modal {
            return;
        }

        let mut close = false;
        let mut run = false;

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }

        egui::Area::new(egui::Id::new("cothink-modal"))
            .order(egui::Order::Foreground)
            .fixed_pos(ctx.content_rect().min)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(ctx.content_rect().size(), Sense::click());
                if resp.clicked() {
                    close = true;
                }
                ui.painter()
                    .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(150));

                let panel = Rect::from_center_size(rect.center(), vec2(540.0, 390.0));
                ui.painter().rect(
                    panel,
                    theme::RADIUS,
                    theme::PANEL,
                    Stroke::new(1.0, theme::LINE_STRONG),
                    egui::StrokeKind::Inside,
                );

                let mut body = ui.new_child(
                    UiBuilder::new()
                        .max_rect(panel.shrink(22.0))
                        .layout(Layout::top_down(Align::Min)),
                );

                body.horizontal(|ui| {
                    ui.label(
                        RichText::new(t(self.locale, Key::CoThinkTitle))
                            .family(theme::medium())
                            .size(15.0)
                            .color(theme::INK),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ghost(ui, Icon::Close, None, t(self.locale, Key::Close), true, 26.0).clicked() {
                            close = true;
                        }
                    });
                });
                body.add_space(4.0);
                body.add(
                    Label::new(
                        RichText::new(t(self.locale, Key::CoThinkSubtitle))
                            .size(12.0)
                            .color(theme::MUTED),
                    )
                    .wrap(),
                );
                body.add_space(14.0);

                let modes = [
                    (cothinker::CoThinkMode::Critique, Key::Critique, Key::CritiqueDesc),
                    (cothinker::CoThinkMode::Synthesis, Key::Synthesis, Key::SynthesisDesc),
                    (cothinker::CoThinkMode::CounterArg, Key::CounterArg, Key::CounterArgDesc),
                    (cothinker::CoThinkMode::DataInsight, Key::DataInsight, Key::DataInsightDesc),
                ];

                for (mode, title_key, desc_key) in modes {
                    let active = self.cothink_mode == mode;
                    let (fill, stroke_color, text_color) = if active {
                        (theme::WASH_SELECTED, theme::ACCENT, theme::ACCENT_INK)
                    } else {
                        (theme::SUNKEN, theme::hairline().color, theme::INK)
                    };

                    let frame = Frame::new()
                        .fill(fill)
                        .corner_radius(theme::RADIUS)
                        .stroke(Stroke::new(1.0, stroke_color))
                        .inner_margin(Margin { left: 10, right: 10, top: 6, bottom: 6 });

                    let resp = frame.show(&mut body, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(t(self.locale, title_key))
                                    .family(theme::medium())
                                    .size(13.0)
                                    .color(text_color),
                            );
                            ui.label(
                                RichText::new(format!("— {}", t(self.locale, desc_key)))
                                    .size(11.5)
                                    .color(theme::MUTED),
                            );
                        });
                    }).response;

                    if resp.interact(Sense::click()).clicked() {
                        self.cothink_mode = mode;
                    }
                    body.add_space(6.0);
                }

                body.add_space(8.0);
                body.label(
                    RichText::new(t(self.locale, Key::CoThinkFocus))
                        .size(12.0)
                        .color(theme::MUTED),
                );
                body.add_space(4.0);
                let text_edit = egui::TextEdit::singleline(&mut self.cothink_focus)
                    .hint_text(t(self.locale, Key::CoThinkFocusPlaceholder))
                    .desired_width(body.available_width());
                body.add(text_edit);

                body.add_space(14.0);
                body.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if secondary_button(ui, Some(Icon::Close), t(self.locale, Key::Cancel), 28.0).clicked() {
                        close = true;
                    }
                    ui.add_space(8.0);
                    if primary_button(ui, t(self.locale, Key::CoThinkRun), 28.0).clicked() {
                        run = true;
                    }
                });
            });

        if run {
            self.execute_cothink();
            close = true;
        }

        if close {
            self.show_cothink_modal = false;
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
                if let Some(pos) = capsule.blocks.iter().position(|blk| blk["type"] == "ai_opponent_block") {
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
            cothinker::CoThinkOutcome::ReplyCapsule { subject, blocks, .. } => {
                let parent_id = Some(capsule.message_id.clone());
                self.create_reply_capsule(parent_id, &subject, blocks);
            }
        }
    }

    fn create_reply_capsule(&mut self, parent_id: Option<String>, subject: &str, blocks: Vec<Value>) {
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

    fn draw_toast(&mut self, ctx: &egui::Context) {
        let Some((text, at)) = self.toast.clone() else {
            return;
        };
        let elapsed = at.elapsed();
        if elapsed > Duration::from_millis(2600) {
            self.toast = None;
            return;
        }
        let alpha = if elapsed > Duration::from_millis(2100) {
            (1.0 - (elapsed.as_millis() as f32 - 2100.0) / 500.0).clamp(0.0, 1.0)
        } else {
            1.0
        };
    let screen = ctx.content_rect();
        let galley = ctx.fonts_mut(|fonts| {
            fonts.layout_no_wrap(
                text.clone(),
                egui::FontId::new(12.0, theme::medium()),
                theme::INK,
            )
        });
        let size = galley.size() + vec2(28.0, 18.0);
        let rect = Rect::from_min_size(
            pos2(screen.center().x - size.x / 2.0, screen.bottom() - size.y - 34.0),
            size,
        );
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("toast"),
        ));
        painter.rect(
            rect,
            theme::RADIUS,
            theme::fade(theme::PANEL, alpha),
            Stroke::new(1.0, theme::fade(theme::LINE_STRONG, alpha)),
            egui::StrokeKind::Inside,
        );
        painter.galley(
            rect.min + vec2(14.0, 9.0),
            galley,
            theme::fade(theme::INK, alpha),
        );
        ctx.request_repaint();
    }
}

// ---------------------------------------------------------------------------
// Shell: title bar
// ---------------------------------------------------------------------------

fn draw_titlebar(
    ui: &mut Ui,
    vault: &Vault,
    dirty: bool,
    status: &str,
    status_error: bool,
    imap_config: &sync_imap::ImapConfig,
    imap_status: &sync_imap::SyncStatus,
    view: View,
    locale: Locale,
) -> ShellAction {
    let mut action = ShellAction::default();
    chrome::title_bar_drag(ui);

    let total_rect = ui.max_rect();

    // 1. Right utility controls: rendered first with right-to-left layout so they
    // are guaranteed visible, anchored, and have strict layout boundaries.
    let right_width = if chrome::custom_caption_buttons() { 360.0 } else { 240.0 };
    let right_rect = Rect::from_min_max(
        pos2((total_rect.right() - right_width).max(total_rect.left()), total_rect.top()),
        total_rect.right_bottom(),
    );
    let mut right_bar = ui.new_child(
        UiBuilder::new()
            .max_rect(right_rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    let right_ui = &mut right_bar;
    right_ui.spacing_mut().item_spacing.x = 2.0;
    if chrome::custom_caption_buttons() {
        chrome::caption_buttons(right_ui, chrome::TITLEBAR_HEIGHT);
    }
    right_ui.add_space(8.0);

    if ghost(
        right_ui,
        Icon::Command,
        Some("K"),
        t(locale, Key::PaletteHint),
        true,
        26.0,
    )
    .clicked()
    {
        action.palette = true;
    }
    if ghost(
        right_ui,
        Icon::Code,
        Some("MCP"),
        t(locale, Key::ConnectMcp),
        true,
        26.0,
    )
    .clicked()
    {
        action.connect_mcp = true;
    }
    let (imap_color, imap_hint) = if !imap_config.enabled {
        (theme::MUTED, t(locale, Key::ImapDisabled))
    } else {
        match imap_status {
            sync_imap::SyncStatus::Syncing => (theme::BADGE_BETA, t(locale, Key::ImapSyncing)),
            sync_imap::SyncStatus::Error(_) => (theme::BAD, t(locale, Key::ImapErrorToast)),
            _ => (theme::ACCENT, t(locale, Key::ImapSynced)),
        }
    };
    let imap_resp = ghost(
        right_ui,
        Icon::Mail,
        Some("IMAP"),
        imap_hint,
        true,
        26.0,
    );
    // Draw status dot on the IMAP icon
    let dot_pos = pos2(imap_resp.rect.left() + 7.0, imap_resp.rect.top() + 7.0);
    right_ui.painter().circle_filled(dot_pos, 2.5, imap_color);
    if imap_resp.clicked() {
        action.open_imap = true;
    }
    if ghost(
        right_ui,
        Icon::Globe,
        Some(locale.label()),
        t(locale, Key::CommandLocale),
        true,
        26.0,
    )
    .clicked()
    {
        action.toggle_locale = true;
    }
    right_ui.add_space(4.0);

    if icon_button(right_ui, Icon::Reload, t(locale, Key::Reload), true, false).clicked() {
        action.reload = true;
    }
    if icon_button(
        right_ui,
        Icon::Save,
        if dirty {
            t(locale, Key::SaveNow)
        } else {
            t(locale, Key::AllSaved)
        },
        dirty,
        false,
    )
    .clicked()
    {
        action.save = true;
    }

    right_ui.add_space(6.0);
    let (dot_color, text) = if status_error {
        (theme::BAD, t(locale, Key::SaveFailed))
    } else if dirty {
        (theme::BADGE_BETA, t(locale, Key::Unsaved))
    } else {
        (theme::ACCENT, status)
    };
    right_ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(vec2(6.0, 6.0), Sense::hover());
        ui.painter().circle_filled(dot.center(), 2.5, dot_color);
        let label = ui.label(
            RichText::new(text)
                .family(theme::mono())
                .size(9.5)
                .color(if status_error {
                    theme::BAD
                } else if dirty {
                    theme::BADGE_BETA
                } else {
                    theme::FAINT
                }),
        );
        if status_error {
            label.on_hover_text(status);
        }
    });

    let right_used_left = right_ui.min_rect().left().min(total_rect.right() - 20.0);

    // 2. Left side: Brand, vault switcher, and navigation tabs.
    // Constrained so it never extends past `right_used_left`.
    let left_rect = Rect::from_min_max(
        total_rect.left_top(),
        pos2(right_used_left.max(total_rect.left()), total_rect.bottom()),
    );
    let mut bar = ui.new_child(
        UiBuilder::new()
            .max_rect(left_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui = &mut bar;
    ui.spacing_mut().item_spacing.x = 6.0;
    ui.add_space(chrome::leading_inset() + 12.0);

    // Mark: a capsule outline with one filled end.
    let (mark, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
    let mark_rect = Rect::from_center_size(mark.center(), vec2(20.0, 20.0));
    if let Some(texture) = brand::texture(ui.ctx()) {
        ui.painter().image(
            texture.id(),
            mark_rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        // Vector fallback: two dots in a capsule outline.
        ui.painter().rect_stroke(
            mark_rect,
            egui::CornerRadius::same(6),
            Stroke::new(1.5, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
        ui.painter().circle_filled(
            mark_rect.center() + vec2(-3.2, 0.0),
            2.0,
            theme::ACCENT,
        );
        ui.painter().circle_filled(
            mark_rect.center() + vec2(3.2, 0.0),
            2.0,
            theme::fade(theme::ACCENT, 0.35),
        );
    }

    ui.label(
        RichText::new(t(locale, Key::AppName))
            .family(theme::display())
            .size(15.0)
            .color(theme::INK),
    );

    let (badge_rect, _) = ui.allocate_exact_size(vec2(45.0, 16.0), Sense::hover());
    ui.painter().rect(
        badge_rect,
        theme::RADIUS,
        theme::WASH,
        Stroke::new(1.0, theme::LINE),
        egui::StrokeKind::Inside,
    );
    let badge = caps(
        ui,
        "NATIVE",
        8.0,
        theme::mono_bold(),
        1.0,
        theme::FAINT,
    );
    ui.painter()
        .galley(badge_rect.center() - badge.size() / 2.0, badge, theme::FAINT);

    ui.add_space(8.0);
    let (divider, _) = ui.allocate_exact_size(vec2(1.0, 18.0), Sense::hover());
    ui.painter()
        .vline(divider.center().x, divider.y_range(), theme::hairline());
    ui.add_space(6.0);

    if vault_switcher(ui, vault).clicked() {
        action.open_vault = true;
    }

    ui.add_space(6.0);
    let (divider, _) = ui.allocate_exact_size(vec2(1.0, 18.0), Sense::hover());
    ui.painter()
        .vline(divider.center().x, divider.y_range(), theme::hairline());
    ui.add_space(6.0);

    // Workspace views. The page editor is one view among six.
    if let Some(chosen) = views::nav(ui, view, locale) {
        action.view = Some(chosen);
    }

    action
}

fn vault_switcher(ui: &mut Ui, vault: &Vault) -> Response {
    let name = folder_name(&vault.dir);
    let name = truncate_chars(&name, 30);
    let font = FontId::new(12.5, theme::medium());
    let galley = ui
        .painter()
        .layout_no_wrap(name.clone(), font, theme::INK);
    let width = 12.0 + 14.0 + 8.0 + galley.size().x + 10.0 + 12.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, 28.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, theme::RADIUS, theme::WASH);
    }
    let ink = if response.hovered() {
        theme::INK
    } else {
        theme::MUTED
    };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 12.0 + 7.0, rect.center().y), vec2(14.0, 14.0)),
        Icon::Folder,
        ink,
    );
    let text_pos = pos2(
        rect.left() + 12.0 + 14.0 + 8.0,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter()
        .galley_with_override_text_color(text_pos, galley, ink);
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), vec2(10.0, 10.0)),
        Icon::ChevronDown,
        theme::FAINT,
    );
    response
        .on_hover_text(vault.dir.display().to_string())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ---------------------------------------------------------------------------
// Shell: sidebar
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn draw_sidebar(
    ui: &mut Ui,
    vault: &Vault,
    tree: &PageTree,
    selected: Option<usize>,
    dragged_page: &mut Option<usize>,
    search: &mut String,
    collapsed: &mut HashSet<String>,
    index: &search::Bm25Index,
    page_texts: &[String],
    watching: bool,
    locale: Locale,
) -> ShellAction {
    let mut action = ShellAction::default();
    let outer = ui.max_rect();
    let inner = outer.shrink2(vec2(14.0, 8.0));
    let header_h = 122.0;
    let footer_h = 122.0;

    let header_rect = Rect::from_min_max(inner.min, pos2(inner.right(), inner.top() + header_h));
    let footer_rect = Rect::from_min_max(
        pos2(inner.left(), inner.bottom() - footer_h),
        inner.max,
    );
    let list_rect = Rect::from_min_max(
        pos2(inner.left(), header_rect.bottom()),
        pos2(inner.right(), footer_rect.top()),
    );

    let mut header = ui.new_child(
        UiBuilder::new()
            .max_rect(header_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    header.spacing_mut().item_spacing = vec2(0.0, 0.0);

    if vault_card(&mut header, vault).clicked() {
        action.open_vault = true;
    }
    header.add_space(10.0);
    if search_box(&mut header, search).changed() {
        // The list is rebuilt from `search` below; no extra state is needed.
    }
    header.add_space(14.0);

    let mut new_page_clicked = false;
    let section_rect = {
        let (rect, _) = header.allocate_exact_size(
            vec2(header.available_width(), 26.0),
            Sense::hover(),
        );
        let label = caps(
            &header,
            t(locale, Key::Pages),
            theme::HUD_SIZE,
            theme::mono_bold(),
            theme::LABEL_SPACING,
            theme::SIDEBAR_INK,
        );
        header.painter().galley(
            pos2(rect.left() + 2.0, rect.center().y - label.size().y / 2.0),
            label.clone(),
            theme::SIDEBAR_INK,
        );
        header.painter().text(
            pos2(rect.left() + 2.0 + label.size().x + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            vault.capsules.len().to_string(),
            FontId::new(9.0, theme::mono()),
            theme::FAINT,
        );

        let plus_rect = Rect::from_center_size(
            pos2(rect.right() - 11.0, rect.center().y),
            vec2(22.0, 22.0),
        );
        let plus = header
            .interact(plus_rect, header.scope_id().with("new-page"), Sense::click())
            .on_hover_text(t(locale, Key::NewPage))
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if plus.hovered() {
            header
                .painter()
                .rect_filled(plus_rect, theme::RADIUS, theme::WASH);
        }
        icons::paint(
            header.painter(),
            Rect::from_center_size(plus_rect.center(), vec2(12.0, 12.0)),
            Icon::Plus,
            if plus.hovered() {
                theme::INK
            } else {
                theme::FAINT
            },
        );
        if plus.clicked() {
            new_page_clicked = true;
        }
        rect
    };
    let _ = section_rect;
    action.new_page |= new_page_clicked;

    let mut list = ui.new_child(
        UiBuilder::new()
            .max_rect(list_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    ScrollArea::vertical()
        .id_salt("page-list")
        .auto_shrink([false, false])
        .show(&mut list, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            let query = search.trim().to_lowercase();
            if query.is_empty() {
                if tree.roots().is_empty() {
                    empty_pages(ui, &mut action, vault.capsules.is_empty());
                } else {
                    let mut drop_target = None;
                    if let Some(index) = draw_page_nodes(
                        ui,
                        &vault.capsules,
                        tree,
                        None,
                        0,
                        selected,
                        collapsed,
                        dragged_page,
                        &mut drop_target,
                        locale,
                    ) {
                        action.select = Some(index);
                    }

                    // Promote to root drop zone at the bottom of the list
                    if let Some(dragged) = *dragged_page {
                        ui.add_space(8.0);
                        let (root_drop_rect, root_drop_resp) = ui.allocate_exact_size(
                            vec2(ui.available_width(), 32.0),
                            Sense::hover(),
                        );
                        let is_already_root = tree.parent_of(dragged).is_none();
                        let hovered = root_drop_resp.hovered() && !is_already_root;

                        ui.painter().rect_stroke(
                            root_drop_rect,
                            theme::RADIUS,
                            Stroke::new(
                                1.0,
                                if hovered {
                                    theme::ACCENT
                                } else {
                                    theme::fade(theme::LINE, 0.4)
                                },
                            ),
                            egui::StrokeKind::Inside,
                        );

                        if hovered {
                            ui.painter().rect_filled(
                                root_drop_rect,
                                theme::RADIUS,
                                theme::fade(theme::ACCENT, 0.1),
                            );
                            drop_target = Some(None);
                        }

                        let text = t(locale, Key::MoveToRoot);
                        ui.painter().text(
                            root_drop_rect.center(),
                            Align2::CENTER_CENTER,
                            text,
                            FontId::new(11.0, theme::mono()),
                            if hovered {
                                theme::ACCENT
                            } else {
                                theme::FAINT
                            },
                        );
                    }

                    // If user was dragging and releases mouse button
                    if let Some(dragged) = *dragged_page {
                        if ui.input(|i| i.pointer.any_released()) {
                            if let Some(target) = drop_target {
                                action.reparent = Some((dragged, target));
                            }
                            *dragged_page = None;
                        }
                    }
                }
            } else {
                // BM25 over the whole page text, not a substring of the
                // subject: a query must be able to find prose.
                let mut rows: Vec<(usize, f32, Option<String>)> = Vec::new();
                for hit in index.rank(search) {
                    let Some(position) = vault
                        .capsules
                        .iter()
                        .position(|capsule| capsule.message_id == hit.id)
                    else {
                        continue;
                    };
                    let capsule = &vault.capsules[position];
                    let boost = search::subject_boost(search, &capsule.subject, &capsule.tags);
                    let text = page_texts.get(position).map(String::as_str).unwrap_or("");
                    rows.push((
                        position,
                        hit.score + boost,
                        search::snippet(text, search, 16),
                    ));
                }
                rows.sort_by(|a, b| {
                    b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
                });
                if rows.is_empty() {
                    ui.add_space(18.0);
                    ui.label(
                        RichText::new(format!(
                            "{} “{}”.",
                            t(locale, Key::NoMatches),
                            search.trim()
                        ))
                        .size(12.0)
                        .color(theme::MUTED),
                    );
                    ui.add_space(6.0);
                    if text_button(ui, None, t(locale, Key::ClearSearch), false).clicked() {
                        search.clear();
                    }
                } else {
                    let _ = hud(
                        ui,
                        &format!("{} · {}", rows.len(), t(locale, Key::Results)),
                        theme::FAINT,
                    );
                    for (index, score, snippet) in rows.iter().take(60) {
                        if draw_search_row(
                            ui,
                            &vault.capsules,
                            tree,
                            *index,
                            selected,
                            *score,
                            snippet.as_deref(),
                            search,
                        ) {
                            action.select = Some(*index);
                        }
                    }
                }
            }
        });

    let mut footer = ui.new_child(
        UiBuilder::new()
            .max_rect(footer_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    footer
        .painter()
        .hline(footer_rect.x_range(), footer_rect.top(), theme::hairline());
    footer.add_space(8.0);

    let hud_text = if watching {
        format!("{} · {}", t(locale, Key::ZeroDb), t(locale, Key::Watching))
    } else {
        t(locale, Key::ZeroDb).to_string()
    };
    footer.label(
        RichText::new(hud_text)
            .family(theme::mono_bold())
            .size(8.5)
            .color(theme::FAINT),
    );

    footer.add_space(2.0);
    let counts = if vault.load_errors.is_empty() {
        format!(
            "{} .eml · {}",
            vault.capsules.len(),
            t(locale, Key::PageCount)
        )
    } else {
        format!(
            "{} .eml · {} {}",
            vault.capsules.len(),
            vault.load_errors.len(),
            t(locale, Key::ReadOnlyCount)
        )
    };
    let counts_label = footer.label(
        RichText::new(counts)
            .family(theme::mono())
            .size(9.5)
            .color(theme::MUTED),
    );
    if !vault.load_errors.is_empty() {
        counts_label.on_hover_text(vault.load_errors.join("\n"));
    }
    footer.add_space(8.0);
    footer.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if icon_button(ui, Icon::Download, t(locale, Key::ImportEml), true, false).clicked() {
            action.import = true;
        }
        if icon_button(ui, Icon::Copy, t(locale, Key::ExportVault), true, false).clicked() {
            action.export_vault = true;
        }
        if icon_button(ui, Icon::Trash, t(locale, Key::DeletePage), selected.is_some(), false)
            .clicked()
        {
            action.delete_page = true;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if icon_button(ui, Icon::Folder, t(locale, Key::ChangeFolder), true, false).clicked() {
                action.open_vault = true;
            }
        });
    });

    action
}

fn vault_card(ui: &mut Ui, vault: &Vault) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), 44.0),
        Sense::click(),
    );
    ui.painter().rect(
        rect,
        theme::RADIUS,
        if response.hovered() {
            theme::WASH_HOVER
        } else {
            theme::WASH
        },
        theme::hairline(),
        egui::StrokeKind::Inside,
    );
    let tile = Rect::from_center_size(
        pos2(rect.left() + 22.0, rect.center().y),
        vec2(28.0, 28.0),
    );
    ui.painter()
        .rect_filled(tile, theme::RADIUS, theme::fade(theme::ACCENT, 0.14));
    icons::paint(
        ui.painter(),
        Rect::from_center_size(tile.center(), vec2(15.0, 15.0)),
        Icon::Folder,
        theme::ACCENT_INK,
    );
    let name = truncate_chars(&folder_name(&vault.dir), 24);
    ui.painter().text(
        pos2(tile.right() + 10.0, rect.center().y - 7.0),
        Align2::LEFT_CENTER,
        name,
        FontId::new(12.5, theme::semibold()),
        theme::INK,
    );
    ui.painter().text(
        pos2(tile.right() + 10.0, rect.center().y + 8.0),
        Align2::LEFT_CENTER,
        "Local .eml folder",
        FontId::new(10.0, theme::mono()),
        theme::FAINT,
    );
    response
        .on_hover_text(vault.dir.display().to_string())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn search_box(ui: &mut Ui, search: &mut String) -> Response {
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), 30.0),
        Sense::hover(),
    );
    ui.painter().rect(
        rect,
        theme::RADIUS,
        theme::WASH,
        theme::hairline(),
        egui::StrokeKind::Inside,
    );
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), vec2(13.0, 13.0)),
        Icon::Search,
        theme::FAINT,
    );
    let text_rect = Rect::from_min_max(
        pos2(rect.left() + 28.0, rect.top() + 4.0),
        pos2(rect.right() - 8.0, rect.bottom() - 4.0),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(text_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let response = child.add(
        TextEdit::singleline(search)
            .frame(Frame::NONE)
            .margin(Margin::same(0))
            .hint_text("Search pages")
            .desired_width(f32::INFINITY)
            .align(egui::Align2::LEFT_CENTER),
    );
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            theme::RADIUS,
            Stroke::new(1.0, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    response
}

fn empty_pages(ui: &mut Ui, action: &mut ShellAction, no_pages: bool) {
    ui.add_space(18.0);
    ui.label(
        RichText::new(if no_pages {
            "No pages yet."
        } else {
            "No native pages in this vault."
        })
        .size(12.0)
        .color(theme::MUTED),
    );
    ui.add_space(8.0);
    if primary_button(ui, "New page", 28.0).clicked() {
        action.new_page = true;
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_page_nodes(
    ui: &mut Ui,
    capsules: &[Capsule],
    tree: &PageTree,
    parent: Option<usize>,
    depth: usize,
    selected: Option<usize>,
    collapsed: &mut HashSet<String>,
    dragged_page: &mut Option<usize>,
    drop_target: &mut Option<Option<String>>,
    locale: Locale,
) -> Option<usize> {
    let mut clicked = None;
    for &index in tree.children_of(parent) {
        let capsule = &capsules[index];
        let has_children = !tree.children_of(Some(index)).is_empty();
        let key = capsule.message_id.clone();
        let is_collapsed = collapsed.contains(&key);
        let depth = depth.min(4);
        let indent = depth as f32 * 12.0;

        let (rect, response) = ui.allocate_exact_size(
            vec2(ui.available_width(), 30.0),
            Sense::click_and_drag(),
        );

        // Drag start / update
        if response.drag_started() && capsule.native {
            *dragged_page = Some(index);
        }

        let is_selected = selected == Some(index);
        let is_dragged = *dragged_page == Some(index);

        // Check drop target if something is currently being dragged
        let mut is_valid_drop_target = false;
        let mut is_cycle_target = false;
        if let Some(dragged) = *dragged_page {
            if dragged != index && response.hovered() {
                if tree.is_descendant_of(index, dragged) {
                    is_cycle_target = true;
                } else {
                    is_valid_drop_target = true;
                    *drop_target = Some(Some(capsule.message_id.clone()));
                }
            }
        }

        if is_dragged {
            ui.painter().rect_stroke(
                rect,
                theme::RADIUS,
                Stroke::new(1.0, theme::fade(theme::LINE, 0.4)),
                egui::StrokeKind::Inside,
            );
        } else if is_valid_drop_target {
            ui.painter().rect_stroke(
                rect,
                theme::RADIUS,
                Stroke::new(1.5, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
            ui.painter().rect_filled(
                rect,
                theme::RADIUS,
                theme::fade(theme::ACCENT, 0.15),
            );
        } else if is_cycle_target {
            ui.painter().rect_stroke(
                rect,
                theme::RADIUS,
                Stroke::new(1.5, theme::BAD),
                egui::StrokeKind::Inside,
            );
            ui.painter().rect_filled(
                rect,
                theme::RADIUS,
                theme::fade(theme::BAD, 0.15),
            );
        } else if is_selected {
            ui.painter()
                .rect_filled(rect, theme::RADIUS, theme::WASH_SELECTED);
            ui.painter().rect_filled(
                Rect::from_min_size(
                    pos2(rect.left() - 7.0, rect.top() + 7.0),
                    vec2(2.0, 16.0),
                ),
                0.0,
                Color32::WHITE,
            );
        } else if response.hovered() {
            ui.painter()
                .rect_filled(rect, theme::RADIUS, theme::WASH);
        }

        let chevron_rect = Rect::from_center_size(
            pos2(rect.left() + indent + 8.0, rect.center().y),
            vec2(14.0, 14.0),
        );
        let mut toggle = false;
        if has_children {
            let chev = ui
                .interact(
                    chevron_rect,
                    ui.scope_id().with(("page-chevron", index)),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            icons::paint(
                ui.painter(),
                Rect::from_center_size(chevron_rect.center(), vec2(10.0, 10.0)),
                if is_collapsed {
                    Icon::ChevronRight
                } else {
                    Icon::ChevronDown
                },
                theme::FAINT,
            );
            toggle = chev.clicked();
        }

        let ink = if is_selected || response.hovered() || is_valid_drop_target {
            Color32::WHITE
        } else if is_dragged {
            theme::FAINT
        } else {
            theme::SIDEBAR_INK
        };
        icons::paint(
            ui.painter(),
            Rect::from_center_size(
                pos2(rect.left() + indent + 25.0, rect.center().y),
                vec2(13.0, 13.0),
            ),
            Icon::Doc,
            ink,
        );

        let title_x = rect.left() + indent + 40.0;
        let title = truncate_chars(&capsule.subject, 34);
        let font = FontId::new(
            12.5,
            if is_selected {
                theme::medium()
            } else {
                FontFamily::Proportional
            },
        );
        let clip = Rect::from_min_max(
            pos2(title_x, rect.top()),
            pos2(rect.right() - 18.0, rect.bottom()),
        );
        ui.painter()
            .with_clip_rect(clip)
            .text(pos2(title_x, rect.center().y), Align2::LEFT_CENTER, title, font, ink);

        if !capsule.native {
            icons::paint(
                ui.painter(),
                Rect::from_center_size(
                    pos2(rect.right() - 12.0, rect.center().y),
                    vec2(11.0, 11.0),
                ),
                Icon::Lock,
                theme::BADGE_BETA,
            );
        }

        if toggle {
            if is_collapsed {
                collapsed.remove(&key);
            } else {
                collapsed.insert(key.clone());
            }
        } else if response.clicked() && dragged_page.is_none() {
            clicked = Some(index);
        }

        // Floating drag preview badge when dragging this page
        if is_dragged {
            if let Some(pointer_pos) = ui.input(|i| i.pointer.hover_pos()) {
                let painter = ui.ctx().layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new("page-drag-preview"),
                ));
                let preview_rect = Rect::from_min_size(
                    pos2(pointer_pos.x + 12.0, pointer_pos.y + 12.0),
                    vec2(140.0, 24.0),
                );
                painter.rect_filled(preview_rect, theme::RADIUS, theme::WASH_SELECTED);
                painter.rect_stroke(
                    preview_rect,
                    theme::RADIUS,
                    Stroke::new(1.0, theme::ACCENT),
                    egui::StrokeKind::Inside,
                );
                painter.text(
                    pos2(preview_rect.left() + 8.0, preview_rect.center().y),
                    Align2::LEFT_CENTER,
                    truncate_chars(&capsule.subject, 18),
                    FontId::new(11.0, theme::medium()),
                    Color32::WHITE,
                );
            }
        }

        let response = if is_cycle_target {
            response.on_hover_text(t(locale, Key::CannotDropInsideSelf))
        } else if is_valid_drop_target {
            response.on_hover_text(format!("{} «{}»", t(locale, Key::MakeChildOf), capsule.subject))
        } else if !capsule.native {
            response.on_hover_text("Imported email — read-only in the native prototype")
        } else {
            response
        };
        let _ = response;

        if has_children && !is_collapsed {
            if let Some(child) = draw_page_nodes(
                ui,
                capsules,
                tree,
                Some(index),
                depth + 1,
                selected,
                collapsed,
                dragged_page,
                drop_target,
                locale,
            ) {
                clicked = Some(child);
            }
        }
    }
    clicked
}

fn draw_search_row(
    ui: &mut Ui,
    capsules: &[Capsule],
    tree: &PageTree,
    index: usize,
    selected: Option<usize>,
    score: f32,
    snippet: Option<&str>,
    query: &str,
) -> bool {
    let capsule = &capsules[index];
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), 42.0),
        Sense::click(),
    );
    let is_selected = selected == Some(index);
    if is_selected {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::WASH_SELECTED);
        ui.painter().rect_filled(
            Rect::from_min_size(
                pos2(rect.left() - 7.0, rect.top() + 10.0),
                vec2(2.0, 18.0),
            ),
            0.0,
            Color32::WHITE,
        );
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::WASH);
    }
    let ink = if is_selected || response.hovered() {
        Color32::WHITE
    } else {
        theme::SIDEBAR_INK
    };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 15.0, rect.top() + 16.0), vec2(13.0, 13.0)),
        Icon::Doc,
        ink,
    );
    let title = truncate_chars(&capsule.subject, 24);
    let title_clip = Rect::from_min_max(
        pos2(rect.left() + 30.0, rect.top()),
        pos2(rect.right() - 34.0, rect.top() + 24.0),
    );
    ui.painter().with_clip_rect(title_clip).text(
        pos2(rect.left() + 30.0, rect.top() + 15.0),
        Align2::LEFT_CENTER,
        title,
        FontId::new(12.5, theme::medium()),
        ink,
    );
    let second = match snippet {
        Some(snippet) => truncate_chars(snippet, 40),
        None => tree
            .parent_of(index)
            .and_then(|parent| capsules.get(parent))
            .map(|parent| format!("in {}", truncate_chars(&parent.subject, 26)))
            .unwrap_or_else(|| "Root page".to_string()),
    };
    // Paint the excerpt in segments so the matched words stand out from the
    // surrounding text instead of being one flat muted string. The row clips
    // rather than dropping the whole line when the excerpt is wider than the
    // sidebar.
    let clip = Rect::from_min_max(
        pos2(rect.left() + 30.0, rect.top() + 22.0),
        pos2(rect.right() - 4.0, rect.bottom()),
    );
    let painter = ui.painter().with_clip_rect(clip);
    let mut x = rect.left() + 30.0;
    let baseline = rect.top() + 30.0;
    for (segment, is_match) in search::highlight_matches(&second, query) {
        if segment.is_empty() {
            continue;
        }
        let color = if is_match {
            if is_selected || response.hovered() {
                Color32::WHITE
            } else {
                theme::INK
            }
        } else {
            theme::FAINT
        };
        let galley = painter.layout_no_wrap(segment, FontId::new(9.5, theme::mono()), color);
        let width = galley.size().x;
        if x > clip.right() {
            break;
        }
        painter.galley(pos2(x, baseline - galley.size().y / 2.0), galley, color);
        x += width;
    }
    ui.painter().text(
        pos2(rect.right() - 10.0, rect.top() + 15.0),
        Align2::RIGHT_CENTER,
        format!("{score:.1}"),
        FontId::new(9.0, theme::mono()),
        theme::FAINT,
    );
    if !capsule.native {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(rect.right() - 12.0, rect.top() + 30.0), vec2(11.0, 11.0)),
            Icon::Lock,
            theme::BADGE_BETA,
        );
    }
    let response = if capsule.native {
        response
    } else {
        response.on_hover_text("Imported email — read-only in the native prototype")
    };
    response.clicked()
}

// ---------------------------------------------------------------------------
// Content panel: header, empty state, document
// ---------------------------------------------------------------------------

fn draw_content_header(
    ui: &mut Ui,
    selected: Option<usize>,
    capsules: &[Capsule],
    tree: &PageTree,
    locale: Locale,
) -> EditorAction {
    let mut action = EditorAction::default();
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), 48.0),
        Sense::hover(),
    );
    ui.painter()
        .hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline());

    let header_rect = rect.shrink2(vec2(22.0, 0.0));

    // 1. First render right action buttons to calculate exact width reserved
    let right_reserve_w = 320.0;
    let right_rect = Rect::from_min_max(
        pos2((header_rect.right() - right_reserve_w).max(header_rect.left()), header_rect.top()),
        header_rect.right_bottom(),
    );
    let mut right_bar = ui.new_child(
        UiBuilder::new()
            .max_rect(right_rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    let right_ui = &mut right_bar;
    right_ui.spacing_mut().item_spacing.x = 4.0;

    let Some(index) = selected.filter(|index| *index < capsules.len()) else {
        let mut left_bar = ui.new_child(
            UiBuilder::new()
                .max_rect(header_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        let left_ui = &mut left_bar;
        left_ui.spacing_mut().item_spacing.x = 7.0;
        let _ = hud(left_ui, t(locale, Key::Vault), theme::MUTED);
        left_ui.label(
            RichText::new(t(locale, Key::NoPageSelected))
                .size(12.0)
                .color(theme::FAINT),
        );
        return action;
    };
    let capsule = &capsules[index];

    let info = ghost(
        right_ui,
        Icon::Info,
        Some(t(locale, Key::Info)),
        t(locale, Key::MimeAndPage),
        true,
        30.0,
    );
    egui::Popup::menu(&info)
        .frame(popover_frame())
        .align(egui::RectAlign::BOTTOM_END)
        .show(|ui| info_popover(ui, capsule));

    if ghost(
        right_ui,
        Icon::Command,
        Some(t(locale, Key::CoThink)),
        t(locale, Key::CoThinkHint),
        true,
        30.0,
    )
    .clicked()
    {
        action.cothink = true;
    }

    if ghost(
        right_ui,
        Icon::Plus,
        Some(t(locale, Key::NewSubpage)),
        t(locale, Key::NewSubpageHint),
        true,
        30.0,
    )
    .clicked()
    {
        action.new_subpage = true;
    }

    let right_used_left = right_ui.min_rect().left().min(header_rect.right() - 20.0);

    // 2. Render left breadcrumbs and subject constrained by right_used_left
    let left_rect = Rect::from_min_max(
        header_rect.left_top(),
        pos2((right_used_left - 12.0).max(header_rect.left()), header_rect.bottom()),
    );
    let mut bar = ui.new_child(
        UiBuilder::new()
            .max_rect(left_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui = &mut bar;
    ui.spacing_mut().item_spacing.x = 7.0;
    let _ = hud(ui, t(locale, Key::Vault), theme::MUTED);

    let ancestors = tree.ancestors(index);
    let skip = ancestors.len().saturating_sub(2);
    if skip > 0 {
        ui.label(RichText::new("…").size(12.0).color(theme::FAINT));
        let _ = chevron(ui, theme::FAINT);
    }
    for ancestor in &ancestors[skip..] {
        if let Some(parent) = capsules.get(*ancestor) {
            if crumb_button(ui, &truncate_chars(&parent.subject, 18)).clicked() {
                action.select = Some(*ancestor);
            }
            let _ = chevron(ui, theme::FAINT);
        }
    }

    let subject_str = if capsule.subject.is_empty() {
        t(locale, Key::Untitled).to_string()
    } else {
        capsule.subject.clone()
    };
    let remaining_w = ui.available_width().max(40.0) - if !capsule.native { 70.0 } else { 0.0 };
    let font_subject = FontId::new(12.5, theme::medium());
    let truncated_subject = truncate_to_width(ui, &subject_str, font_subject, remaining_w);

    ui.label(
        RichText::new(truncated_subject)
            .family(theme::medium())
            .size(12.5)
            .color(theme::INK),
    );
    if !capsule.native {
        ui.add_space(3.0);
        let _ = tag_chip(ui, t(locale, Key::ReadOnly), theme::BADGE_BETA);
    }

    action
}

fn draw_document(
    ui: &mut Ui,
    capsule: &mut Capsule,
    locale: Locale,
    link_context: &LinkContext,
    scroll: Option<f32>,
) -> EditorAction {
    let mut action = EditorAction::default();
    let mut area = ScrollArea::vertical().id_salt("document");
    if let Some(offset) = scroll {
        area = area.vertical_scroll_offset(offset);
    }
    area
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let available = ui.available_width();
            let content_w = (available - 96.0).clamp(320.0, CONTENT_MAX_WIDTH);
            let pad = ((available - content_w) / 2.0).max(28.0);
            ui.horizontal(|ui| {
                ui.add_space(pad);
                ui.allocate_ui_with_layout(
                    vec2(content_w, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(content_w);
                        ui.add_space(40.0);

                        if capsule.native {
                            // Strip newlines if pasted or typed, ensuring subject remains single-concept
                            if capsule.subject.contains('\n') || capsule.subject.contains('\r') {
                                capsule.subject = capsule.subject.replace(['\r', '\n'], " ");
                                action.changed = true;
                            }
                            let response = ui.add(
                                TextEdit::multiline(&mut capsule.subject)
                                    .font(FontId::new(34.0, theme::display()))
                                    .text_color(theme::INK)
                                    .frame(Frame::NONE)
                                    .margin(Margin::same(0))
                                    .desired_width(content_w)
                                    .desired_rows(1)
                                    .hint_text(t(locale, Key::UntitledPage)),
                            );
                            if response.changed() {
                                if capsule.subject.contains('\n') || capsule.subject.contains('\r') {
                                    capsule.subject = capsule.subject.replace(['\r', '\n'], " ");
                                }
                                action.changed = true;
                            }
                            // A rename is committed when the title loses focus,
                            // not on every keystroke: rewriting every `[[link]]`
                            // in the vault mid-word would be destructive.
                            let title_id =
                                egui::Id::new(("subject-before-edit", capsule.message_id.clone()));
                            if response.gained_focus() {
                                ui.ctx().data_mut(|data| {
                                    data.insert_temp(title_id, capsule.subject.clone())
                                });
                            }
                            if response.lost_focus() {
                                let before = ui
                                    .ctx()
                                    .data_mut(|data| data.remove_temp::<String>(title_id));
                                if let Some(before) = before {
                                    if !before.trim().is_empty()
                                        && links::normalise(&before)
                                            != links::normalise(&capsule.subject)
                                    {
                                        action.rename_from = Some(before);
                                    }
                                }
                            }
                        } else {
                            ui.add(
                                Label::new(
                                    RichText::new(if capsule.subject.is_empty() {
                                        t(locale, Key::Untitled)
                                    } else {
                                        &capsule.subject
                                    })
                                    .family(theme::display())
                                    .size(34.0)
                                    .color(theme::INK),
                                )
                                .wrap(),
                            );
                        }

                        ui.add_space(12.0);
                        if document_meta(ui, capsule, locale) {
                            action.changed = true;
                        }
                        ui.add_space(28.0);

                        if !capsule.native {
                            read_only_notice(ui, capsule, locale);
                            ui.add_space(18.0);
                            if secondary_button(ui, Some(Icon::Mail), t(locale, Key::NavPreview), 30.0)
                                .clicked()
                            {
                                action.view = Some(View::Mail);
                            }
                            ui.add_space(70.0);
                        } else {
                            let mut block_op: Option<(usize, BlockOp)> = None;
                            let block_count = capsule.blocks.len();
                            for index in 0..block_count {
                                let mut block = capsule.blocks[index].clone();
                                let kind = block_type(&block).to_string();
                                let (changed, op) = draw_block_row(
                                    ui,
                                    &mut block,
                                    index,
                                    block_count,
                                    &mut capsule.attachments,
                                    locale,
                                );
                                if changed {
                                    capsule.blocks[index] = block;
                                    action.changed = true;
                                }
                                if op.is_some() {
                                    block_op = op.map(|op| (index, op));
                                }
                                ui.add_space(match kind.as_str() {
                                    "heading" => 20.0,
                                    "todo_list" => 16.0,
                                    "callout" => 16.0,
                                    "unsupported" => 12.0,
                                    _ => 10.0,
                                });
                            }

                            if let Some((index, op)) = block_op {
                                match op {
                                    BlockOp::Delete => {
                                        if index < capsule.blocks.len() {
                                            capsule.blocks.remove(index);
                                            action.changed = true;
                                        }
                                    }
                                    BlockOp::Up if index > 0 => {
                                        capsule.blocks.swap(index, index - 1);
                                        action.changed = true;
                                    }
                                    BlockOp::Down if index + 1 < capsule.blocks.len() => {
                                        capsule.blocks.swap(index, index + 1);
                                        action.changed = true;
                                    }
                                    BlockOp::SetLevel(level) => {
                                        if let Some(block) = capsule.blocks.get_mut(index) {
                                            block["data"]["level"] = json!(level);
                                            action.changed = true;
                                        }
                                    }
                                    _ => {}
                                }
                            }

                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.add_space(2.0);
                                if let Some(kind) = add_block_menu(ui, locale) {
                                    capsule.blocks.push(kind.block(locale));
                                    action.changed = true;
                                }
                                ui.add_space(4.0);
                                let _ = hud(ui, t(locale, Key::SlashHint), theme::FAINT);
                            });
                            ui.add_space(18.0);
                            links_panel(ui, link_context, locale, &mut action);
                            ui.add_space(18.0);
                            if attachments_editor(ui, capsule, locale) {
                                action.changed = true;
                            }
                            ui.add_space(90.0);
                        }
                    },
                );
            });
        });
    action
}

/// Outgoing `[[links]]` and the pages that link back, drawn under the blocks.
///
/// A link the user clicks either navigates or — when no page has that subject
/// yet — offers to create it, so the wiki can grow from prose alone.
fn links_panel(ui: &mut Ui, context: &LinkContext, locale: Locale, action: &mut EditorAction) {
    ui.vertical(|ui| {
        ui.set_max_width(ui.available_width());
        views::section(ui, t(locale, Key::LinksTitle));
        if context.outgoing.is_empty() {
            let _ = hud(ui, t(locale, Key::NoLinks), theme::FAINT);
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for (title, resolved) in &context.outgoing {
                    let missing = resolved.is_none();
                    let response = link_chip(ui, title, missing);
                    let response = if missing {
                        response.on_hover_text(t(locale, Key::CreateLinkedPage))
                    } else {
                        response.on_hover_text(t(locale, Key::OpenLinkedPage))
                    };
                    if response.clicked() {
                        match resolved {
                            Some(index) => action.select = Some(*index),
                            None => action.create_page = Some(title.clone()),
                        }
                    }
                }
            });
        }
        if !context.backlinks.is_empty() {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                let label = caps(
                    ui,
                    t(locale, Key::Backlinks),
                    theme::HUD_SIZE,
                    theme::mono_bold(),
                    theme::LABEL_SPACING,
                    theme::FAINT,
                );
                let (rect, _) = ui.allocate_exact_size(
                    vec2(label.size().x, label.size().y + 4.0),
                    Sense::hover(),
                );
                ui.painter()
                    .galley(rect.min + vec2(0.0, 2.0), label, theme::FAINT);
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for (index, subject) in &context.backlinks {
                    let name = if subject.is_empty() {
                        t(locale, Key::Untitled).to_string()
                    } else {
                        subject.clone()
                    };
                    if link_chip(ui, &name, false)
                        .on_hover_text(t(locale, Key::OpenLinkedPage))
                        .clicked()
                    {
                        action.select = Some(*index);
                    }
                }
            });
        }
        if !context.related.is_empty() {
            ui.add_space(10.0);
            let label = caps(
                ui,
                t(locale, Key::Related),
                theme::HUD_SIZE,
                theme::mono_bold(),
                theme::LABEL_SPACING,
                theme::FAINT,
            );
            let (rect, _) =
                ui.allocate_exact_size(vec2(label.size().x, label.size().y + 4.0), Sense::hover());
            ui.painter()
                .galley(rect.min + vec2(0.0, 2.0), label, theme::FAINT);
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for (index, subject, score) in &context.related {
                    let name = if subject.is_empty() {
                        t(locale, Key::Untitled).to_string()
                    } else {
                        subject.clone()
                    };
                    let label = format!("{} · {:.0}%", name, score * 100.0);
                    if link_chip(ui, &label, false)
                        .on_hover_text(t(locale, Key::OpenLinkedPage))
                        .clicked()
                    {
                        action.select = Some(*index);
                    }
                }
            });
        }
    });
}

/// A clickable page chip. Missing targets are drawn muted with a leading `+`.
fn link_chip(ui: &mut Ui, label: &str, missing: bool) -> Response {
    let text = if missing {
        format!("+ {label}")
    } else {
        label.to_string()
    };
    let galley = ui.painter().layout_no_wrap(
        text,
        FontId::new(11.5, theme::medium()),
        if missing { theme::MUTED } else { theme::ACCENT_INK },
    );
    let size = galley.size() + vec2(18.0, 10.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if response.hovered() {
        theme::WASH_HOVER
    } else {
        theme::WASH
    };
    ui.painter()
        .rect(rect, theme::RADIUS, fill, theme::hairline(), egui::StrokeKind::Inside);
    ui.painter().galley_with_override_text_color(
        rect.min + vec2(9.0, 5.0),
        galley,
        if missing { theme::MUTED } else { theme::ACCENT_INK },
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn document_meta(ui: &mut Ui, capsule: &mut Capsule, locale: Locale) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(7.0, 6.0);
        ui.label(
            RichText::new(format_date(capsule))
                .family(theme::mono())
                .size(9.5)
                .color(theme::FAINT),
        );
        dot_sep(ui);
        if capsule.native {
            ui.label(
                RichText::new(i18n::plural(
                    locale,
                    capsule.blocks.len(),
                    Key::BlockOne,
                    Key::BlockFew,
                    Key::BlockMany,
                ))
                .family(theme::mono())
                .size(9.5)
                .color(theme::FAINT),
            );
        } else {
            ui.label(
                RichText::new(t(locale, Key::ImportedEmail))
                    .family(theme::mono())
                    .size(9.5)
                    .color(theme::BADGE_BETA),
            );
        }

        // Page status: the same four values the web app writes into the
        // `X-Capsule-Status` header.
        if capsule.native {
            ui.add_space(2.0);
            let pill = status_pill(ui, &capsule.status);
            egui::Popup::menu(&pill)
                .frame(popover_frame())
                .align(egui::RectAlign::BOTTOM_START)
                .show(|ui| {
                    ui.set_min_width(170.0);
                    menu_header(ui, t(locale, Key::Status));
                    for (value, key) in [
                        ("draft", Key::StatusDraft),
                        ("active", Key::StatusActive),
                        ("synthesis", Key::StatusSynthesis),
                        ("archived", Key::StatusArchived),
                    ] {
                        if menu_row(
                            ui,
                            None,
                            t(locale, key),
                            None,
                            capsule.status == value,
                            true,
                            false,
                        )
                        .clicked()
                        {
                            capsule.status = value.to_string();
                            changed = true;
                        }
                    }
                });
        }

        let tag_id = egui::Id::new(("tag-input", capsule.message_id.clone()));
        let mut buffer: String = ui.data(|data| data.get_temp(tag_id)).unwrap_or_default();
        let mut remove_tag: Option<usize> = None;
        for (index, tag) in capsule.tags.iter().enumerate() {
            ui.add_space(2.0);
            let chip = tag_chip(ui, tag, theme::MUTED);
            egui::Popup::menu(&chip)
                .frame(popover_frame())
                .align(egui::RectAlign::BOTTOM_START)
                .show(|ui| {
                    ui.set_min_width(150.0);
                    if menu_row(
                        ui,
                        Some(Icon::Trash),
                        t(locale, Key::Remove),
                        Some(tag.as_str()),
                        false,
                        true,
                        true,
                    )
                    .clicked()
                    {
                        remove_tag = Some(index);
                    }
                });
        }
        if let Some(index) = remove_tag {
            capsule.tags.remove(index);
            changed = true;
        }

        if capsule.native {
            ui.add_space(2.0);
            let add = text_button(ui, Some(Icon::Plus), t(locale, Key::AddTag), false);
            egui::Popup::menu(&add)
                .frame(popover_frame())
                .align(egui::RectAlign::BOTTOM_START)
                .show(|ui| {
                    ui.set_min_width(190.0);
                    menu_header(ui, t(locale, Key::Tags));
                    let response = ui.add(
                        TextEdit::singleline(&mut buffer)
                            .font(FontId::new(12.0, FontFamily::Proportional))
                            .desired_width(170.0)
                            .hint_text(t(locale, Key::Keyword)),
                    );
                    if response.changed() {
                        ui.data_mut(|data| data.insert_temp(tag_id, buffer.clone()));
                    }
                    let enter =
                        response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                    if enter
                        || menu_row(
                            ui,
                            None,
                            t(locale, Key::AddTag),
                            None,
                            false,
                            true,
                            false,
                        )
                        .clicked()
                    {
                        let tag = buffer.trim().to_string();
                        if !tag.is_empty() && !capsule.tags.iter().any(|existing| existing == &tag)
                        {
                            capsule.tags.push(tag);
                            changed = true;
                        }
                        buffer.clear();
                        ui.data_mut(|data| data.insert_temp(tag_id, String::new()));
                    }
                });
        }

        if !capsule.attachments.is_empty() {
            dot_sep(ui);
            let bytes: usize = capsule.attachments.iter().map(|a| a.size_bytes).sum();
            ui.label(
                RichText::new(format!(
                    "{} · {}",
                    format!(
                        "{} {}",
                        capsule.attachments.len(),
                        t(locale, Key::Attachment)
                    ),
                    human_bytes(bytes)
                ))
                .family(theme::mono())
                .size(9.5)
                .color(theme::FAINT),
            );
        }
    });
    changed
}

fn read_only_notice(ui: &mut Ui, capsule: &Capsule, locale: Locale) {
    let inner = Frame::new()
        .fill(theme::WASH)
        .corner_radius(theme::RADIUS)
        .inner_margin(Margin {
            left: 16,
            right: 14,
            top: 12,
            bottom: 12,
        })
        .show(ui, |ui| {
            let _ = hud(ui, t(locale, Key::ExternalReadOnlyNotice), theme::BADGE_BETA);
            ui.add_space(7.0);
            ui.add(
                Label::new(
                    RichText::new(
                        capsule
                            .parse_note
                            .as_deref()
                            .unwrap_or(t(locale, Key::ImportedEmail)),
                    )
                    .size(13.0)
                    .color(theme::INK),
                )
                .wrap(),
            );
            ui.add_space(5.0);
            ui.add(
                Label::new(
                    RichText::new(t(locale, Key::HtmlRenderingNotice))
                        .size(11.5)
                        .color(theme::MUTED),
                )
                .wrap(),
            );
        });
    ui.painter().rect_filled(
        Rect::from_min_size(
            inner.response.rect.min,
            vec2(2.0, inner.response.rect.height()),
        ),
        0.0,
        theme::BADGE_BETA,
    );
}

fn draw_empty_state(ui: &mut Ui, action: &mut EditorAction, locale: Locale) {
    ScrollArea::vertical()
        .id_salt("empty")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(110.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(t(locale, Key::EmptyVaultTitle))
                        .family(theme::display())
                        .size(32.0)
                        .color(theme::INK),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(t(locale, Key::EmptyVaultBody))
                        .size(13.0)
                        .color(theme::MUTED),
                );
                ui.add_space(22.0);
                ui.horizontal(|ui| {
                    let total = 280.0;
                    ui.add_space((ui.available_width() - total) / 2.0);
                    if primary_button(ui, t(locale, Key::CreateFirstPage), 34.0).clicked() {
                        action.new_page = true;
                    }
                    ui.add_space(8.0);
                    if secondary_button(ui, Some(Icon::Folder), t(locale, Key::ChangeFolder), 34.0)
                        .clicked()
                    {
                        action.open_vault = true;
                    }
                });
                ui.add_space(10.0);
                ui.label(
                    RichText::new(t(locale, Key::ZeroDb))
                        .family(theme::mono())
                        .size(9.0)
                        .color(theme::FAINT),
                );
            });
        });
}

// ---------------------------------------------------------------------------
// Blocks
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum BlockOp {
    Up,
    Down,
    Delete,
    SetLevel(u8),
}

#[derive(Clone, Copy)]
enum BlockKind {
    Text,
    Heading,
    Tasks,
    Callout,
    Code,
    Grid,
    Audio,
    Canvas,
    Opponent,
}

impl BlockKind {
    fn block(self, locale: Locale) -> Value {
        match self {
            BlockKind::Text => json!({
                "id": new_block_id("paragraph"),
                "type": "paragraph",
                "data": { "text": "" }
            }),
            BlockKind::Heading => json!({
                "id": new_block_id("heading"),
                "type": "heading",
                "data": { "level": 2, "text": "" }
            }),
            BlockKind::Tasks => json!({
                "id": new_block_id("todo_list"),
                "type": "todo_list",
                "data": { "title": t(locale, Key::BlockTodo), "items": [] }
            }),
            BlockKind::Callout => json!({
                "id": new_block_id("callout"),
                "type": "callout",
                "data": {
                    "title": t(locale, Key::BlockCallout),
                    "content": "",
                    "icon": "info",
                    "tone": "blue"
                }
            }),
            BlockKind::Code => json!({
                "id": new_block_id("code_repl"),
                "type": "code_repl",
                "data": { "language": "javascript", "code": "" }
            }),
            BlockKind::Grid => json!({
                "id": new_block_id("data_grid_view"),
                "type": "data_grid_view",
                "data": { "source_cid": "", "view": "table" }
            }),
            BlockKind::Audio => json!({
                "id": new_block_id("audio_note"),
                "type": "audio_note",
                "data": {
                    "duration": "0:00",
                    "transcript": "",
                    "waveform": [4, 9, 14, 7, 17, 11, 5, 13, 8, 16, 6, 12],
                    "author": t(locale, Key::NativeBadge).to_lowercase()
                }
            }),
            BlockKind::Canvas => json!({
                "id": new_block_id("interactive_canvas"),
                "type": "interactive_canvas",
                "data": { "scene_cid": "", "elements": [] }
            }),
            BlockKind::Opponent => json!({
                "id": new_block_id("ai_opponent_block"),
                "type": "ai_opponent_block",
                "data": { "criticism": "", "sourceClaim": "", "confidence": 0.5 }
            }),
        }
    }
}

fn add_block_menu(ui: &mut Ui, locale: Locale) -> Option<BlockKind> {
    let response = ghost(
        ui,
        Icon::Plus,
        Some(t(locale, Key::AddBlock)),
        t(locale, Key::AddBlockHint),
        true,
        30.0,
    );
    let mut chosen = None;
    egui::Popup::menu(&response)
        .frame(popover_frame())
        .align(egui::RectAlign::BOTTOM_START)
        .show(|ui| {
            ui.set_min_width(200.0);
            menu_header(ui, t(locale, Key::AddBlock));
            let items = [
                (Icon::Doc, Key::BlockParagraph, BlockKind::Text, None),
                (Icon::Doc, Key::BlockHeading, BlockKind::Heading, Some("H2")),
                (Icon::Check, Key::BlockTodo, BlockKind::Tasks, None),
                (Icon::Info, Key::BlockCallout, BlockKind::Callout, None),
                (Icon::Code, Key::BlockCode, BlockKind::Code, None),
                (Icon::Table, Key::BlockGrid, BlockKind::Grid, None),
                (Icon::Audio, Key::BlockAudio, BlockKind::Audio, None),
                (Icon::Board, Key::BlockCanvas, BlockKind::Canvas, None),
                (Icon::Command, Key::BlockAi, BlockKind::Opponent, None),
            ];
            for (icon, key, kind, hint) in items {
                if menu_row(ui, Some(icon), t(locale, key), hint, false, true, false).clicked() {
                    chosen = Some(kind);
                }
            }
        });
    chosen
}

fn draw_block_row(
    ui: &mut Ui,
    block: &mut Value,
    index: usize,
    block_count: usize,
    attachments: &mut [Attachment],
    locale: Locale,
) -> (bool, Option<BlockOp>) {
    let kind = block_type(block).to_string();
    let mut changed = false;
    let mut op = None;
    let row_top = ui.cursor().min;

    let response = ui
        .allocate_ui_with_layout(
            vec2(ui.available_width(), 0.0),
            Layout::top_down(Align::Min),
            |ui| {
                ui.horizontal_top(|ui| {
                    ui.add_space(GUTTER);
                    ui.vertical(|ui| {
                        ui.set_max_width(ui.available_width());
                        changed = draw_block_content(ui, block, &kind, attachments, locale);
                    });
                });
            },
        )
        .response;

    let row_rect = response.rect;
    let hovered = ui.rect_contains_pointer(row_rect);
    let handle_rect = Rect::from_min_size(
        pos2(row_top.x + 1.0, row_top.y + 3.0),
        vec2(20.0, 18.0),
    );
    let handle = ui
        .interact(
            handle_rect,
            ui.scope_id().with(("block-menu", index)),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(t(locale, Key::AddBlockHint));
    icons::paint(
        ui.painter(),
        Rect::from_center_size(handle_rect.center(), vec2(13.0, 13.0)),
        Icon::Grip,
        if hovered || handle.hovered() {
            theme::MUTED
        } else {
            theme::fade(theme::FAINT, 0.45)
        },
    );

    egui::Popup::menu(&handle)
        .frame(popover_frame())
        .align(egui::RectAlign::BOTTOM_START)
        .show(|ui| {
            ui.set_min_width(178.0);
            if kind == "heading" {
                let level = block
                    .get("data")
                    .and_then(|data| data.get("level"))
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    .clamp(1, 3);
                menu_header(ui, t(locale, Key::HeadingLevel));
                for candidate in 1..=3 {
                    if menu_row(
                        ui,
                        None,
                        &format!("H{candidate}"),
                        None,
                        level == candidate as u64,
                        true,
                        false,
                    )
                    .clicked()
                    {
                        op = Some(BlockOp::SetLevel(candidate));
                    }
                }
                menu_separator(ui);
            } else {
                menu_header(ui, &kind.replace('_', " "));
            }
            if menu_row(
                ui,
                Some(Icon::ArrowUp),
                t(locale, Key::MoveUp),
                None,
                false,
                index > 0,
                false,
            )
            .clicked()
            {
                op = Some(BlockOp::Up);
            }
            if menu_row(
                ui,
                Some(Icon::ArrowDown),
                t(locale, Key::MoveDown),
                None,
                false,
                index + 1 < block_count,
                false,
            )
            .clicked()
            {
                op = Some(BlockOp::Down);
            }
            menu_separator(ui);
            if menu_row(
                ui,
                Some(Icon::Trash),
                t(locale, Key::DeleteBlock),
                None,
                false,
                true,
                true,
            )
            .clicked()
            {
                op = Some(BlockOp::Delete);
            }
        });

    (changed, op)
}

fn draw_block_content(
    ui: &mut Ui,
    block: &mut Value,
    kind: &str,
    attachments: &mut [Attachment],
    locale: Locale,
) -> bool {
    match kind {
        "heading" => {
            let level = block
                .get("data")
                .and_then(|data| data.get("level"))
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 3);
            let size = match level {
                1 => 26.0,
                2 => 21.0,
                _ => 17.0,
            };
            let mut text = block_data_text(block);
            let response = ui.add(
                TextEdit::multiline(&mut text)
                    .font(FontId::new(size, theme::display()))
                    .text_color(theme::INK)
                    .frame(Frame::NONE)
                    .margin(Margin::same(0))
                    .desired_width(f32::INFINITY)
                    .desired_rows(1)
                    .hint_text(t(locale, Key::PlaceholderHeading)),
            );
            if response.changed() {
                set_block_text(block, text);
                true
            } else {
                false
            }
        }
        "paragraph" => {
            let mut text = block_data_text(block);
            let rows = text.lines().count().max(1);
            let mut layouter = |ui: &Ui, buffer: &dyn egui::TextBuffer, wrap: f32| {
                links::layout(ui, buffer.as_str(), 15.5, wrap)
            };
            let response = ui.add(
                TextEdit::multiline(&mut text)
                    .font(FontId::new(15.5, FontFamily::Proportional))
                    .text_color(theme::INK)
                    .frame(Frame::NONE)
                    .margin(Margin::same(0))
                    .desired_width(f32::INFINITY)
                    .desired_rows(rows)
                    .layouter(&mut layouter)
                    .hint_text(t(locale, Key::PlaceholderParagraph)),
            );
            if response.changed() {
                set_block_text(block, text);
                true
            } else {
                false
            }
        }
        "todo_list" => draw_todo_block(ui, block, locale),
        "callout" => draw_callout_block(ui, block, locale),
        "code_repl" => draw_code_block(ui, block, locale),
        "data_grid_view" => draw_grid_block(ui, block, attachments, locale),
        "audio_note" => draw_audio_block(ui, block, attachments, locale),
        "interactive_canvas" => draw_canvas_block(ui, block, locale),
        "ai_opponent_block" => draw_opponent_block(ui, block, locale),
        _ => {
            let inner = Frame::new()
                .fill(theme::SUNKEN)
                .corner_radius(theme::RADIUS)
                .stroke(theme::hairline())
                .inner_margin(Margin {
                    left: 14,
                    right: 14,
                    top: 11,
                    bottom: 11,
                })
                .show(ui, |ui| {
                    let _ = hud(
                        ui,
                        &format!(
                            "{} · {}",
                            t(locale, Key::BlockPreserved),
                            kind.replace('_', " ")
                        ),
                        theme::FAINT,
                    );
                    ui.add_space(6.0);
                    ui.add(
                        Label::new(
                            RichText::new(t(locale, Key::BlockPreservedHint))
                                .size(11.5)
                                .color(theme::MUTED),
                        )
                        .wrap(),
                    );
                });
            let _ = inner;
            false
        }
    }
}

fn draw_todo_block(ui: &mut Ui, block: &mut Value, locale: Locale) -> bool {
    let mut changed = false;
    let mut title = block
        .get("data")
        .and_then(|data| data.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let title_response = ui.add(
        TextEdit::singleline(&mut title)
            .font(FontId::new(15.0, theme::semibold()))
            .text_color(theme::INK)
            .frame(Frame::NONE)
            .margin(Margin::same(0))
            .desired_width(f32::INFINITY)
            .hint_text(t(locale, Key::BlockTodo)),
    );
    if title_response.changed() {
        block["data"]["title"] = json!(title);
        changed = true;
    }
    ui.add_space(7.0);

    let mut items = block
        .get("data")
        .and_then(|data| data.get("items"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut items_changed = false;
    let mut remove_item: Option<usize> = None;

    if items.is_empty() {
        ui.label(
            RichText::new(t(locale, Key::NoTasks))
                .size(11.5)
                .color(theme::FAINT),
        );
    }

    for (item_index, item) in items.iter_mut().enumerate() {
        let mut done = item
            .get("status")
            .and_then(Value::as_str)
            .map(|status| status == "done")
            .or_else(|| item.get("done").and_then(Value::as_bool))
            .unwrap_or(false);
        let due = item
            .get("due")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut next_status: Option<String> = None;
        let mut delete = false;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add_space(1.0);
            if checkbox(ui, &mut done).changed() {
                item["done"] = json!(done);
                item["status"] = json!(if done { "done" } else { "todo" });
                items_changed = true;
            }

            let trailing = 56.0
                + if due.is_empty() { 0.0 } else { 66.0 }
                + 8.0;
            let text_w = (ui.available_width() - trailing).max(80.0);
            let mut text = item
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let text_response = ui.add_sized(
                vec2(text_w, 22.0),
                TextEdit::singleline(&mut text)
                    .frame(Frame::NONE)
                    .margin(Margin::same(0))
                    .font(FontId::new(13.5, FontFamily::Proportional))
                    .text_color(if done { theme::MUTED } else { theme::INK }),
            );
            if text_response.changed() {
                item["text"] = json!(text);
                items_changed = true;
            }

            if !due.is_empty() {
                ui.label(
                    RichText::new(due)
                        .family(theme::mono())
                        .size(9.0)
                        .color(theme::FAINT),
                );
            }

            let current = item
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or(if done { "done" } else { "todo" })
                .to_string();
            let status = status_pill(ui, &current);
            egui::Popup::menu(&status)
                .frame(popover_frame())
                .align(egui::RectAlign::BOTTOM_END)
                .show(|ui| {
                    ui.set_min_width(170.0);
                    menu_header(ui, t(locale, Key::TaskStatus));
                    for candidate in ["todo", "doing", "done"] {
                        if menu_row(
                            ui,
                            None,
                            t(
                                locale,
                                match candidate {
                                    "todo" => Key::StatusTodo,
                                    "doing" => Key::StatusDoing,
                                    _ => Key::StatusDone,
                                },
                            ),
                            None,
                            current == candidate,
                            true,
                            false,
                        )
                        .clicked()
                        {
                            next_status = Some(candidate.to_string());
                        }
                    }
                    menu_separator(ui);
                    if menu_row(
                        ui,
                        Some(Icon::Trash),
                        t(locale, Key::DeleteTask),
                        None,
                        false,
                        true,
                        true,
                    )
                    .clicked()
                    {
                        delete = true;
                    }
                });
        });

        if let Some(status) = next_status {
            item["status"] = json!(status);
            item["done"] = json!(status == "done");
            items_changed = true;
        }
        if delete {
            remove_item = Some(item_index);
        }
    }

    if let Some(index) = remove_item {
        if index < items.len() {
            items.remove(index);
            items_changed = true;
        }
    }

    ui.add_space(6.0);
    if text_button(ui, Some(Icon::Plus), t(locale, Key::AddTaskShort), false).clicked() {
        items.push(json!({
            "id": format!("item_{}", now_nanos()),
            "text": "",
            "done": false,
            "status": "todo",
            "priority": "normal"
        }));
        items_changed = true;
    }

    if items_changed {
        block["data"]["items"] = Value::Array(items);
        changed = true;
    }
    changed
}

fn draw_callout_block(ui: &mut Ui, block: &mut Value, locale: Locale) -> bool {
    let mut changed = false;
    let tone = block
        .get("data")
        .and_then(|data| data.get("tone"))
        .and_then(Value::as_str)
        .unwrap_or("blue");
    let tone_color = match tone {
        "emerald" => Color32::from_rgb(52, 211, 153),
        "amber" => theme::BADGE_BETA,
        "purple" => Color32::from_rgb(167, 139, 250),
        "rose" => Color32::from_rgb(251, 113, 133),
        _ => theme::ACCENT,
    };

    let inner = Frame::new()
        .fill(theme::WASH)
        .corner_radius(theme::RADIUS)
        .inner_margin(Margin {
            left: 16,
            right: 14,
            top: 11,
            bottom: 11,
        })
        .show(ui, |ui| {
            let mut title = block
                .get("data")
                .and_then(|data| data.get("title"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let title_response = ui.add(
                TextEdit::singleline(&mut title)
                    .font(FontId::new(13.5, theme::semibold()))
                    .text_color(theme::INK)
                    .frame(Frame::NONE)
                    .margin(Margin::same(0))
                    .desired_width(f32::INFINITY)
                    .hint_text(t(locale, Key::BlockCallout)),
            );
            if title_response.changed() {
                block["data"]["title"] = json!(title);
                changed = true;
            }
            ui.add_space(4.0);

            let mut content = block
                .get("data")
                .and_then(|data| data.get("content"))
                .and_then(Value::as_str)
                .or_else(|| {
                    block
                        .get("data")
                        .and_then(|data| data.get("text"))
                        .and_then(Value::as_str)
                })
                .unwrap_or("")
                .to_string();
            let rows = content.lines().count().max(1);
            let content_response = ui.add(
                TextEdit::multiline(&mut content)
                    .font(FontId::new(13.5, FontFamily::Proportional))
                    .text_color(theme::MUTED)
                    .frame(Frame::NONE)
                    .margin(Margin::same(0))
                    .desired_width(f32::INFINITY)
                    .desired_rows(rows)
                    .hint_text("Write a note…"),
            );
            if content_response.changed() {
                block["data"]["content"] = json!(content);
                changed = true;
            }
        });

    ui.painter().rect_filled(
        Rect::from_min_size(
            inner.response.rect.min,
            vec2(2.0, inner.response.rect.height()),
        ),
        0.0,
        tone_color,
    );
    changed
}

// ---------------------------------------------------------------------------
// Blocks: code, data grid, audio, canvas, counter-argument, attachments
// ---------------------------------------------------------------------------

fn draw_code_block(ui: &mut Ui, block: &mut Value, locale: Locale) -> bool {
    let mut changed = false;
    Frame::new()
        .fill(theme::SUNKEN)
        .corner_radius(theme::RADIUS)
        .stroke(theme::hairline())
        .inner_margin(Margin {
            left: 14,
            right: 14,
            top: 10,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let language = block
                    .get("data")
                    .and_then(|data| data.get("language"))
                    .and_then(Value::as_str)
                    .unwrap_or("javascript")
                    .to_string();
                let response = ghost(
                    ui,
                    Icon::Code,
                    Some(language.as_str()),
                    t(locale, Key::Language),
                    true,
                    24.0,
                );
                egui::Popup::menu(&response)
                    .frame(popover_frame())
                    .align(egui::RectAlign::BOTTOM_START)
                    .show(|ui| {
                        ui.set_min_width(150.0);
                        menu_header(ui, t(locale, Key::Language));
                        for candidate in ["javascript", "typescript", "python", "rust", "sql", "json"]
                        {
                            if menu_row(
                                ui,
                                None,
                                candidate,
                                None,
                                language == candidate,
                                true,
                                false,
                            )
                            .clicked()
                            {
                                block["data"]["language"] = json!(candidate);
                                changed = true;
                            }
                        }
                    });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let _ = hud(ui, t(locale, Key::NotExecuted), theme::FAINT);
                });
            });
            ui.add_space(8.0);

            let mut code = block
                .get("data")
                .and_then(|data| data.get("code"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let rows = code.lines().count().max(4);
            let response = ui.add(
                TextEdit::multiline(&mut code)
                    .font(FontId::new(12.5, theme::mono()))
                    .text_color(theme::INK)
                    .frame(Frame::NONE)
                    .margin(Margin::same(0))
                    .desired_width(f32::INFINITY)
                    .desired_rows(rows)
                    .code_editor()
                    .hint_text(t(locale, Key::PlaceholderCode)),
            );
            if response.changed() {
                block["data"]["code"] = json!(code);
                changed = true;
            }

            if let Some(output) = block
                .get("data")
                .and_then(|data| data.get("output"))
                .and_then(Value::as_str)
                .filter(|output| !output.trim().is_empty())
            {
                ui.add_space(10.0);
                let _ = hud(ui, t(locale, Key::CodeOutput), theme::FAINT);
                ui.add_space(3.0);
                ui.add(
                    Label::new(
                        RichText::new(output)
                            .family(theme::mono())
                            .size(11.5)
                            .color(theme::MUTED),
                    )
                    .wrap(),
                );
            }
            ui.add_space(6.0);
            let _ = hud(ui, t(locale, Key::RunUnavailable), theme::fade(theme::FAINT, 0.85));
        });
    changed
}

fn draw_grid_block(
    ui: &mut Ui,
    block: &mut Value,
    attachments: &mut [Attachment],
    locale: Locale,
) -> bool {
    let source = block
        .get("data")
        .and_then(|data| data.get("source_cid"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let clean = source
        .trim_start_matches("cid:")
        .trim_matches(|ch| ch == '<' || ch == '>')
        .to_string();
    let found = attachments.iter().position(|attachment| {
        attachment.cid == clean
            || format!("cid:{}", attachment.cid) == source
            || attachment.name == clean
    });

    let mut changed = false;
    Frame::new()
        .fill(theme::SUNKEN)
        .corner_radius(theme::RADIUS)
        .stroke(theme::hairline())
        .inner_margin(Margin {
            left: 12,
            right: 12,
            top: 10,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let name = found
                    .map(|index| attachments[index].name.clone())
                    .unwrap_or_else(|| t(locale, Key::BlockGrid).to_string());
                let _ = hud(ui, &name, theme::FAINT);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if found.is_some() {
                        if text_button(ui, None, t(locale, Key::AddRow), false).clicked() {
                            block["data"]["__add_row"] = json!(true);
                        }
                        if text_button(ui, None, t(locale, Key::AddColumn), false).clicked() {
                            block["data"]["__add_col"] = json!(true);
                        }
                    }
                });
            });
            ui.add_space(8.0);

            let Some(index) = found else {
                let _ = hud(ui, t(locale, Key::DataGridMissing), theme::MUTED);
                return;
            };

            let mut table = parse_csv(&attachments[index].content);
            if table.is_empty() {
                table.push(vec![t(locale, Key::RenameField).to_string()]);
                table.push(vec![String::new()]);
            }
            let add_row = block["data"]
                .get("__add_row")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let add_col = block["data"]
                .get("__add_col")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if let Some(data) = block.get_mut("data").and_then(Value::as_object_mut) {
                data.remove("__add_row");
                data.remove("__add_col");
            }
            if add_row {
                let width = table[0].len();
                table.push(vec![String::new(); width]);
            }
            if add_col {
                for row in table.iter_mut() {
                    row.push(String::new());
                }
            }

            let columns = table.iter().map(Vec::len).max().unwrap_or(0).max(1);
            let cell_width = ((ui.available_width() - 24.0) / columns as f32 - 8.0).max(70.0);
            let mut table_changed = false;

            ScrollArea::horizontal()
                .id_salt((
                    "grid-scroll",
                    block.get("id").and_then(Value::as_str).unwrap_or(""),
                ))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for (row_index, row) in table.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            for column in 0..columns {
                                if row.len() <= column {
                                    row.push(String::new());
                                }
                                let mut cell = row[column].clone();
                                let font = if row_index == 0 {
                                    FontId::new(11.0, theme::mono_bold())
                                } else {
                                    FontId::new(11.5, theme::mono())
                                };
                                let response = ui.add_sized(
                                    vec2(cell_width, 22.0),
                                    TextEdit::singleline(&mut cell)
                                        .font(font)
                                        .text_color(if row_index == 0 {
                                            theme::MUTED
                                        } else {
                                            theme::INK
                                        })
                                        .frame(Frame::NONE)
                                        .margin(Margin::symmetric(4, 2))
                                        .desired_width(cell_width),
                                );
                                if response.changed() {
                                    row[column] = cell;
                                    table_changed = true;
                                }
                            }
                        });
                        let line_y = ui.cursor().min.y;
                        let line_x = ui.cursor().min.x;
                        ui.painter().hline(
                            egui::Rangef::new(line_x, line_x + cell_width * columns as f32),
                            line_y,
                            Stroke::new(1.0, theme::fade(theme::LINE, 0.55)),
                        );
                        ui.add_space(2.0);
                    }
                });

            if table_changed {
                attachments[index].content = to_csv(&table);
                attachments[index].size_bytes = attachments[index].content.len();
                changed = true;
            }
            if add_row || add_col {
                attachments[index].content = to_csv(&table);
                attachments[index].size_bytes = attachments[index].content.len();
                changed = true;
            }
        });
    changed
}

fn draw_audio_block(
    ui: &mut Ui,
    block: &mut Value,
    attachments: &[Attachment],
    locale: Locale,
) -> bool {
    let mut changed = false;
    let audio_cid = block
        .get("data")
        .and_then(|data| data.get("audioCid"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim_start_matches("cid:")
        .to_string();
    let file = attachments
        .iter()
        .find(|attachment| attachment.cid == audio_cid);

    Frame::new()
        .fill(theme::SUNKEN)
        .corner_radius(theme::RADIUS)
        .stroke(theme::hairline())
        .inner_margin(Margin {
            left: 14,
            right: 14,
            top: 12,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::paint(
                    ui.painter(),
                    Rect::from_min_size(ui.cursor().min + vec2(0.0, 2.0), vec2(14.0, 14.0)),
                    Icon::Audio,
                    theme::ACCENT_INK,
                );
                ui.add_space(20.0);
                let name = file
                    .map(|attachment| attachment.name.clone())
                    .unwrap_or_else(|| t(locale, Key::AudioNoFile).to_string());
                ui.label(RichText::new(name).size(12.0).color(theme::INK));
                ui.add_space(6.0);
                let duration = block
                    .get("data")
                    .and_then(|data| data.get("duration"))
                    .and_then(Value::as_str)
                    .unwrap_or("0:00");
                let _ = hud(ui, duration, theme::FAINT);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if text_button(ui, Some(Icon::Plus), t(locale, Key::AttachAudio), false).clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_title(t(locale, Key::AttachAudio))
                            .add_filter("audio", &["mp3", "wav", "ogg", "m4a", "flac", "webm"])
                            .pick_file()
                        {
                            if let Ok(bytes) = std::fs::read(&path) {
                                let name = path
                                    .file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or("audio")
                                    .to_string();
                                let cid = format!("audio-{}", sanitize_cid(&name));
                                let mime = guess_mime(&name);
                                let encoded =
                                    base64::engine::general_purpose::STANDARD.encode(&bytes);
                                let size = bytes.len();
                                block["data"]["audioCid"] = json!(format!("cid:{cid}"));
                                block["data"]["audioMime"] = json!(mime.clone());
                                block["data"]["audioName"] = json!(name.clone());
                                block["data"]["__attach"] = json!({
                                    "name": name,
                                    "cid": cid,
                                    "mime": mime,
                                    "b64": encoded,
                                    "size": size,
                                });
                                changed = true;
                            }
                        }
                    }
                });
            });
            ui.add_space(10.0);

            // Waveform: real amplitude values when the file carried them, bars otherwise.
            let waveform: Vec<f32> = block
                .get("data")
                .and_then(|data| data.get("waveform"))
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_f64)
                        .map(|value| value as f32)
                        .collect()
                })
                .unwrap_or_default();
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
            let bars = waveform.len().max(24);
            let peak = waveform.iter().cloned().fold(1.0_f32, f32::max);
            for index in 0..bars {
                let value = waveform
                    .get(index)
                    .copied()
                    .unwrap_or_else(|| 3.0 + ((index * 7) % 11) as f32);
                let height = (value / peak * 26.0).clamp(2.0, 26.0);
                let x = rect.left() + index as f32 * (rect.width() / bars as f32);
                ui.painter().vline(
                    x,
                    egui::Rangef::new(
                        rect.center().y - height / 2.0,
                        rect.center().y + height / 2.0,
                    ),
                    Stroke::new(1.0, theme::fade(theme::ACCENT_INK, 0.55)),
                );
            }

            ui.add_space(8.0);
            let mut transcript = block
                .get("data")
                .and_then(|data| data.get("transcript"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let rows = transcript.lines().count().max(2);
            let response = ui.add(
                TextEdit::multiline(&mut transcript)
                    .font(FontId::new(13.0, FontFamily::Proportional))
                    .text_color(theme::MUTED)
                    .frame(Frame::NONE)
                    .margin(Margin::same(0))
                    .desired_width(f32::INFINITY)
                    .desired_rows(rows)
                    .hint_text(t(locale, Key::Transcript)),
            );
            if response.changed() {
                block["data"]["transcript"] = json!(transcript);
                changed = true;
            }
            ui.add_space(6.0);
            let _ = hud(ui, t(locale, Key::NoPlayback), theme::FAINT);
        });
    changed
}

fn draw_canvas_block(ui: &mut Ui, block: &mut Value, locale: Locale) -> bool {
    let mut changed = false;
    let block_id = block
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("canvas")
        .to_string();
    let mut elements: Vec<Value> = block
        .get("data")
        .and_then(|data| data.get("elements"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    Frame::new()
        .fill(theme::SUNKEN)
        .corner_radius(theme::RADIUS)
        .stroke(theme::hairline())
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let _ = hud(ui, t(locale, Key::BlockCanvas), theme::FAINT);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if text_button(ui, None, t(locale, Key::CanvasReset), false).clicked() {
                        elements.clear();
                        changed = true;
                    }
                    if text_button(ui, None, t(locale, Key::CanvasAddText), false).clicked() {
                        elements.push(canvas_element("text", elements.len(), locale));
                        changed = true;
                    }
                    if text_button(ui, None, t(locale, Key::CanvasAddRect), false).clicked() {
                        elements.push(canvas_element("rectangle", elements.len(), locale));
                        changed = true;
                    }
                    if text_button(ui, None, t(locale, Key::CanvasAddSticky), false).clicked() {
                        elements.push(canvas_element("sticky", elements.len(), locale));
                        changed = true;
                    }
                });
            });
            ui.add_space(8.0);

            let (canvas_rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), 300.0), Sense::hover());
            let painter = ui.painter_at(canvas_rect);
            painter.rect_filled(canvas_rect, theme::RADIUS, theme::BG);
            painter.rect_stroke(
                canvas_rect,
                theme::RADIUS,
                theme::hairline(),
                egui::StrokeKind::Inside,
            );

            let selected_id: Option<String> = ui
                .data(|data| data.get_temp(egui::Id::new(("canvas-selected", block_id.clone()))));
            let mut next_selected = selected_id.clone();

            for element in elements.iter_mut() {
                let x = element.get("x").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                let y = element.get("y").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                let width = element
                    .get("width")
                    .and_then(Value::as_f64)
                    .unwrap_or(140.0) as f32;
                let height = element
                    .get("height")
                    .and_then(Value::as_f64)
                    .unwrap_or(80.0) as f32;
                let id = element
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let kind = element
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("sticky")
                    .to_string();
                let rect = Rect::from_min_size(canvas_rect.min + vec2(x, y), vec2(width, height))
                    .intersect(canvas_rect);
                if rect.width() < 4.0 || rect.height() < 4.0 {
                    continue;
                }
                let response = ui.interact(
                    rect,
                    egui::Id::new(("canvas-element", block_id.clone(), id.clone())),
                    Sense::click_and_drag(),
                );
                if response.dragged() {
                    let delta = response.drag_delta();
                    element["x"] = json!(x + delta.x);
                    element["y"] = json!(y + delta.y);
                    changed = true;
                }
                if response.clicked() {
                    next_selected = Some(id.clone());
                }

                let is_selected = selected_id.as_deref() == Some(id.as_str());
                let (fill, stroke) = match kind.as_str() {
                    "rectangle" => (theme::WASH, theme::LINE_STRONG),
                    "text" => (Color32::TRANSPARENT, theme::fade(theme::LINE, 0.0)),
                    _ => (Color32::from_rgb(45, 129, 255), theme::fade(theme::ACCENT_INK, 0.4)),
                };
                if kind != "text" {
                    painter.rect(
                        rect,
                        if kind == "sticky" {
                            egui::CornerRadius::same(4)
                        } else {
                            theme::RADIUS
                        },
                        fill,
                        Stroke::new(
                            if is_selected { 1.6 } else { 1.0 },
                            if is_selected {
                                theme::ACCENT_INK
                            } else {
                                stroke
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                } else if is_selected {
                    painter.rect_stroke(
                        rect,
                        theme::RADIUS,
                        Stroke::new(1.0, theme::ACCENT_INK),
                        egui::StrokeKind::Inside,
                    );
                }
                let text = element
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                painter.text(
                    rect.min + vec2(8.0, 7.0),
                    egui::Align2::LEFT_TOP,
                    text,
                    FontId::new(11.5, FontFamily::Proportional),
                    if kind == "sticky" {
                        theme::ACCENT_INK
                    } else {
                        theme::INK
                    },
                );
            }

            if elements.is_empty() {
                painter.text(
                    canvas_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    t(locale, Key::CanvasEmpty),
                    FontId::new(11.5, FontFamily::Proportional),
                    theme::FAINT,
                );
            }

            if let Some(selected) = &next_selected {
                let selected = selected.clone();
                ui.horizontal(|ui| {
                    ui.add_space(2.0);
                    if text_button(ui, Some(Icon::Trash), t(locale, Key::CanvasDelete), true)
                        .clicked()
                    {
                        elements.retain(|element| {
                            element.get("id").and_then(Value::as_str) != Some(selected.as_str())
                        });
                        next_selected = None;
                        changed = true;
                    }
                    let _ = hud(ui, t(locale, Key::CanvasHint), theme::FAINT);
                });
            } else {
                ui.add_space(4.0);
                let _ = hud(ui, t(locale, Key::CanvasHint), theme::FAINT);
            }

            ui.data_mut(|data| {
                data.insert_temp(egui::Id::new(("canvas-selected", block_id)), next_selected)
            });
        });

    if changed {
        block["data"]["elements"] = Value::Array(elements);
    }
    changed
}

fn canvas_element(kind: &str, index: usize, locale: Locale) -> Value {
    let offset = (index % 6) as f64 * 18.0;
    json!({
        "id": format!("el_{}", now_nanos()),
        "type": kind,
        "x": 24.0 + offset,
        "y": 24.0 + offset,
        "width": if kind == "text" { 180.0 } else { 150.0 },
        "height": if kind == "text" { 28.0 } else { 90.0 },
        "text": match kind {
            "rectangle" => String::new(),
            "text" => t(locale, Key::CanvasAddText).to_string(),
            _ => t(locale, Key::CanvasAddSticky).to_string(),
        },
        "color": "",
    })
}

fn draw_opponent_block(ui: &mut Ui, block: &mut Value, locale: Locale) -> bool {
    let mut changed = false;
    Frame::new()
        .fill(theme::WASH)
        .corner_radius(theme::RADIUS)
        .stroke(theme::hairline())
        .inner_margin(Margin {
            left: 14,
            right: 14,
            top: 11,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let _ = hud(ui, t(locale, Key::CounterArg), theme::BADGE_BETA);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let confidence = block
                        .get("data")
                        .and_then(|data| data.get("confidence"))
                        .and_then(Value::as_f64)
                        .unwrap_or(0.5);
                    let _ = hud(
                        ui,
                        &format!(
                            "{} {}%",
                            t(locale, Key::Confidence),
                            (confidence * 100.0).round() as i64
                        ),
                        theme::FAINT,
                    );
                });
            });
            ui.add_space(6.0);

            let mut criticism = block
                .get("data")
                .and_then(|data| data.get("criticism"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let rows = criticism.lines().count().max(2);
            let response = ui.add(
                TextEdit::multiline(&mut criticism)
                    .font(FontId::new(13.5, FontFamily::Proportional))
                    .text_color(theme::INK)
                    .frame(Frame::NONE)
                    .margin(Margin::same(0))
                    .desired_width(f32::INFINITY)
                    .desired_rows(rows)
                    .hint_text(t(locale, Key::Critique)),
            );
            if response.changed() {
                block["data"]["criticism"] = json!(criticism);
                changed = true;
            }

            ui.add_space(8.0);
            let mut claim = block
                .get("data")
                .and_then(|data| data.get("sourceClaim"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let response = ui.add(
                TextEdit::singleline(&mut claim)
                    .font(FontId::new(11.5, FontFamily::Proportional))
                    .text_color(theme::MUTED)
                    .frame(Frame::NONE)
                    .margin(Margin::same(0))
                    .desired_width(f32::INFINITY)
                    .hint_text(t(locale, Key::SourceClaim)),
            );
            if response.changed() {
                block["data"]["sourceClaim"] = json!(claim);
                changed = true;
            }

            ui.add_space(4.0);
            let mut confidence = block
                .get("data")
                .and_then(|data| data.get("confidence"))
                .and_then(Value::as_f64)
                .unwrap_or(0.5) as f32;
            if ui
                .add(
                    egui::Slider::new(&mut confidence, 0.0..=1.0)
                        .show_value(false)
                        .trailing_fill(true),
                )
                .changed()
            {
                block["data"]["confidence"] = json!(confidence as f64);
                changed = true;
            }
            ui.add_space(4.0);
            let _ = hud(ui, t(locale, Key::HeuristicNotice), theme::FAINT);
        });
    changed
}

/// Attachment list for one page. Files are stored base64-encoded in the MIME
/// part, exactly like the web app does, so the .eml stays self-contained.
fn attachments_editor(ui: &mut Ui, capsule: &mut Capsule, locale: Locale) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let _ = hud(ui, t(locale, Key::AttachmentsTitle), theme::FAINT);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if text_button(ui, Some(Icon::Plus), t(locale, Key::AddAttachment), false).clicked() {
                if let Some(paths) = rfd::FileDialog::new()
                    .set_title(t(locale, Key::AddAttachment))
                    .pick_files()
                {
                    for path in paths {
                        let Ok(bytes) = std::fs::read(&path) else {
                            continue;
                        };
                        let name = path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("attachment")
                            .to_string();
                        capsule.attachments.push(Attachment {
                            name: name.clone(),
                            cid: format!("{}@capsuleos", sanitize_cid(&name)),
                            mime_type: guess_mime(&name),
                            content: String::from_utf8_lossy(&bytes).to_string(),
                            size_bytes: bytes.len(),
                            raw_b64: Some(
                                base64::engine::general_purpose::STANDARD.encode(&bytes),
                            ),
                        });
                        changed = true;
                    }
                }
            }
        });
    });
    ui.add_space(6.0);

    if capsule.attachments.is_empty() {
        let _ = hud(ui, t(locale, Key::EmptyAttachments), theme::FAINT);
        return changed;
    }

    let mut remove: Option<usize> = None;
    for (index, attachment) in capsule.attachments.iter().enumerate() {
        ui.horizontal(|ui| {
            icons::paint(
                ui.painter(),
                Rect::from_min_size(ui.cursor().min + vec2(0.0, 3.0), vec2(12.0, 12.0)),
                Icon::Doc,
                theme::FAINT,
            );
            ui.add_space(18.0);
            ui.label(
                RichText::new(truncate_chars(&attachment.name, 48))
                    .size(12.0)
                    .color(theme::MUTED),
            );
            ui.add_space(6.0);
            let _ = hud(ui, &human_bytes(attachment.size_bytes), theme::FAINT);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(ui, Icon::Trash, t(locale, Key::RemoveAttachment), true, false)
                    .clicked()
                {
                    remove = Some(index);
                }
            });
        });
    }
    if let Some(index) = remove {
        capsule.attachments.remove(index);
        changed = true;
    }
    changed
}

/// Split CSV into rows. Deliberately simple: no quoted fields, no embedded
/// commas — the same limitation the web grid has.
fn parse_csv(raw: &str) -> Vec<Vec<String>> {
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            line.split(',')
                .map(|cell| cell.trim().to_string())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn to_csv(table: &[Vec<String>]) -> String {
    table
        .iter()
        .map(|row| row.join(","))
        .collect::<Vec<_>>()
        .join("\n")
}

fn sanitize_cid(name: &str) -> String {
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

fn guess_mime(name: &str) -> String {
    let extension = name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "csv" => "text/csv",
        "json" => "application/json",
        "md" => "text/markdown",
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "m4a" => "audio/mp4",
        "flac" => "audio/flac",
        "webm" => "audio/webm",
        _ => "application/octet-stream",
    }
    .to_string()
}

// ---------------------------------------------------------------------------
// Widgets
// ---------------------------------------------------------------------------

pub(crate) fn caps(
    ui: &Ui,
    text: &str,
    size: f32,
    family: FontFamily,
    spacing: f32,
    color: Color32,
) -> Arc<Galley> {
    let format = egui::TextFormat {
        font_id: FontId::new(size, family),
        color,
        extra_letter_spacing: spacing,
        ..Default::default()
    };
    ui.painter()
        .layout_job(egui::text::LayoutJob::single_section(
            text.to_uppercase(),
            format,
        ))
}

pub(crate) fn hud(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let galley = caps(
        ui,
        text,
        theme::HUD_SIZE,
        theme::mono_bold(),
        theme::HUD_SPACING,
        color,
    );
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::hover());
    ui.painter().galley(rect.min, galley, color);
    response
}

pub(crate) fn chevron(ui: &mut Ui, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), vec2(9.0, 9.0)),
        Icon::ChevronRight,
        color,
    );
    response
}

pub(crate) fn crumb_button(ui: &mut Ui, text: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        FontId::new(12.0, FontFamily::Proportional),
        theme::MUTED,
    );
    let (rect, response) = ui.allocate_exact_size(galley.size() + vec2(4.0, 2.0), Sense::click());
    let ink = if response.hovered() {
        theme::INK
    } else {
        theme::MUTED
    };
    ui.painter().galley_with_override_text_color(
        pos2(rect.left() + 2.0, rect.top() + 1.0),
        galley,
        ink,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn dot_sep(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(4.0, 10.0), Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), 1.2, theme::fade(theme::FAINT, 0.8));
}

pub(crate) fn tag_chip(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        FontId::new(9.5, theme::mono()),
        color,
    );
    let size = galley.size() + vec2(14.0, 6.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect(
        rect,
        theme::RADIUS,
        theme::fade(color, 0.07),
        Stroke::new(1.0, theme::fade(color, 0.3)),
        egui::StrokeKind::Inside,
    );
    ui.painter()
        .galley(pos2(rect.left() + 7.0, rect.top() + 3.0), galley, color);
    response
}

pub(crate) fn icon_button(
    ui: &mut Ui,
    icon: Icon,
    tip: &str,
    enabled: bool,
    active: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        vec2(28.0, 28.0),
        if enabled { Sense::click() } else { Sense::hover() },
    );
    let hovered = response.hovered() && enabled;
    if hovered || active {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::WASH);
    }
    let ink = if !enabled {
        theme::fade(theme::DIM, 0.4)
    } else if hovered || active {
        theme::INK
    } else {
        theme::DIM
    };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), vec2(15.0, 15.0)),
        icon,
        ink,
    );
    let response = response.on_hover_text(tip);
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

pub(crate) fn ghost(
    ui: &mut Ui,
    icon: Icon,
    label: Option<&str>,
    tip: &str,
    enabled: bool,
    height: f32,
) -> Response {
    let galley = label.map(|label| {
        ui.painter()
            .layout_no_wrap(label.to_string(), FontId::new(12.0, theme::medium()), theme::INK)
    });
    let width = match &galley {
        Some(galley) => 10.0 + 15.0 + 6.0 + galley.size().x + 10.0,
        None => 30.0,
    };
    let (rect, response) = ui.allocate_exact_size(
        vec2(width, height),
        if enabled { Sense::click() } else { Sense::hover() },
    );
    let hovered = response.hovered() && enabled;
    if hovered {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::WASH);
    }
    let ink = match (enabled, hovered) {
        (false, _) => theme::fade(theme::DIM, 0.4),
        (true, true) => theme::INK,
        (true, false) => theme::DIM,
    };
    match galley {
        Some(galley) => {
            let icon_rect = Rect::from_center_size(
                pos2(rect.left() + 10.0 + 7.5, rect.center().y),
                vec2(14.0, 14.0),
            );
            icons::paint(ui.painter(), icon_rect, icon, ink);
            ui.painter().galley_with_override_text_color(
                pos2(icon_rect.right() + 6.0, rect.center().y - galley.size().y / 2.0),
                galley,
                ink,
            );
        }
        None => {
            icons::paint(
                ui.painter(),
                Rect::from_center_size(rect.center(), vec2(15.0, 15.0)),
                icon,
                ink,
            );
        }
    }
    let response = response.on_hover_text(tip);
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

pub(crate) fn text_button(ui: &mut Ui, icon: Option<Icon>, label: &str, active: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        FontId::new(11.5, theme::medium()),
        theme::MUTED,
    );
    let width = 8.0 + if icon.is_some() { 14.0 + 6.0 } else { 0.0 } + galley.size().x + 8.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, 24.0), Sense::click());
    let hovered = response.hovered();
    if hovered || active {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::WASH);
    }
    let ink = if hovered { theme::INK } else { theme::MUTED };
    let mut x = rect.left() + 8.0;
    if let Some(icon) = icon {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(x + 7.0, rect.center().y), vec2(13.0, 13.0)),
            icon,
            ink,
        );
        x += 20.0;
    }
    ui.painter().galley_with_override_text_color(
        pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn primary_button(ui: &mut Ui, label: &str, height: f32) -> Response {
    let galley = caps(
        ui,
        label,
        10.0,
        theme::mono_bold(),
        1.2,
        theme::INVERT_FG,
    );
    let width = galley.size().x + 36.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let hovered = response.hovered();
    ui.painter().rect_filled(
        rect,
        theme::RADIUS,
        if hovered {
            theme::fade(theme::INVERT_BG, 0.9)
        } else {
            theme::INVERT_BG
        },
    );
    ui.painter().galley(
        rect.center() - galley.size() / 2.0,
        galley,
        theme::INVERT_FG,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn secondary_button(
    ui: &mut Ui,
    icon: Option<Icon>,
    label: &str,
    height: f32,
) -> Response {
    let galley = caps(ui, label, 10.0, theme::mono_bold(), 1.2, theme::INK);
    let icon_w = if icon.is_some() { 13.0 + 8.0 } else { 0.0 };
    let width = galley.size().x + icon_w + 36.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let hovered = response.hovered();
    let (fill, stroke, ink) = if hovered {
        (theme::INVERT_BG, Stroke::NONE, theme::INVERT_FG)
    } else {
        (
            Color32::TRANSPARENT,
            Stroke::new(1.0, theme::LINE_STRONG),
            theme::INK,
        )
    };
    ui.painter().rect(
        rect,
        theme::RADIUS,
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );
    let content_w = icon_w + galley.size().x;
    let mut x = rect.center().x - content_w / 2.0;
    if let Some(icon) = icon {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(x + 6.5, rect.center().y), vec2(13.0, 13.0)),
            icon,
            ink,
        );
        x += icon_w;
    }
    ui.painter().galley_with_override_text_color(
        pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn checkbox(ui: &mut Ui, done: &mut bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
    let hovered = response.hovered();
    let fill = if *done {
        theme::ACCENT
    } else if hovered {
        theme::WASH_HOVER
    } else {
        Color32::TRANSPARENT
    };
    let stroke = if *done {
        Stroke::NONE
    } else {
        Stroke::new(1.0, if hovered { theme::INK } else { theme::LINE_STRONG })
    };
    ui.painter()
        .rect(rect, theme::RADIUS, fill, stroke, egui::StrokeKind::Inside);
    if *done {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(rect.center(), vec2(11.0, 11.0)),
            Icon::Check,
            Color32::WHITE,
        );
    }
    if response.clicked() {
        *done = !*done;
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn status_pill(ui: &mut Ui, status: &str) -> Response {
    let (label, color) = match status {
        "doing" => ("DOING", theme::ACCENT_INK),
        "done" => ("DONE", theme::FAINT),
        _ => ("TO DO", theme::MUTED),
    };
    let galley = caps(ui, label, 8.0, theme::mono_bold(), 0.9, color);
    let width = 8.0 + 5.0 + 5.0 + galley.size().x + 8.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, 20.0), Sense::click());
    let hovered = response.hovered();
    ui.painter().rect(
        rect,
        theme::RADIUS,
        if hovered {
            theme::WASH_HOVER
        } else {
            theme::WASH
        },
        Stroke::new(1.0, theme::fade(color, if hovered { 0.5 } else { 0.25 })),
        egui::StrokeKind::Inside,
    );
    ui.painter().circle_filled(
        pos2(rect.left() + 9.0, rect.center().y),
        2.0,
        color,
    );
    ui.painter().galley(
        pos2(
            rect.left() + 17.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
    response
        .on_hover_text("Task status and actions")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) fn popover_frame() -> Frame {
    Frame::new()
        .fill(theme::CONTROLS)
        .stroke(theme::hairline())
        .corner_radius(theme::RADIUS)
        .shadow(theme::shadow())
        .inner_margin(Margin::same(4))
}

pub(crate) fn menu_header(ui: &mut Ui, text: &str) {
    let galley = caps(
        ui,
        text,
        theme::HUD_SIZE,
        theme::mono_bold(),
        theme::LABEL_SPACING,
        theme::fade(theme::INK, 0.5),
    );
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    ui.painter().galley(
        pos2(rect.left() + 7.0, rect.center().y - galley.size().y / 2.0),
        galley,
        theme::INK,
    );
}

pub(crate) fn menu_separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, theme::hairline());
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn menu_row(
    ui: &mut Ui,
    icon: Option<Icon>,
    label: &str,
    shortcut: Option<&str>,
    selected: bool,
    enabled: bool,
    danger: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), 30.0),
        if enabled { Sense::click() } else { Sense::hover() },
    );
    if response.hovered() && enabled {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::WASH_HOVER);
    }
    let ink = if !enabled {
        theme::FAINT
    } else if danger {
        theme::BAD
    } else {
        theme::INK
    };
    let mut x = rect.left() + 8.0;
    if let Some(icon) = icon {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(x + 7.0, rect.center().y), vec2(14.0, 14.0)),
            icon,
            ink,
        );
        x += 22.0;
    }
    ui.painter().text(
        pos2(x, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::new(12.5, theme::medium()),
        ink,
    );
    if let Some(shortcut) = shortcut {
        ui.painter().text(
            pos2(rect.right() - 8.0, rect.center().y),
            Align2::RIGHT_CENTER,
            shortcut,
            FontId::new(9.0, theme::mono()),
            theme::FAINT,
        );
    }
    if selected {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(12.0, 12.0)),
            Icon::Check,
            theme::INK,
        );
    }
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

pub(crate) fn info_popover(ui: &mut Ui, capsule: &Capsule) {
    ui.set_width(342.0);
    ui.add_space(2.0);
    menu_header(ui, "Page info");
    info_row(ui, "Message-ID", &capsule.message_id, true);
    info_row(ui, "File", &capsule.file_name, true);
    info_row(ui, "Date", &format_date_long(capsule), false);
    info_row(ui, "From", &capsule.from, false);
    info_row(ui, "To", &capsule.to, false);
    info_row(
        ui,
        "Format",
        if capsule.native {
            "CapsuleCraft canvas · writable"
        } else {
            "Imported email · read-only"
        },
        false,
    );
    if !capsule.tags.is_empty() {
        info_row(ui, "Tags", &capsule.tags.join(" · "), false);
    }
    if capsule.attachments.is_empty() {
        info_row(ui, "Attachments", "None", false);
    } else {
        info_row(
            ui,
            "Attachments",
            &format!("{} files", capsule.attachments.len()),
            false,
        );
        for attachment in &capsule.attachments {
            ui.label(
                RichText::new(format!(
                    "{}  {}",
                    attachment.name,
                    human_bytes(attachment.size_bytes)
                ))
                .family(theme::mono())
                .size(9.5)
                .color(theme::MUTED),
            );
        }
    }
    ui.add_space(6.0);
}

fn info_row(ui: &mut Ui, label: &str, value: &str, mono: bool) {
    ui.add_space(8.0);
    let _ = hud(ui, label, theme::FAINT);
    ui.add_space(3.0);
    let rich = if mono {
        RichText::new(value)
            .family(theme::mono())
            .size(10.5)
            .color(theme::INK)
    } else {
        RichText::new(value)
            .size(11.5)
            .color(theme::INK)
    };
    ui.add(Label::new(rich).wrap().selectable(true));
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn folder_name(dir: &Path) -> String {
    dir.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "Vault".to_string())
}

pub(crate) fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let mut out: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

pub(crate) fn truncate_to_width(ui: &Ui, text: &str, font: FontId, max_width: f32) -> String {
    let galley = ui.painter().layout_no_wrap(text.to_string(), font.clone(), theme::INK);
    if galley.size().x <= max_width {
        return text.to_string();
    }
    let ellipsis = "…";
    let ellipsis_w = ui.painter().layout_no_wrap(ellipsis.to_string(), font.clone(), theme::INK).size().x;
    let target_w = (max_width - ellipsis_w).max(0.0);

    let mut result = String::new();
    for ch in text.chars() {
        let mut test = result.clone();
        test.push(ch);
        let test_g = ui.painter().layout_no_wrap(test, font.clone(), theme::INK);
        if test_g.size().x > target_w {
            break;
        }
        result.push(ch);
    }
    result.push('…');
    result
}

pub(crate) fn format_date(capsule: &Capsule) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(capsule.timestamp)
        .map(|date| {
            date.with_timezone(&chrono::Local)
                .format("%b %e, %Y · %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| capsule.date.clone())
}

fn format_date_long(capsule: &Capsule) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(capsule.timestamp)
        .map(|date| {
            date.with_timezone(&chrono::Local)
                .format("%A, %b %e, %Y at %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| capsule.date.clone())
}

pub(crate) fn human_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

pub(crate) fn status_label(status: &str) -> &'static str {
    match status {
        "doing" => "Doing",
        "done" => "Done",
        _ => "To do",
    }
}

pub(crate) fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

// Re-exported from mime for block content helpers.
fn block_data_text(block: &Value) -> String {
    mime::block_data_text(block)
}

fn set_block_text(block: &mut Value, text: String) {
    mime::set_block_text(block, text)
}

// Keep unused-import warnings honest while the prototype is small.
#[allow(dead_code)]
fn _vec2(_: Vec2, _: Pos2) {}
