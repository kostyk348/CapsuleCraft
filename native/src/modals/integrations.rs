//! Integrations modal: the components catalog. Download, connect, remove —
//! everything the local AI engines and viewers need, in one place.

use egui::{
    vec2, Align, Color32, Frame, Label, Layout, Margin, Rect, RichText, ScrollArea, Sense, Stroke,
    UiBuilder,
};

use crate::components::{ComponentId, JobPhase, Manager};
use crate::i18n::{t, Key, Locale};
use crate::icons::{self, Icon};
use crate::theme;
use crate::ui_state::{CustomModelKind, CustomModels};
use crate::{hud, secondary_button, text_button};

pub enum IntegrationsAction {
    None,
    Close,
    /// A custom model folder was added or removed — persist the settings.
    Changed,
}

const GROUPS: &[(Key, &[ComponentId])] = &[
    (
        Key::CompGroupOcr,
        &[ComponentId::OcrEngine, ComponentId::OcrModel],
    ),
    (
        Key::CompGroupStt,
        &[
            ComponentId::SttWhisperTiny,
            ComponentId::SttWhisperBase,
            ComponentId::SttWhisperSmall,
        ],
    ),
    (
        Key::CompGroupTts,
        &[
            ComponentId::TtsRuntime,
            ComponentId::TtsVoiceIrina,
            ComponentId::TtsVoiceDmitri,
            ComponentId::TtsVoiceAmy,
        ],
    ),
    (Key::CompGroupViewers, &[ComponentId::PdfEngine]),
];

pub fn draw_integrations_modal(
    show: bool,
    manager: &Manager,
    custom_models: &mut CustomModels,
    locale: Locale,
    ctx: &egui::Context,
) -> IntegrationsAction {
    if !show {
        return IntegrationsAction::None;
    }
    let mut action = IntegrationsAction::None;
    let mut custom_changed = false;
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        action = IntegrationsAction::Close;
    }

    egui::Area::new(egui::Id::new("integrations-modal"))
        .order(egui::Order::Foreground)
        .fixed_pos(ctx.content_rect().min)
        .show(ctx, |ui| {
            let full = ctx.content_rect();
            let (rect, resp) = ui.allocate_exact_size(full.size(), Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_black_alpha(150));

            // The dialog never exceeds the window: on short or unusual
            // viewports the fixed 680x560 would clip its right edge — where
            // every row keeps its action buttons.
            let panel = Rect::from_center_size(
                rect.center(),
                vec2(680.0, 560.0).min(rect.size() - vec2(24.0, 24.0)),
            );
            ui.painter().rect(
                panel,
                theme::RADIUS,
                theme::panel(),
                Stroke::new(1.0, theme::line_strong()),
                egui::StrokeKind::Inside,
            );
            // The painted dialog is not enough: without a click-sensing widget
            // on top of it, every poke that missed a button (labels, status
            // pills, progress bars, empty card space) fell through to the
            // full-screen backdrop below and closed the whole modal. Claim the
            // panel area so clicks land here first; dismiss only on clicks
            // that hit the dimmed backdrop outside the dialog.
            let _panel_hit = ui.interact(
                panel,
                egui::Id::new("integrations-modal-panel"),
                Sense::click(),
            );
            if resp.clicked() {
                action = IntegrationsAction::Close;
            }

            let mut body = ui.new_child(
                UiBuilder::new()
                    .max_rect(panel.shrink(20.0))
                    .layout(Layout::top_down(Align::Min)),
            );

            body.horizontal(|ui| {
                ui.label(
                    RichText::new(t(locale, Key::IntegrationsTitle))
                        .family(theme::medium())
                        .size(15.0)
                        .color(theme::ink()),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if secondary_button(ui, Some(Icon::Close), "", 26.0).clicked() {
                        action = IntegrationsAction::Close;
                    }
                });
            });
            body.add(
                Label::new(
                    RichText::new(t(locale, Key::IntegrationsSubtitle))
                        .size(12.0)
                        .color(theme::muted()),
                )
                .wrap(),
            );
            let _ = hud(&mut body, t(locale, Key::IntegrationsHint), theme::faint());
            body.add_space(10.0);

            ScrollArea::vertical()
                .id_salt("integrations-body")
                .auto_shrink([false, false])
                .show(&mut body, |ui| {
                    for (group_key, ids) in GROUPS {
                        crate::views::section(ui, t(locale, *group_key));
                        for id in ids.iter().copied() {
                            component_card(ui, manager, id, locale);
                            ui.add_space(6.0);
                        }
                    }
                    // "Подключить свои": any local folder beats a download for
                    // users who already keep models on disk.
                    ui.add_space(4.0);
                    crate::views::section(ui, t(locale, Key::CustomModel));
                    let _ = hud(ui, t(locale, Key::CustomModelHint), theme::faint());
                    ui.add_space(6.0);
                    for kind in CustomModelKind::ALL {
                        if custom_model_row(ui, custom_models, kind, locale).changed {
                            custom_changed = true;
                        }
                        ui.add_space(6.0);
                    }
                    ui.add_space(8.0);
                });
        });
    // Keep polling while downloads run so the progress bars animate.
    let busy = ComponentId::ALL.iter().any(|id| {
        manager
            .job(*id)
            .map(|phase| phase.is_active())
            .unwrap_or(false)
    });
    if busy {
        ctx.request_repaint_after(std::time::Duration::from_millis(120));
    }
    if custom_changed && matches!(action, IntegrationsAction::None) {
        action = IntegrationsAction::Changed;
    }
    action
}

