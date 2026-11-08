//! Sidebar view: vault navigation, page hierarchy tree, drag-and-drop, and full-text search.

pub mod dnd;
pub mod tree;

use std::collections::HashSet;

use egui::{
    pos2, vec2, Align, Align2, FontId, Frame, Layout, Margin, Rect, Response, RichText, ScrollArea,
    Sense, Stroke, TextEdit, Ui, UiBuilder,
};

use crate::caps;
use crate::hud;
use crate::i18n::{self, t, Key, Locale};
use crate::icon_button;
use crate::icons::{self, Icon};
use crate::primary_button;
use crate::search;
use crate::text_button;
use crate::theme;
use crate::truncate_chars;
use crate::ui_state::{PageSort, UiState};
use crate::vault::{PageTree, Vault};
use crate::{menu_header, menu_row, menu_separator, popover_frame, ShellAction};

#[allow(unused_imports)]
pub use dnd::{draw_drag_preview, draw_root_drop_zone, handle_drag_release};
pub use tree::{draw_page_nodes, draw_search_row, expand_ancestors};

pub fn render_vault_menu_contents(
    ui: &mut Ui,
    vault: &Vault,
    locale: Locale,
    action: &mut ShellAction,
) {
    ui.set_min_width(290.0);

    menu_header(ui, t(locale, Key::CurrentVaultHeader));
    let (current_name, is_project) = crate::vault::get_vault_display_info(&vault.dir);
    let icon = if is_project { Icon::Doc } else { Icon::Folder };
    let badge = if is_project {
        t(locale, Key::ProjectVaultBadge)
    } else {
        t(locale, Key::PersonalVaultBadge)
    };
    let current_label = format!("{current_name}  [{badge}]");
    menu_row(ui, Some(icon), &current_label, None, true, false, false)
        .on_hover_text(vault.dir.display().to_string());

    if menu_row(
        ui,
        Some(Icon::Reload),
        &t(locale, Key::SyncFolderMenu).to_string(),
        Some(&t(locale, Key::SyncFolderMenuHint)),
        true,
        false,
        false,
    )
    .clicked()
    {
        action.sync_folder = true;
    }

    let recent = crate::vault::get_recent_vaults();
    let other_recent: Vec<std::path::PathBuf> =
        recent.into_iter().filter(|p| p != &vault.dir).collect();
    if !other_recent.is_empty() {
        menu_separator(ui);
        menu_header(ui, t(locale, Key::RecentVaults));
        for path in other_recent {
            let (name, is_project) = crate::vault::get_vault_display_info(&path);
            let icon = if is_project { Icon::Doc } else { Icon::Folder };
            let prefix = if is_project { "📦 " } else { "" };
            let label = format!("{prefix}{name}");
            if menu_row(ui, Some(icon), &label, None, false, true, false)
                .on_hover_text(path.display().to_string())
                .clicked()
            {
                action.switch_vault = Some(path);
            }
        }
    }

    menu_separator(ui);
    if menu_row(
        ui,
        Some(Icon::Folder),
        t(locale, Key::OpenOtherFolder),
        None,
        false,
        true,
        false,
    )
    .clicked()
    {
        action.open_vault = true;
    }

    if menu_row(
        ui,
        Some(Icon::Doc),
        t(locale, Key::OpenProjectAsVault),
        None,
        false,
        true,
        false,
    )
    .clicked()
    {
        action.open_project_dialog = true;
    }
}

pub fn draw_vault_dropdown(ui: &mut Ui, vault: &Vault, locale: Locale, action: &mut ShellAction) {
    let button = vault_switcher(ui, vault);
    egui::Popup::menu(&button)
        .frame(popover_frame())
        .align(egui::RectAlign::BOTTOM_START)
        .show(|ui| {
            render_vault_menu_contents(ui, vault, locale, action);
        });
}

pub fn vault_switcher(ui: &mut Ui, vault: &Vault) -> Response {
    let (name, is_project) = crate::vault::get_vault_display_info(&vault.dir);
    let prefix = if is_project { "📦 " } else { "" };
    let full_name = format!("{prefix}{name}");
    let name = truncate_chars(&full_name, 30);
    let font = FontId::new(12.5, theme::medium());
    let galley = ui
        .painter()
        .layout_no_wrap(name.clone(), font, theme::ink());
    let width = 12.0 + 14.0 + 8.0 + galley.size().x + 10.0 + 12.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, 28.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, theme::RADIUS, theme::wash());
    }
    let ink = if response.hovered() {
        theme::ink()
    } else {
        theme::muted()
    };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(
            pos2(rect.left() + 12.0 + 7.0, rect.center().y),
            vec2(14.0, 14.0),
        ),
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
        theme::faint(),
    );
    response
        .on_hover_text(vault.dir.display().to_string())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[allow(clippy::too_many_arguments)]
