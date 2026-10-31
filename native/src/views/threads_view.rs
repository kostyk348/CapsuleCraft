//! Thought threads: reply chains built from `In-Reply-To` / `References`.
//!
//! Ported from `src/components/ThoughtThreadView.tsx`. Chains are assembled in
//! the view (not the vault) so a malformed header can never hide a page: a
//! reply whose parent is missing is promoted to a root, the same rule the page
//! tree uses for `X-Capsule-Page-Parent`.

use std::collections::{HashMap, HashSet};

use egui::{vec2, Align, Layout, RichText, Sense, Stroke, Ui};

use crate::i18n::{t, Key, Locale};
use crate::icons::Icon;
use crate::mime::Capsule;
use crate::theme;
use crate::{hud, tag_chip, truncate_chars};

use super::{empty_state, header, ViewAction, VIEW_PAD};

#[derive(Default)]
pub struct ThreadsState {
    pub collapsed: HashSet<String>,
    pub show_all: bool,
}

pub fn draw(
    ui: &mut Ui,
    capsules: &[Capsule],
    state: &mut ThreadsState,
    locale: Locale,
    selected: Option<usize>,
) -> ViewAction {
    let mut action = ViewAction::default();
    let chains = build(capsules);
    let replies = capsules.iter().filter(|c| c.in_reply_to.is_some()).count();
    let thread_count = chains
        .iter()
        .filter(|chain| !chain.replies.is_empty())
        .count();
    // A page with no replies is still a valid chain, but it is not a *thread*:
    // keep it behind the "All pages" toggle so the view answers its own title.
    let shown: Vec<&Chain> = chains
        .iter()
        .filter(|chain| state.show_all || !chain.replies.is_empty())
        .collect();

    header(
        ui,
        t(locale, Key::ThreadsTitle),
        &format!(
            "{} · {}",
            crate::i18n::plural(
                locale,
                thread_count,
                Key::ThreadOne,
                Key::ThreadFew,
                Key::ThreadMany
            ),
            crate::i18n::plural(
                locale,
                replies,
                Key::ReplyOne,
                Key::ReplyFew,
                Key::ReplyMany
            )
        ),
    );

    let (_, bar_rect) = super::toolbar(ui);
    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar_rect.shrink2(vec2(VIEW_PAD, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let _ = hud(
        &mut bar,
        &format!(
            "{} · {} · {}",
            crate::i18n::plural(
                locale,
                shown.len(),
                Key::PageOne,
                Key::PageFew,
                Key::PageMany
            ),
            crate::i18n::plural(
                locale,
                replies,
                Key::ReplyOne,
                Key::ReplyFew,
                Key::ReplyMany
            ),
            if state.show_all {
                t(locale, Key::AllPages)
            } else {
                t(locale, Key::ThreadsOnly)
            },
        ),
        theme::FAINT,
    );
    bar.with_layout(
        Layout::right_to_left(Align::Center),
        |ui| match super::segments(
            ui,
            &[
                (t(locale, Key::ThreadsOnly), !state.show_all),
                (t(locale, Key::AllPages), state.show_all),
            ],
        ) {
            Some(0) => state.show_all = false,
            Some(1) => state.show_all = true,
            _ => {}
        },
    );

    if shown.is_empty() {
        empty_state(
            ui,
            Icon::Mail,
            t(locale, Key::ThreadsTitle),
            t(locale, Key::ThreadsEmpty),
        );
        return action;
    }

    egui::ScrollArea::vertical()
        .id_salt("threads-body")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(10.0);
            ui.horizontal_top(|ui| {
                ui.add_space(VIEW_PAD);
                ui.vertical(|ui| {
                    ui.set_max_width(ui.available_width() - VIEW_PAD);
                    for chain in &shown {
                        let root = &capsules[chain.root];
                        let is_selected = selected == Some(chain.root);
                        let collapsed = state.collapsed.contains(&root.message_id);
                        let (rect, response) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
                        if response.hovered() || is_selected {
                            ui.painter().rect_filled(
                                rect,
                                theme::RADIUS,
                                if is_selected {
                                    theme::WASH_HOVER
                                } else {
                                    theme::WASH
                                },
                            );
                        }
                        let mut row = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(rect.shrink2(vec2(8.0, 0.0)))
                                .layout(Layout::left_to_right(Align::Center)),
                        );
                        // Scoped so the row's child UI cannot shadow the list UI:
                        // replies must be appended to the list, not to the row.
                        {
                            let row_ui = &mut row;
                            row_ui.spacing_mut().item_spacing.x = 8.0;
                            let arrow = crate::chevron(row_ui, theme::FAINT);
                            if arrow.clicked() {
                                if collapsed {
                                    state.collapsed.remove(&root.message_id);
                                } else {
                                    state.collapsed.insert(root.message_id.clone());
                                }
                            }
                            crate::icons::paint(
                                row_ui.painter(),
                                egui::Rect::from_min_size(
                                    row_ui.cursor().min + vec2(1.0, 4.0),
                                    vec2(13.0, 13.0),
                                ),
                                Icon::Mail,
                                theme::MUTED,
                            );
                            row_ui.add_space(19.0);
                            row_ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                ui.set_max_width((rect.width() - 210.0).max(140.0));
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(truncate_chars(
                                            &if root.subject.is_empty() {
                                                t(locale, Key::Untitled).to_string()
                                            } else {
                                                root.subject.clone()
                                            },
                                            72,
                                        ))
                                        .family(theme::medium())
                                        .size(12.5)
                                        .color(theme::INK),
                                    )
                                    .truncate(),
                                );
                                let _ = hud(
                                    ui,
                                    &format!(
                                        "{} · {} · {}",
                                        truncate_chars(&root.from, 24),
                                        truncate_chars(&root.date, 22),
                                        crate::i18n::plural(
                                            locale,
                                            chain.replies.len(),
                                            Key::ReplyOne,
                                            Key::ReplyFew,
                                            Key::ReplyMany
                                        )
                                    ),
                                    theme::FAINT,
                                );
                            });
                            row_ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                for tag in root.tags.iter().take(2) {
                                    let _ = tag_chip(ui, tag, theme::MUTED);
                                }
                                if !root.native {
                                    let _ =
                                        tag_chip(ui, t(locale, Key::ReadOnly), theme::BADGE_BETA);
                                }
                            });
                        }
                        if response.clicked() {
                            action.select_id = Some(root.message_id.clone());
                        }
                        ui.painter().hline(
                            rect.x_range(),
                            rect.bottom(),
                            Stroke::new(1.0, theme::fade(theme::LINE, 0.5)),
                        );

                        if !collapsed {
                            for (depth, index) in &chain.replies {
                                reply_row(ui, capsules, *index, *depth, locale, &mut action);
                            }
                        }
                    }
                    ui.add_space(60.0);
                });
            });
        });

    action
}

