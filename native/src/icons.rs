//! Small line icons painted with egui's painter, so the shell does not depend
//! on a default widget font for glyphs.

use egui::{pos2, Color32, Painter, Pos2, Rect, Shape, Stroke};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Doc,
    Folder,
    Search,
    Plus,
    ChevronRight,
    ChevronDown,
    ChevronLeft,
    Check,
    ArrowUp,
    ArrowDown,
    Trash,
    Grip,
    Info,
    Save,
    Reload,
    Lock,
    Graph,
    Calendar,
    Mail,
    Code,
    Table,
    Audio,
    Board,
    Filter,
    Copy,
    Download,
    Command,
    Tag,
    Globe,
    Close,
    Sidebar,
    Record,
}

fn at(rect: Rect, x: f32, y: f32) -> Pos2 {
    pos2(
        rect.left() + rect.width() * x,
        rect.top() + rect.height() * y,
    )
}

/// Paint `icon` inside `rect` with a 1.25 px line.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let stroke = Stroke::new(1.25, color);
    let p = painter;
    let line = |points: &[(f32, f32)]| {
        let pts: Vec<Pos2> = points.iter().map(|(x, y)| at(rect, *x, *y)).collect();
        p.add(Shape::line(pts, stroke));
    };
    let dot = |x: f32, y: f32, r: f32| {
        p.circle_filled(at(rect, x, y), rect.width().min(rect.height()) * r, color);
    };

    match icon {
        Icon::Record => {
            // Filled dot — the universal "recording" affordance.
            dot(0.5, 0.5, 0.42);
        }
        Icon::Doc => {
            p.rect_stroke(
                Rect::from_min_max(at(rect, 0.24, 0.13), at(rect, 0.76, 0.87)),
                1.5,
                stroke,
                egui::StrokeKind::Inside,
            );
            line(&[(0.38, 0.36), (0.62, 0.36)]);
            line(&[(0.38, 0.5), (0.62, 0.5)]);
            line(&[(0.38, 0.64), (0.55, 0.64)]);
        }
        Icon::Folder => {
            line(&[
                (0.13, 0.34),
                (0.13, 0.79),
                (0.87, 0.79),
                (0.87, 0.3),
                (0.46, 0.3),
                (0.38, 0.21),
                (0.13, 0.21),
                (0.13, 0.34),
            ]);
        }
        Icon::Search => {
            p.circle_stroke(at(rect, 0.43, 0.43), rect.width() * 0.25, stroke);
            line(&[(0.61, 0.61), (0.84, 0.84)]);
        }
        Icon::Plus => {
            line(&[(0.5, 0.18), (0.5, 0.82)]);
            line(&[(0.18, 0.5), (0.82, 0.5)]);
        }
        Icon::ChevronRight => line(&[(0.4, 0.25), (0.62, 0.5), (0.4, 0.75)]),
        Icon::ChevronDown => line(&[(0.25, 0.4), (0.5, 0.62), (0.75, 0.4)]),
        Icon::Check => line(&[(0.18, 0.52), (0.4, 0.73), (0.82, 0.27)]),
        Icon::ArrowUp => {
            line(&[(0.5, 0.82), (0.5, 0.2)]);
            line(&[(0.27, 0.43), (0.5, 0.2), (0.73, 0.43)]);
        }
        Icon::ArrowDown => {
            line(&[(0.5, 0.18), (0.5, 0.8)]);
            line(&[(0.27, 0.57), (0.5, 0.8), (0.73, 0.57)]);
        }
        Icon::Trash => {
            line(&[(0.18, 0.27), (0.82, 0.27)]);
            line(&[(0.4, 0.18), (0.6, 0.18)]);
            line(&[(0.28, 0.3), (0.33, 0.84), (0.67, 0.84), (0.72, 0.3)]);
            line(&[(0.44, 0.42), (0.44, 0.72)]);
            line(&[(0.56, 0.42), (0.56, 0.72)]);
        }
        Icon::Grip => {
            for y in [0.28, 0.5, 0.72] {
                dot(0.38, y, 0.055);
                dot(0.62, y, 0.055);
            }
        }
        Icon::Info => {
            p.circle_stroke(at(rect, 0.5, 0.5), rect.width() * 0.32, stroke);
            dot(0.5, 0.32, 0.05);
            line(&[(0.5, 0.45), (0.5, 0.7)]);
        }
        Icon::Save => {
            p.rect_stroke(
                Rect::from_min_max(at(rect, 0.2, 0.18), at(rect, 0.8, 0.82)),
                1.5,
                stroke,
                egui::StrokeKind::Inside,
            );
            line(&[(0.35, 0.18), (0.35, 0.42), (0.65, 0.42), (0.65, 0.18)]);
            p.rect_filled(
                Rect::from_min_max(at(rect, 0.35, 0.6), at(rect, 0.65, 0.82)),
                1.0,
                color,
            );
        }
        Icon::Reload => {
            let mut pts = Vec::new();
            let start = std::f32::consts::PI * 0.35;
            let end = std::f32::consts::PI * 1.95;
            for i in 0..=16 {
                let a = start + (end - start) * (i as f32 / 16.0);
                pts.push(pos2(
                    rect.center().x + rect.width() * 0.34 * a.cos(),
                    rect.center().y + rect.height() * 0.34 * a.sin(),
                ));
            }
            p.add(Shape::line(pts, stroke));
            let tip = at(rect, 0.78, 0.52);
            line(&[(0.64, 0.4), (0.78, 0.52), (0.66, 0.66)]);
            let _ = tip;
        }
        Icon::Lock => {
            p.rect_stroke(
                Rect::from_min_max(at(rect, 0.25, 0.45), at(rect, 0.75, 0.85)),
                1.0,
                stroke,
                egui::StrokeKind::Inside,
            );
            let mut pts = Vec::new();
            for i in 0..=10 {
                let a = std::f32::consts::PI * (1.0 + i as f32 / 10.0);
                pts.push(pos2(
                    rect.center().x + rect.width() * 0.18 * a.cos(),
                    rect.center().y + rect.height() * 0.08 + rect.height() * 0.18 * a.sin(),
                ));
            }
            p.add(Shape::line(pts, stroke));
            dot(0.5, 0.63, 0.05);
        }
        Icon::ChevronLeft => line(&[(0.6, 0.25), (0.38, 0.5), (0.6, 0.75)]),
        Icon::Graph => {
            dot(0.5, 0.2, 0.09);
            dot(0.2, 0.76, 0.09);
            dot(0.8, 0.76, 0.09);
            line(&[(0.44, 0.28), (0.27, 0.66)]);
            line(&[(0.56, 0.28), (0.73, 0.66)]);
            line(&[(0.29, 0.78), (0.71, 0.78)]);
        }
        Icon::Calendar => {
            p.rect_stroke(
                Rect::from_min_max(at(rect, 0.18, 0.24), at(rect, 0.82, 0.82)),
                1.5,
                stroke,
                egui::StrokeKind::Inside,
            );
            line(&[(0.18, 0.4), (0.82, 0.4)]);
            line(&[(0.34, 0.15), (0.34, 0.3)]);
            line(&[(0.66, 0.15), (0.66, 0.3)]);
            dot(0.34, 0.58, 0.05);
            dot(0.5, 0.58, 0.05);
            dot(0.66, 0.58, 0.05);
        }
        Icon::Mail => {
            p.rect_stroke(
                Rect::from_min_max(at(rect, 0.15, 0.27), at(rect, 0.85, 0.75)),
                1.5,
                stroke,
                egui::StrokeKind::Inside,
            );
            line(&[(0.18, 0.32), (0.5, 0.56), (0.82, 0.32)]);
        }
        Icon::Code => {
            line(&[(0.36, 0.3), (0.16, 0.5), (0.36, 0.7)]);
            line(&[(0.64, 0.3), (0.84, 0.5), (0.64, 0.7)]);
            line(&[(0.56, 0.22), (0.44, 0.78)]);
        }
        Icon::Table => {
            p.rect_stroke(
                Rect::from_min_max(at(rect, 0.16, 0.24), at(rect, 0.84, 0.78)),
                1.5,
                stroke,
                egui::StrokeKind::Inside,
            );
            line(&[(0.16, 0.42), (0.84, 0.42)]);
            line(&[(0.46, 0.24), (0.46, 0.78)]);
            line(&[(0.16, 0.6), (0.84, 0.6)]);
        }
        Icon::Audio => {
            for (x, h) in [
                (0.22, 0.2),
                (0.34, 0.5),
                (0.46, 0.34),
                (0.58, 0.66),
                (0.7, 0.28),
                (0.82, 0.44),
            ] {
                line(&[(x, 0.5 - h / 2.0), (x, 0.5 + h / 2.0)]);
            }
        }
        Icon::Board => {
            p.rect_stroke(
                Rect::from_min_max(at(rect, 0.14, 0.26), at(rect, 0.86, 0.76)),
                1.5,
                stroke,
                egui::StrokeKind::Inside,
            );
            line(&[
                (0.28, 0.34),
                (0.44, 0.34),
                (0.44, 0.6),
                (0.28, 0.6),
                (0.28, 0.34),
            ]);
            line(&[
                (0.56, 0.44),
                (0.72, 0.44),
                (0.72, 0.68),
                (0.56, 0.68),
                (0.56, 0.44),
            ]);
        }
        Icon::Filter => {
            line(&[
                (0.16, 0.26),
                (0.84, 0.26),
                (0.58, 0.52),
                (0.58, 0.8),
                (0.42, 0.72),
                (0.42, 0.52),
                (0.16, 0.26),
            ]);
        }
        Icon::Copy => {
            p.rect_stroke(
                Rect::from_min_max(at(rect, 0.3, 0.3), at(rect, 0.84, 0.84)),
                1.0,
                stroke,
                egui::StrokeKind::Inside,
            );
            p.rect_stroke(
                Rect::from_min_max(at(rect, 0.16, 0.16), at(rect, 0.7, 0.7)),
                1.0,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Icon::Download => {
            line(&[(0.5, 0.16), (0.5, 0.62)]);
            line(&[(0.3, 0.45), (0.5, 0.65), (0.7, 0.45)]);
            line(&[(0.2, 0.82), (0.8, 0.82)]);
        }
        Icon::Command => {
            for (x, y) in [(0.3, 0.3), (0.7, 0.3), (0.3, 0.7), (0.7, 0.7)] {
                p.circle_stroke(at(rect, x, y), rect.width() * 0.1, stroke);
            }
            line(&[(0.4, 0.4), (0.6, 0.6)]);
            line(&[(0.6, 0.4), (0.4, 0.6)]);
        }
        Icon::Tag => {
            line(&[
                (0.16, 0.42),
                (0.42, 0.16),
                (0.84, 0.16),
                (0.84, 0.58),
                (0.58, 0.84),
                (0.16, 0.42),
            ]);
            dot(0.66, 0.34, 0.05);
        }
        Icon::Globe => {
            p.circle_stroke(at(rect, 0.5, 0.5), rect.width() * 0.32, stroke);
            line(&[(0.18, 0.5), (0.82, 0.5)]);
            p.add(Shape::line(
                (0..=12)
                    .map(|i| {
                        let a = std::f32::consts::PI * 2.0 * i as f32 / 12.0;
                        pos2(
                            rect.center().x + rect.width() * 0.14 * a.sin(),
                            rect.center().y + rect.height() * 0.32 * a.cos(),
                        )
                    })
                    .collect(),
                stroke,
            ));
        }
        Icon::Close => {
            line(&[(0.28, 0.28), (0.72, 0.72)]);
            line(&[(0.72, 0.28), (0.28, 0.72)]);
        }
        Icon::Sidebar => {
            p.rect_stroke(
                Rect::from_min_max(at(rect, 0.16, 0.2), at(rect, 0.84, 0.8)),
                1.5,
                stroke,
                egui::StrokeKind::Inside,
            );
            line(&[(0.42, 0.2), (0.42, 0.8)]);
        }
    }
}