pub fn draw_sidebar(
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
    ui_state: &UiState,
    words: usize,
    threads: usize,
    locale: Locale,
) -> ShellAction {
    let mut action = ShellAction::default();
    // A drag whose release never reached us (the pointer left the window, a
    // modal ate the button-up) must not poison the list: row clicks are
    // gated on `dragged_page` being empty, so a stuck state would make the
    // whole tree dead, and the next stray release would move a page to
    // wherever the pointer happens to be. Cancel when no button is down and
    // no release arrived this frame — a real release frame keeps its drop.
    if dragged_page.is_some()
        && !ui.input(|i| i.pointer.any_down())
        && !ui.input(|i| i.pointer.any_released())
    {
        *dragged_page = None;
    }
    // Was the sidebar search focused at the START of this frame? A singleline
    // TextEdit surrenders focus while processing Enter/Escape, so probes made
    // later in the frame see `None` and the keyboard flow dies silently. The
    // frame-start value is the honest "the user was typing in the field".
    let search_was_focused = ui.memory(|m| m.focused()) == Some(egui::Id::new("sidebar_search"));
    let outer = ui.max_rect();
    ui.allocate_rect(outer, Sense::hover());
    ui.set_min_width(outer.width());
    let inner = outer.shrink2(vec2(14.0, 8.0));
    let header_h = 122.0;
    let footer_h = 138.0;

    let header_rect = Rect::from_min_max(inner.min, pos2(inner.right(), inner.top() + header_h));
    let footer_rect = Rect::from_min_max(pos2(inner.left(), inner.bottom() - footer_h), inner.max);
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

    let card = vault_card(&mut header, vault);
    egui::Popup::menu(&card)
        .frame(popover_frame())
        .align(egui::RectAlign::BOTTOM_START)
        .show(|ui| {
            render_vault_menu_contents(ui, vault, locale, &mut action);
        });
    header.add_space(10.0);
    if search_box(&mut header, search, locale).changed() {
        // The list is rebuilt from `search` below; no extra state is needed.
    }
    header.add_space(14.0);

    let mut new_page_clicked = false;
    let section_rect = {
        let (rect, _) =
            header.allocate_exact_size(vec2(header.available_width(), 26.0), Sense::hover());
        let label = caps(
            &header,
            t(locale, Key::Pages),
            theme::HUD_SIZE,
            theme::mono_bold(),
            theme::LABEL_SPACING,
            theme::sidebar_ink(),
        );
        header.painter().galley(
            pos2(rect.left() + 2.0, rect.center().y - label.size().y / 2.0),
            label.clone(),
            theme::sidebar_ink(),
        );
        header.painter().text(
            pos2(rect.left() + 2.0 + label.size().x + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            vault.capsules.len().to_string(),
            FontId::new(9.0, theme::mono()),
            theme::faint(),
        );

        let is_project = crate::vault::is_project_vault(&vault.dir);

        let subfolder_ids: Vec<String> = (0..vault.capsules.len())
            .filter(|&idx| !tree.children_of(Some(idx)).is_empty() && !tree.roots().contains(&idx))
            .map(|idx| vault.capsules[idx].message_id.clone())
            .collect();
        let has_subfolders = !subfolder_ids.is_empty();
        let all_subfolders_collapsed =
            has_subfolders && subfolder_ids.iter().all(|id| collapsed.contains(id));

        let fold_rect =
            Rect::from_center_size(pos2(rect.right() - 35.0, rect.center().y), vec2(22.0, 22.0));
        let fold_resp = header
            .interact(
                fold_rect,
                header.scope_id().with("toggle-subfolders"),
                Sense::click(),
            )
            .on_hover_text(if all_subfolders_collapsed {
                t(locale, Key::ExpandAllFolders)
            } else {
                t(locale, Key::CollapseAllFolders)
            })
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if fold_resp.hovered() {
            header
                .painter()
                .rect_filled(fold_rect, theme::RADIUS, theme::wash());
        }
        icons::paint(
            header.painter(),
            Rect::from_center_size(fold_rect.center(), vec2(12.0, 12.0)),
            if all_subfolders_collapsed {
                Icon::Folder
            } else {
                Icon::ChevronDown
            },
            if fold_resp.hovered() {
                theme::ink()
            } else {
                theme::faint()
            },
        );
        if fold_resp.clicked() {
            if all_subfolders_collapsed {
                for id in &subfolder_ids {
                    collapsed.remove(id);
                }
            } else {
                for id in &subfolder_ids {
                    collapsed.insert(id.clone());
                }
            }
        }

        let mut x_offset = 59.0;
        if is_project {
            let sync_rect = Rect::from_center_size(
                pos2(rect.right() - x_offset, rect.center().y),
                vec2(22.0, 22.0),
            );
            let sync_resp = header
                .interact(
                    sync_rect,
                    header.scope_id().with("sync-project"),
                    Sense::click(),
                )
                .on_hover_text(t(locale, Key::SyncProjectTooltip))
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if sync_resp.hovered() {
                header
                    .painter()
                    .rect_filled(sync_rect, theme::RADIUS, theme::wash());
            }
            icons::paint(
                header.painter(),
                Rect::from_center_size(sync_rect.center(), vec2(12.0, 12.0)),
                Icon::Reload,
                if sync_resp.hovered() {
                    theme::ink()
                } else {
                    theme::faint()
                },
            );
            if sync_resp.clicked() {
                action.sync_project = true;
            }
            x_offset += 24.0;
        }

        let collapse_rect = Rect::from_center_size(
            pos2(rect.right() - x_offset, rect.center().y),
            vec2(22.0, 22.0),
        );
        let collapse = header
            .interact(
                collapse_rect,
                header.scope_id().with("collapse-sidebar"),
                Sense::click(),
            )
            .on_hover_text(format!(
                "{} (Ctrl + \\)",
                t(locale, Key::CommandToggleSidebar)
            ))
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if collapse.hovered() {
            header
                .painter()
                .rect_filled(collapse_rect, theme::RADIUS, theme::wash());
        }
        icons::paint(
            header.painter(),
            Rect::from_center_size(collapse_rect.center(), vec2(12.0, 12.0)),
            Icon::Sidebar,
            if collapse.hovered() {
                theme::ink()
            } else {
                theme::faint()
            },
        );
        if collapse.clicked() {
            action.toggle_sidebar = true;
        }

        let plus_rect =
            Rect::from_center_size(pos2(rect.right() - 11.0, rect.center().y), vec2(22.0, 22.0));
        let plus = header
            .interact(
                plus_rect,
                header.scope_id().with("new-page"),
                Sense::click(),
            )
            .on_hover_text(t(locale, Key::NewPage))
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if plus.hovered() {
            header
                .painter()
                .rect_filled(plus_rect, theme::RADIUS, theme::wash());
        }
        icons::paint(
            header.painter(),
            Rect::from_center_size(plus_rect.center(), vec2(12.0, 12.0)),
            Icon::Plus,
            if plus.hovered() {
                theme::ink()
            } else {
                theme::faint()
            },
        );
        if plus.clicked() {
            new_page_clicked = true;
        }
        rect
    };
    let _ = section_rect;
    action.new_page |= new_page_clicked;

    let last_selected = ui
        .ctx()
        .data_mut(|data| data.get_temp::<Option<usize>>(egui::Id::unique("sidebar-last-selected")))
        .flatten();

    if selected != last_selected {
        if let Some(target) = selected {
            expand_ancestors(target, tree, &vault.capsules, collapsed);
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::unique("sidebar-should-scroll"), true);
            });
        }
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::unique("sidebar-last-selected"), selected);
        });
    }

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
                // Pinned pages: first, in pin order, so one click reaches the
                // pages the user actually lives in.
                let pinned_positions: Vec<usize> = ui_state
                    .pinned_ids
                    .iter()
                    .filter_map(|id| {
                        vault
                            .capsules
                            .iter()
                            .position(|capsule| &capsule.message_id == id)
                    })
                    .collect();
                if !pinned_positions.is_empty() {
                    menu_header(ui, t(locale, Key::Pinned));
                    for position in pinned_positions.iter() {
                        if draw_search_row(
                            ui,
                            &vault.capsules,
                            tree,
                            *position,
                            selected,
                            0.0,
                            None,
                            search,
                            locale,
                        ) {
                            action.select = Some(*position);
                        }
                    }
                    ui.add_space(8.0);
                }

                // Sort control: a label-shaped popup; the choice persists in
                // ui_state.json, so the list stops reshuffling on its own.
                let sort_button = text_button(
                    ui,
                    None,
                    &format!(
                        "{}: {}",
                        t(locale, Key::PageSort),
                        t(locale, ui_state.sort_mode.key())
                    ),
                    false,
                );
                egui::Popup::menu(&sort_button)
                    .frame(popover_frame())
                    .align(egui::RectAlign::BOTTOM_START)
                    .show(|ui| {
                        ui.set_min_width(190.0);
                        for sort in PageSort::ALL {
                            if menu_row(
                                ui,
                                None,
                                t(locale, sort.key()),
                                None,
                                ui_state.sort_mode == sort,
                                true,
                                false,
                            )
                            .clicked()
                            {
                                action.set_sort = Some(sort);
                            }
                        }
                    });
                ui.add_space(8.0);

                if tree.roots().is_empty() {
                    empty_pages(ui, &mut action, vault.capsules.is_empty(), locale);
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

                    if let Some(dragged) = *dragged_page {
                        draw_root_drop_zone(ui, tree, dragged, &mut drop_target, locale);
                    }

                    handle_drag_release(ui, dragged_page, drop_target, &mut action);
                    // Recently visited pages (excluding the open one and the
                    // pins already shown above).
                    let recents: Vec<usize> = ui_state
                        .recent_page_ids
                        .iter()
                        .filter_map(|id| {
                            vault
                                .capsules
                                .iter()
                                .position(|capsule| &capsule.message_id == id)
                        })
                        .filter(|position| {
                            Some(*position) != selected
                                && !ui_state
                                    .pinned_ids
                                    .contains(&vault.capsules[*position].message_id)
                        })
                        .take(5)
                        .collect();
                    if !recents.is_empty() {
                        ui.add_space(10.0);
                        menu_header(ui, t(locale, Key::RecentPages));
                        for position in recents {
                            if draw_search_row(
                                ui,
                                &vault.capsules,
                                tree,
                                position,
                                selected,
                                0.0,
                                None,
                                search,
                                locale,
                            ) {
                                action.select = Some(position);
                            }
                        }
                    }
                }
            } else {
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
                rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                if rows.is_empty() {
                    ui.add_space(18.0);
                    ui.label(
                        RichText::new(format!(
                            "{} “{}”.",
                            t(locale, Key::NoMatches),
                            search.trim()
                        ))
                        .size(12.0)
                        .color(theme::muted()),
                    );
                    ui.add_space(6.0);
                    if text_button(ui, None, t(locale, Key::ClearSearch), false).clicked() {
                        search.clear();
                    }
                    // Create the page the search could not find: type a
                    // title, press Enter or click — the page exists and is
                    // open before the menu would have closed.
                    let query = search.trim().to_string();
                    if !query.is_empty() {
                        let label = format!(
                            "{} \u{201c}{}\u{201d}",
                            t(locale, Key::CreatePageNamed),
                            query
                        );
                        let create_clicked = text_button(ui, None, &label, false).clicked();
                        let create_enter = search_was_focused
                            && ui.input(|i| {
                                i.events.iter().any(|e| {
                                    matches!(
                                        e,
                                        egui::Event::Key {
                                            key: egui::Key::Enter,
                                            pressed: true,
                                            repeat: false,
                                            ..
                                        }
                                    )
                                })
                            });
                        if create_clicked || create_enter {
                            action.create_page_named = Some(query);
                            search.clear();
                        }
                    }
                } else {
                    let _ = hud(
                        ui,
                        &format!("{} · {}", rows.len(), t(locale, Key::Results)),
                        theme::faint(),
                    );
                    // While the search field owns the keyboard: Enter opens
                    // the top result, ↑/↓ walk the results with the cursor.
                    if search_was_focused {
                        let (enter, down, up) = ui.input(|i| {
                            (
                                i.key_pressed(egui::Key::Enter),
                                i.key_pressed(egui::Key::ArrowDown),
                                i.key_pressed(egui::Key::ArrowUp),
                            )
                        });
                        if (enter || down || up) && !rows.is_empty() {
                            let cursor_id = egui::Id::new("sidebar_search_cursor");
                            let mut cursor =
                                ui.data(|d| d.get_temp::<usize>(cursor_id)).unwrap_or(0);
                            if down {
                                cursor = (cursor + 1).min(rows.len() - 1);
                            }
                            if up {
                                cursor = cursor.saturating_sub(1);
                            }
                            cursor = cursor.min(rows.len() - 1);
                            ui.data_mut(|d| d.insert_temp(cursor_id, cursor));
                            action.select = Some(rows[cursor].0);
                        }
                    }
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
                            locale,
                        ) {
                            action.select = Some(*index);
                        }
                    }
                }
            }

            ui.ctx().data_mut(|data| {
                data.remove_temp::<bool>(egui::Id::unique("sidebar-should-scroll"));
            });
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
            .color(theme::faint()),
    );

    footer.add_space(2.0);
    let counts = if vault.load_errors.is_empty() {
        format!(
            "{} .eml · {}",
            vault.capsules.len(),
            i18n::page_range(
                locale,
                selected.map(|index| index + 1),
                vault.capsules.len()
            )
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
            .color(theme::muted()),
    );
    if !vault.load_errors.is_empty() {
        counts_label.on_hover_text(vault.load_errors.join("\n"));
    }
    footer.add_space(2.0);
    footer.label(
        RichText::new(format!(
            "{} {} · {} {}",
            words,
            t(locale, Key::WordsShort),
            threads,
            t(locale, Key::FooterThreads)
        ))
        .family(theme::mono())
        .size(9.5)
        .color(theme::faint()),
    );
    footer.add_space(8.0);
    footer.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if icon_button(ui, Icon::Download, t(locale, Key::ImportEml), true, false).clicked() {
            action.import = true;
        }
        if icon_button(ui, Icon::Copy, t(locale, Key::ExportVault), true, false).clicked() {
            action.export_vault = true;
        }
        if icon_button(
            ui,
            Icon::Trash,
            t(locale, Key::DeletePage),
            selected.is_some(),
            false,
        )
        .clicked()
        {
            action.delete_page = true;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if icon_button(ui, Icon::Folder, t(locale, Key::ChangeFolder), true, false).clicked() {
                action.open_vault = true;
            }
            if icon_button(
                ui,
                Icon::Reload,
                t(locale, Key::SyncFolderHint),
                true,
                false,
            )
            .clicked()
            {
                action.sync_folder = true;
            }
        });
    });

    action
}

