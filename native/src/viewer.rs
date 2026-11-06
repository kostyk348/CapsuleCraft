//! Full-screen attachment viewer: images and PDF pages, drawn natively.
//!
//! Images decode once via the `image` crate; PDF pages render on demand
//! through pdfium (the shared library arrives as a downloadable component —
//! `Pdfium::bind_to_library` loads it from the components directory).
//!
//! Interaction contract (v1.12.3): the overlay closes only through the ✕
//! button or `Escape` — clicks inside the page never close it — the content
//! scrolls in both axes, pages flip with the arrows / ← → keys, zoom works
//! with the toolbar or +/− keys, and an image can be sent straight to OCR.

use std::path::Path;
use std::sync::Arc;

use egui::{vec2, Align, Color32, Frame, Label, Layout, Margin, Rect, RichText, ScrollArea, Sense};

use crate::i18n::{t, Key, Locale};
use crate::icons::{self, Icon};
use crate::theme;
use crate::{hud, secondary_button};

/// An open viewer overlay; `None` in the shell means closed.
pub struct ViewerState {
    pub name: String,
    /// The exact attachment bytes for images — kept so the viewer can offer
    /// "send to OCR" without going back through the page.
    raw_image: Option<Vec<u8>>,
    image: Option<Arc<egui::ColorImage>>,
    pdf: Option<PdfDoc>,
    pub page: usize,
    pub zoom: f32,
    /// Cached texture keyed by (page, zoom, width bucket) for PDFs.
    texture: Option<(u64, egui::TextureHandle)>,
    pub error: Option<String>,
    /// Requested render width from the last frame.
    last_width: f32,
}

struct PdfDoc {
    document: pdfium_render::prelude::PdfDocument<'static>,
}

/// What the overlay asked the shell to do this frame.
#[derive(Default)]
pub struct ViewerOutcome {
    /// Send the open image to the OCR engine: `(file name, bytes)`.
    pub ocr: Option<(String, Vec<u8>)>,
}

impl ViewerState {
    pub fn new_image(name: &str, bytes: &[u8]) -> Self {
        let (image, error) = match image::load_from_memory(bytes) {
            Ok(decoded) => {
                let rgba = decoded.to_rgba8();
                let (width, height) = rgba.dimensions();
                (
                    Some(Arc::new(egui::ColorImage::from_rgba_unmultiplied(
                        [width as usize, height as usize],
                        rgba.as_raw(),
                    ))),
                    None,
                )
            }
            Err(error) => (None, Some(error.to_string())),
        };
        Self {
            name: name.to_string(),
            raw_image: Some(bytes.to_vec()),
            image,
            pdf: None,
            page: 0,
            zoom: 1.0,
            texture: None,
            error,
            last_width: 0.0,
        }
    }

    pub fn new_pdf(name: &str, bytes: Vec<u8>, pdfium_lib: &Path) -> Self {
        match PdfDoc::open(pdfium_lib, bytes) {
            Ok(pdf) => Self {
                name: name.to_string(),
                raw_image: None,
                image: None,
                pdf: Some(pdf),
                page: 0,
                zoom: 1.0,
                texture: None,
                error: None,
                last_width: 0.0,
            },
            Err(error) => Self {
                name: name.to_string(),
                raw_image: None,
                image: None,
                pdf: None,
                page: 0,
                zoom: 1.0,
                texture: None,
                error: Some(error),
                last_width: 0.0,
            },
        }
    }

    pub fn page_count(&self) -> usize {
        self.pdf.as_ref().map(|pdf| pdf.page_count()).unwrap_or(1)
    }

    pub fn is_pdf(&self) -> bool {
        self.pdf.is_some()
    }