fn reply_row(
    ui: &mut Ui,
    capsules: &[Capsule],
    index: usize,
    depth: usize,
    locale: Locale,
    action: &mut ViewAction,
) {
    let capsule = &capsules[index];
    let indent = 26.0 + (depth as f32) * 18.0;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, theme::RADIUS, theme::WASH);
    }
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(8.0 + indent, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui = &mut row;
    ui.spacing_mut().item_spacing.x = 7.0;
    ui.label(
        RichText::new("↳")
            .size(11.0)
            .color(theme::fade(theme::FAINT, 0.8)),
    );
    ui.add(
        egui::Label::new(
            RichText::new(truncate_chars(
                &if capsule.subject.is_empty() {
                    t(locale, Key::Untitled).to_string()
                } else {
                    capsule.subject.clone()
                },
                64,
            ))
            .size(12.0)
            .color(if response.hovered() {
                theme::INK
            } else {
                theme::MUTED
            }),
        )
        .truncate(),
    );
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let _ = hud(ui, &capsule.date, theme::FAINT);
        ui.add_space(8.0);
        let _ = hud(ui, &truncate_chars(&capsule.from, 24), theme::FAINT);
    });
    if response.clicked() {
        action.select_id = Some(capsule.message_id.clone());
    }
}

pub struct Chain {
    pub root: usize,
    /// `(depth, capsule index)` in display order.
    pub replies: Vec<(usize, usize)>,
}

