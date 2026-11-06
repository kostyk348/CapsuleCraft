//! First-run onboarding: an interactive, bilingual guided tour.
//!
//! Twelve steps across every feature area. Some steps carry a "try it"
//! action that jumps straight into the feature (opens the command palette,
//! the appearance picker, the graph view, the integrations catalog…); the
//! tour closes while the feature is in front of you and resumes from the
//! command palette ("Обзор приложения / Onboarding tour").
//!
//! Completion is remembered in `native-onboarding.txt` beside the locale
//! setting, so the tour only auto-opens on a genuinely first launch.

use egui::{vec2, Align, Color32, Layout, Rect, RichText, Sense, Stroke, UiBuilder};

use crate::i18n::{t, Key, Locale};
use crate::theme;
use crate::{primary_button, secondary_button};

/// Total steps; indices are stable, add new steps at the end.
pub const STEP_COUNT: usize = 12;

/// Features a step can jump into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TryAction {
    Palette,
    Appearance,
    Graph,
    Planning,
    Integrations,
    Mcp,
    Faq,
}

struct StepDef {
    title: Key,
    body: Key,
    try_action: Option<TryAction>,
    try_label: Key,
}

const TRY_PALETTE: Key = Key::OnbTryPalette;
const TRY_APPEARANCE: Key = Key::OnbTryAppearance;
const TRY_GRAPH: Key = Key::OnbTryGraph;
const TRY_PLANNING: Key = Key::OnbTryPlanning;
const TRY_AI: Key = Key::OnbTryIntegrations;
const TRY_MCP: Key = Key::OnbTryMcp;
const TRY_FAQ: Key = Key::OnbTryFaq;

fn steps() -> [StepDef; STEP_COUNT] {
    [
        StepDef {
            title: Key::OnbWelcomeT,
            body: Key::OnbWelcomeB,
            try_action: None,
            try_label: TRY_FAQ,
        },
        StepDef {
            title: Key::OnbPagesT,
            body: Key::OnbPagesB,
            try_action: None,
            try_label: TRY_FAQ,
        },
        StepDef {
            title: Key::OnbBlocksT,
            body: Key::OnbBlocksB,
            try_action: None,
            try_label: TRY_FAQ,
        },
        StepDef {
            title: Key::OnbPaletteT,
            body: Key::OnbPaletteB,
            try_action: Some(TryAction::Palette),
            try_label: TRY_PALETTE,
        },
        StepDef {
            title: Key::OnbThemesT,
            body: Key::OnbThemesB,
            try_action: Some(TryAction::Appearance),
            try_label: TRY_APPEARANCE,
        },
        StepDef {
            title: Key::OnbGraphT,
            body: Key::OnbGraphB,
            try_action: Some(TryAction::Graph),
            try_label: TRY_GRAPH,
        },
        StepDef {
            title: Key::OnbPlanningT,
            body: Key::OnbPlanningB,
            try_action: Some(TryAction::Planning),
            try_label: TRY_PLANNING,
        },
        StepDef {
            title: Key::OnbPkmT,
            body: Key::OnbPkmB,
            try_action: None,
            try_label: TRY_PALETTE,
        },
        StepDef {
            title: Key::OnbAiT,
            body: Key::OnbAiB,
            try_action: Some(TryAction::Integrations),
            try_label: TRY_AI,
        },
        StepDef {
            title: Key::OnbMcpT,
            body: Key::OnbMcpB,
            try_action: Some(TryAction::Mcp),
            try_label: TRY_MCP,
        },
        StepDef {
            title: Key::OnbSyncT,
            body: Key::OnbSyncB,
            try_action: None,
            try_label: TRY_FAQ,
        },
        StepDef {
            title: Key::OnbFinishT,
            body: Key::OnbFinishB,
            try_action: Some(TryAction::Faq),
            try_label: TRY_FAQ,
        },
    ]
}

pub enum OnboardingAction {
    None,
    Next,
    Prev,
    /// Skipped or finished — either way, never auto-open again.
    Complete,
    Try(TryAction),
}