/// Geometry of one "use my own model folder" row: the card frame and the two
/// action buttons, so tests can assert the buttons stay inside the card even
/// with a very long model path on the hud line. Non-test builds only use
/// `changed`.
#[allow(dead_code)]
struct RowMetrics {
    changed: bool,
    frame: Rect,
    pick: Option<Rect>,
    clear: Option<Rect>,
}

/// One "use my own model folder" row: current path, pick, remove.
fn custom_model_row(
    ui: &mut egui::Ui,
    models: &mut CustomModels,
    kind: CustomModelKind,
    locale: Locale,
) -> RowMetrics {
    let mut changed = false;
    let current = models.slot(kind).map(str::to_string);
    let valid = current
        .as_deref()
        .map(|path| crate::ai::custom::validate(kind, std::path::Path::new(path)).is_ok())
        .unwrap_or(false);
    let mut pick = None;
    let mut clear = None;

    let frame = Frame::new()
        .fill(theme::wash())
        .corner_radius(theme::RADIUS)
        .stroke(theme::hairline())
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::paint(
                    ui.painter(),
                    Rect::from_min_size(ui.cursor().min + vec2(0.0, 2.0), vec2(14.0, 14.0)),
                    Icon::Folder,
                    theme::accent_ink(),
                );
                ui.add_space(20.0);
                ui.label(
                    RichText::new(t(locale, kind.key()))
                        .family(theme::medium())
                        .size(13.0)
                        .color(theme::ink()),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if current.is_some()
                        && {
                            let response = text_button(
                                ui,
                                Some(Icon::Trash),
                                t(locale, Key::CustomModelClear),
                                false,
                            );
                            clear = Some(response.rect);
                            response.clicked()
                        }
                    {
                        models.set(kind, None);
                        changed = true;
                    }
                    if {
                        let response = text_button(
                            ui,
                            Some(Icon::Folder),
                            t(locale, Key::CustomModelPick),
                            false,
                        );
                        pick = Some(response.rect);
                        response.clicked()
                    } {
                        if let Some(dir) = rfd::FileDialog::new()
                            .set_title(t(locale, Key::CustomModelPick))
                            .pick_folder()
                        {
                            models.set(kind, Some(dir.display().to_string()));
                            changed = true;
                        }
                    }
                });
            });
            match &current {
                Some(path) if valid => {
                    let _ = hud(ui, path, theme::ok());
                }
                Some(path) => {
                    let _ = hud(ui, path, theme::bad());
                    let _ = hud(ui, t(locale, Key::CustomModelBad), theme::bad());
                }
                None => {
                    let _ = hud(ui, t(locale, Key::CustomModelHint), theme::faint());
                }
            }
        });
    RowMetrics {
        changed,
        frame: frame.response.rect,
        pick,
        clear,
    }
}