/// Build reply chains. Unresolved parents become roots; cycles are broken by
/// only following a reply edge when the parent was seen earlier in the walk.
pub fn build(capsules: &[Capsule]) -> Vec<Chain> {
    let by_id: HashMap<&str, usize> = capsules
        .iter()
        .enumerate()
        .map(|(index, capsule)| (capsule.message_id.as_str(), index))
        .collect();

    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut is_reply = vec![false; capsules.len()];
    for (index, capsule) in capsules.iter().enumerate() {
        let parent = capsule
            .in_reply_to
            .as_deref()
            .and_then(|id| by_id.get(id.trim()).copied());
        if let Some(parent) = parent {
            if parent != index {
                children.entry(parent).or_default().push(index);
                is_reply[index] = true;
            }
        }
    }

    let mut roots: Vec<usize> = (0..capsules.len())
        .filter(|index| !is_reply[*index])
        .collect();
    roots.sort_by(|a, b| {
        capsules[*b]
            .timestamp
            .cmp(&capsules[*a].timestamp)
            .then_with(|| capsules[*a].subject.cmp(&capsules[*b].subject))
    });

    let mut seen: HashSet<usize> = HashSet::new();
    let mut chains = Vec::new();
    for root in roots {
        let mut replies = Vec::new();
        // Mark the root before walking: a reply may point back at its own
        // ancestor, and without this a cycle would re-enter the root.
        seen.insert(root);
        walk(root, 0, &children, &mut seen, &mut replies);
        chains.push(Chain { root, replies });
    }

    // A pure cycle has no root: surface its earliest page as a root so nothing
    // disappears from the view.
    for index in 0..capsules.len() {
        if !seen.contains(&index) {
            let mut replies = Vec::new();
            seen.insert(index);
            walk(index, 0, &children, &mut seen, &mut replies);
            chains.push(Chain {
                root: index,
                replies,
            });
        }
    }

    chains
}

fn walk(
    parent: usize,
    depth: usize,
    children: &HashMap<usize, Vec<usize>>,
    seen: &mut HashSet<usize>,
    out: &mut Vec<(usize, usize)>,
) {
    let Some(list) = children.get(&parent) else {
        return;
    };
    let mut list = list.clone();
    list.sort_unstable();
    for child in list {
        if !seen.insert(child) {
            continue;
        }
        out.push((depth + 1, child));
        walk(child, depth + 1, children, seen, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capsule(id: &str, reply_to: Option<&str>) -> Capsule {
        let mut capsule = Capsule::blank(id.to_string(), format!("{id}.eml"), id.to_string());
        capsule.in_reply_to = reply_to.map(str::to_string);
        capsule
    }

    #[test]
    fn chains_replies_and_promotes_orphans() {
        let capsules = vec![
            capsule("a", None),
            capsule("b", Some("a")),
            capsule("c", Some("b")),
            capsule("d", Some("missing")),
        ];
        let chains = build(&capsules);
        let root_a = chains.iter().find(|chain| chain.root == 0).unwrap();
        assert_eq!(root_a.replies.len(), 2);
        assert_eq!(root_a.replies[0], (1, 1));
        assert_eq!(root_a.replies[1], (2, 2));
        assert!(chains.iter().any(|chain| chain.root == 3));
    }

    #[test]
    fn cycles_are_surfaced_once() {
        let capsules = vec![capsule("a", Some("b")), capsule("b", Some("a"))];
        let chains = build(&capsules);
        let total: usize = chains.iter().map(|chain| chain.replies.len() + 1).sum();
        assert_eq!(total, 2);
    }
}
