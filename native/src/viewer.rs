//! Full-screen attachment viewer: images and PDF pages, drawn natively.
//!
//! Images decode once via the `image` crate; PDF pages render on demand
//! through pdfium (the shared library arrives as a downloadable component —
//! `Pdfium::bind_to_library` loads it from the components directory).

use std::path::Path;
use std::sync::Arc;

use egui::{vec2, Align, Color32, Frame, Label, Layout, Margin, Rect, RichText, Sense};

use crate::i18n::{t, Key, Locale};
use crate::icons::{self, Icon};
use crate::theme;
use crate::{hud, secondary_button};

/// An open viewer overlay; `None` in the shell means closed.
pub struct ViewerState {
    pub name: String,
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
}

impl PdfDoc {
    fn open(lib_path: &Path, bytes: Vec<u8>) -> Result<Self, String> {
        use pdfium_render::prelude::*;
        let bindings =
            Pdfium::bind_to_library(lib_path).map_err(|e| format!("pdfium load: {e}"))?;
        // Leak: PdfDocument borrows the Pdfium for its lifetime.
        let pdfium: &'static Pdfium = Box::leak(Box::new(Pdfium::new(bindings)));
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
            .set_maximum_height(6000);
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

/// Draw the overlay and handle closing. `Escape` and backdrop clicks close.
pub fn draw(viewer: &mut Option<ViewerState>, locale: Locale, ctx: &egui::Context) {
    // Take the state out so the Area closure can own it exclusively; it is
    // put back unless the overlay closed this frame.
    let mut state = match viewer.take() {
        Some(state) => state,
        None => return,
    };
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return;
    }
    let full = ctx.content_rect();
    let mut close = false;

    egui::Area::new(egui::Id::new("attachment-viewer"))
        .order(egui::Order::Foreground)
        .fixed_pos(full.min)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(full.size(), Sense::click());
            if resp.clicked() {
                close = true;
            }
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_black_alpha(190));

            // Top toolbar.
            let bar = Rect::from_min_size(full.min + vec2(0.0, 0.0), vec2(full.width(), 46.0));
            ui.painter().rect_filled(bar, 0.0, theme::PANEL);
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
                Icon::Doc,
                theme::ACCENT_INK,
            );
            bar_ui.add_space(26.0);
            bar_ui.label(
                RichText::new(crate::util::truncate_chars(&state.name, 52))
                    .family(theme::medium())
                    .size(12.5)
                    .color(theme::INK),
            );
            bar_ui.add_space(10.0);
            let _ = hud(
                &mut bar_ui,
                &format!(
                    "{} {} / {}",
                    t(locale, Key::ViewerPage),
                    state.page + 1,
                    state.page_count()
                ),
                theme::FAINT,
            );

            bar_ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if secondary_button(ui, Some(Icon::Close), "", 30.0).clicked() {
                    close = true;
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
                if secondary_button(ui, Some(Icon::Plus), "", 30.0).clicked() {
                    state.zoom = (state.zoom * 1.25).min(5.0);
                }
                let _ = hud(
                    ui,
                    &format!("{}%", (state.zoom * 100.0) as i32),
                    theme::FAINT,
                );
                if secondary_button(ui, Some(Icon::Download), "", 30.0).clicked() {
                    state.zoom = (state.zoom / 1.25).max(0.2);
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
                            .color(theme::BAD),
                    )
                    .wrap(),
                );
                return;
            }

            if let Some(image) = state.image.clone() {
                draw_zoomable(&mut content_ui, ctx, image, state.zoom, "viewer-image");
                return;
            }

            if state.pdf.is_some() {
                // Render width tracks the available area, bucketed to avoid
                // re-rendering on every pixel of resize.
                let target_width = content.width() * state.zoom;
                let bucket = (target_width / 64.0).round() * 64.0;
                let key = ((state.page as u64) << 32) | (bucket.max(1.0) as u64 & 0xFFFF_FFFF);
                let needs = state
                    .texture
                    .as_ref()
                    .map(|(cached, _)| *cached != key)
                    .unwrap_or(true);
                let Some(pdf) = state.pdf.as_ref() else {
                    return;
                };
                if needs || bucket != state.last_width {
                    state.last_width = bucket;
                    match pdf.render_page(state.page, bucket) {
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
                    content_ui.add_space(14.0);
                    Frame::new()
                        .fill(Color32::WHITE)
                        .corner_radius(4.0)
                        .inner_margin(Margin::same(0))
                        .show(&mut content_ui, |ui| {
                            ui.add(egui::Image::new((handle.id(), handle.size_vec2())));
                        });
                }
            }
        });

    if !close {
        *viewer = Some(state);
    }
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
    let size = handle.size_vec2() * zoom;
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
