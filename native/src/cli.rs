//! Command line for the native shell.
//!
//! The app is still "double-click to open your vault": a single positional
//! argument is the vault folder. The flags exist so the same binary can render
//! deterministic screenshots for the README and for visual regressions without
//! any window-manager automation.

use std::path::PathBuf;

use crate::i18n::Locale;
use crate::views::View;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Launch {
    /// Vault folder; `None` means "the default vault".
    pub vault: Option<PathBuf>,
    /// Where `--snapshot` writes its PNG.
    pub snapshot: Option<PathBuf>,
    /// View to render before capturing.
    pub view: Option<View>,
    /// Page index (sidebar order) to select before capturing.
    pub page: Option<usize>,
    /// Frames to draw before the capture, so layout and fonts settle.
    pub frames: u32,
    /// Locale override (otherwise the saved / OS default is used).
    pub locale: Option<Locale>,
    /// Pre-filled sidebar search, so a snapshot can show ranked results.
    pub search: Option<String>,
    /// Force the onboarding tour open (first-run docs and screenshots).
    pub onboarding: bool,
    /// Pixels to scroll the document before capturing (screenshots only).
    pub scroll: Option<f32>,
    /// Run as a Model Context Protocol (MCP) server over stdin/stdout.
    pub mcp: bool,
    /// Open a modal on startup (`--modal integrations`) — screenshots of the
    /// dialogs without any window-manager automation.
    pub modal: Option<String>,
    /// Replace the vault with N synthetic stress pages and open the graph.
    pub stress_graph: Option<usize>,
    /// Initial graph view zoom (`--zoom 0.2` reaches the cluster LOD band).
    pub zoom: Option<f32>,
    /// Window size override in logical points (`--window-size 980x640`),
    /// snapshot-only: lets a capture target the app's minimum window.
    pub window_size: Option<[f32; 2]>,
    /// Import an Obsidian vault into CapsuleCraft (.eml) and exit.
    pub import_obsidian: Option<PathBuf>,
    /// Ingest a software project codebase into .capsule and exit.
    pub ingest_project: Option<PathBuf>,
    /// Verify knowledge graph integrity of a project's .capsule and exit.
    pub verify_project: Option<PathBuf>,
    /// Print MCP client configuration snippet and exit.
    pub print_mcp_config: bool,
    /// Print the parsed launch configuration and exit.
    pub print_config: bool,
    /// Print usage information and exit.
    pub help: bool,
}

pub const DEFAULT_SNAPSHOT_FRAMES: u32 = 6;

