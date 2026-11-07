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

mod ai;
mod audio_player;
mod audio_recorder;
mod blocks;
mod brand;
mod chrome;
mod cli;
mod components;
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
mod pdf;
mod planning;
mod preview;
pub mod rich_text;
mod search;
mod snapshot;
mod sync_imap;
mod theme;
mod ui_state;
mod project_parser;
mod project_skill;
mod project_verify;
mod util;
mod vault;
mod viewer;
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
    let window = launch.window_size.unwrap_or(if launch.snapshot.is_some() {
        [1440.0, 900.0]
    } else {
        [1380.0, 880.0]
    });
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
            theme::load();
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
    /// Block index pending confirmation (None = no block delete in flight).
    confirm_block: Option<usize>,
    show_appearance: bool,
    show_onboarding: bool,
    onboarding_step: usize,
    show_mcp_modal: bool,
    mcp_tab: usize,
    show_imap_modal: bool,
    imap_config: sync_imap::ImapConfig,
    imap_status: sync_imap::SyncStatus,
    /// Result of the background sync thread, if one is running.
    imap_job: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    imap_last_sync: Option<Instant>,
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
    /// Downloadable integrations: catalog state + install markers.
    components: components::Manager,
    /// Integrations modal visibility.
    show_integrations: bool,
    /// Open attachment viewer (image/PDF overlay).
    viewer: Option<viewer::ViewerState>,
    /// Inline page previews (images + PDF first pages), cached per session.
    previews: preview::PreviewCache,
    /// Running local OCR engine actor (process lives in a worker thread).
    ocr: Option<ai::ocr::OcrActor>,
    /// Model directory the running actor was spawned with; a custom-model
    /// change restarts the engine instead of silently ignoring the new path.
    ocr_model_dir: Option<PathBuf>,
    /// The single in-flight OCR job; progress is streamed as events and
    /// surfaced in the bottom-right status card.
    ocr_job: Option<OcrJob>,
    /// Last recognized text — a persistent result card ("куда оно добавилось?")
    /// that stays until dismissed, unlike the toast.
    ocr_result: Option<OcrResult>,
    /// (block id, since) of the paragraph OCR just appended: the page scrolls
    /// to it and strokes it until it expires.
    ocr_highlight: Option<(String, Instant)>,
    /// Speech-to-text jobs keyed by attachment cid.
    stt: ai::stt::SttJobs,
    /// TTS result receiver for the single in-flight speak job.
    tts_rx: Option<std::sync::mpsc::Receiver<Result<Vec<u8>, String>>>,
}

struct SnapshotJob {
    path: PathBuf,
    frames_left: u32,
    requested: bool,
}

/// One in-flight OCR recognition with everything the status card needs.
struct OcrJob {
    /// Page the recognized text must land on.
    target: String,
    /// Attachment file name, shown in the status card.
    file: String,
    started: Instant,
    /// Flips to true when the engine answered `/v1/models`.
    engine_ready: bool,
    rx: std::sync::mpsc::Receiver<ai::ocr::OcrEvent>,
}

/// The result card shown after a recognition finishes; stays until dismissed.
struct OcrResult {
    file: String,
    text: String,
    /// Localized note about where the text went (page block / clipboard).
    destination: String,
    #[allow(dead_code)]
    at: Instant,
    copied: bool,
}