    /// Raster bytes of what the viewer is showing, for the OCR engine: the
    /// attachment itself for images, a fresh high-resolution render of the
    /// current page for PDFs (an OCR engine can only read pixels — the v2.1.0
    /// request was that a scanned page be readable straight from the viewer).
    pub fn ocr_payload(&self) -> Result<Vec<u8>, String> {
        if let Some(bytes) = &self.raw_image {
            return Ok(bytes.clone());
        }
        let pdf = self
            .pdf
            .as_ref()
            .ok_or_else(|| "nothing to recognize".to_string())?;
        // 2000 px keeps small print legible; the visible texture can be much
        // smaller at zoom 1.0 and OCR quality tracks resolution.
        let page = pdf.render_page(self.page, 2000.0)?;
        crate::snapshot::encode_png(&page)
    }
}

impl PdfDoc {
    fn open(lib_path: &Path, bytes: Vec<u8>) -> Result<Self, String> {
        // Binding is process-wide and idempotent (`crate::pdf`): the inline
        // preview and this viewer share one engine. Binding per document was
        // the v2.0.0 bug — the second bind failed with "already initialised".
        let pdfium = crate::pdf::engine(lib_path)?;
        let document = pdfium
            .load_pdf_from_byte_vec(bytes, None)
            .map_err(|e| format!("pdf open: {e}"))?;
        Ok(Self { document })
    }

    fn page_count(&self) -> usize {
        self.document.pages().len() as usize
    }

    fn render_page(&self, index: usize, width: f32) -> Result<egui::ColorImage, String> {
        use pdfium_render::prelude::*;
        let page = self
            .document
            .pages()
            .get(index as PdfPageIndex)
            .map_err(|e| format!("page {}: {e}", index + 1))?;
        let target_width = (width.max(320.0) as i32).min(4000);
        let config = PdfRenderConfig::new()
            .set_target_width(target_width)
            .set_maximum_height(8000);
        let dynamic = page
            .render_with_config(&config)
            .map_err(|e| format!("render: {e}"))?
            .as_image()
            .map_err(|e| format!("bitmap: {e}"))?;
        let rgba = dynamic.to_rgba8();
        let (w, h) = rgba.dimensions();
        Ok(egui::ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize],
            rgba.as_raw(),
        ))
    }
}

/// Open a PDF from raw bytes and render one page — used by the inline page
/// previews. Returns the rendered page and the total page count.
pub(crate) fn render_pdf_page(
    bytes: &[u8],
    lib_path: &Path,
    index: usize,
    width: f32,
) -> Result<(egui::ColorImage, usize), String> {
    let doc = PdfDoc::open(lib_path, bytes.to_vec())?;
    let pages = doc.page_count();
    let image = doc.render_page(index, width)?;
    Ok((image, pages))
}

