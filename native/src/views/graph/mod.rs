//! Relationship graph view: a zoomable canvas over page parents, replies,
//! references and tag clusters that stays readable at vault scale.

pub mod interaction;
pub mod render_clusters;
pub mod render_density;
pub mod render_pages;

use egui::{pos2, vec2, Align, FontId, Layout, Rect, RichText, Sense, Ui};

use crate::graph::{self, Graph, GraphMetrics, LodMode, PositionCache, SYNC_LAYOUT_MAX};
use crate::i18n::{t, Key, Locale};
use crate::icons::Icon;
use crate::mime::Capsule;
use crate::theme;
use crate::{ghost, hud, menu_header, menu_row, popover_frame, tag_chip, truncate_chars};

use super::{empty_state, header, View, ViewAction, VIEW_PAD};
use interaction::{handle_interaction, GraphTransform};
use render_clusters::{draw_cluster_panel, render_clusters};
use render_density::render_density;
use render_pages::{draw_page_legend, render_pages};

#[derive(Default)]
pub struct GraphState {
    pub tag: Option<String>,
    pub pan: egui::Vec2,
    pub zoom: f32,
    pub edges: bool,
    /// Focused cluster: members render at full strength, everything else
    /// fades; entering a cluster centres and zooms onto its bounds.
    pub focus: Option<u32>,
    /// Currently dragged node slot (in `LodMode::Pages`).
    pub dragged_node: Option<u32>,
    /// Temporary position overrides for dragged nodes.
    pub drag_overrides: std::collections::HashMap<u32, [f32; 2]>,
    /// Search query to filter/highlight matching nodes.
    pub search_query: String,
    /// Hide nodes with zero incident edges.
    pub hide_orphans: bool,
    /// Local graph mode focusing on neighborhood of selected page.
    pub local_mode: bool,
    /// BFS depth in local graph mode (1..=3).
    pub local_depth: usize,
    /// Node slot for context menu.
    pub context_node: Option<u32>,
    /// Whether selection highlighting on the canvas was cleared by clicking empty space.
    pub selection_cleared: bool,
    /// Last seen external selected page index to detect external selection changes.
    pub last_external_selected: Option<usize>,
    /// Background layout progress channel, when a big vault is relaxing.
    layout_rx: Option<std::sync::mpsc::Receiver<graph::LayoutMsg>>,
    /// Vault signature the running job was started for — a vault edit
    /// invalidates in-flight snapshots, so they are dropped instead of
    /// being applied to a graph with different nodes.
    layout_sig: Option<u64>,
    layout_done: usize,
    layout_total: usize,
    /// Layout is far too expensive to redo every frame: keep the last graph
    /// plus the vault signature it was built from. Rebuild only when either
    /// changes (edits, imports, tag filter).
    cached: Option<(
        u64,
        Option<String>,
        std::sync::Arc<Graph>,
        std::sync::Arc<GraphMetrics>,
    )>,
    /// Message-ID → position, persisted next to `ui_state.json`.
    cache: Option<PositionCache>,
    cache_path: Option<std::path::PathBuf>,
    /// Packed bubble layout for the cluster overview, keyed by an input
    /// fingerprint — the packing relaxation must not run every frame.
    pack_cache: Option<(u64, Vec<[f32; 2]>, Vec<f32>)>,
}

fn vault_signature(capsules: &[Capsule]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    capsules.len().hash(&mut hasher);
    for capsule in capsules {
        capsule.message_id.hash(&mut hasher);
        capsule.parent_id.hash(&mut hasher);
        capsule.in_reply_to.hash(&mut hasher);
        capsule.references.len().hash(&mut hasher);
        capsule.tags.len().hash(&mut hasher);
        capsule.file_name.hash(&mut hasher);
        for r in &capsule.references {
            r.hash(&mut hasher);
        }
    }
    hasher.finish()
}

impl GraphState {
    pub fn new() -> Self {
        Self {
            tag: None,
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            edges: true,
            focus: None,
            dragged_node: None,
            drag_overrides: std::collections::HashMap::new(),
            search_query: String::new(),
            hide_orphans: false,
            local_mode: false,
            local_depth: 1,
            context_node: None,
            selection_cleared: false,
            last_external_selected: None,
            layout_rx: None,
            layout_sig: None,
            layout_done: 0,
            layout_total: 0,
            cached: None,
            cache: None,
            cache_path: None,
            pack_cache: None,
        }
    }
}