/// Localized stopwatch for the OCR status card: "42 с" / "2 мин 05 с".
fn fmt_elapsed(elapsed: Duration, locale: Locale) -> String {
    let total = elapsed.as_secs();
    match locale {
        Locale::Ru => {
            if total < 60 {
                format!("{total} с")
            } else {
                format!("{} мин {:02} с", total / 60, total % 60)
            }
        }
        Locale::En => {
            if total < 60 {
                format!("{total} s")
            } else {
                format!("{} m {:02} s", total / 60, total % 60)
            }
        }
    }
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
            confirm_block: None,
            show_appearance: false,
            show_onboarding: false,
            onboarding_step: 0,
            show_mcp_modal: false,
            mcp_tab: 0,
            show_imap_modal: false,
            imap_config,
            imap_status: sync_imap::SyncStatus::Idle,
            imap_job: None,
            imap_last_sync: None,
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
            components: components::Manager::load(),
            show_integrations: false,
            viewer: None,
            previews: preview::PreviewCache::default(),
            ocr: None,
            ocr_model_dir: None,
            ocr_job: None,
            ocr_result: None,
            ocr_highlight: None,
            stt: ai::stt::SttJobs::default(),
            tts_rx: None,
        }
        .with_links();
        app.show_faq_modal = matches!(launch.view, Some(View::Faq));
        app.show_integrations = launch.modal.as_deref() == Some("integrations");
        if launch.onboarding {
            app.show_onboarding = true;
        }
        // First-run auto-open: never inside tests (the overlay would eat
        // synthetic clicks) and never in --snapshot captures.
        #[cfg(not(test))]
        if !launch.onboarding && app.snapshot_job.is_none() {
            if let Some(dir) = i18n::settings_dir() {
                app.show_onboarding = !modals::onboarding::is_done(&dir);
            }
        }
        app.watcher = watch::VaultWatch::start(&app.vault.dir);
        app
    }

    fn with_links(mut self) -> Self {
        self.refresh_links();
        self
    }

    // -----------------------------------------------------------------------
    // Integrations: viewer, OCR, STT, TTS
    // -----------------------------------------------------------------------

    fn open_viewer(&mut self, name: &str, mime: &str, bytes: Vec<u8>) {
        if mime == "application/pdf" {
            match self
                .components
                .lib_path(components::ComponentId::PdfEngine, "pdfium")
            {
                Some(path) => {
                    self.viewer = Some(viewer::ViewerState::new_pdf(name, bytes, &path));
                }
                // Not just a toast: open the catalog where the fix lives.
                None => {
                    self.toast(t(self.locale, Key::PdfEngineMissing));
                    self.show_integrations = true;
                }
            }
        } else {
            self.viewer = Some(viewer::ViewerState::new_image(name, &bytes));
        }
    }

    fn start_ocr(&mut self, file: &str, image: Vec<u8>, ctx: &egui::Context) {
        if let Some(job) = &self.ocr_job {
            // One engine, one request: a second click must read as "already
            // working", not as a broken button that silently resets.
            self.toast(format!(
                "{} ({})",
                t(self.locale, Key::OcrAlreadyRunning),
                job.file
            ));
            return;
        }
        let Some((engine_bin, model_dir)) =
            ai::ocr::resolve_paths(&self.components, self.ui_state.custom_models.ocr.as_deref())
        else {
            // Point the user straight at the download, not just at a notice.
            self.toast(t(self.locale, Key::NeedComponentsToast));
            self.show_integrations = true;
            return;
        };
        let Some(target) = self
            .selected
            .and_then(|index| self.vault.capsules.get(index))
            .map(|capsule| capsule.message_id.clone())
        else {
            return;
        };
        let mime = ai::ocr::sniff_image_mime(&image).to_string();
        // A different model folder means a different engine process: drop the
        // old actor (it kills its child on Drop) and spawn with the new path.
        if self.ocr_model_dir.as_deref() != Some(model_dir.as_path()) {
            self.ocr = None;
            self.ocr_model_dir = Some(model_dir.clone());
        }
        let actor = self.ocr.get_or_insert_with(|| {
            ai::ocr::OcrActor::start(engine_bin, model_dir, ai::ocr::DEFAULT_PORT)
        });
        let Some(rx) = actor.recognize(image, mime, t(self.locale, Key::OcrPrompt).to_string())
        else {
            return;
        };
        self.ocr_job = Some(OcrJob {
            target,
            file: file.to_string(),
            started: Instant::now(),
            engine_ready: false,
            rx,
        });
        self.toast(t(self.locale, Key::OcrRunning));
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }

    fn transcribe_audio(&mut self, cid: &str) {
        if self.stt.is_running(cid) {
            return;
        }
        let Some(model_dir) = ai::stt::best_model_dir(
            &self.components,
            self.ui_state.custom_models.stt.as_deref(),
        ) else {
            self.toast(t(self.locale, Key::NeedComponentsToast));
            self.show_integrations = true;
            return;
        };
        let Some(bytes) = self.audio_attachment_bytes(cid) else {
            return;
        };
        if self
            .stt
            .start(
                model_dir,
                cid,
                bytes,
                ai::whisper::Language::from_locale(self.locale),
            )
            .is_ok()
        {
            self.toast(t(self.locale, Key::Transcribing));
        }
    }

    fn audio_attachment_bytes(&self, cid: &str) -> Option<Vec<u8>> {
        use base64::Engine as _;
        self.selected
            .and_then(|index| self.vault.capsules.get(index))
            .and_then(|capsule| {
                capsule
                    .attachments
                    .iter()
                    .find(|attachment| attachment.cid == cid)
            })
            .and_then(|attachment| attachment.raw_b64.as_deref())
            .and_then(|raw| base64::engine::general_purpose::STANDARD.decode(raw).ok())
            .filter(|bytes| !bytes.is_empty())
    }

    fn speak_selected_page(&mut self) {
        if self.tts_rx.is_some() {
            return;
        }
        // Requirements first (runtime library + at least one voice, catalog or
        // user-supplied): "нет компонентов" must be answered with the download
        // modal, not with the later "empty page" toast.
        let Some(lib) = self
            .components
            .lib_path(components::ComponentId::TtsRuntime, "sherpa-onnx-c-api")
        else {
            self.toast(t(self.locale, Key::NeedComponentsToast));
            self.show_integrations = true;
            return;
        };
        let custom_voice = ai::custom::resolve(
            ui_state::CustomModelKind::Tts,
            self.ui_state.custom_models.tts.as_deref(),
        );
        let catalog_voice = [
            components::ComponentId::TtsVoiceIrina,
            components::ComponentId::TtsVoiceDmitri,
            components::ComponentId::TtsVoiceAmy,
        ]
        .into_iter()
        .any(|id| self.components.ready(id));
        if custom_voice.is_none() && !catalog_voice {
            self.toast(t(self.locale, Key::NeedComponentsToast));
            self.show_integrations = true;
            return;
        }
        let Some(index) = self.selected else { return };
        let text = links::page_text(&self.vault.capsules[index]);
        if text.trim().is_empty() {
            self.toast(t(self.locale, Key::TtsEmptyToast));
            return;
        }
        // v2.1.0: the voice follows the language of the page — a Cyrillic page
        // gets a Russian voice, a Latin one the English voice. A user-supplied
        // voice folder wins over the catalog, so "подключить свои модели"
        // never requires a download first.
        let voice_dir = match custom_voice {
            Some(dir) => dir,
            None => {
                let candidates: [components::ComponentId; 3] = if ai::tts::looks_russian(&text) {
                    [
                        components::ComponentId::TtsVoiceIrina,
                        components::ComponentId::TtsVoiceDmitri,
                        components::ComponentId::TtsVoiceAmy,
                    ]
                } else {
                    [
                        components::ComponentId::TtsVoiceAmy,
                        components::ComponentId::TtsVoiceIrina,
                        components::ComponentId::TtsVoiceDmitri,
                    ]
                };
                let Some(voice) = candidates.into_iter().find(|id| self.components.ready(*id)) else {
                    self.toast(t(self.locale, Key::NeedComponentsToast));
                    self.show_integrations = true;
                    return;
                };
                self.components.dir(voice)
            }
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.tts_rx = Some(rx);
        std::thread::spawn(move || {
            let result = ai::tts::synthesize(&lib, &voice_dir, &text, 1.0);
            let _ = tx.send(result);
        });
    }

    /// Land the recognized text where the user can see it: appended to a
    /// native page (scrolled to + stroked) or copied from a read-only email
    /// page — and always in the persistent result card. This is the answer to
    /// "куда оно добавится?" asked in words the user reads, not in toasts
    /// that vanish in two seconds.
    fn apply_ocr_text(&mut self, job: OcrJob, text: String, ctx: &egui::Context) {
        let Some(index) = self
            .vault
            .capsules
            .iter()
            .position(|capsule| capsule.message_id == job.target)
        else {
            return;
        };
        let capsule = &mut self.vault.capsules[index];
        let (destination, block_id) = if capsule.native {
            let block = json!({
                "id": mime::new_block_id("paragraph"),
                "type": "paragraph",
                "data": { "text": text }
            });
            let id = block["id"].as_str().unwrap_or_default().to_string();
            capsule.blocks.push(block);
            self.record_change();
            (t(self.locale, Key::OcrAddedToEnd).to_string(), Some(id))
        } else {
            // Imported .eml pages never take blocks — the file is rendered
            // read-only — so the text goes to the clipboard, and the card
            // says exactly that instead of a generic "added to the page".
            ctx.copy_text(text.clone());
            (t(self.locale, Key::OcrCopiedReadonly).to_string(), None)
        };
        self.toast(destination.clone());
        self.ocr_result = Some(OcrResult {
            file: job.file,
            text,
            destination,
            at: Instant::now(),
            copied: false,
        });
        if let Some(id) = block_id {
            self.ocr_highlight = Some((id, Instant::now()));
        }
    }

    /// Persistent AI overlays, bottom-right: the running OCR job card and the
    /// recognition result card. Toasts vanish in seconds; a cold start of a
    /// 2 GB model takes minutes — this card is the honest signal that the
    /// engine is actually working (elapsed timer keeps moving).
    fn draw_ai_overlays(&mut self, ctx: &egui::Context) {
        let mut close_result = false;
        egui::Area::new(egui::Id::new("ai-status"))
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
            .show(ctx, |ui| {
                if let Some(job) = &self.ocr_job {
                    let phase = if job.engine_ready {
                        t(self.locale, Key::OcrPhaseReady)
                    } else {
                        t(self.locale, Key::OcrPhaseStarting)
                    };
                    egui::Frame::new()
                        .fill(theme::panel())
                        .stroke(egui::Stroke::new(1.0, theme::accent()))
                        .corner_radius(theme::RADIUS)
                        .inner_margin(Margin::same(12))
                        .show(ui, |ui| {
                            ui.set_width(372.0);
                            ui.horizontal(|ui| {
                                ui.add(egui::Spinner::new().size(14.0));
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new("OCR")
                                        .family(theme::mono_bold())
                                        .size(10.5)
                                        .color(theme::accent()),
                                );
                                ui.label(
                                    egui::RichText::new(truncate_chars(&job.file, 26))
                                        .family(theme::medium())
                                        .size(12.0)
                                        .color(theme::ink()),
                                );
                            });
                            ui.add_space(7.0);
                            ui.label(egui::RichText::new(phase).size(11.5).color(theme::ink()));
                            if !job.engine_ready {
                                ui.add_space(2.0);
                                ui.label(
                                    egui::RichText::new(t(self.locale, Key::OcrFirstRunHint))
                                        .size(10.5)
                                        .color(theme::faint()),
                                );
                            }
                            ui.add_space(5.0);
                            ui.label(
                                egui::RichText::new(fmt_elapsed(
                                    job.started.elapsed(),
                                    self.locale,
                                ))
                                .family(theme::mono())
                                .size(10.5)
                                .color(theme::faint()),
                            );
                        });
                    ui.add_space(8.0);
                }
                if let Some(result) = self.ocr_result.as_mut() {
                    egui::Frame::new()
                        .fill(theme::panel())
                        .stroke(egui::Stroke::new(1.0, theme::line_strong()))
                        .corner_radius(theme::RADIUS)
                        .inner_margin(Margin::same(12))
                        .show(ui, |ui| {
                            ui.set_width(372.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(t(self.locale, Key::OcrResultTitle))
                                        .family(theme::medium())
                                        .size(12.0)
                                        .color(theme::ink()),
                                );
                                ui.label(
                                    egui::RichText::new(truncate_chars(&result.file, 20))
                                        .size(11.0)
                                        .color(theme::faint()),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if secondary_button(ui, Some(Icon::Close), "", 26.0)
                                            .clicked()
                                        {
                                            close_result = true;
                                        }
                                    },
                                );
                            });
                            ui.add_space(5.0);
                            ui.label(
                                egui::RichText::new(&result.destination)
                                    .size(11.0)
                                    .color(theme::accent()),
                            );
                            ui.add_space(7.0);
                            let max_h = (ctx.content_rect().height() * 0.35).max(90.0);
                            egui::ScrollArea::vertical()
                                .max_height(max_h)
                                .id_salt("ocr-result-text")
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&result.text)
                                                .size(11.5)
                                                .color(theme::ink()),
                                        )
                                        .wrap()
                                        .selectable(true),
                                    );
                                });
                            ui.add_space(9.0);
                            let copy_label = if result.copied {
                                t(self.locale, Key::OcrCopied)
                            } else {
                                t(self.locale, Key::OcrCopy)
                            };
                            if secondary_button(ui, None, copy_label, 26.0).clicked() {
                                ctx.copy_text(result.text.clone());
                                result.copied = true;
                            }
                        });
                }
            });
        if close_result {
            self.ocr_result = None;
        }
    }

    /// Drain finished AI jobs and apply their results to the vault/UI.
    fn poll_ai_jobs(&mut self, ctx: &egui::Context) {
        // STT: write transcripts back into the matching audio block.
        let mut finished: Vec<(String, ai::stt::SttPhase)> = Vec::new();
        for index in 0..self.vault.capsules.len() {
            let Some(capsule) = self.vault.capsules.get(index) else {
                continue;
            };
            for attachment in &capsule.attachments {
                if let Some(phase) = self.stt.take_finished(&attachment.cid) {
                    finished.push((attachment.cid.clone(), phase));
                }
            }
        }
        for (cid, phase) in finished {
            match phase {
                ai::stt::SttPhase::Done(text) => {
                    let mut applied = false;
                    for capsule in &mut self.vault.capsules {
                        for block in &mut capsule.blocks {
                            let block_cid = block
                                .get("data")
                                .and_then(|data| data.get("audioCid"))
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("")
                                .trim_start_matches("cid:");
                            if block_cid == cid {
                                block["data"]["transcript"] = serde_json::json!(text);
                                applied = true;
                            }
                        }
                    }
                    if applied {
                        self.record_change();
                        self.toast(t(self.locale, Key::SttDoneToast));
                    }
                }
                ai::stt::SttPhase::Failed(message) => {
                    self.toast(format!(
                        "{}: {message}",
                        t(self.locale, Key::SttFailedToast)
                    ));
                }
                ai::stt::SttPhase::Running => {}
            }
        }

        // IMAP sync: drain the background thread and keep auto_sync ticking.
        self.poll_imap_sync();
        self.maybe_autosync();
        if self.imap_job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }

        // OCR: stream engine progress into the status card, then land the
        // text where the user can actually see it (page block + highlight, or
        // clipboard for read-only email pages).
        let mut finished_ocr: Option<ai::ocr::OcrEvent> = None;
        if let Some(job) = self.ocr_job.as_mut() {
            while let Ok(event) = job.rx.try_recv() {
                match event {
                    ai::ocr::OcrEvent::Starting => job.engine_ready = false,
                    ai::ocr::OcrEvent::Ready => job.engine_ready = true,
                    ai::ocr::OcrEvent::Done(_) | ai::ocr::OcrEvent::Failed(_) => {
                        finished_ocr = Some(event);
                        break;
                    }
                }
            }
        }
        if let Some(event) = finished_ocr {
            let job = self.ocr_job.take().expect("ocr job present");
            match event {
                ai::ocr::OcrEvent::Done(text) => self.apply_ocr_text(job, text, ctx),
                ai::ocr::OcrEvent::Failed(message) => {
                    self.toast(format!(
                        "{}: {message}",
                        t(self.locale, Key::OcrFailedToast)
                    ));
                }
                _ => {}
            }
        }

        // TTS: play the generated WAV through the existing player.
        if let Some(rx) = &self.tts_rx {
            if let Ok(result) = rx.try_recv() {
                self.tts_rx = None;
                match result {
                    Ok(wav) => match audio_player::play("tts-preview", &wav) {
                        Ok(()) => self.toast(t(self.locale, Key::TtsDoneToast)),
                        Err(_) => self.toast(t(self.locale, Key::TtsFailedToast)),
                    },
                    Err(message) => {
                        self.toast(format!(
                            "{}: {message}",
                            t(self.locale, Key::TtsFailedToast)
                        ));
                    }
                }
            }
        }
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
            Stroke::new(1.5, theme::accent()),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            t(self.locale, Key::DropHint),
            FontId::new(17.0, theme::display()),
            theme::ink(),
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

    fn switch_to_vault(&mut self, dir: PathBuf) {
        let dir = vault::resolve_vault_path(dir);
        vault::set_active_vault_dir(&dir);
        self.vault = Vault::load(dir);
        self.selected = self.vault.selected_root();
        self.collapsed.clear();

        // For project vaults: keep root open, collapse all child subfolders by default
        if vault::is_project_vault(&self.vault.dir) {
            let tree = vault::PageTree::build(&self.vault.capsules);
            for idx in 0..self.vault.capsules.len() {
                if !tree.children_of(Some(idx)).is_empty() && !tree.roots().contains(&idx) {
                    self.collapsed.insert(self.vault.capsules[idx].message_id.clone());
                }
            }
        }

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

    fn open_vault_dialog(&mut self) {
        if !self.save_selected() {
            return;
        }
        if let Some(dir) = rfd::FileDialog::new()
            .set_title("Choose a CapsuleCraft .eml vault folder")
            .set_directory(&self.vault.dir)
            .pick_folder()
        {
            self.switch_to_vault(dir);
        }
    }

    fn open_project_dialog(&mut self) {
        if !self.save_selected() {
            return;
        }
        let default_dir = vault::get_active_vault_dir();
        if let Some(folder) = rfd::FileDialog::new()
            .set_title(t(self.locale, Key::OpenProjectAsVault))
            .set_directory(&default_dir)
            .pick_folder()
        {
            let target = folder.join(".capsule");
            match project_parser::ingest_project(&folder, &target) {
                Ok(summary) => {
                    self.switch_to_vault(target);
                    self.toast(format!(
                        "Project {} synchronized: {} modules ({} updated, {} removed)",
                        summary.project_name, summary.modules_count, summary.capsules_updated, summary.capsules_removed
                    ));
                }
                Err(err) => {
                    self.toast(format!("Error indexing project: {err}"));
                }
            }
        }
    }

    fn ingest_current_project(&mut self) {
        let root = self.vault.dir.parent().unwrap_or(&self.vault.dir);
        let target = if vault::is_project_vault(&self.vault.dir) {
            self.vault.dir.clone()
        } else {
            root.join(".capsule")
        };
        match project_parser::ingest_project(root, &target) {
            Ok(summary) => {
                self.switch_to_vault(target);
                self.toast(format!(
                    "Project {} synchronized: {} modules ({} updated, {} removed)",
                    summary.project_name, summary.modules_count, summary.capsules_updated, summary.capsules_removed
                ));
            }
            Err(err) => {
                self.toast(format!("Error syncing project: {err}"));
            }
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
        // Straight rm is a one-way door: send the page file to the OS trash
        // first (recoverable), and only fall back to a hard delete when the
        // trash is unavailable (e.g. network mounts).
        let trashed = match trash::delete(&path) {
            Ok(()) => true,
            Err(err) => {
                eprintln!("trash::delete failed, falling back to rm: {err}");
                std::fs::remove_file(&path).is_ok()
            }
        };
        if !trashed {
            self.status = "delete failed".to_string();
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
        if trashed {
            self.toast(t(self.locale, Key::PageTrashed));
        } else {
            self.toast(t(self.locale, Key::PageDeleted));
        }
    }

    /// Confirmed block removal (the modal has already asked).
    fn delete_block_now(&mut self, block_index: usize) {
        let Some(index) = self.selected else {
            return;
        };
        let Some(capsule) = self.vault.capsules.get_mut(index) else {
            return;
        };
        if block_index < capsule.blocks.len() {
            capsule.blocks.remove(block_index);
            self.dirty = true;
            self.last_write = Instant::now();
            self.toast(t(self.locale, Key::BlockDeleted));
        }
        self.confirm_block = None;
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
    pub(crate) open_integrations: bool,
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
    pub(crate) ingest_project: bool,
    pub(crate) open_project_dialog: bool,
    pub(crate) sync_project: bool,
    pub(crate) switch_vault: Option<PathBuf>,
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
        // The v1.12.0 branch added the field and the button but never wired it
        // through absorb — the ⚙ chip set `action.open_integrations`, the value
        // was dropped here, and the modal never opened ("nothing happens").
        self.open_integrations |= other.open_integrations;
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
        self.ingest_project |= other.ingest_project;
        self.open_project_dialog |= other.open_project_dialog;
        self.sync_project |= other.sync_project;
        self.switch_vault = other.switch_vault.or_else(|| self.switch_vault.take());
    }
}

impl eframe::App for NativeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame_ui(ui);
    }
}

