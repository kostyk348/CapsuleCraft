//! Command palette (Ctrl/Cmd+K).
//!
//! Ported from `src/components/CommandPalette.tsx`: fuzzy command scoring plus
//! BM25 over page titles, tags and block text. The palette is a pure overlay —
//! it returns the chosen `Command` and the shell performs it, so the overlay
//! never touches the vault.

use egui::{
    pos2, vec2, Align, Color32, FontId, Frame, Id, Key as EguiKey, Layout, Margin, Order, Rect,
    RichText, Sense, Stroke, TextEdit, Ui,
};

use crate::i18n::{t, Key, Locale};
use crate::icons::{self, Icon};
use crate::mime::{block_data_text, Capsule};
use crate::search::{self, PaletteCommand};
use crate::theme;
use crate::{hud, truncate_chars};

use super::View;

#[derive(Default)]
pub struct PaletteState {
    pub open: bool,
    pub query: String,
    pub cursor: usize,
    pub just_opened: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    NewPage,
    SavePage,
    Undo,
    Redo,
    ExportPage,
    ExportMarkdown,
    ExportVault,
    ImportEml,
    ImportObsidian,
    ToggleLocale,
    SetView(View),
    OpenPage(usize),
    DeletePage,
    ConnectMcp,
    CoThink,
    CoThinkMode(crate::cothinker::CoThinkMode),
    OpenImapSync,
    SyncImapNow,
    Faq,
    ExportEncrypted,
    ImportEncrypted,
    ToggleZenMode,
    ToggleSidebar,
    ZoomIn,
    ZoomOut,
    ResetZoom,
    ToggleWideCanvas,
    ToggleIconStyle,
}

