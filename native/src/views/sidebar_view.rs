use std::collections::HashSet;

use egui::{
    pos2, vec2, Align, Align2, Color32, FontFamily, FontId, Frame, Layout, Margin, Rect, Response,
    RichText, ScrollArea, Sense, Stroke, TextEdit, Ui, UiBuilder,
};

use crate::caps;
use crate::folder_name;
use crate::hud;
use crate::i18n::{self, t, Key, Locale};
use crate::icon_button;
use crate::icons::{self, Icon};
use crate::mime::Capsule;
use crate::primary_button;
use crate::search;
use crate::text_button;
use crate::theme;
use crate::truncate_chars;
use crate::ui_state::{PageSort, UiState};
use crate::vault::{PageTree, Vault};
use crate::{menu_header, menu_row, menu_separator, popover_frame, ShellAction};

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

    // Two-way folder sync without any mail account: previously reachable only
    // through the tiny footer icon, which users could not find.
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

// ---------------------------------------------------------------------------
// Shell: sidebar
// ---------------------------------------------------------------------------

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

        // Subfolders toggle button (collapse/expand all subfolders)
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

        // Project sync button
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

                    // Promote to root drop zone at the bottom of the list
                    if let Some(dragged) = *dragged_page {
                        ui.add_space(8.0);
                        let (root_drop_rect, root_drop_resp) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
                        let is_already_root = tree.parent_of(dragged).is_none();
                        let hovered = root_drop_resp.hovered() && !is_already_root;

                        ui.painter().rect_stroke(
                            root_drop_rect,
                            theme::RADIUS,
                            Stroke::new(
                                1.0,
                                if hovered {
                                    theme::accent()
                                } else {
                                    theme::fade(theme::line(), 0.4)
                                },
                            ),
                            egui::StrokeKind::Inside,
                        );

                        if hovered {
                            ui.painter().rect_filled(
                                root_drop_rect,
                                theme::RADIUS,
                                theme::fade(theme::accent(), 0.1),
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
                                theme::accent()
                            } else {
                                theme::faint()
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
                } else {
                    let _ = hud(
                        ui,
                        &format!("{} · {}", rows.len(), t(locale, Key::Results)),
                        theme::faint(),
                    );
                    // While the search field owns the keyboard: Enter opens
                    // the top result, ↑/↓ walk the results with the cursor.
                    let search_focused =
                        ui.memory(|m| m.focused()) == Some(egui::Id::new("sidebar_search"));
                    if search_focused {
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
    // Ctrl+F from the shell lands here: focus the field once, then drop the
    // flag so the field is not re-focused on every frame.
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
    let mut response = child.add(search_edit);
    if focus_search {
        response.request_focus();
    }
    // One-click clear instead of backspacing through the whole query; Esc
    // clears too while the field is focused.
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

        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click_and_drag());

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
                Stroke::new(1.0, theme::fade(theme::line(), 0.4)),
                egui::StrokeKind::Inside,
            );
        } else if is_valid_drop_target {
            ui.painter().rect_stroke(
                rect,
                theme::RADIUS,
                Stroke::new(1.5, theme::accent()),
                egui::StrokeKind::Inside,
            );
            ui.painter()
                .rect_filled(rect, theme::RADIUS, theme::fade(theme::accent(), 0.15));
        } else if is_cycle_target {
            ui.painter().rect_stroke(
                rect,
                theme::RADIUS,
                Stroke::new(1.5, theme::bad()),
                egui::StrokeKind::Inside,
            );
            ui.painter()
                .rect_filled(rect, theme::RADIUS, theme::fade(theme::bad(), 0.15));
        } else if is_selected {
            ui.painter()
                .rect_filled(rect, theme::RADIUS, theme::wash_selected());
            ui.painter().rect_filled(
                Rect::from_min_size(pos2(rect.left() - 7.0, rect.top() + 7.0), vec2(2.0, 16.0)),
                0.0,
                Color32::WHITE,
            );
        } else if response.hovered() {
            ui.painter().rect_filled(rect, theme::RADIUS, theme::wash());
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
                theme::faint(),
            );
            toggle = chev.clicked();
        }

        let ink = if is_selected || response.hovered() || is_valid_drop_target {
            Color32::WHITE
        } else if is_dragged {
            theme::faint()
        } else {
            theme::sidebar_ink()
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
        let title = if capsule.subject.trim().is_empty() {
            t(locale, Key::Untitled).to_string()
        } else {
            capsule.subject.clone()
        };
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
        ui.painter().with_clip_rect(clip).text(
            pos2(title_x, rect.center().y),
            Align2::LEFT_CENTER,
            &title,
            font,
            ink,
        );

        if !capsule.native {
            icons::paint(
                ui.painter(),
                Rect::from_center_size(
                    pos2(rect.right() - 12.0, rect.center().y),
                    vec2(11.0, 11.0),
                ),
                Icon::Lock,
                theme::badge_beta(),
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
                painter.rect_filled(preview_rect, theme::RADIUS, theme::wash_selected());
                painter.rect_stroke(
                    preview_rect,
                    theme::RADIUS,
                    Stroke::new(1.0, theme::accent()),
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
            response.on_hover_text(format!(
                "{} «{}»",
                t(locale, Key::MakeChildOf),
                capsule.subject
            ))
        } else if !capsule.native {
            response.on_hover_text(format!("{} (read-only)", capsule.subject))
        } else {
            response.on_hover_text(&capsule.subject)
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
    locale: Locale,
) -> bool {
    let capsule = &capsules[index];
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::click());
    let is_selected = selected == Some(index);
    if is_selected {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, theme::wash_selected());
        ui.painter().rect_filled(
            Rect::from_min_size(pos2(rect.left() - 7.0, rect.top() + 10.0), vec2(2.0, 18.0)),
            0.0,
            Color32::WHITE,
        );
    } else if response.hovered() {
        ui.painter().rect_filled(rect, theme::RADIUS, theme::wash());
    }
    let ink = if is_selected || response.hovered() {
        Color32::WHITE
    } else {
        theme::sidebar_ink()
    };
    icons::paint(
        ui.painter(),
        Rect::from_center_size(
            pos2(rect.left() + 15.0, rect.top() + 16.0),
            vec2(13.0, 13.0),
        ),
        Icon::Doc,
        ink,
    );
    let title = if capsule.subject.trim().is_empty() {
        t(locale, Key::Untitled).to_string()
    } else {
        capsule.subject.clone()
    };
    let title_clip = Rect::from_min_max(
        pos2(rect.left() + 30.0, rect.top()),
        pos2(rect.right() - 34.0, rect.top() + 24.0),
    );
    ui.painter().with_clip_rect(title_clip).text(
        pos2(rect.left() + 30.0, rect.top() + 15.0),
        Align2::LEFT_CENTER,
        &title,
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
                theme::ink()
            }
        } else {
            theme::faint()
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
        theme::faint(),
    );
    if !capsule.native {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(
                pos2(rect.right() - 12.0, rect.top() + 30.0),
                vec2(11.0, 11.0),
            ),
            Icon::Lock,
            theme::badge_beta(),
        );
    }
    let response = if capsule.native {
        response.on_hover_text(&capsule.subject)
    } else {
        response.on_hover_text(format!("{} (read-only)", capsule.subject))
    };
    response.clicked()
}