/// Parse the process arguments (without the program name).
///
/// Unknown flags are ignored rather than fatal: a typo must never stop the app
/// from opening the user's vault.
pub fn parse(args: &[String]) -> Launch {
    let mut launch = Launch {
        frames: DEFAULT_SNAPSHOT_FRAMES,
        ..Launch::default()
    };
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].clone();
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) => (flag.to_string(), Some(value.to_string())),
            None => (arg.clone(), None),
        };
        let mut take_value = |consumed: &mut usize| -> Option<String> {
            if let Some(value) = inline.clone() {
                return Some(value);
            }
            let next = args.get(*consumed + 1).cloned();
            if next.is_some() {
                *consumed += 1;
            }
            next
        };
        match flag.as_str() {
            "--snapshot" => launch.snapshot = take_value(&mut index).map(PathBuf::from),
            "--view" => launch.view = take_value(&mut index).and_then(|v| parse_view(&v)),
            "--page" => launch.page = take_value(&mut index).and_then(|v| v.parse().ok()),
            "--frames" => {
                launch.frames = take_value(&mut index)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(DEFAULT_SNAPSHOT_FRAMES)
            }
            "--locale" => {
                launch.locale = take_value(&mut index).and_then(|v| parse_locale(&v));
            }
            "--search" => launch.search = take_value(&mut index),
            "--onboarding" => launch.onboarding = true,
            "--scroll" => launch.scroll = take_value(&mut index).and_then(|v| v.parse().ok()),
            "--mcp" => launch.mcp = true,
            "--modal" => launch.modal = take_value(&mut index).map(|v| v.trim().to_lowercase()),
            // `--stress-graph N`: swap the vault for N synthetic pages across
            // many folders and open the graph view — the scale harness for
            // screenshots and smoke runs (no files are written).
            "--stress-graph" => {
                launch.stress_graph = take_value(&mut index).and_then(|v| v.parse().ok());
            }
            "--zoom" => {
                launch.zoom = take_value(&mut index).and_then(|v| v.parse().ok());
            }
            "--window-size" => {
                launch.window_size = take_value(&mut index).and_then(|v| parse_window_size(&v))
            }
            "import-obsidian" => {
                if let Some(next) = args.get(index + 1) {
                    if next == "--from" {
                        index += 1;
                        launch.import_obsidian = take_value(&mut index).map(PathBuf::from);
                    } else if !next.starts_with('-') {
                        launch.import_obsidian = take_value(&mut index).map(PathBuf::from);
                    }
                }
            }
            "--import-obsidian" => {
                launch.import_obsidian = take_value(&mut index).map(PathBuf::from);
            }
            "--ingest" | "--ingest-project" => {
                launch.ingest_project = take_value(&mut index).map(PathBuf::from);
            }
            "--verify" | "--verify-project" => {
                launch.verify_project = take_value(&mut index).map(PathBuf::from);
            }
            "--from" => {
                if launch.import_obsidian.is_none() {
                    launch.import_obsidian = take_value(&mut index).map(PathBuf::from);
                }
            }
            "--vault" => {
                launch.vault = take_value(&mut index).map(PathBuf::from);
            }
            "--print-mcp-config" | "--mcp-config" => launch.print_mcp_config = true,
            "--print-config" => launch.print_config = true,
            "--help" | "-h" => launch.help = true,
            other if !other.starts_with('-') => {
                if launch.vault.is_none() {
                    launch.vault = Some(PathBuf::from(other));
                }
            }
            _ => {}
        }
        index += 1;
    }
    launch
}

fn parse_view(value: &str) -> Option<View> {
    match value.trim().to_lowercase().as_str() {
        "page" | "editor" => Some(View::Page),
        "planning" | "tasks" => Some(View::Planning),
        "graph" => Some(View::Graph),
        "threads" => Some(View::Threads),
        "mime" => Some(View::Mime),
        "mail" | "email" => Some(View::Mail),
        "faq" | "help" => Some(View::Faq),
        _ => None,
    }
}

fn parse_locale(value: &str) -> Option<Locale> {
    match value.trim().to_lowercase().as_str() {
        "ru" => Some(Locale::Ru),
        "en" => Some(Locale::En),
        _ => None,
    }
}