pub fn commands(locale: Locale) -> Vec<(Command, PaletteCommand)> {
    let nav = |view: View| -> Vec<String> {
        vec![
            "go".into(),
            "view".into(),
            "navigation".into(),
            view.title(locale).to_lowercase(),
        ]
    };
    // Search keywords: English always, Russian aliases only in the RU locale
    // (the EN locale must stay 100% Latin — strict purism).
    let kw = |en: &[&str], ru: &[&str]| -> Vec<String> {
        let mut v: Vec<String> = en.iter().map(|s| s.to_string()).collect();
        if locale == Locale::Ru {
            v.extend(ru.iter().map(|s| s.to_string()));
        }
        v
    };
    vec![
        (
            Command::NewPage,
            PaletteCommand {
                id: "new_page",
                title: t(locale, Key::CommandNewPage).into(),
                hint: t(locale, Key::CommandNewPageHint).into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: vec!["new".into(), "page".into(), "create".into()],
                shortcut: Some("Ctrl N".into()),
            },
        ),
        (
            Command::SavePage,
            PaletteCommand {
                id: "save_page",
                title: t(locale, Key::SaveNow).into(),
                hint: t(locale, Key::MimeHint).into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: vec!["save".into(), "write".into()],
                shortcut: Some("Ctrl S".into()),
            },
        ),
        (
            Command::Undo,
            PaletteCommand {
                id: "undo",
                title: t(locale, Key::CommandUndo).into(),
                hint: String::new(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: vec!["undo".into(), "back".into()],
                shortcut: Some("Ctrl Z".into()),
            },
        ),
        (
            Command::Redo,
            PaletteCommand {
                id: "redo",
                title: t(locale, Key::CommandRedo).into(),
                hint: String::new(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: vec!["redo".into(), "forward".into()],
                shortcut: Some("Ctrl Shift Z".into()),
            },
        ),
        (
            Command::ImportEml,
            PaletteCommand {
                id: "import",
                title: t(locale, Key::CommandImport).into(),
                hint: String::new(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: vec!["import".into(), "email".into(), "eml".into()],
                shortcut: None,
            },
        ),
        (
            Command::ImportObsidian,
            PaletteCommand {
                id: "import_obsidian",
                title: t(locale, Key::CommandImportObsidian).into(),
                hint: "vault".into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: vec![
                    "import".into(),
                    "obsidian".into(),
                    "markdown".into(),
                    "md".into(),
                    "vault".into(),
                ],
                shortcut: None,
            },
        ),
        (
            Command::ExportPage,
            PaletteCommand {
                id: "export_page",
                title: t(locale, Key::CommandExportPage).into(),
                hint: String::new(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: vec!["export".into(), "eml".into(), "download".into()],
                shortcut: None,
            },
        ),
        (
            Command::ExportMarkdown,
            PaletteCommand {
                id: "export_markdown",
                title: t(locale, Key::CommandExportMarkdown).into(),
                hint: "md".into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: vec![
                    "export".into(),
                    "markdown".into(),
                    "md".into(),
                    "paste".into(),
                ],
                shortcut: None,
            },
        ),
        (
            Command::ExportVault,
            PaletteCommand {
                id: "export_vault",
                title: t(locale, Key::CommandExportVault).into(),
                hint: t(locale, Key::ZeroDb).into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: vec!["export".into(), "vault".into(), "folder".into()],
                shortcut: None,
            },
        ),
        (
            Command::DeletePage,
            PaletteCommand {
                id: "delete_page",
                title: t(locale, Key::DeletePage).into(),
                hint: String::new(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: vec!["delete".into(), "remove".into(), "page".into()],
                shortcut: None,
            },
        ),
        (
            Command::ToggleLocale,
            PaletteCommand {
                id: "locale",
                title: t(locale, Key::CommandLocale).into(),
                hint: format!("{} → {}", locale.label(), locale.other().label()),
                group: t(locale, Key::PaletteActions).into(),
                keywords: vec!["language".into(), "locale".into(), "ru".into(), "en".into()],
                shortcut: None,
            },
        ),
        (
            Command::ConnectMcp,
            PaletteCommand {
                id: "connect_mcp",
                title: t(locale, Key::ConnectMcp).into(),
                hint: "Model Context Protocol (Claude, Antigravity, Cursor)".into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: vec![
                    "mcp".into(),
                    "ai".into(),
                    "agent".into(),
                    "claude".into(),
                    "antigravity".into(),
                    "cursor".into(),
                    "llm".into(),
                ],
                shortcut: None,
            },
        ),
        (
            Command::SyncImapNow,
            PaletteCommand {
                id: "sync_imap_now",
                title: t(locale, Key::ImapSyncNow).into(),
                hint: "IMAP".into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: kw(
                    &["sync", "imap", "email", "mail"],
                    &["почта", "синхронизация"],
                ),
                shortcut: Some("Ctrl Shift S".into()),
            },
        ),
        (
            Command::Faq,
            PaletteCommand {
                id: "faq",
                title: t(locale, Key::Faq).into(),
                hint: t(locale, Key::FaqSubtitle).into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: kw(&["faq", "eml"], &["помощь", "вопросы"]),
                shortcut: None,
            },
        ),
        (
            Command::ExportEncrypted,
            PaletteCommand {
                id: "export_encrypted",
                title: t(locale, Key::EncryptCapsuleCmd).into(),
                hint: t(locale, Key::EncryptHint).into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: kw(
                    &["encrypt", "gpg", "openpgp", "asc", "password", "security"],
                    &["шифровать", "шифрование", "пароль"],
                ),
                shortcut: None,
            },
        ),
        (
            Command::ImportEncrypted,
            PaletteCommand {
                id: "import_encrypted",
                title: t(locale, Key::DecryptEmlCmd).into(),
                hint: t(locale, Key::EncryptHint).into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: kw(
                    &["decrypt", "gpg", "openpgp", "asc", "password"],
                    &["расшифровать"],
                ),
                shortcut: None,
            },
        ),
        (
            Command::ToggleZenMode,
            PaletteCommand {
                id: "zen_mode",
                title: t(locale, Key::CommandZenMode).into(),
                hint: "F11 / Esc".into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: kw(&["zen", "focus", "fullscreen"], &["дзэн", "фокус"]),
                shortcut: Some("F11".into()),
            },
        ),
        (
            Command::ToggleSidebar,
            PaletteCommand {
                id: "toggle_sidebar",
                title: t(locale, Key::CommandToggleSidebar).into(),
                hint: "Ctrl \\".into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: kw(&["sidebar", "toggle"], &["панель", "боковая"]),
                shortcut: Some("Ctrl \\".into()),
            },
        ),
        (
            Command::ZoomIn,
            PaletteCommand {
                id: "zoom_in",
                title: t(locale, Key::CommandZoomIn).into(),
                hint: "+10%".into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: kw(&["zoom", "in", "scale"], &["масштаб", "увеличить"]),
                shortcut: Some("Ctrl +".into()),
            },
        ),
        (
            Command::ZoomOut,
            PaletteCommand {
                id: "zoom_out",
                title: t(locale, Key::CommandZoomOut).into(),
                hint: "-10%".into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: kw(&["zoom", "out", "scale"], &["масштаб", "уменьшить"]),
                shortcut: Some("Ctrl -".into()),
            },
        ),
        (
            Command::ResetZoom,
            PaletteCommand {
                id: "reset_zoom",
                title: t(locale, Key::CommandResetZoom).into(),
                hint: "110%".into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: kw(&["zoom", "reset", "default"], &["масштаб", "сброс"]),
                shortcut: Some("Ctrl 0".into()),
            },
        ),
        (
            Command::ToggleWideCanvas,
            PaletteCommand {
                id: "wide_canvas",
                title: t(locale, Key::CommandWideCanvas).into(),
                hint: "Wide / Standard".into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: kw(&["canvas", "wide", "width"], &["холст", "широкий"]),
                shortcut: Some("Ctrl Shift W".into()),
            },
        ),
        (
            Command::ToggleIconStyle,
            PaletteCommand {
                id: "toggle_icon_style",
                title: t(locale, Key::CommandToggleIconStyle).into(),
                hint: "Mascot / Document".into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: kw(
                    &["icon", "brand", "mascot", "document"],
                    &["логотип", "иконка", "маскот"],
                ),
                shortcut: None,
            },
        ),
        (
            Command::OpenImapSync,
            PaletteCommand {
                id: "open_imap_sync",
                title: t(locale, Key::ImapSyncTitle).into(),
                hint: t(locale, Key::ImapSyncDesc).into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: kw(
                    &["imap", "email", "mail", "sync", "settings"],
                    &["почта", "настройки"],
                ),
                shortcut: None,
            },
        ),
        (
            Command::CoThink,
            PaletteCommand {
                id: "cothink",
                title: t(locale, Key::CoThinkTitle).into(),
                hint: t(locale, Key::CoThinkSubtitle).into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: kw(
                    &["cothink", "ai", "analysis", "offline"],
                    &["мыслитель", "анализ"],
                ),
                shortcut: Some("Ctrl Shift A".into()),
            },
        ),
        (
            Command::CoThinkMode(crate::cothinker::CoThinkMode::Critique),
            PaletteCommand {
                id: "cothink_critique",
                title: format!(
                    "{}: {}",
                    t(locale, Key::CoThinkTitle),
                    t(locale, Key::Critique)
                ),
                hint: t(locale, Key::CritiqueDesc).into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: kw(&["critique", "criticism"], &["критика", "риски"]),
                shortcut: None,
            },
        ),
        (
            Command::CoThinkMode(crate::cothinker::CoThinkMode::Synthesis),
            PaletteCommand {
                id: "cothink_synthesis",
                title: format!(
                    "{}: {}",
                    t(locale, Key::CoThinkTitle),
                    t(locale, Key::Synthesis)
                ),
                hint: t(locale, Key::SynthesisDesc).into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: kw(&["synthesis"], &["синтез", "шаги", "выводы"]),
                shortcut: None,
            },
        ),
        (
            Command::CoThinkMode(crate::cothinker::CoThinkMode::CounterArg),
            PaletteCommand {
                id: "cothink_counterarg",
                title: format!(
                    "{}: {}",
                    t(locale, Key::CoThinkTitle),
                    t(locale, Key::CounterArg)
                ),
                hint: t(locale, Key::CounterArgDesc).into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: kw(
                    &["counter", "counterarg", "reply"],
                    &["контраргумент", "ответ", "тред"],
                ),
                shortcut: None,
            },
        ),
        (
            Command::CoThinkMode(crate::cothinker::CoThinkMode::DataInsight),
            PaletteCommand {
                id: "cothink_data",
                title: format!(
                    "{}: {}",
                    t(locale, Key::CoThinkTitle),
                    t(locale, Key::DataInsight)
                ),
                hint: t(locale, Key::DataInsightDesc).into(),
                group: t(locale, Key::PaletteActions).into(),
                keywords: kw(&["data", "csv", "ols", "trend"], &["данные", "тренд"]),
                shortcut: None,
            },
        ),
        (
            Command::SetView(View::Page),
            PaletteCommand {
                id: "view_page",
                title: View::Page.title(locale).into(),
                hint: t(locale, Key::NavPage).into(),
                group: t(locale, Key::PaletteNavigation).into(),
                keywords: nav(View::Page),
                shortcut: None,
            },
        ),
        (
            Command::SetView(View::Planning),
            PaletteCommand {
                id: "view_planning",
                title: View::Planning.title(locale).into(),
                hint: t(locale, Key::PlanningTitle).into(),
                group: t(locale, Key::PaletteNavigation).into(),
                keywords: nav(View::Planning),
                shortcut: None,
            },
        ),
        (
            Command::SetView(View::Graph),
            PaletteCommand {
                id: "view_graph",
                title: View::Graph.title(locale).into(),
                hint: t(locale, Key::GraphTitle).into(),
                group: t(locale, Key::PaletteNavigation).into(),
                keywords: nav(View::Graph),
                shortcut: None,
            },
        ),
        (
            Command::SetView(View::Threads),
            PaletteCommand {
                id: "view_threads",
                title: View::Threads.title(locale).into(),
                hint: t(locale, Key::ThreadsTitle).into(),
                group: t(locale, Key::PaletteNavigation).into(),
                keywords: nav(View::Threads),
                shortcut: None,
            },
        ),
        (
            Command::SetView(View::Mime),
            PaletteCommand {
                id: "view_mime",
                title: View::Mime.title(locale).into(),
                hint: t(locale, Key::MimeTitle).into(),
                group: t(locale, Key::PaletteNavigation).into(),
                keywords: nav(View::Mime),
                shortcut: None,
            },
        ),
        (
            Command::SetView(View::Mail),
            PaletteCommand {
                id: "view_mail",
                title: View::Mail.title(locale).into(),
                hint: t(locale, Key::ExternalPreview).into(),
                group: t(locale, Key::PaletteNavigation).into(),
                keywords: nav(View::Mail),
                shortcut: None,
            },
        ),
    ]
}

enum Row {
    Command(Command),
    Page(usize),
}

/// Draw the overlay. Returns a command when the user confirms one.
pub fn draw(
    ctx: &egui::Context,
    capsules: &[Capsule],
    locale: Locale,
    state: &mut PaletteState,
) -> Option<Command> {
    if !state.open {
        return None;
    }

    let screen = ctx.content_rect();
    let width = 560.0_f32.min(screen.width() - 40.0);
    let top = screen.top() + (screen.height() * 0.16).min(140.0);
    let origin = pos2(screen.center().x - width / 2.0, top);

    let mut chosen: Option<Command> = None;
    let mut close = false;

    // Scrim: subtle darkening behind palette so document text remains readable
    // without completely blacking out the app.
    egui::Area::new(Id::new("palette-scrim"))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, response) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_black_alpha(45));
            if response.clicked() {
                close = true;
            }
        });

    egui::Area::new(Id::new("palette"))
        .order(Order::Foreground)
        .fixed_pos(origin)
        .show(ctx, |ui| {
            ui.set_width(width);
            Frame::new()
                .fill(theme::PANEL)
                .corner_radius(theme::RADIUS)
                .stroke(Stroke::new(1.0, theme::LINE_STRONG))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 18],
                    blur: 44,
                    spread: 0,
                    color: Color32::from_black_alpha(160),
                })
                .inner_margin(Margin::same(0))
                .show(ui, |ui| {
                    ui.set_width(width);
                    // Query row.
                    let (row_rect, _) = ui.allocate_exact_size(vec2(width, 46.0), Sense::hover());
                    ui.painter().hline(
                        row_rect.x_range(),
                        row_rect.bottom() - 0.5,
                        theme::hairline(),
                    );
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(
                            pos2(row_rect.left() + 18.0, row_rect.center().y),
                            vec2(14.0, 14.0),
                        ),
                        Icon::Search,
                        theme::FAINT,
                    );
                    let mut input = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(row_rect.shrink2(vec2(38.0, 8.0)))
                            .layout(Layout::left_to_right(Align::Center)),
                    );
                    let response = input.add_sized(
                        vec2(width - 90.0, 28.0),
                        TextEdit::singleline(&mut state.query)
                            .frame(Frame::NONE)
                            .margin(Margin::same(0))
                            .font(FontId::new(14.0, egui::FontFamily::Proportional))
                            .hint_text(t(locale, Key::PalettePlaceholder))
                            .desired_width(f32::INFINITY),
                    );
                    if state.just_opened {
                        response.request_focus();
                        state.just_opened = false;
                    }
                    input.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let _ = hud(ui, "ESC", theme::FAINT);
                    });

                    // Rank commands and pages.
                    let all_commands = commands(locale);
                    let command_list: Vec<PaletteCommand> =
                        all_commands.iter().map(|(_, c)| c.clone()).collect();
                    let ranked = search::rank_commands(&state.query, &command_list);
                    let command_rows: Vec<(i32, Command)> = ranked
                        .into_iter()
                        .map(|(cmd, score)| {
                            let command = all_commands
                                .iter()
                                .find(|(_, c)| c.id == cmd.id)
                                .map(|(command, _)| command.clone())
                                .unwrap_or(Command::NewPage);
                            (score, command)
                        })
                        .collect();

                    let index = build_index(capsules);
                    let hits = index.rank(&state.query);
                    let page_rows: Vec<(usize, f32)> = hits
                        .iter()
                        .filter_map(|hit| {
                            capsules
                                .iter()
                                .position(|capsule| capsule.message_id == hit.id)
                                .map(|position| (position, hit.score))
                        })
                        .collect();

                    let mut rows: Vec<Row> = Vec::new();
                    let query_empty = state.query.trim().is_empty();
                    if !query_empty {
                        for (_, command) in command_rows.iter().take(4) {
                            rows.push(Row::Command(command.clone()));
                        }
                    } else {
                        // Empty query: offer the actions first, pages after.
                        for (_, command) in command_rows.iter().take(6) {
                            rows.push(Row::Command(command.clone()));
                        }
                    }
                    for (position, _) in page_rows.iter().take(9 - rows.len().min(9)) {
                        rows.push(Row::Page(*position));
                    }

                    if rows.is_empty() {
                        ui.add_space(18.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new(t(locale, Key::PaletteEmpty))
                                    .size(12.0)
                                    .color(theme::FAINT),
                            );
                        });
                        ui.add_space(18.0);
                        return;
                    }

                    state.cursor = state.cursor.min(rows.len().saturating_sub(1));

                    // Keyboard.
                    let (down, up, enter, esc) = ctx.input(|i| {
                        (
                            i.key_pressed(EguiKey::ArrowDown),
                            i.key_pressed(EguiKey::ArrowUp),
                            i.key_pressed(EguiKey::Enter),
                            i.key_pressed(EguiKey::Escape),
                        )
                    });
                    if down {
                        state.cursor = (state.cursor + 1) % rows.len();
                    }
                    if up {
                        state.cursor = (state.cursor + rows.len() - 1) % rows.len();
                    }
                    if esc {
                        close = true;
                    }
                    if enter {
                        chosen = match &rows[state.cursor] {
                            Row::Command(command) => Some(command.clone()),
                            Row::Page(index) => Some(Command::OpenPage(*index)),
                        };
                    }

                    let mut last_group: Option<String> = None;
                    for (position, row) in rows.iter().enumerate() {
                        let group = match row {
                            Row::Command(command) => all_commands
                                .iter()
                                .find(|(c, _)| c == command)
                                .map(|(_, meta)| meta.group.clone()),
                            Row::Page(_) => Some(t(locale, Key::PalettePages).to_string()),
                        };
                        if group != last_group {
                            if let Some(group) = &group {
                                let mut header = ui.new_child(
                                    egui::UiBuilder::new()
                                        .max_rect(Rect::from_min_size(
                                            pos2(ui.cursor().min.x + 14.0, ui.cursor().min.y + 4.0),
                                            vec2(width - 28.0, 20.0),
                                        ))
                                        .layout(Layout::left_to_right(Align::Center)),
                                );
                                let _ = hud(&mut header, group, theme::FAINT);
                                ui.add_space(20.0);
                            }
                            last_group = group;
                        }
                        let active = position == state.cursor;
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(width, 40.0), Sense::click());
                        if active {
                            ui.painter().rect_filled(
                                rect.shrink2(vec2(6.0, 0.0)),
                                theme::RADIUS,
                                theme::WASH_HOVER,
                            );
                        } else if response.hovered() {
                            ui.painter().rect_filled(
                                rect.shrink2(vec2(6.0, 0.0)),
                                theme::RADIUS,
                                theme::WASH,
                            );
                            state.cursor = position;
                        }
                        match row {
                            Row::Command(command) => {
                                let meta = all_commands
                                    .iter()
                                    .find(|(c, _)| c == command)
                                    .map(|(_, meta)| meta);
                                let (title, hint, shortcut) = meta
                                    .map(|meta| {
                                        (
                                            meta.title.clone(),
                                            meta.hint.clone(),
                                            meta.shortcut.clone(),
                                        )
                                    })
                                    .unwrap_or_default();
                                command_row(ui, rect, &title, &hint, shortcut.as_deref(), active);
                            }
                            Row::Page(index) => {
                                let capsule = &capsules[*index];
                                page_row(ui, rect, capsule, &state.query, active);
                            }
                        }
                        if response.clicked() {
                            chosen = match row {
                                Row::Command(command) => Some(command.clone()),
                                Row::Page(index) => Some(Command::OpenPage(*index)),
                            };
                        }
                    }
                    ui.add_space(4.0);
                });
        });

    if chosen.is_some() || close {
        state.open = false;
        state.query.clear();
        state.cursor = 0;
    }
    chosen
}

