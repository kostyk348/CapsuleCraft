//! MCP connect configuration modal dialog.

use std::path::{Path, PathBuf};

use egui::{
    vec2, Align, Color32, FontFamily, Frame, Label, Layout, Margin, Rect, RichText, ScrollArea,
    Sense, Stroke, UiBuilder,
};

use crate::i18n::{t, Key, Locale};
use crate::icons::Icon;
use crate::mcp;
use crate::secondary_button;
use crate::theme;

pub enum McpModalAction {
    None,
    Close,
    CopyConfig(String),
}

pub fn draw_mcp_modal(
    show: bool,
    mcp_tab: &mut usize,
    vault_dir: &Path,
    locale: Locale,
    ctx: &egui::Context,
) -> McpModalAction {
    if !show {
        return McpModalAction::None;
    }

    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("capsuleos-native.exe"));
    let cfg = mcp::mcp_config_json(&exe, vault_dir);
    let cfg_str = serde_json::to_string_pretty(&cfg).unwrap_or_default();
    let claude_cmd = format!(
        "claude mcp add capsulecraft \"{}\" --mcp \"{}\"",
        exe.display(),
        vault_dir.display()
    );

    let mut action = McpModalAction::None;

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        action = McpModalAction::Close;
    }

    egui::Area::new(egui::Id::new("mcp-connect-modal"))
        .order(egui::Order::Foreground)
        .fixed_pos(ctx.content_rect().min)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(ctx.content_rect().size(), Sense::click());
            if resp.clicked() {
                action = McpModalAction::Close;
            }
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(150));

            let panel = Rect::from_center_size(rect.center(), vec2(580.0, 420.0));
            ui.painter().rect(
                panel,
                theme::RADIUS,
                theme::PANEL,
                Stroke::new(1.0, theme::LINE_STRONG),
                egui::StrokeKind::Inside,
            );

            let mut body = ui.new_child(
                UiBuilder::new()
                    .max_rect(panel.shrink(20.0))
                    .layout(Layout::top_down(Align::Min)),
            );

            body.label(
                RichText::new(t(locale, Key::McpTitle))
                    .family(theme::medium())
                    .size(15.0)
                    .color(theme::INK),
            );
            body.add_space(4.0);
            body.add(
                Label::new(
                    RichText::new(t(locale, Key::McpSubtitle))
                        .size(12.0)
                        .color(theme::MUTED),
                )
                .wrap(),
            );
            body.add_space(12.0);

            // Tabs for clients
            body.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let tabs = [
                    (0, t(locale, Key::McpTabAntigravity)),
                    (1, t(locale, Key::McpTabClaudeDesktop)),
                    (2, t(locale, Key::McpTabClaudeCode)),
                    (3, t(locale, Key::McpTabCursor)),
                ];
                for (idx, label) in tabs {
                    let active = *mcp_tab == idx;
                    let bg = if active {
                        theme::CONTROLS
                    } else {
                        Color32::TRANSPARENT
                    };
                    let frame = Frame::new()
                        .fill(bg)
                        .corner_radius(theme::RADIUS)
                        .inner_margin(Margin::symmetric(10, 5));
                    let resp = frame
                        .show(ui, |ui| {
                            let text_color = if active { theme::INK } else { theme::MUTED };
                            ui.label(
                                RichText::new(label)
                                    .family(theme::medium())
                                    .size(12.0)
                                    .color(text_color),
                            )
                        })
                        .response;
                    if resp.interact(Sense::click()).clicked() {
                        *mcp_tab = idx;
                    }
                }
            });

            body.add_space(10.0);

            let (desc, content_to_copy) = match *mcp_tab {
                0 => (
                    "Config location: ~/.gemini/antigravity/mcp/ or AGY workspace mcp.json",
                    cfg_str.clone(),
                ),
                1 => (
                    "Config location: %APPDATA%\\Claude\\claude_desktop_config.json",
                    cfg_str.clone(),
                ),
                2 => (
                    "Terminal command to register the MCP server with Claude CLI:",
                    claude_cmd.clone(),
                ),
                _ => ("Config location: .cursor/mcp.json", cfg_str.clone()),
            };

            body.label(RichText::new(desc).size(11.0).color(theme::FAINT));
            body.add_space(6.0);

            let text_frame = Frame::new()
                .fill(theme::SUNKEN)
                .corner_radius(theme::RADIUS)
                .stroke(Stroke::new(1.0, theme::LINE))
                .inner_margin(Margin::same(10));

            text_frame.show(&mut body, |ui| {
                ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                    ui.add(
                        Label::new(
                            RichText::new(&content_to_copy)
                                .family(FontFamily::Monospace)
                                .size(11.0)
                                .color(theme::INK),
                        )
                        .wrap(),
                    );
                });
            });

            body.add_space(14.0);

            body.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if secondary_button(ui, Some(Icon::Close), t(locale, Key::Close), 28.0).clicked() {
                    action = McpModalAction::Close;
                }
                ui.add_space(8.0);
                if secondary_button(ui, Some(Icon::Copy), t(locale, Key::CopyConfig), 28.0)
                    .clicked()
                {
                    action = McpModalAction::CopyConfig(content_to_copy.clone());
                }
            });
        });

    action
}