fn vault_card(ui: &mut Ui, vault: &Vault) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
    ui.painter().rect(
        rect,
        theme::RADIUS,
        if response.hovered() {
            theme::wash_hover()
        } else {
            theme::wash()
        },
        theme::hairline(),
        egui::StrokeKind::Inside,
    );
    let tile = Rect::from_center_size(pos2(rect.left() + 22.0, rect.center().y), vec2(28.0, 28.0));
    ui.painter()
        .rect_filled(tile, theme::RADIUS, theme::fade(theme::accent(), 0.14));
    icons::paint(
        ui.painter(),
        Rect::from_center_size(tile.center(), vec2(15.0, 15.0)),
        Icon::Folder,
        theme::accent_ink(),
    );
    let (name, is_project) = crate::vault::get_vault_display_info(&vault.dir);
    let subtext = if is_project {
        "Project .capsule"
    } else {
        "Local .eml folder"
    };
    let display_title = if is_project {
        format!("📦 {name}")
    } else {
        name
    };
    let truncated_name = truncate_chars(&display_title, 24);
    ui.painter().text(
        pos2(tile.right() + 10.0, rect.center().y - 7.0),
        Align2::LEFT_CENTER,
        truncated_name,
        FontId::new(12.5, theme::semibold()),
        theme::ink(),
    );
    ui.painter().text(
        pos2(tile.right() + 10.0, rect.center().y + 8.0),
        Align2::LEFT_CENTER,
        subtext,
        FontId::new(10.0, theme::mono()),
        theme::faint(),
    );
    response
        .on_hover_text(vault.dir.display().to_string())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn search_box(ui: &mut Ui, search: &mut String, locale: Locale) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    ui.painter().rect(
        rect,
        theme::RADIUS,
        theme::wash(),
        theme::hairline(),
        egui::StrokeKind::Inside,
    );
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), vec2(13.0, 13.0)),
        Icon::Search,
        theme::faint(),
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
    let focus_search = ui
        .ctx()
        .data_mut(|data| data.remove_temp::<bool>(egui::Id::new("focus-sidebar-search")))
        .unwrap_or(false);
    let search_edit = TextEdit::singleline(search)
        .frame(Frame::NONE)
        .margin(Margin::same(0))
        .hint_text(t(locale, Key::SearchPagesHint))
        .desired_width(f32::INFINITY)
        .align(egui::Align2::LEFT_CENTER)
        .id(egui::Id::new("sidebar_search"));
    // Esc with the field focused empties the query first — muscle memory
    // from every search box; the (now unfocused) next Esc reaches the
    // global handlers. The field's focus must come from the PREVIOUS
    // frame's state: egui surrenders a text field's focus while still
    // processing an incoming Escape, so `memory.focused()` is already
    // empty by the time this frame draws.
    let had_focus_last_frame = ui
        .ctx()
        .data_mut(|d| d.remove_temp::<bool>(egui::Id::new("sidebar-search-focus")))
        .unwrap_or(false);
    // Probed unconditionally (no `was_focused` short-circuit): whether the
    // event survives in the raw list depends on egui internals, but if it
    // is visible at all it is visible before the TextEdit consumes it.
    let esc_pressed = ui.input(|i| {
        i.events.iter().any(|e| {
            matches!(
                e,
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    repeat: false,
                    ..
                }
            )
        })
    });
    let mut response = child.add(search_edit);
    if focus_search {
        response.request_focus();
    }
    // Read the flag before taking the context write lock: Response methods
    // go through the same RwLock, and nesting them deadlocks.
    let now_has_focus = response.has_focus();
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::new("sidebar-search-focus"), now_has_focus));
    if esc_pressed && had_focus_last_frame && !search.is_empty() {
        search.clear();
    }
    if !search.is_empty() {
        let clear_rect =
            Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(18.0, 18.0));
        let clear = ui.interact(
            clear_rect,
            egui::Id::new("sidebar_search_clear"),
            Sense::click(),
        );
        icons::paint(
            ui.painter(),
            clear_rect.shrink(5.0),
            Icon::Close,
            if clear.hovered() {
                theme::ink()
            } else {
                theme::faint()
            },
        );
        if clear.clicked() {
            search.clear();
        }
        clear
            .on_hover_text(t(locale, Key::ClearSearch))
            .on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    if response.has_focus() && !search.is_empty() && ui.input(|i| i.key_pressed(egui::Key::Escape))
    {
        search.clear();
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            theme::RADIUS,
            Stroke::new(1.0, theme::accent()),
            egui::StrokeKind::Inside,
        );
    }
    response
}