fn command_row(
    ui: &mut Ui,
    rect: Rect,
    title: &str,
    hint: &str,
    shortcut: Option<&str>,
    active: bool,
) {
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(16.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui = &mut row;
    ui.spacing_mut().item_spacing.x = 8.0;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(RichText::new(title).size(12.5).color(if active {
            theme::INK
        } else {
            theme::MUTED
        }));
        if !hint.is_empty() {
            let _ = hud(ui, &truncate_chars(hint, 64), theme::FAINT);
        }
    });
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if let Some(shortcut) = shortcut {
            let (rect, _) = ui.allocate_exact_size(
                vec2(shortcut.chars().count() as f32 * 6.0 + 12.0, 16.0),
                Sense::hover(),
            );
            ui.painter().rect(
                rect,
                theme::RADIUS,
                theme::SUNKEN,
                theme::hairline(),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                shortcut,
                FontId::new(9.0, theme::mono()),
                theme::FAINT,
            );
        }
    });
}

fn page_row(ui: &mut Ui, rect: Rect, capsule: &Capsule, query: &str, active: bool) {
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(16.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui = &mut row;
    ui.spacing_mut().item_spacing.x = 8.0;
    icons::paint(
        ui.painter(),
        Rect::from_center_size(
            pos2(ui.cursor().min.x + 6.0, rect.center().y),
            vec2(12.0, 12.0),
        ),
        Icon::Doc,
        if active {
            theme::ACCENT_INK
        } else {
            theme::FAINT
        },
    );
    ui.add_space(18.0);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add(
            egui::Label::new(
                RichText::new(if capsule.subject.is_empty() {
                    "Untitled".to_string()
                } else {
                    capsule.subject.clone()
                })
                .size(12.5)
                .color(if active { theme::INK } else { theme::MUTED }),
            )
            .truncate(),
        );
        let snippet = search::highlight_matches(&truncate_chars(&preview_text(capsule), 78), query);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (text, hit) in snippet {
                ui.label(RichText::new(text).size(10.0).color(if hit {
                    theme::ACCENT_INK
                } else {
                    theme::FAINT
                }));
            }
        });
    });
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if !capsule.native {
            let _ = hud(ui, t(Locale::En, Key::ReadOnly), theme::BADGE_BETA);
        }
        let _ = hud(ui, &capsule.date, theme::FAINT);
    });
}