fn component_card(ui: &mut egui::Ui, manager: &Manager, id: ComponentId, locale: Locale) {
    let spec = id.spec();
    let installed = manager.is_installed(id);
    let job = manager.job(id);
    let ready = manager.ready(id);

    Frame::new()
        .fill(theme::wash())
        .corner_radius(theme::RADIUS)
        .stroke(theme::hairline())
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::paint(
                    ui.painter(),
                    Rect::from_min_size(ui.cursor().min + vec2(0.0, 2.0), vec2(14.0, 14.0)),
                    group_icon(id),
                    theme::accent_ink(),
                );
                ui.add_space(20.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(id.title(locale))
                                .family(theme::medium())
                                .size(13.0)
                                .color(theme::ink()),
                        );
                        let version = manager
                            .installed_version(id)
                            .unwrap_or_else(|| spec.version.to_string());
                        let _ = hud(
                            ui,
                            &format!("{} {}", t(locale, Key::CompVersion), version),
                            theme::faint(),
                        );
                        let _ = hud(
                            ui,
                            &crate::human_bytes(spec.size_hint as usize),
                            theme::faint(),
                        );
                    });
                    ui.add(
                        Label::new(
                            RichText::new(t(locale, desc_key(id)))
                                .size(11.5)
                                .color(theme::muted()),
                        )
                        .wrap(),
                    );
                });
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                // Status pill.
                let (label, color) = match (&job, installed) {
                    (Some(JobPhase::Done), _) => (t(locale, Key::CompInstalled), theme::ok()),
                    (Some(JobPhase::Failed(_)), _) => (t(locale, Key::CompFailed), theme::bad()),
                    (Some(phase), _) if phase.is_active() => (
                        match phase {
                            JobPhase::Extracting => t(locale, Key::CompExtracting),
                            JobPhase::Queued => t(locale, Key::CompDownloading),
                            _ => t(locale, Key::CompDownloading),
                        },
                        theme::accent_ink(),
                    ),
                    (None, true) if ready => (t(locale, Key::CompReady), theme::ok()),
                    (None, true) => (t(locale, Key::CompNotReady), theme::badge_beta()),
                    (None, false) => (t(locale, Key::CompDownload), theme::muted()),
                    _ => (t(locale, Key::CompDownload), theme::muted()),
                };
                pill(ui, label, color);

                let missing = manager.missing_requirements(id);
                if !missing.is_empty() {
                    let names: Vec<String> = missing.iter().map(|dep| dep.title(locale)).collect();
                    let _ = hud(
                        ui,
                        &format!("{} {}", t(locale, Key::CompRequires), names.join(", ")),
                        theme::faint(),
                    );
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let busy = job.as_ref().map(|p| p.is_active()).unwrap_or(false);
                    if busy {
                        // No cancel in v1.12: downloads are short except the
                        // model, and aborting mid-extract leaves a partial dir.
                        ui.label(
                            RichText::new(t(locale, Key::CompDownloading))
                                .size(11.0)
                                .color(theme::faint()),
                        );
                    } else if installed {
                        if text_button(ui, Some(Icon::Trash), "", false).clicked() {
                            manager.remove(id);
                        }
                        if text_button(
                            ui,
                            Some(Icon::Folder),
                            t(locale, Key::CompOpenFolder),
                            false,
                        )
                        .clicked()
                        {
                            let _ = open_folder(manager, id);
                        }
                    } else {
                        let label = matches!(job, Some(JobPhase::Failed(_)))
                            .then(|| t(locale, Key::CompRetry))
                            .unwrap_or_else(|| t(locale, Key::CompDownload));
                        if text_button(ui, Some(Icon::Download), label, false).clicked() {
                            let _ = manager.start(id);
                        }
                    }
                });
            });

            if let Some(phase @ JobPhase::Fetching { got, total }) = &job {
                ui.add_space(4.0);
                ui.add(egui::ProgressBar::new(phase.progress().unwrap_or(0.0)).desired_height(4.0));
                let _ = hud(
                    ui,
                    &format!(
                        "{} / {}",
                        crate::human_bytes(*got as usize),
                        total
                            .map(|total| crate::human_bytes(total as usize))
                            .unwrap_or_else(|| "?".to_string())
                    ),
                    theme::faint(),
                );
            }
            if let Some(JobPhase::Failed(message)) = &job {
                ui.add(
                    Label::new(
                        RichText::new(message.clone())
                            .family(theme::mono())
                            .size(9.5)
                            .color(theme::bad()),
                    )
                    .wrap(),
                );
            }
        });
}