fn empty_pages(ui: &mut Ui, action: &mut ShellAction, no_pages: bool, locale: Locale) {
    ui.add_space(18.0);
    ui.label(
        RichText::new(if no_pages {
            t(locale, Key::PagesEmptyTitle)
        } else {
            t(locale, Key::PagesEmptyNative)
        })
        .size(12.0)
        .color(theme::muted()),
    );
    ui.add_space(8.0);
    if primary_button(ui, t(locale, Key::NewPage), 28.0).clicked() {
        action.new_page = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mime::Capsule;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn capsule(id: &str, subject: &str) -> Capsule {
        Capsule {
            message_id: id.into(),
            subject: subject.into(),
            from: String::new(),
            to: String::new(),
            date: String::new(),
            timestamp: 0,
            in_reply_to: None,
            parent_id: None,
            references: Vec::new(),
            tags: vec!["#plan".into()],
            status: String::new(),
            file_name: format!("{id}.eml"),
            blocks: vec![serde_json::json!({
                "id": format!("blk-{id}"),
                "type": "paragraph",
                "data": {"text": "body"}
            })],
            page_canvas: None,
            attachments: Vec::new(),
            native: true,
            parse_note: None,
            plain_body: None,
            html_body: None,
            raw_eml: None,
        }
    }

    /// Dragging a page row onto another page must emit a reparent. This
    /// guards the egui pitfall that silently killed the whole feature: while
    /// a drag is in flight, `hovered()` is suppressed for every widget
    /// except the one being dragged ("while dragging or clicking something,
    /// nothing else is hovered"), so drop targets have to be probed with
    /// `contains_pointer()`. Before the fix the pointer never "hovered" any
    /// row while dragging, `drop_target` stayed empty and every release was
    /// a no-op.
    #[test]
    fn dragging_a_page_onto_another_reparents_it() {
        let ctx = egui::Context::default();
        let vault = Vault {
            dir: std::path::PathBuf::new(),
            capsules: vec![capsule("msg-a", "Alpha"), capsule("msg-b", "Beta")],
            load_errors: Vec::new(),
        };
        let tree = PageTree::build(&vault.capsules);
        let b_index = 1;
        let action: Rc<RefCell<Option<ShellAction>>> = Rc::new(RefCell::new(None));
        let dragged: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));

        let run = {
            let ctx = ctx.clone();
            let action = action.clone();
            let dragged = dragged.clone();
            move |events: Vec<egui::Event>| {
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(260.0, 900.0),
                    )),
                    events,
                    ..Default::default()
                };
                let mut search = String::new();
                let mut collapsed: HashSet<String> = HashSet::new();
                ctx.run_pass(raw, |ctx| {
                    egui::Area::new(egui::Id::unique("sidebar-harness"))
                        .fixed_pos(egui::Pos2::ZERO)
                        .show(ctx, |ui| {
                            let mut act = draw_sidebar(
                                ui,
                                &vault,
                                &tree,
                                None,
                                &mut dragged.borrow_mut(),
                                &mut search,
                                &mut collapsed,
                                &crate::search::Bm25Index::default(),
                                &["".to_string(), "".to_string()],
                                false,
                                &crate::ui_state::UiState::default(),
                                0,
                                0,
                                Locale::En,
                            );
                            // Absorb, not overwrite: settle frames after the
                            // release must not erase the reparent.
                            let mut acc = action.borrow_mut();
                            match acc.as_mut() {
                                Some(existing) => existing.absorb(act),
                                None => *acc = Some(act),
                            }
                        });
                })
                .textures_delta
                .clear();
            }
        };

        run(vec![]);
        run(vec![]);
        let a = ctx
            .read_response(egui::Id::unique(("page-row", "msg-a".to_string())))
            .expect("row Alpha response")
            .rect;
        let b = ctx
            .read_response(egui::Id::unique(("page-row", "msg-b".to_string())))
            .expect("row Beta response")
            .rect;
        assert!(
            a.top() < b.top(),
            "roots must render in vault order: Alpha above Beta"
        );

        // Press on Beta, drag over Alpha, release: Beta becomes a child of
        // Alpha.
        run(vec![egui::Event::PointerMoved(b.center())]);
        run(vec![egui::Event::PointerButton {
            pos: b.center(),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        }]);
        run(vec![egui::Event::PointerMoved(a.center())]);
        run(vec![egui::Event::PointerMoved(a.center())]);
        run(vec![egui::Event::PointerButton {
            pos: a.center(),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        run(vec![]);
        let action = action.borrow_mut().take().expect("action");
        assert_eq!(
            action.reparent,
            Some((b_index, Some("msg-a".to_string()))),
            "dropping Beta onto Alpha must request a reparent"
        );
        assert_eq!(*dragged.borrow(), None, "drag state must clear on release");
    }

    /// The inverse drop — onto a page inside the dragged page's own subtree —
    /// must stay a rejected cycle and not emit a reparent.
    #[test]
    fn dropping_a_page_into_its_own_subtree_is_rejected() {
        let ctx = egui::Context::default();
        let mut vault = Vault {
            dir: std::path::PathBuf::new(),
            capsules: vec![capsule("msg-a", "Alpha"), capsule("msg-b", "Beta")],
            load_errors: Vec::new(),
        };
        vault.capsules[1].parent_id = Some("msg-a".to_string());
        let tree = PageTree::build(&vault.capsules);
        let action: Rc<RefCell<Option<ShellAction>>> = Rc::new(RefCell::new(None));
        let dragged: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));

        let run = {
            let ctx = ctx.clone();
            let action = action.clone();
            let dragged = dragged.clone();
            move |events: Vec<egui::Event>| {
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(260.0, 900.0),
                    )),
                    events,
                    ..Default::default()
                };
                let mut search = String::new();
                let mut collapsed: HashSet<String> = HashSet::new();
                ctx.run_pass(raw, |ctx| {
                    egui::Area::new(egui::Id::unique("sidebar-harness-cycle"))
                        .fixed_pos(egui::Pos2::ZERO)
                        .show(ctx, |ui| {
                            let mut act = draw_sidebar(
                                ui,
                                &vault,
                                &tree,
                                None,
                                &mut dragged.borrow_mut(),
                                &mut search,
                                &mut collapsed,
                                &crate::search::Bm25Index::default(),
                                &["".to_string(), "".to_string()],
                                false,
                                &crate::ui_state::UiState::default(),
                                0,
                                0,
                                Locale::En,
                            );
                            let mut acc = action.borrow_mut();
                            match acc.as_mut() {
                                Some(existing) => existing.absorb(act),
                                None => *acc = Some(act),
                            }
                        });
                })
                .textures_delta
                .clear();
            }
        };

        run(vec![]);
        run(vec![]);
        let a = ctx
            .read_response(egui::Id::unique(("page-row", "msg-a".to_string())))
            .expect("row Alpha response")
            .rect;
        let b = ctx
            .read_response(egui::Id::unique(("page-row", "msg-b".to_string())))
            .expect("row Beta response")
            .rect;
        // Beta is Alpha's child now, so it renders indented below Alpha.
        assert!(a.top() < b.top(), "child Beta renders below parent Alpha");

        run(vec![egui::Event::PointerMoved(a.center())]);
        run(vec![egui::Event::PointerButton {
            pos: a.center(),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        }]);
        run(vec![egui::Event::PointerMoved(b.center())]);
        run(vec![egui::Event::PointerButton {
            pos: b.center(),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        run(vec![]);
        let action = action.borrow_mut().take().expect("action");
        assert!(
            action.reparent.is_none(),
            "dropping a page into its own subtree must stay a no-op"
        );
        assert_eq!(*dragged.borrow(), None);
    }

    /// A drag that lost its release (the pointer left the window, a modal
    /// ate the button-up) must not stick: the tree gates row clicks on the
    /// drag state being empty, so a stuck `dragged_page` used to make every
    /// click dead until some later stray release moved a page.
    #[test]
    fn a_lost_release_does_not_stick_the_drag_state() {
        let ctx = egui::Context::default();
        let vault = Vault {
            dir: std::path::PathBuf::new(),
            capsules: vec![capsule("msg-a", "Alpha"), capsule("msg-b", "Beta")],
            load_errors: Vec::new(),
        };
        let tree = PageTree::build(&vault.capsules);
        let dragged: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));

        let run = {
            let ctx = ctx.clone();
            let dragged = dragged.clone();
            move |events: Vec<egui::Event>| {
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(260.0, 900.0),
                    )),
                    events,
                    ..Default::default()
                };
                let mut search = String::new();
                let mut collapsed: HashSet<String> = HashSet::new();
                ctx.run_pass(raw, |ctx| {
                    egui::Area::new(egui::Id::unique("sidebar-harness-stale"))
                        .fixed_pos(egui::Pos2::ZERO)
                        .show(ctx, |ui| {
                            let _ = draw_sidebar(
                                ui,
                                &vault,
                                &tree,
                                None,
                                &mut dragged.borrow_mut(),
                                &mut search,
                                &mut collapsed,
                                &crate::search::Bm25Index::default(),
                                &["".to_string(), "".to_string()],
                                false,
                                &crate::ui_state::UiState::default(),
                                0,
                                0,
                                Locale::En,
                            );
                        });
                })
                .textures_delta
                .clear();
            }
        };

        run(vec![]);
        // Simulate a drag whose release never arrived: the state points at a
        // page while no button is down.
        *dragged.borrow_mut() = Some(0);
        run(vec![]);
        assert_eq!(
            *dragged.borrow(),
            None,
            "a stale drag state must be cancelled on the next frame"
        );
    }

    /// Shared harness for the keyboard-driven search tests: the search
    /// string lives outside the frame so a test can type into it, and the
    /// focus flag ("focus-sidebar-search", consumed by search_box) puts the
    /// caret into the field for the frame after it is set.
    fn run_search_frame(
        ctx: &egui::Context,
        vault: &Vault,
        tree: &PageTree,
        action: &Rc<RefCell<Option<ShellAction>>>,
        search: &Rc<RefCell<String>>,
        events: Vec<egui::Event>,
    ) {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(260.0, 900.0),
            )),
            events,
            ..Default::default()
        };
        let mut collapsed: HashSet<String> = HashSet::new();
        ctx.run_pass(raw, |ctx| {
            egui::Area::new(egui::Id::unique("sidebar-harness-search"))
                .fixed_pos(egui::Pos2::ZERO)
                .show(ctx, |ui| {
                    let mut act = draw_sidebar(
                        ui,
                        vault,
                        tree,
                        None,
                        &mut None,
                        &mut search.borrow_mut(),
                        &mut collapsed,
                        &crate::search::Bm25Index::default(),
                        &["".to_string(), "".to_string()],
                        false,
                        &crate::ui_state::UiState::default(),
                        0,
                        0,
                        Locale::En,
                    );
                    let mut acc = action.borrow_mut();
                    match acc.as_mut() {
                        Some(existing) => existing.absorb(act),
                        None => *acc = Some(act),
                    }
                });
        })
        .textures_delta
        .clear();
    }

    /// Type a title that matches nothing, press Enter: the no-results state
    /// must offer (and Enter must fire) "create page" with that title, and
    /// the field must clear.
    #[test]
    fn enter_in_empty_search_requests_page_creation() {
        let ctx = egui::Context::default();
        let vault = Vault {
            dir: std::path::PathBuf::new(),
            capsules: vec![capsule("msg-a", "Alpha"), capsule("msg-b", "Beta")],
            load_errors: Vec::new(),
        };
        let tree = PageTree::build(&vault.capsules);
        let action: Rc<RefCell<Option<ShellAction>>> = Rc::new(RefCell::new(None));
        let search: Rc<RefCell<String>> = Rc::new(RefCell::new("xylophone".to_string()));

        ctx.data_mut(|d| d.insert_temp(egui::Id::new("focus-sidebar-search"), true));
        run_search_frame(&ctx, &vault, &tree, &action, &search, vec![]);
        // Focus lands on the frame after the flag is consumed.
        run_search_frame(
            &ctx,
            &vault,
            &tree,
            &action,
            &search,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        let action = action.borrow_mut().take().expect("action");
        assert_eq!(
            action.create_page_named.as_deref(),
            Some("xylophone"),
            "Enter over an empty result set must request page creation"
        );
        assert_eq!(search.borrow().as_str(), "", "the query must clear");
    }

    /// Esc with the field focused empties the query and must NOT create
    /// anything.
    #[test]
    fn escape_clears_a_focused_search_without_creating() {
        let ctx = egui::Context::default();
        let vault = Vault {
            dir: std::path::PathBuf::new(),
            capsules: vec![capsule("msg-a", "Alpha"), capsule("msg-b", "Beta")],
            load_errors: Vec::new(),
        };
        let tree = PageTree::build(&vault.capsules);
        let action: Rc<RefCell<Option<ShellAction>>> = Rc::new(RefCell::new(None));
        let search: Rc<RefCell<String>> = Rc::new(RefCell::new("alp".to_string()));

        ctx.data_mut(|d| d.insert_temp(egui::Id::new("focus-sidebar-search"), true));
        run_search_frame(&ctx, &vault, &tree, &action, &search, vec![]);
        run_search_frame(&ctx, &vault, &tree, &action, &search, vec![]);
        run_search_frame(
            &ctx,
            &vault,
            &tree,
            &action,
            &search,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        let action = action.borrow_mut().take().expect("action");
        assert!(action.create_page_named.is_none());
        assert_eq!(search.borrow().as_str(), "", "Esc must clear the query");
    }
}