fn preview_text(capsule: &Capsule) -> String {
    let mut text = String::new();
    for block in capsule.blocks.iter().take(4) {
        let line = block_data_text(block);
        if !line.is_empty() {
            text.push_str(&line);
            text.push(' ');
        }
    }
    if text.trim().is_empty() {
        capsule
            .plain_body
            .clone()
            .or_else(|| capsule.html_body.clone())
            .unwrap_or_default()
    } else {
        text
    }
}

fn build_index(capsules: &[Capsule]) -> search::Bm25Index {
    let mut index = search::Bm25Index::default();
    for capsule in capsules {
        let blocks: String = capsule
            .blocks
            .iter()
            .map(block_data_text)
            .collect::<Vec<_>>()
            .join(" ");
        let attachments: String = capsule
            .attachments
            .iter()
            .map(|attachment| attachment.name.clone())
            .collect::<Vec<_>>()
            .join(" ");
        index.add(
            &capsule.message_id,
            search::capsule_tokens(&capsule.subject, &capsule.tags, &blocks, &attachments),
        );
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_cyrillic(text: &str) -> bool {
        text.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
    }

    #[test]
    fn english_palette_is_cyrillic_free() {
        // Strict EN purism: titles, hints and search keywords must never
        // leak Russian when the locale is English.
        for (command, palette) in commands(Locale::En) {
            let _ = command;
            for text in [
                palette.title.as_str(),
                palette.hint.as_str(),
                palette.group.as_str(),
            ] {
                assert!(
                    !has_cyrillic(text),
                    "EN palette text contains Cyrillic: {text}"
                );
            }
            for keyword in &palette.keywords {
                assert!(
                    !has_cyrillic(keyword),
                    "EN palette keyword contains Cyrillic: {keyword}"
                );
            }
        }
    }

    #[test]
    fn russian_palette_keeps_its_aliases() {
        // RU keeps both English keywords and Cyrillic aliases.
        let cyr = commands(Locale::Ru)
            .iter()
            .flat_map(|(_, p)| p.keywords.iter().cloned())
            .any(|k| has_cyrillic(&k));
        assert!(cyr, "RU locale should still carry Cyrillic aliases");
    }
}