impl NativeApp {
    /// The whole UI frame, split out of `eframe::App::ui` so headless tests
    /// can drive the real app with synthetic input (no window required).
    fn frame_ui(&mut self, ui: &mut egui::Ui) {
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
            || self.confirm_block.is_some()
            || self.show_appearance
            || self.show_onboarding
            || self.show_mcp_modal
            || self.show_imap_modal
            || self.show_cothink_modal
            || self.show_faq_modal
            || self.show_integrations
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
                        .fill(theme::chrome())
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
                        .fill(theme::chrome())
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
                        .fill(theme::chrome())
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
                    .fill(theme::chrome())
                    .inner_margin(Margin::same(8)),
            )
            .show(ui, |ui| {
                Frame::new()
                    .fill(theme::bg())
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
                                    &mut self.previews,
                                    self.components
                                        .lib_path(components::ComponentId::PdfEngine, "pdfium")
                                        .as_deref(),
                                    self.ocr_highlight.as_ref().map(|(id, _)| id.as_str()),
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
        for attachment_action in editor.attachments {
            match attachment_action {
                views::document_view::AttachmentAction::View { name, mime, bytes } => {
                    self.open_viewer(&name, &mime, bytes);
                }
                views::document_view::AttachmentAction::Ocr { name, bytes } => {
                    self.start_ocr(&name, bytes, &ctx);
                }
            }
        }
        if editor.speak {
            self.speak_selected_page();
        }
        if let Some(cid) = editor.transcribe {
            self.transcribe_audio(&cid);
        }
        self.poll_ai_jobs(&ctx);
        // Attachment viewer overlay: may ask to OCR the open image.
        let viewer_outcome =
            viewer::draw(&mut self.viewer, self.locale, &ctx, self.ocr_job.is_some());
        if let Some((name, bytes)) = viewer_outcome.ocr {
            self.start_ocr(&name, bytes, &ctx);
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
        if let Some(block_index) = editor.delete_block {
            self.confirm_block = Some(block_index);
        }
        if shell.connect_mcp {
            self.show_mcp_modal = true;
        }
        if shell.open_integrations {
            self.show_integrations = true;
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
        if shell.open_project_dialog {
            self.open_project_dialog();
        }
        if shell.sync_project || shell.ingest_project {
            self.ingest_current_project();
        }
        if let Some(path) = shell.switch_vault {
            self.switch_to_vault(path);
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

        let (del_title, del_body) = if self.confirm_block.is_some() {
            (
                t(self.locale, Key::DeleteBlock).to_string(),
                t(self.locale, Key::DeleteBlockConfirm).to_string(),
            )
        } else {
            (
                t(self.locale, Key::DeletePage).to_string(),
                t(self.locale, Key::DeletePageConfirm).to_string(),
            )
        };
        match modals::draw_confirm_delete(
            self.confirm_delete || self.confirm_block.is_some(),
            del_title,
            del_body,
            self.locale,
            &ctx,
        ) {
            modals::ConfirmDeleteAction::Confirm => {
                if let Some(block_index) = self.confirm_block.take() {
                    self.delete_block_now(block_index);
                } else {
                    self.delete_selected_page();
                }
                self.confirm_delete = false;
            }
            modals::ConfirmDeleteAction::Cancel => {
                self.confirm_delete = false;
                self.confirm_block = None;
            }
            modals::ConfirmDeleteAction::None => {}
        }

        match modals::draw_appearance(self.show_appearance, self.locale, &ctx) {
            modals::AppearanceAction::Close => self.show_appearance = false,
            modals::AppearanceAction::None => {}
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

        match modals::integrations::draw_integrations_modal(
            self.show_integrations,
            &self.components,
            &mut self.ui_state.custom_models,
            self.locale,
            &ctx,
        ) {
            modals::integrations::IntegrationsAction::Close => self.show_integrations = false,
            modals::integrations::IntegrationsAction::Changed => self.ui_state.save(),
            modals::integrations::IntegrationsAction::None => {}
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

        match modals::draw_onboarding(
            self.show_onboarding,
            self.onboarding_step,
            self.locale,
            &ctx,
        ) {
            modals::OnboardingAction::Next => {
                self.onboarding_step = (self.onboarding_step + 1).min(modals::STEP_COUNT - 1);
            }
            modals::OnboardingAction::Prev => {
                self.onboarding_step = self.onboarding_step.saturating_sub(1);
            }
            modals::OnboardingAction::Complete => {
                self.show_onboarding = false;
                self.onboarding_step = 0;
                if let Some(dir) = i18n::settings_dir() {
                    modals::onboarding::mark_done(&dir);
                }
            }
            modals::OnboardingAction::Try(try_action) => {
                // Close the tour so the feature is front and centre; the
                // tour resumes from the palette with its step preserved.
                self.show_onboarding = false;
                match try_action {
                    modals::TryAction::Palette => self.palette.open = true,
                    modals::TryAction::Appearance => self.show_appearance = true,
                    modals::TryAction::Graph => self.view = View::Graph,
                    modals::TryAction::Planning => self.view = View::Planning,
                    modals::TryAction::Integrations => self.show_integrations = true,
                    modals::TryAction::Mcp => self.show_mcp_modal = true,
                    modals::TryAction::Faq => self.show_faq_modal = true,
                }
            }
            modals::OnboardingAction::None => {}
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

        // The OCR highlight expires on its own; the status/result cards live
        // in the bottom-right corner until the job finishes / user dismisses.
        if let Some((_, since)) = &self.ocr_highlight {
            if since.elapsed() > Duration::from_secs(4) {
                self.ocr_highlight = None;
            }
        }
        self.draw_ai_overlays(&ctx);

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
            PaletteCommand::ToggleTheme => {
                theme::toggle();
                theme::persist();
                theme::apply(ctx);
            }
            PaletteCommand::Appearance => self.show_appearance = true,
            PaletteCommand::Onboarding => {
                self.onboarding_step = 0;
                self.show_onboarding = true;
            }
            PaletteCommand::SetView(view) => self.view = view,
            PaletteCommand::OpenPage(index) => {
                self.select_page(index);
                self.view = View::Page;
            }
            PaletteCommand::DeletePage => self.confirm_delete = true,
            PaletteCommand::ConnectMcp => self.show_mcp_modal = true,
            PaletteCommand::Integrations => self.show_integrations = true,
            PaletteCommand::SpeakPage => self.speak_selected_page(),
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
            PaletteCommand::IndexProject => self.ingest_current_project(),
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

    /// Runs IMAP sync in the background: the network round-trip (TLS connect,
    /// folder scan, body fetches) takes seconds to minutes, and v2.3 ran it
    /// on the UI thread — the whole interface froze mid-sync. The result
    /// lands through a channel polled in [`Self::poll_imap_sync`].
    fn trigger_imap_sync(&mut self) {
        if !self.imap_config.enabled {
            self.show_imap_modal = true;
            return;
        }
        if self.imap_job.is_some() {
            self.toast(t(self.locale, Key::ImapSyncing));
            return;
        }

        self.imap_status = sync_imap::SyncStatus::Syncing;
        self.imap_last_sync = Some(Instant::now());
        let engine =
            sync_imap::ImapSyncEngine::new(self.imap_config.clone(), self.vault.dir.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("imap-sync".into())
            .spawn(move || {
                let _ = tx.send(engine.run_sync());
            })
            .map_err(|e| format!("spawn sync: {e}"))
            .ok();
        self.imap_job = Some(rx);
    }

    /// Drains the background sync channel and lands the outcome on the UI
    /// thread: status pill, toast, vault reload.
    fn poll_imap_sync(&mut self) {
        let Some(rx) = &self.imap_job else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("sync thread died".into()),
        };
        self.imap_job = None;
        match result {
            Ok(msg) => {
                self.imap_status = sync_imap::SyncStatus::Success(msg.clone());
                self.toast(msg);
                self.reload();
            }
            Err(err) => {
                self.imap_status = sync_imap::SyncStatus::Error(err.clone());
                self.toast(format!("{}: {err}", t(self.locale, Key::ImapErrorToast)));
            }
        }
    }

    /// `auto_sync`: sync once shortly after startup, then every 15 minutes
    /// while the setting stays on. Manual "Sync Now" ignores this entirely.
    fn maybe_autosync(&mut self) {
        if !self.imap_config.enabled || !self.imap_config.auto_sync {
            return;
        }
        if self.imap_job.is_some() {
            return;
        }
        let interval = Duration::from_secs(15 * 60);
        if self
            .imap_last_sync
            .map(|t| t.elapsed() < interval)
            .unwrap_or(false)
        {
            return;
        }
        self.trigger_imap_sync();
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

#[cfg(test)]
mod app_tests {
    use super::*;

    fn test_app() -> (NativeApp, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "cc_ui_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        (
            NativeApp::new(cli::Launch {
                vault: Some(dir.clone()),
                ..Default::default()
            }),
            dir,
        )
    }

    /// One full app frame driven headlessly, exactly like eframe does it
    /// (`Context::run_ui` → `App::ui`).
    fn run_frame(ctx: &egui::Context, app: &mut NativeApp, raw: egui::RawInput) {
        let mut out = ctx.run_ui(raw, |ui| app.frame_ui(ui));
        // Headless: drop texture updates instead of painting them.
        out.textures_delta.clear();
    }

    /// Move, press and release the primary button at `pos`.
    fn click(ctx: &egui::Context, app: &mut NativeApp, raw: &mut egui::RawInput, pos: egui::Pos2) {
        raw.events.push(egui::Event::PointerMoved(pos));
        run_frame(ctx, app, raw.clone());
        raw.events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        run_frame(ctx, app, raw.clone());
        raw.events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        run_frame(ctx, app, raw.clone());
        raw.events.clear();
    }

    fn close_overlays(app: &mut NativeApp) {
        app.palette.open = false;
        app.confirm_delete = false;
        app.show_mcp_modal = false;
        app.show_imap_modal = false;
        app.show_cothink_modal = false;
        app.show_faq_modal = false;
        app.show_integrations = false;
    }

    fn tiny_test_png() -> Vec<u8> {
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 3, 2);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[255, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 9, 9, 9, 1, 2, 3])
                .unwrap();
        }
        png
    }

    /// v1.12.3: OCR with nothing installed must not dead-end in a toast — it
    /// opens the Integrations modal where the fix (a download) lives.
    #[test]
    fn missing_ocr_opens_integrations() {
        let (mut app, dir) = test_app();
        let ctx = egui::Context::default();
        if app
            .components
            .ready(crate::components::ComponentId::OcrEngine)
            && app
                .components
                .ready(crate::components::ComponentId::OcrModel)
        {
            // Real components installed on this machine — the branch is
            // unreachable here; skip rather than lie.
            let _ = std::fs::remove_dir_all(dir);
            return;
        }
        app.start_ocr("scan.png", vec![0xFF; 64], &ctx);
        assert!(
            app.show_integrations,
            "OCR without components must open Integrations"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// v1.12.3: the same contract for TTS ("Озвучить страницу").
    #[test]
    fn missing_tts_opens_integrations() {
        let (mut app, dir) = test_app();
        if app
            .components
            .ready(crate::components::ComponentId::TtsVoiceIrina)
            && app
                .components
                .ready(crate::components::ComponentId::TtsRuntime)
        {
            let _ = std::fs::remove_dir_all(dir);
            return;
        }
        app.speak_selected_page();
        assert!(
            app.show_integrations,
            "TTS without components must open Integrations"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// v1.12.3: opening a PDF without the pdfium component opens Integrations.
    #[test]
    fn missing_pdf_engine_opens_integrations() {
        let (mut app, dir) = test_app();
        if app
            .components
            .ready(crate::components::ComponentId::PdfEngine)
        {
            let _ = std::fs::remove_dir_all(dir);
            return;
        }
        app.open_viewer("doc.pdf", "application/pdf", b"%PDF-".to_vec());
        assert!(app.viewer.is_none(), "no engine - no overlay");
        assert!(app.show_integrations);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// v1.12.3: an imported .eml page shows its image attachments inline —
    /// the draw pass must populate the preview cache for the PNG.
    #[test]
    fn imported_page_previews_attachments() {
        use base64::Engine as _;
        let ctx = egui::Context::default();
        let mut capsule = Capsule::blank("preview-test".into(), "x.eml".into(), "Scan".into());
        capsule.native = false;
        let png = tiny_test_png();
        capsule.attachments.push(crate::mime::Attachment {
            name: "scan.png".into(),
            cid: "scan@capsuleos".into(),
            mime_type: "image/png".into(),
            content: String::new(),
            size_bytes: png.len(),
            raw_b64: Some(base64::engine::general_purpose::STANDARD.encode(&png)),
        });
        let mut previews = crate::preview::PreviewCache::default();
        let key = crate::preview::preview_key(&capsule.message_id, &capsule.attachments[0]);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            ..Default::default()
        };
        let mut frame = ctx.run_ui(raw, |ui| {
            let action = draw_document(
                ui,
                &mut capsule,
                &[],
                Locale::En,
                &LinkContext::default(),
                None,
                false,
                &mut previews,
                None,
                None,
            );
            assert!(action.attachments.is_empty());
        });
        // Headless: the preview texture upload has nowhere to go.
        frame.textures_delta.clear();
        assert!(
            previews.get(&key).is_some(),
            "PNG attachment must get an inline preview"
        );
    }

    /// Regression for "clicking Integrations does nothing at all": sweeping
    /// the title bar with a synthetic mouse must find the ⚙ chip and open the
    /// integrations modal. Runs the REAL `frame_ui` end to end.
    #[test]
    fn titlebar_chip_click_opens_integrations() {
        let (mut app, dir) = test_app();
        let ctx = egui::Context::default();
        let width = 1280.0;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 800.0));
        let mut raw = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        for _ in 0..4 {
            run_frame(&ctx, &mut app, raw.clone());
        }
        assert!(!app.show_integrations);

        // Title bar right block (right-to-left): [caption buttons][6][Info][⌘]
        // [⚙ Integrations][MCP]… — sweep past the captions until something opens
        // the integrations modal.
        let y = crate::chrome::TITLEBAR_HEIGHT / 2.0;
        let mut found_at = None;
        for offset in (140..=266).step_by(2) {
            close_overlays(&mut app);
            run_frame(&ctx, &mut app, raw.clone());
            let pos = egui::pos2(width - offset as f32, y);
            click(&ctx, &mut app, &mut raw, pos);
            if app.show_integrations {
                found_at = Some(pos);
                break;
            }
        }
        assert!(
            found_at.is_some(),
            "no click in the title bar opened the Integrations modal"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