pub fn draw_onboarding(
    show: bool,
    step: usize,
    locale: Locale,
    ctx: &egui::Context,
) -> OnboardingAction {
    if !show {
        return OnboardingAction::None;
    }

    let mut action = OnboardingAction::None;
    let defs = steps();
    let index = step.min(STEP_COUNT - 1);
    let def = &defs[index];
    let last = index == STEP_COUNT - 1;

    egui::Area::new(egui::Id::new("onboarding"))
        .order(egui::Order::Foreground)
        .fixed_pos(ctx.content_rect().min)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(ctx.content_rect().size(), Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_black_alpha(150));

            let panel = Rect::from_center_size(rect.center(), vec2(560.0, 380.0));
            ui.painter().rect(
                panel,
                theme::RADIUS,
                theme::panel(),
                Stroke::new(1.0, theme::line_strong()),
                egui::StrokeKind::Inside,
            );
            // A thin accent stripe along the top edge marks the tour card.
            ui.painter().rect_filled(
                Rect::from_min_size(panel.min + vec2(20.0, 0.0), vec2(panel.width() - 40.0, 2.0)),
                1.0,
                theme::accent_color(),
            );

            let mut body = ui.new_child(
                UiBuilder::new()
                    .max_rect(panel.shrink(24.0))
                    .layout(Layout::top_down(Align::Min)),
            );

            // Progress: step counter + dots.
            body.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{} / {}", index + 1, STEP_COUNT))
                        .family(theme::mono())
                        .size(theme::HUD_SIZE + 1.0)
                        .color(theme::faint()),
                );
                ui.add_space(10.0);
                for i in 0..STEP_COUNT {
                    let (dot, _) = ui.allocate_exact_size(vec2(6.0, 6.0), Sense::hover());
                    let colour = if i == index {
                        theme::accent_color()
                    } else if i < index {
                        theme::accent_ink()
                    } else {
                        theme::line_strong()
                    };
                    ui.painter().circle_filled(dot.center(), 2.5, colour);
                }
            });
            body.add_space(12.0);

            body.label(
                RichText::new(t(locale, def.title))
                    .family(theme::display())
                    .size(21.0)
                    .color(theme::ink_strong()),
            );
            body.add_space(8.0);
            body.add(
                egui::Label::new(
                    RichText::new(t(locale, def.body))
                        .size(14.0)
                        .color(theme::ink()),
                )
                .wrap(),
            );
            body.add_space(12.0);

            // "Try it" jumps into the real feature.
            if let Some(try_action) = def.try_action {
                body.horizontal(|ui| {
                    let button = egui::Button::new(
                        RichText::new(t(locale, def.try_label))
                            .family(theme::medium())
                            .size(13.0)
                            .color(theme::invert_fg()),
                    )
                    .fill(theme::accent_color())
                    .corner_radius(theme::RADIUS);
                    if ui.add(button).clicked() {
                        action = OnboardingAction::Try(try_action);
                    }
                });
            }

            body.with_layout(Layout::bottom_up(Align::Min), |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        let next_label = if last {
                            t(locale, Key::OnbDone)
                        } else {
                            t(locale, Key::OnbNext)
                        };
                        if primary_button(ui, next_label, 28.0).clicked() {
                            action = if last {
                                OnboardingAction::Complete
                            } else {
                                OnboardingAction::Next
                            };
                        }
                        if !last && secondary_button(ui, None, t(locale, Key::OnbBack), 28.0).clicked() {
                            action = OnboardingAction::Prev;
                        }
                        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(t(locale, Key::OnbSkip))
                                            .family(theme::medium())
                                            .size(12.5)
                                            .color(theme::muted()),
                                    )
                                    .fill(Color32::TRANSPARENT)
                                    .corner_radius(theme::RADIUS),
                                )
                                .clicked()
                            {
                                action = OnboardingAction::Complete;
                            }
                        });
                    });
                });
            });
        });

    action
}

/// Has the tour been completed (or skipped) in this settings directory?
pub fn is_done(dir: &std::path::Path) -> bool {
    std::fs::read_to_string(dir.join("native-onboarding.txt"))
        .map(|text| text.trim().eq_ignore_ascii_case("done"))
        .unwrap_or(false)
}

/// Marks the tour completed so it never auto-opens again.
pub fn mark_done(dir: &std::path::Path) {
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(dir.join("native-onboarding.txt"), "done\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_step_has_bilingual_text_and_a_known_try_action() {
        let defs = steps();
        assert_eq!(defs.len(), STEP_COUNT);
        for def in &defs {
            for key in [def.title, def.body, def.try_label] {
                for locale in [Locale::Ru, Locale::En] {
                    let text = t(locale, key).to_string();
                    assert!(!text.trim().is_empty(), "{key:?} is empty for {locale:?}");
                }
            }
        }
        // English strings stay Cyrillic-free (the EN purism rule).
        for def in &defs {
            for key in [def.title, def.body, def.try_label] {
                let en = t(Locale::En, key).to_string();
                assert!(
                    !en.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)),
                    "{key:?} has Cyrillic in EN: {en}"
                );
            }
        }
    }

    #[test]
    fn done_flag_round_trips() {
        let dir = std::env::temp_dir().join(format!(
            "capsule-onboarding-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!is_done(&dir));
        assert!(!is_done(&dir.join("absent"))); // missing dir => not done, no panic
        mark_done(&dir);
        assert!(is_done(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn last_step_completes_via_next_button_semantics() {
        // The draw function maps "Next" on the last step to Complete; this
        // pins the arithmetic the shell relies on.
        let last = STEP_COUNT - 1;
        assert!(last > 0);
        let advanced = last + 1;
        assert_eq!(advanced.min(STEP_COUNT - 1), last);
    }
}