pub fn draw(
    ui: &mut Ui,
    capsules: &[Capsule],
    state: &mut GraphState,
    locale: Locale,
    selected: Option<usize>,
) -> ViewAction {
    let mut action = ViewAction::default();
    if selected != state.last_external_selected {
        state.last_external_selected = selected;
        state.selection_cleared = false;
    }
    let signature = vault_signature(capsules);
    let tag = state.tag.clone();
    let rebuild = !matches!(
        &state.cached,
        Some((sig, cached_tag, _, _)) if *sig == signature && *cached_tag == tag
    );
    if rebuild {
        if state.cache.is_none() {
            let path = state
                .cache_path
                .clone()
                .unwrap_or_else(|| crate::vault::app_data_dir().join("graph_layout.json"));
            state.cache = PositionCache::load(&path);
            state.cache_path = Some(path);
        }
        let built = std::sync::Arc::new(graph::build_with_cache(
            capsules,
            state.tag.as_deref(),
            state.cache.as_ref(),
        ));
        let node_count = built.nodes.len();
        let metrics = std::sync::Arc::new(graph::build_metrics(&built));
        state.cached = Some((signature, tag.clone(), built.clone(), metrics.clone()));
        if node_count >= SYNC_LAYOUT_MAX && state.layout_rx.is_none() {
            state.layout_sig = Some(signature);
            state.layout_rx = Some(graph::layout_in_background(
                built,
                graph::auto_iterations(node_count),
            ));
        }
    }
    let (mut graph, mut metrics) = match &state.cached {
        Some((_, _, graph, metrics)) => (graph.clone(), metrics.clone()),
        None => {
            let built = std::sync::Arc::new(graph::build(capsules, state.tag.as_deref()));
            let metrics = std::sync::Arc::new(graph::build_metrics(&built));
            (built, metrics)
        }
    };

    if state.layout_rx.is_some() && state.layout_sig != Some(signature) {
        state.layout_rx = None;
        state.layout_sig = None;
    }
    if let Some(rx) = &state.layout_rx {
        let mut latest: Option<(usize, usize, Vec<[f32; 2]>)> = None;
        let mut done: Option<Vec<[f32; 2]>> = None;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                graph::LayoutMsg::Progress {
                    done: step,
                    total,
                    positions,
                } => latest = Some((step, total, positions)),
                graph::LayoutMsg::Done(positions) => done = Some(positions),
            }
        }
        let apply = done.clone().or_else(|| latest.clone().map(|(_, _, p)| p));
        if let Some(positions) = apply {
            let mut updated = (*graph).clone();
            for (node, pos) in updated.nodes.iter_mut().zip(positions) {
                node.pos = pos;
            }
            let updated = std::sync::Arc::new(updated);
            metrics = std::sync::Arc::new(graph::build_metrics(&updated));
            state.cached = Some((signature, tag.clone(), updated.clone(), metrics.clone()));
            graph = updated;
        }
        if let Some((step, total, _)) = latest {
            state.layout_done = step;
            state.layout_total = total;
        }
        if let Some(_final_positions) = done {
            state.layout_rx = None;
            state.layout_done = state.layout_total;
            let cache = PositionCache::from_graph(&graph, capsules);
            if let Some(path) = &state.cache_path.clone() {
                let _ = cache.save(path);
            }
            state.cache = Some(cache);
        }
    }

    header(
        ui,
        t(locale, Key::GraphTitle),
        &format!(
            "{} {} · {} {} · {} {}",
            graph.nodes.len(),
            t(locale, Key::Nodes),
            graph.edges.len(),
            t(locale, Key::Edges),
            graph.clusters.len(),
            t(locale, Key::GraphClustersPanel),
        ),
    );

    let (_, bar_rect) = super::toolbar(ui);
    let is_compact = bar_rect.width() < 600.0;
    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar_rect.shrink2(vec2(VIEW_PAD, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    let label = state
        .tag
        .clone()
        .unwrap_or_else(|| t(locale, Key::All).to_string());
    let tag_button = ghost(
        &mut bar,
        Icon::Tag,
        Some(&label),
        t(locale, Key::TagFilter),
        true,
        26.0,
    );
    egui::Popup::menu(&tag_button)
        .frame(popover_frame())
        .align(egui::RectAlign::BOTTOM_START)
        .show(|ui| {
            ui.set_min_width(180.0);
            if menu_row(
                ui,
                None,
                t(locale, Key::All),
                None,
                state.tag.is_none(),
                true,
                false,
            )
            .clicked()
            {
                state.tag = None;
                state.focus = None;
                state.drag_overrides.clear();
                state.dragged_node = None;
            }
            for tag in &graph.tags {
                if menu_row(
                    ui,
                    Some(Icon::Tag),
                    tag,
                    None,
                    state.tag.as_deref() == Some(tag.as_str()),
                    true,
                    false,
                )
                .clicked()
                {
                    state.tag = Some(tag.clone());
                    state.focus = None;
                    state.drag_overrides.clear();
                    state.dragged_node = None;
                }
            }
        });

    let edge_label = if is_compact {
        None
    } else {
        Some(if state.edges { "edges on" } else { "edges off" })
    };
    if ghost(
        &mut bar,
        if state.edges {
            Icon::Check
        } else {
            Icon::Close
        },
        edge_label,
        "Toggle relation edges",
        true,
        26.0,
    )
    .clicked()
    {
        state.edges = !state.edges;
    }

    let search_edit = egui::TextEdit::singleline(&mut state.search_query)
        .hint_text(t(locale, Key::GraphSearchPlaceholder))
        .desired_width(if is_compact { 75.0 } else { 110.0 });
    bar.add(search_edit);
    if !state.search_query.is_empty() {
        if ghost(
            &mut bar,
            Icon::Close,
            None,
            t(locale, Key::ClearSearch),
            true,
            26.0,
        )
        .clicked()
        {
            state.search_query.clear();
        }
    }

    if ghost(
        &mut bar,
        if state.hide_orphans {
            Icon::Check
        } else {
            Icon::Doc
        },
        if is_compact { None } else { Some(t(locale, Key::GraphHideOrphans)) },
        t(locale, Key::GraphHideOrphans),
        true,
        26.0,
    )
    .clicked()
    {
        state.hide_orphans = !state.hide_orphans;
    }

    let local_depth = state.local_depth.clamp(1, 3);
    let mode_label = if is_compact {
        None
    } else if state.local_mode {
        Some(format!(
            "{}: {} {}",
            t(locale, Key::GraphLocalMode),
            t(locale, Key::GraphDepth),
            local_depth
        ))
    } else {
        Some(t(locale, Key::GraphGlobalMode).to_string())
    };
    if ghost(
        &mut bar,
        Icon::Graph,
        mode_label.as_deref(),
        if state.local_mode {
            t(locale, Key::GraphLocalMode)
        } else {
            t(locale, Key::GraphGlobalMode)
        },
        true,
        26.0,
    )
    .clicked()
    {
        if state.local_mode {
            if state.local_depth < 3 {
                state.local_depth += 1;
            } else {
                state.local_mode = false;
                state.local_depth = 1;
            }
        } else {
            state.local_mode = true;
            state.local_depth = 1;
        }
    }

    bar.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if ghost(
            ui,
            Icon::Reload,
            if is_compact { None } else { Some(t(locale, Key::Fit)) },
            t(locale, Key::GraphHint),
            true,
            26.0,
        )
        .clicked()
        {
            state.pan = egui::Vec2::ZERO;
            state.zoom = 1.0;
            state.focus = None;
            state.drag_overrides.clear();
            state.dragged_node = None;
        }
        if !is_compact {
            ui.add_space(8.0);
            let hint = if state.layout_rx.is_some() {
                format!(
                    "{} · {}",
                    t(locale, Key::GraphLayingOut),
                    t(locale, Key::GraphHint)
                )
            } else {
                t(locale, Key::GraphHint).to_string()
            };
            let _ = hud(ui, &hint, theme::faint());
        }
    });

    if graph.nodes.is_empty() {
        empty_state(
            ui,
            Icon::Graph,
            t(locale, Key::GraphTitle),
            t(locale, Key::EmptyVaultBody),
        );
        return action;
    }

    let rect = Rect::from_min_max(
        pos2(ui.max_rect().left(), ui.cursor().top()),
        ui.max_rect().right_bottom(),
    );
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    let transform = GraphTransform::new(rect, state.pan, state.zoom);
    let (world_min, world_max) = transform.world_bounds(rect);
    let positions: Vec<[f32; 2]> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(slot, node)| {
            state
                .drag_overrides
                .get(&(slot as u32))
                .copied()
                .unwrap_or(node.pos)
        })
        .collect();
    let visible = metrics.grid.query(world_min, world_max);
    let mode = graph::pick_mode(state.zoom, visible.len());

    handle_interaction(
        ui, &response, &transform, mode, &positions, &graph, capsules, &metrics, state,
    );

    let context_hovered = response.hover_pos().and_then(|p| {
        metrics
            .grid
            .nearest(&positions, transform.to_graph(p), 12.0 / transform.radius)
    });
    if response.clicked() {
        if context_hovered.is_none() {
            state.selection_cleared = true;
        } else {
            state.selection_cleared = false;
        }
    }
    if response.secondary_clicked() {
        state.context_node = context_hovered;
    }

    response.context_menu(|ui| {
        ui.set_min_width(180.0);
        if let Some(slot) = state.context_node {
            if let Some(node) = graph.nodes.get(slot as usize) {
                if let Some(capsule) = capsules.get(node.capsule) {
                    let title = if capsule.subject.is_empty() {
                        t(locale, Key::Untitled).to_string()
                    } else {
                        truncate_chars(&capsule.subject, 28)
                    };
                    menu_header(ui, &title);
                    if menu_row(
                        ui,
                        Some(Icon::Doc),
                        t(locale, Key::GraphContextMenuOpen),
                        None,
                        false,
                        true,
                        false,
                    )
                    .clicked()
                    {
                        action.select = Some(node.capsule);
                        action.view = Some(View::Page);
                        ui.close();
                    }
                    if menu_row(
                        ui,
                        Some(Icon::Copy),
                        t(locale, Key::GraphContextMenuCopyLink),
                        None,
                        false,
                        true,
                        false,
                    )
                    .clicked()
                    {
                        ui.ctx()
                            .copy_text(format!("capsule://{}", capsule.message_id));
                        action.toast = Some(t(locale, Key::CopiedToClipboard).to_string());
                        ui.close();
                    }
                    if menu_row(
                        ui,
                        Some(Icon::Doc),
                        t(locale, Key::GraphContextMenuCopyWikiLink),
                        None,
                        false,
                        true,
                        false,
                    )
                    .clicked()
                    {
                        ui.ctx().copy_text(format!("[[{}]]", capsule.subject));
                        action.toast = Some(t(locale, Key::CopiedToClipboard).to_string());
                        ui.close();
                    }
                    return;
                }
            }
        }
        if menu_row(
            ui,
            Some(Icon::Reload),
            t(locale, Key::Fit),
            None,
            false,
            true,
            false,
        )
        .clicked()
        {
            state.pan = egui::Vec2::ZERO;
            state.zoom = 1.0;
            state.focus = None;
            state.drag_overrides.clear();
            state.dragged_node = None;
            ui.close();
        }
    });
    painter.rect_filled(rect, 0.0, theme::bg());

    if mode == LodMode::Clusters && graph.clusters.len() > 1 {
        render_clusters(&painter, rect, &response, &graph, &metrics, state, locale);
    } else if mode == LodMode::Density {
        render_density(
            &painter, rect, &response, &transform, &graph, &metrics, state, &visible, &positions,
            locale,
        );
    } else if mode == LodMode::Pages {
        render_pages(
            &painter,
            rect,
            &response,
            &transform,
            &graph,
            &metrics,
            state,
            capsules,
            &visible,
            &positions,
            selected,
            locale,
            &mut action,
        );
    }

    draw_cluster_panel(ui, &painter, rect, &graph, &metrics, state, locale);

    let band = match mode {
        LodMode::Clusters => t(locale, Key::GraphBandClusters).to_string(),
        LodMode::Density => format!("{} · {}", t(locale, Key::GraphBandDensity), visible.len()),
        LodMode::Pages => format!("{} · {}", t(locale, Key::GraphBandPages), visible.len()),
    };
    let chip_galley = painter.layout_no_wrap(band, FontId::new(9.5, theme::mono()), theme::faint());
    painter.galley(
        pos2(
            rect.right() - chip_galley.size().x - 12.0,
            rect.bottom() - 26.0,
        ),
        chip_galley,
        theme::faint(),
    );

    if mode == LodMode::Pages {
        draw_page_legend(&painter, rect, locale);
    }

    if state.layout_rx.is_some() {
        let text = format!(
            "{} {}/{}",
            t(locale, Key::GraphLayingOut),
            state.layout_done,
            state.layout_total.max(1)
        );
        let galley = painter.layout_no_wrap(text, FontId::new(10.5, theme::mono()), theme::faint());
        let pos = pos2(rect.center().x - galley.size().x / 2.0, rect.top() + 12.0);
        let box_rect = Rect::from_min_size(pos, galley.size() + vec2(16.0, 10.0));
        painter.rect(
            box_rect,
            theme::RADIUS,
            theme::fade(theme::panel(), 0.94),
            theme::hairline(),
            egui::StrokeKind::Inside,
        );
        painter.galley(box_rect.min + vec2(8.0, 5.0), galley, theme::faint());
    }

    let effective_selected = if state.selection_cleared {
        None
    } else {
        selected
    };

    if let Some(index) = effective_selected {
        if let Some(capsule) = capsules.get(index) {
            if graph.nodes.iter().any(|node| node.capsule == index) {
                let card_anchor = pos2(rect.right() - 262.0, rect.top() + 14.0);
                let max_rect = Rect::from_min_size(card_anchor, vec2(248.0, rect.height() - 28.0));
                let mut card_ui = ui.new_child(egui::UiBuilder::new().max_rect(max_rect));

                egui::Frame::new()
                    .fill(theme::fade(theme::panel(), 0.94))
                    .corner_radius(theme::RADIUS)
                    .stroke(theme::hairline())
                    .inner_margin(egui::Margin::same(10))
                    .show(&mut card_ui, |ui| {
                        ui.set_width(228.0);
                        let _ = hud(
                            ui,
                            if capsule.native { "PAGE" } else { "IMPORTED" },
                            theme::faint(),
                        );
                        ui.add_space(5.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(truncate_chars(&capsule.subject, 34))
                                    .family(theme::medium())
                                    .size(12.5)
                                    .color(theme::ink()),
                            )
                            .wrap(),
                        );
                        ui.add_space(4.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(truncate_chars(&capsule.from, 32))
                                    .size(10.5)
                                    .color(theme::muted()),
                            )
                            .truncate(),
                        );
                        if !capsule.tags.is_empty() {
                            ui.add_space(6.0);
                            ui.horizontal_wrapped(|ui| {
                                for tag in capsule.tags.iter().take(3) {
                                    let _ = tag_chip(ui, tag, theme::muted());
                                }
                            });
                        }
                    });
            }
        }
    }

    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_state_resets_zoom() {
        let state = GraphState::new();
        assert_eq!(state.zoom, 1.0);
        assert_eq!(state.pan, egui::Vec2::ZERO);
        assert!(state.edges);
        assert!(state.focus.is_none());
        assert!(state.dragged_node.is_none());
        assert!(state.drag_overrides.is_empty());
    }

    #[test]
    fn drag_overrides_priority_and_reset() {
        let mut state = GraphState::new();
        state.drag_overrides.insert(2, [0.42, -0.42]);
        assert_eq!(state.drag_overrides.get(&2), Some(&[0.42, -0.42]));
        assert_eq!(state.drag_overrides.get(&0), None);
        state.drag_overrides.clear();
        assert!(state.drag_overrides.is_empty());
    }

    #[test]
    fn density_threshold_matches_model() {
        assert_eq!(graph::pick_mode(0.1, 50), LodMode::Clusters);
        assert_eq!(graph::pick_mode(1.0, 50), LodMode::Pages);
        assert_eq!(graph::pick_mode(1.0, 7000), LodMode::Density);
    }

    #[test]
    fn cluster_colors_are_distinct_and_stable() {
        let a = render_clusters::cluster_color(0);
        let b = render_clusters::cluster_color(1);
        let c = render_clusters::cluster_color(2);
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_eq!(a, render_clusters::cluster_color(0));
    }

    #[test]
    fn cluster_names_localize_root() {
        assert_eq!(
            render_clusters::cluster_display_name("", Locale::En),
            "Vault root"
        );
        assert_eq!(
            render_clusters::cluster_display_name("", Locale::Ru),
            "Корень хранилища"
        );
        assert_eq!(
            render_clusters::cluster_display_name("projects", Locale::En),
            "projects"
        );
    }

    #[test]
    fn label_filter_skips_overlaps() {
        let a = egui::Rect::from_min_size(pos2(10.0, 10.0), vec2(40.0, 14.0));
        let b = egui::Rect::from_min_size(pos2(20.0, 12.0), vec2(40.0, 14.0));
        let c = egui::Rect::from_min_size(pos2(80.0, 10.0), vec2(40.0, 14.0));
        let kept = render_clusters::keep_non_overlapping(&[a, b, c]);
        assert_eq!(kept, vec![0, 2]);
    }

    #[test]
    fn bubbles_pack_without_overlap() {
        let anchors = vec![[100.0, 100.0], [105.0, 102.0], [110.0, 98.0]];
        let radii = vec![20.0, 18.0, 15.0];
        let (centers, _) =
            render_clusters::pack_bubbles(&anchors, &radii, ([0.0, 0.0], [400.0, 400.0]), 0.5, 120);
        for a in 0..centers.len() {
            for b in (a + 1)..centers.len() {
                let dx = centers[a][0] - centers[b][0];
                let dy = centers[a][1] - centers[b][1];
                let dist = (dx * dx + dy * dy).sqrt();
                assert!(dist >= (radii[a] + radii[b]) * 0.95);
            }
        }
    }

    #[test]
    fn bubble_radii_shrink_to_area_budget() {
        let anchors = vec![[50.0, 50.0], [60.0, 50.0]];
        let radii = vec![100.0, 100.0];
        let (_, final_radii) =
            render_clusters::pack_bubbles(&anchors, &radii, ([0.0, 0.0], [100.0, 100.0]), 0.3, 20);
        assert!(final_radii[0] < radii[0]);
    }

    #[test]
    fn packing_keeps_geography_and_is_deterministic() {
        let anchors = vec![[50.0, 100.0], [350.0, 100.0]];
        let radii = vec![30.0, 30.0];
        let (first_c, _) =
            render_clusters::pack_bubbles(&anchors, &radii, ([0.0, 0.0], [400.0, 400.0]), 0.4, 60);
        let (second_c, _) =
            render_clusters::pack_bubbles(&anchors, &radii, ([0.0, 0.0], [400.0, 400.0]), 0.4, 60);
        assert_eq!(first_c, second_c);
        assert!(first_c[0][0] < first_c[1][0]);
    }

    #[test]
    fn graph_state_defaults_and_toggles() {
        let mut state = GraphState::new();
        assert_eq!(state.search_query, "");
        assert!(!state.hide_orphans);
        assert!(!state.local_mode);
        assert_eq!(state.local_depth, 1);
        assert!(state.context_node.is_none());

        state.hide_orphans = true;
        state.local_mode = true;
        state.local_depth = 2;
        state.search_query = "project".into();
        assert!(state.hide_orphans);
        assert!(state.local_mode);
        assert_eq!(state.local_depth, 2);
        assert_eq!(state.search_query, "project");
    }

    #[test]
    fn i18n_keys_graph_features() {
        assert_eq!(t(Locale::En, Key::GraphLocalMode), "Local graph");
        assert_eq!(t(Locale::Ru, Key::GraphLocalMode), "Локальный граф");
        assert_eq!(t(Locale::En, Key::GraphGlobalMode), "Global graph");
        assert_eq!(t(Locale::Ru, Key::GraphGlobalMode), "Глобальный граф");
        assert_eq!(t(Locale::En, Key::GraphDepth), "Depth");
        assert_eq!(t(Locale::Ru, Key::GraphDepth), "Глубина");
        assert_eq!(t(Locale::En, Key::GraphHideOrphans), "Hide orphans");
        assert_eq!(t(Locale::Ru, Key::GraphHideOrphans), "Скрыть сироты");
        assert_eq!(t(Locale::En, Key::GraphContextMenuOpen), "Open note");
        assert_eq!(t(Locale::Ru, Key::GraphContextMenuOpen), "Открыть заметку");
        assert_eq!(t(Locale::En, Key::GraphContextMenuCopyLink), "Copy link");
        assert_eq!(
            t(Locale::Ru, Key::GraphContextMenuCopyLink),
            "Копировать ссылку"
        );
    }
}