fn pill(ui: &mut egui::Ui, label: &str, color: Color32) {
    let width = label.chars().count() as f32 * 6.2 + 18.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, 18.0), Sense::hover());
    ui.painter().rect(
        rect,
        theme::RADIUS,
        color.gamma_multiply(0.16),
        Stroke::new(0.0, Color32::TRANSPARENT),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::new(9.0, theme::mono_bold()),
        color,
    );
}

fn group_icon(id: ComponentId) -> Icon {
    match id {
        ComponentId::OcrEngine | ComponentId::OcrModel => Icon::Chip,
        ComponentId::SttWhisperTiny
        | ComponentId::SttWhisperBase
        | ComponentId::SttWhisperSmall
        | ComponentId::TtsRuntime
        | ComponentId::TtsVoiceIrina
        | ComponentId::TtsVoiceDmitri
        | ComponentId::TtsVoiceAmy => Icon::Audio,
        ComponentId::PdfEngine => Icon::Doc,
    }
}

fn desc_key(id: ComponentId) -> Key {
    match id {
        ComponentId::OcrEngine => Key::OcrEngineDesc,
        ComponentId::OcrModel => Key::OcrModelDesc,
        ComponentId::SttWhisperTiny => Key::SttTinyDesc,
        ComponentId::SttWhisperBase => Key::SttBaseDesc,
        ComponentId::SttWhisperSmall => Key::SttSmallDesc,
        ComponentId::TtsRuntime => Key::TtsRuntimeDesc,
        ComponentId::TtsVoiceIrina => Key::TtsIrinaDesc,
        ComponentId::TtsVoiceDmitri => Key::TtsDmitriDesc,
        ComponentId::TtsVoiceAmy => Key::TtsAmyDesc,
        ComponentId::PdfEngine => Key::PdfEngineDesc,
    }
}

