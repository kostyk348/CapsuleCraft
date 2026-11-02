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
use crate::{hud, secondary_button, text_button};

pub enum IntegrationsAction {
    None,
    Close,
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
        ],
    ),
    (Key::CompGroupViewers, &[ComponentId::PdfEngine]),
];

pub fn draw_integrations_modal(
    show: bool,
    manager: &Manager,
    locale: Locale,
    ctx: &egui::Context,
) -> IntegrationsAction {
    if !show {
        return IntegrationsAction::None;
    }
    let mut action = IntegrationsAction::None;
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

            let panel = Rect::from_center_size(rect.center(), vec2(680.0, 560.0));
            ui.painter().rect(
                panel,
                theme::RADIUS,
                theme::PANEL,
                Stroke::new(1.0, theme::LINE_STRONG),
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
                        .color(theme::INK),
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
                        .color(theme::MUTED),
                )
                .wrap(),
            );
            let _ = hud(&mut body, t(locale, Key::IntegrationsHint), theme::FAINT);
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
    action
}

fn component_card(ui: &mut egui::Ui, manager: &Manager, id: ComponentId, locale: Locale) {
    let spec = id.spec();
    let installed = manager.is_installed(id);
    let job = manager.job(id);
    let ready = manager.ready(id);

    Frame::new()
        .fill(theme::WASH)
        .corner_radius(theme::RADIUS)
        .stroke(theme::hairline())
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icons::paint(
                    ui.painter(),
                    Rect::from_min_size(ui.cursor().min + vec2(0.0, 2.0), vec2(14.0, 14.0)),
                    group_icon(id),
                    theme::ACCENT_INK,
                );
                ui.add_space(20.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(id.title(locale))
                                .family(theme::medium())
                                .size(13.0)
                                .color(theme::INK),
                        );
                        let version = manager
                            .installed_version(id)
                            .unwrap_or_else(|| spec.version.to_string());
                        let _ = hud(
                            ui,
                            &format!("{} {}", t(locale, Key::CompVersion), version),
                            theme::FAINT,
                        );
                        let _ = hud(
                            ui,
                            &crate::human_bytes(spec.size_hint as usize),
                            theme::FAINT,
                        );
                    });
                    ui.add(
                        Label::new(
                            RichText::new(t(locale, desc_key(id)))
                                .size(11.5)
                                .color(theme::MUTED),
                        )
                        .wrap(),
                    );
                });
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                // Status pill.
                let (label, color) = match (&job, installed) {
                    (Some(JobPhase::Done), _) => (t(locale, Key::CompInstalled), theme::OK),
                    (Some(JobPhase::Failed(_)), _) => (t(locale, Key::CompFailed), theme::BAD),
                    (Some(phase), _) if phase.is_active() => (
                        match phase {
                            JobPhase::Extracting => t(locale, Key::CompExtracting),
                            JobPhase::Queued => t(locale, Key::CompDownloading),
                            _ => t(locale, Key::CompDownloading),
                        },
                        theme::ACCENT_INK,
                    ),
                    (None, true) if ready => (t(locale, Key::CompReady), theme::OK),
                    (None, true) => (t(locale, Key::CompNotReady), theme::BADGE_BETA),
                    (None, false) => (t(locale, Key::CompDownload), theme::MUTED),
                    _ => (t(locale, Key::CompDownload), theme::MUTED),
                };
                pill(ui, label, color);

                let missing = manager.missing_requirements(id);
                if !missing.is_empty() {
                    let names: Vec<String> = missing.iter().map(|dep| dep.title(locale)).collect();
                    let _ = hud(
                        ui,
                        &format!("{} {}", t(locale, Key::CompRequires), names.join(", ")),
                        theme::FAINT,
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
                                .color(theme::FAINT),
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
                    theme::FAINT,
                );
            }
            if let Some(JobPhase::Failed(message)) = &job {
                ui.add(
                    Label::new(
                        RichText::new(message.clone())
                            .family(theme::mono())
                            .size(9.5)
                            .color(theme::BAD),
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
        | ComponentId::TtsVoiceDmitri => Icon::Audio,
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

        let mut raw = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let mut action = IntegrationsAction::None;
        let sink = &mut action;
        let mut run = |ctx: &egui::Context| {
            *sink = draw_integrations_modal(true, &manager, Locale::En, ctx);
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
}