/// Draw the overlay and handle closing. `Escape` and the ✕ button close;
/// clicks on the backdrop or the page no longer do. `ocr_busy` swaps the OCR
/// button for a live spinner — a second click must never read as a reset.
pub fn draw(
    viewer: &mut Option<ViewerState>,
    locale: Locale,
    ctx: &egui::Context,
    ocr_busy: bool,
) -> ViewerOutcome {
    let mut outcome = ViewerOutcome::default();
    // Take the state out so the Area closure can own it exclusively; it is
    // put back unless the overlay closed this frame.
    let mut state = match viewer.take() {
        Some(state) => state,
        None => return outcome,
    };
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return outcome;
    }

    // Keyboard: ← → flip pages, +/− zoom. Only while the overlay is open.
    let (arrow_next, arrow_prev, zoom_in_key, zoom_out_key) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowRight),
            i.key_pressed(egui::Key::ArrowLeft),
            i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals),
            i.key_pressed(egui::Key::Minus),
        )
    });
    if state.is_pdf() {
        if arrow_next && state.page + 1 < state.page_count() {
            state.page += 1;
        }
        if arrow_prev && state.page > 0 {
            state.page -= 1;
        }
    }
    if zoom_in_key {
        state.zoom = (state.zoom * 1.25).min(5.0);
    }
    if zoom_out_key {
        state.zoom = (state.zoom / 1.25).max(0.2);
    }

    let full = ctx.content_rect();
    let mut close = false;

    egui::Area::new(egui::Id::new("attachment-viewer"))
        .order(egui::Order::Foreground)
        .fixed_pos(full.min)
        .show(ctx, |ui| {
            // Dimmed backdrop. Deliberately NOT click-to-close: a click is how
            // the user drags/scrolls the document, losing it to a mis-click
            // was the old viewer's worst habit.
            let (rect, _) = ui.allocate_exact_size(full.size(), Sense::hover());
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_black_alpha(190));

            // Top toolbar.
            let bar = Rect::from_min_size(full.min + vec2(0.0, 0.0), vec2(full.width(), 46.0));
            ui.painter().rect_filled(bar, 0.0, theme::panel());
            ui.painter()
                .hline(bar.x_range(), bar.bottom(), theme::hairline());

            let mut bar_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(bar.shrink2(vec2(14.0, 0.0)))
                    .layout(Layout::left_to_right(Align::Center)),
            );
            icons::paint(
                bar_ui.painter(),
                Rect::from_center_size(
                    bar_ui.cursor().min + vec2(8.0, bar.height() / 2.0),
                    vec2(15.0, 15.0),
                ),
                if state.is_pdf() { Icon::Doc } else { Icon::Eye },
                theme::accent_ink(),
            );
            bar_ui.add_space(26.0);
            bar_ui.label(
                RichText::new(crate::util::truncate_chars(&state.name, 52))
                    .family(theme::medium())
                    .size(12.5)
                    .color(theme::ink()),
            );
            bar_ui.add_space(10.0);
            let pages_label = if state.is_pdf() {
                format!(
                    "{} {} / {}",
                    t(locale, Key::ViewerPage),
                    state.page + 1,
                    state.page_count()
                )
            } else {
                match &state.image {
                    Some(image) => format!("{}×{}", image.width(), image.height()),
                    None => String::new(),
                }
            };
            let _ = hud(&mut bar_ui, &pages_label, theme::faint());

            bar_ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if secondary_button(ui, Some(Icon::Close), t(locale, Key::ViewerClose), 30.0)
                    .clicked()
                {
                    close = true;
                }
                // OCR affordance: images are sent as-is, PDFs as a fresh
                // render of the page on screen (v2.1.0 — a scanned PDF was
                // previously a dead end: only raster attachments had the
                // button). While a job runs the slot becomes a spinner.
                if state.raw_image.is_some() || state.is_pdf() {
                    if ocr_busy {
                        ui.add(egui::Spinner::new().size(15.0))
                            .on_hover_text(t(locale, Key::OcrAlreadyRunning));
                    } else if secondary_button(
                        ui,
                        Some(Icon::Sparkle),
                        t(locale, Key::OcrAttachment),
                        30.0,
                    )
                    .on_hover_text(t(locale, Key::OcrAttachment))
                    .clicked()
                    {
                        match state.ocr_payload() {
                            Ok(bytes) => outcome.ocr = Some((state.name.clone(), bytes)),
                            Err(message) => state.error = Some(message),
                        }
                    }
                }
                let fit = secondary_button(ui, None, t(locale, Key::FitWidth), 26.0)
                    .on_hover_text(t(locale, Key::FitWidth));
                if fit.clicked() {
                    state.zoom = 1.0;
                }
                if secondary_button(ui, Some(Icon::Plus), t(locale, Key::ViewerZoom), 30.0)
                    .on_hover_text(t(locale, Key::ViewerZoom))
                    .clicked()
                {
                    state.zoom = (state.zoom * 1.25).min(5.0);
                }
                let _ = hud(
                    ui,
                    &format!("{}%", (state.zoom * 100.0) as i32),
                    theme::faint(),
                );
                if secondary_button(ui, Some(Icon::Minus), t(locale, Key::ViewerZoom), 30.0)
                    .on_hover_text(t(locale, Key::ViewerZoom))
                    .clicked()
                {
                    state.zoom = (state.zoom / 1.25).max(0.2);
                }
                if state.is_pdf() && state.page_count() > 1 {
                    if secondary_button(ui, Some(Icon::ChevronRight), "", 30.0).clicked()
                        && state.page + 1 < state.page_count()
                    {
                        state.page += 1;
                    }
                    if secondary_button(ui, Some(Icon::ChevronLeft), "", 30.0).clicked()
                        && state.page > 0
                    {
                        state.page -= 1;
                    }
                }
            });

            // Content area.
            let content = Rect::from_min_max(full.min + vec2(0.0, 46.0), full.max);
            let mut content_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(content)
                    .layout(Layout::top_down(Align::Center)),
            );

            if let Some(error) = state.error.clone() {
                content_ui.add_space(80.0);
                content_ui.add(
                    Label::new(
                        RichText::new(error)
                            .family(theme::medium())
                            .size(13.0)
                            .color(theme::bad()),
                    )
                    .wrap(),
                );
                return;
            }

            if let Some(image) = state.image.clone() {
                draw_zoomable(&mut content_ui, ctx, image, state.zoom, "viewer-image");
                return;
            }

            if let Some(pdf) = state.pdf.as_ref() {
                // Render width tracks the available area × zoom, floored to
                // 64 px buckets so a resize does not re-render per pixel (and
                // never renders wider than the viewport at zoom 1.0).
                let target_width = content.width() * state.zoom;
                let bucket = ((target_width / 64.0).floor() * 64.0).max(320.0);
                let key = ((state.page as u64) << 32) | (bucket.max(1.0) as u64 & 0xFFFF_FFFF);
                let needs = state
                    .texture
                    .as_ref()
                    .map(|(cached, _)| *cached != key)
                    .unwrap_or(true);
                if needs || bucket != state.last_width {
                    state.last_width = bucket;
                    match pdf.render_page(state.page, bucket as f32) {
                        Ok(image) => {
                            let handle = ctx.load_texture(
                                format!("pdf-{}-{}", state.page, bucket),
                                image,
                                egui::TextureOptions::LINEAR,
                            );
                            state.texture = Some((key, handle));
                        }
                        Err(message) => {
                            state.error = Some(message);
                            return;
                        }
                    }
                }
                if let Some((_, handle)) = &state.texture {
                    // The scroll area is the whole point: a page taller than
                    // the window must scroll, not clip. The salt includes the
                    // page + bucket so flipping a page or zooming starts at
                    // the top instead of keeping a stale offset.
                    ScrollArea::both()
                        .id_salt(format!("pdf-view-{}-{}", state.page, bucket as u64))
                        .auto_shrink([false, false])
                        .show(&mut content_ui, |ui| {
                            ui.add_space(14.0);
                            Frame::new()
                                .fill(Color32::WHITE)
                                .corner_radius(4.0)
                                .inner_margin(Margin::same(0))
                                .show(ui, |ui| {
                                    ui.add(egui::Image::new((handle.id(), handle.size_vec2())));
                                });
                            ui.add_space(24.0);
                        });
                }
            }
        });

    if !close {
        *viewer = Some(state);
    }
    outcome
}

fn draw_zoomable(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    image: Arc<egui::ColorImage>,
    zoom: f32,
    id: &str,
) {
    let handle = ctx.load_texture(
        format!("viewer-{id}-{}", image.width()),
        (*image).clone(),
        egui::TextureOptions::LINEAR,
    );
    let natural = handle.size_vec2();
    // At zoom 1.0 a huge image must FIT the viewport, not overflow it:
    // `fit` shrinks oversized images to the window, zoom then scales from
    // that base (so 200 % of an already-fitted photo is genuinely bigger).
    let viewport = ui.available_rect_before_wrap().size();
    let fit = if natural.x > 0.0 && natural.y > 0.0 {
        (viewport.x / natural.x)
            .min(viewport.y / natural.y)
            .min(1.0)
            .max(0.05)
    } else {
        1.0
    };
    let size = natural * (fit * zoom);
    egui::ScrollArea::both()
        .id_salt(format!("{id}-scroll"))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(16.0);
            Frame::new()
                .fill(Color32::WHITE)
                .corner_radius(4.0)
                .show(ui, |ui| {
                    ui.add(egui::Image::new((handle.id(), size)));
                });
        });
}
