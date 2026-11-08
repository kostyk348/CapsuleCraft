//! Sidebar view: vault navigation, page hierarchy tree, drag-and-drop, and full-text search.

pub mod dnd;
pub mod tree;

use std::collections::HashSet;

use egui::{
    pos2, vec2, Align, Align2, FontId, Frame, Layout, Margin, Rect, Response, RichText,
    ScrollArea, Sense, Stroke, TextEdit, Ui, UiBuilder,
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
use crate::vault::{PageTree, Vault};
use crate::{menu_header, menu_row, menu_separator, popover_frame, ShellAction};

pub use dnd::{draw_drag_preview, draw_root_drop_zone, handle_drag_release};
pub use tree::{draw_page_nodes, draw_search_row};

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
    locale: Locale,
) -> ShellAction {
    let mut action = ShellAction::default();
    let outer = ui.max_rect();
    ui.allocate_rect(outer, Sense::hover());
    ui.set_min_width(outer.width());
    let inner = outer.shrink2(vec2(14.0, 8.0));
    let header_h = 122.0;
    let footer_h = 122.0;

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
                } else {
                    let _ = hud(
                        ui,
                        &format!("{} · {}", rows.len(), t(locale, Key::Results)),
                        theme::faint(),
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
    let mut response = child.add(search_edit);
    if focus_search {
        response.request_focus();
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