/// `980x640` (or `980X640`) → logical window size; nonsense yields `None` and
/// the snapshot keeps its default.
fn parse_window_size(value: &str) -> Option<[f32; 2]> {
    let (w, h) = value.split_once(['x', 'X'])?;
    let w = w.trim().parse::<f32>().ok()?;
    let h = h.trim().parse::<f32>().ok()?;
    if !w.is_finite() || !h.is_finite() || w < 200.0 || h < 200.0 || w > 8000.0 || h > 8000.0 {
        return None;
    }
    Some([w, h])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn vault_is_the_first_positional_argument() {
        let launch = parse(&args(&["C:/vault"]));
        assert_eq!(launch.vault, Some(PathBuf::from("C:/vault")));
        assert!(launch.snapshot.is_none());
        assert_eq!(launch.frames, DEFAULT_SNAPSHOT_FRAMES);
    }

    #[test]
    fn snapshot_accepts_both_spellings() {
        let spaced = parse(&args(&["C:/vault", "--snapshot", "shots/ui.png"]));
        assert_eq!(spaced.snapshot, Some(PathBuf::from("shots/ui.png")));
        assert_eq!(spaced.vault, Some(PathBuf::from("C:/vault")));

        let inline = parse(&args(&["--snapshot=shots/ui.png", "C:/vault"]));
        assert_eq!(inline.snapshot, Some(PathBuf::from("shots/ui.png")));
        assert_eq!(inline.vault, Some(PathBuf::from("C:/vault")));
    }

    #[test]
    fn view_and_page_are_typed() {
        let launch = parse(&args(&["--view", "graph", "--page=3", "--frames", "12"]));
        assert_eq!(launch.view, Some(View::Graph));
        assert_eq!(launch.page, Some(3));
        assert_eq!(launch.frames, 12);
    }

    #[test]
    fn onboarding_flag_parses() {
        let launch = parse(&args(&["--onboarding"]));
        assert!(launch.onboarding);
        let plain = parse(&args(&[]));
        assert!(!plain.onboarding);
    }

    #[test]
    fn faq_is_a_view_and_snapshot_target() {
        let launch = parse(&args(&["--view", "faq"]));
        assert_eq!(launch.view, Some(View::Faq));
        let alias = parse(&args(&["--view", "help"]));
        assert_eq!(alias.view, Some(View::Faq));
    }

    #[test]
    fn search_query_is_preserved() {
        let launch = parse(&args(&["--search", "mmap headers"]));
        assert_eq!(launch.search.as_deref(), Some("mmap headers"));
    }

    #[test]
    fn unknown_values_do_not_break_startup() {
        let launch = parse(&args(&["--view", "nope", "--page", "x", "--frames", "y"]));
        assert_eq!(launch.view, None);
        assert_eq!(launch.page, None);
        assert_eq!(launch.frames, DEFAULT_SNAPSHOT_FRAMES);
    }

    #[test]
    fn a_trailing_flag_without_value_is_harmless() {
        let launch = parse(&args(&["--view"]));
        assert_eq!(launch.view, None);
        assert_eq!(launch.vault, None);
    }

    #[test]
    fn parses_mcp_flags() {
        let mcp = parse(&args(&["--mcp", "D:/vault"]));
        assert!(mcp.mcp);
        assert_eq!(mcp.vault, Some(PathBuf::from("D:/vault")));

        let config = parse(&args(&["--print-mcp-config"]));
        assert!(config.print_mcp_config);
    }

    #[test]
    fn parses_import_obsidian_flags() {
        let cmd = parse(&args(&[
            "import-obsidian",
            "--from",
            "D:/notes",
            "--vault",
            "D:/vault",
        ]));
        assert_eq!(cmd.import_obsidian, Some(PathBuf::from("D:/notes")));
        assert_eq!(cmd.vault, Some(PathBuf::from("D:/vault")));

        let flag = parse(&args(&["--import-obsidian=D:/notes2"]));
        assert_eq!(flag.import_obsidian, Some(PathBuf::from("D:/notes2")));
    }

    #[test]
    fn parses_verify_project_flags() {
        let cmd = parse(&args(&["--verify-project", "D:/project"]));
        assert_eq!(cmd.verify_project, Some(PathBuf::from("D:/project")));
    }

    #[test]
    fn parses_modal_and_window_size_flags() {
        let launch = parse(&args(&[
            "--modal",
            "Integrations",
            "--window-size",
            "980x640",
        ]));
        assert_eq!(launch.modal.as_deref(), Some("integrations"));
        assert_eq!(launch.window_size, Some([980.0, 640.0]));
        // Inline spelling and the capital X both work.
        let launch = parse(&args(&["--modal=integrations", "--window-size=1280X800"]));
        assert_eq!(launch.modal.as_deref(), Some("integrations"));
        assert_eq!(launch.window_size, Some([1280.0, 800.0]));
        // Nonsense falls back to the defaults instead of crashing startup.
        let launch = parse(&args(&["--window-size", "tiny"]));
        assert_eq!(launch.window_size, None);
        let launch = parse(&args(&["--window-size", "10x10"]));
        assert_eq!(launch.window_size, None);
    }
}