fn open_folder(manager: &Manager, id: ComponentId) -> Result<(), std::io::Error> {
    let dir = manager.dir(id);
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(dir).spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(dir).spawn()?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(dir).spawn()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Manager;

    /// Draws the open modal headlessly and feeds a synthetic click at `click`
    /// (press + release on consecutive frames). Returns the action produced by
    /// the frame that carries the release — the frame where `clicked()` fires.
    fn action_after_click(click: Option<egui::Pos2>) -> IntegrationsAction {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(1200.0, 800.0));
        let manager = Manager::load_for_tests();
        let mut custom_models = CustomModels::default();

        let mut raw = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let mut action = IntegrationsAction::None;
        let sink = &mut action;
        let mut run = |ctx: &egui::Context| {
            *sink = draw_integrations_modal(true, &manager, &mut custom_models, Locale::En, ctx);
        };
        let mut pass = |input: egui::RawInput| {
            let mut out = ctx.run_pass(input, &mut run);
            // Headless: drop the texture updates instead of painting them.
            out.textures_delta.clear();
        };
        // Frame 1: layout the modal with no pointer around.
        pass(raw.clone());
        if let Some(pos) = click {
            raw.events.push(egui::Event::PointerMoved(pos));
            pass(raw.clone());
            raw.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            });
            pass(raw.clone());
            raw.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            });
            pass(raw);
        }
        action
    }

    /// Regression: the panel was only painted, so a click anywhere that missed
    /// a button fell through to the full-screen backdrop and closed the whole
    /// modal — "интеграции не тыкаются". The dialog body must swallow clicks.
    #[test]
    fn click_inside_the_dialog_does_not_close_it() {
        // Screen 1200x800 → the 680x560 dialog spans (260..940, 120..680).
        // Dead space in the card area and next to the title both land on the
        // dialog, not on any button.
        for pos in [
            egui::pos2(600.0, 400.0), // inside a component card, off any button
            egui::pos2(450.0, 148.0), // title row, left of the close button
            egui::pos2(930.0, 670.0), // dialog corner padding
        ] {
            let action = action_after_click(Some(pos));
            assert!(
                matches!(action, IntegrationsAction::None),
                "click at {pos:?} closed the modal"
            );
        }
    }

    /// Click-to-dismiss still works outside the dialog.
    #[test]
    fn click_outside_the_dialog_closes_it() {
        for pos in [
            egui::pos2(40.0, 400.0),   // far left of the dialog
            egui::pos2(600.0, 40.0),   // above the dialog
            egui::pos2(1150.0, 760.0), // bottom-right corner
        ] {
            let action = action_after_click(Some(pos));
            assert!(
                matches!(action, IntegrationsAction::Close),
                "click at {pos:?} did not close the modal"
            );
        }
    }

    /// Regression: the model path is printed on the row as an unclamped HUD
    /// line, so a long folder path widened the whole card frame past the
    /// scroll viewport — and the right-aligned pick/remove buttons visually
    /// slid off the card ("кнопки с добавлением своей модели едут"). The row
    /// must stay as wide as its viewport with the buttons inside the frame.
    #[test]
    fn long_model_path_keeps_the_row_and_buttons_in_place() {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::Vec2::new(1200.0, 800.0),
            )),
            ..Default::default()
        };
        let mut custom_models = CustomModels::default();
        let long_path = "/home/user/Загрузки/models/whisper/very/deep/nested/folder/that/goes/on/and/on/and/overflows/quantized-tiny/с-russian-буквами";
        custom_models.set(CustomModelKind::Stt, Some(long_path.to_string()));

        let mut metrics: Vec<RowMetrics> = Vec::new();
        let sink = &mut metrics;
        let mut out = ctx.run_pass(raw, |ctx| {
            egui::Area::new(egui::Id::new("row-test"))
                .fixed_pos(egui::Pos2::ZERO)
                .show(ctx, |ui| {
                    let mut column = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::Vec2::new(640.0, 600.0),
                            ))
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    for kind in CustomModelKind::ALL {
                        sink.push(custom_model_row(&mut column, &mut custom_models, kind, Locale::En));
                        column.add_space(6.0);
                    }
                });
        });
        // Headless: drop the texture updates instead of painting them.
        out.textures_delta.clear();

        let stt = &metrics[0];
        // The card did not stretch past its viewport (640 wide).
        assert!(
            stt.frame.width() <= 641.0,
            "card stretched to {} px by the long path",
            stt.frame.width()
        );
        // Both buttons stayed inside the card, anchored to its right edge.
        for (name, rect) in [("pick", stt.pick), ("clear", stt.clear)] {
            let rect = rect.expect("button rect recorded");
            assert!(
                rect.right() <= stt.frame.right() + 1.0 && rect.left() >= stt.frame.left(),
                "{name} button at {rect:?} left the card {:?}",
                stt.frame
            );
        }
        // The untouched rows keep their buttons on the right edge too.
        for (name, row) in [("tts", &metrics[1]), ("ocr", &metrics[2])] {
            let rect = row.pick.expect("pick rect recorded");
            assert!(
                (rect.right() - row.frame.right() + 10.0).abs() < 2.0,
                "{name} pick button detached from the card edge: {rect:?} vs {:?}",
                row.frame
            );
        }
    }
}
