//! MaterialCraft's own tool and panel icons, drawn with the painter (no image assets), one
//! consistent line style (WS-14).

use eframe::egui::{Color32, Painter, Pos2, Rect, Stroke, vec2};

/// Every icon the strips and toolbars use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Brush,
    Eraser,
    Projection,
    PolygonFill,
    Smudge,
    Clone,
    Picker,
    Layers,
    /// A stack of layers seen from the side (the layer panel).
    Stack,
    Report,
    /// A padlock (the blockers panel).
    Lock,
    Display,
    Keyboard,
    Interface,
    /// The session log.
    Log,
    /// The asset tray: a grid of tiles.
    Assets,
    /// The bakers panel: rays falling onto a surface.
    Bake,
    /// Shader settings: a lit sphere.
    Shader,
    Viewport,
    More,
}

impl Icon {
    /// Paints the icon centred in `rect` in `color`.
    pub fn paint(self, painter: &Painter, rect: Rect, color: Color32) {
        let s = rect.width().min(rect.height()) * 0.5;
        let c = rect.center();
        let p = |x: f32, y: f32| c + vec2(x * s, y * s);
        let line = Stroke::new((s * 0.12).max(1.2), color);
        let poly = |pts: &[Pos2]| {
            for w in pts.windows(2) {
                if let [a, b] = w {
                    painter.line_segment([*a, *b], line);
                }
            }
        };
        match self {
            Icon::Brush => {
                poly(&[p(0.55, -0.6), p(-0.05, 0.0)]);
                painter.circle_filled(p(-0.25, 0.25), s * 0.28, color);
            }
            Icon::Eraser => {
                poly(&[p(-0.6, 0.15), p(0.05, -0.5), p(0.6, 0.05), p(-0.05, 0.7), p(-0.6, 0.15)]);
                poly(&[p(-0.25, -0.18), p(0.3, 0.37)]);
            }
            Icon::Projection => {
                painter.rect_stroke(Rect::from_min_max(p(-0.6, -0.6), p(0.1, 0.1)), 1.0, line, eframe::egui::StrokeKind::Middle);
                poly(&[p(0.1, 0.1), p(0.6, 0.6)]);
                poly(&[p(-0.6, 0.1), p(0.1, 0.6), p(0.6, 0.6)]);
            }
            Icon::PolygonFill => {
                painter.add(eframe::egui::Shape::convex_polygon(vec![p(-0.6, 0.5), p(0.0, -0.6), p(0.6, 0.5)], color.gamma_multiply(0.5), line));
            }
            Icon::Smudge => {
                poly(&[p(-0.6, 0.3), p(-0.3, -0.1), p(0.0, 0.3), p(0.3, -0.1), p(0.6, 0.3)]);
                poly(&[p(-0.6, -0.2), p(-0.3, -0.6), p(0.0, -0.2), p(0.3, -0.6), p(0.6, -0.2)]);
            }
            Icon::Clone => {
                painter.circle_stroke(p(-0.25, -0.2), s * 0.32, line);
                painter.circle_stroke(p(0.25, 0.25), s * 0.32, line);
            }
            Icon::Picker => {
                poly(&[p(0.6, -0.6), p(-0.4, 0.4)]);
                painter.circle_filled(p(-0.5, 0.5), s * 0.15, color);
                poly(&[p(0.2, -0.6), p(0.6, -0.2)]);
            }
            Icon::Layers => {
                for dy in [-0.4, 0.0, 0.4] {
                    poly(&[p(-0.6, dy), p(0.0, dy - 0.25), p(0.6, dy), p(0.0, dy + 0.25), p(-0.6, dy)]);
                }
            }
            Icon::Stack => {
                for dy in [-0.45, 0.0, 0.45] {
                    painter.rect_stroke(Rect::from_min_max(p(-0.6, dy - 0.15), p(0.6, dy + 0.15)), 1.0, line, eframe::egui::StrokeKind::Middle);
                }
            }
            Icon::Report => {
                painter.rect_stroke(Rect::from_min_max(p(-0.5, -0.65), p(0.5, 0.65)), 1.0, line, eframe::egui::StrokeKind::Middle);
                for dy in [-0.3, 0.0, 0.3] {
                    poly(&[p(-0.25, dy), p(0.25, dy)]);
                }
            }
            Icon::Lock => {
                painter.rect_stroke(Rect::from_min_max(p(-0.55, -0.1), p(0.55, 0.65)), 1.0, line, eframe::egui::StrokeKind::Middle);
                poly(&[p(-0.32, -0.1), p(-0.32, -0.4), p(-0.15, -0.6), p(0.15, -0.6), p(0.32, -0.4), p(0.32, -0.1)]);
                painter.circle_filled(p(0.0, 0.25), s * 0.11, color);
            }
            Icon::Shader => {
                painter.circle_stroke(c, s * 0.62, line);
                painter.circle_filled(p(-0.22, -0.22), s * 0.16, color);
                poly(&[p(0.05, 0.55), p(0.35, 0.42), p(0.52, 0.2), p(0.6, -0.05)]);
            }
            Icon::Display => {
                painter.circle_stroke(c, s * 0.3, line);
                for k in 0..8 {
                    let a = k as f32 * std::f32::consts::FRAC_PI_4;
                    let d = vec2(a.cos(), a.sin());
                    painter.line_segment([c + d * s * 0.48, c + d * s * 0.68], line);
                }
            }
            Icon::Keyboard => {
                painter.rect_stroke(Rect::from_min_max(p(-0.7, -0.4), p(0.7, 0.45)), 2.0, line, eframe::egui::StrokeKind::Middle);
                for (x, y) in [(-0.4, -0.1), (0.0, -0.1), (0.4, -0.1)] {
                    painter.circle_filled(p(x, y), s * 0.07, color);
                }
                poly(&[p(-0.35, 0.2), p(0.35, 0.2)]);
            }
            Icon::Log => {
                for (dy, w) in [(-0.45, 0.5), (-0.15, 0.3), (0.15, 0.55), (0.45, 0.2)] {
                    painter.circle_filled(p(-0.55, dy), s * 0.08, color);
                    poly(&[p(-0.3, dy), p(-0.3 + w + 0.3, dy)]);
                }
            }
            Icon::Assets => {
                for (x, y) in [(-0.33, -0.33), (0.33, -0.33), (-0.33, 0.33)] {
                    painter.rect_stroke(Rect::from_center_size(p(x, y), vec2(s * 0.5, s * 0.5)), 1.0, line, eframe::egui::StrokeKind::Middle);
                }
                painter.circle_filled(p(0.33, 0.33), s * 0.25, color);
            }
            Icon::Bake => {
                poly(&[p(-0.65, 0.5), p(0.65, 0.5)]);
                for x in [-0.4, 0.0, 0.4] {
                    poly(&[p(x, -0.6), p(x, 0.25)]);
                    poly(&[p(x - 0.15, 0.1), p(x, 0.25), p(x + 0.15, 0.1)]);
                }
            }
            Icon::Interface => {
                for (dy, x) in [(-0.4, 0.2), (0.0, -0.3), (0.4, 0.4)] {
                    poly(&[p(-0.6, dy), p(0.6, dy)]);
                    painter.circle_filled(p(x, dy), s * 0.14, color);
                }
            }
            Icon::Viewport => {
                poly(&[p(0.0, -0.6), p(0.6, -0.3), p(0.6, 0.35), p(0.0, 0.65), p(-0.6, 0.35), p(-0.6, -0.3), p(0.0, -0.6)]);
                poly(&[p(-0.6, -0.3), p(0.0, 0.0), p(0.6, -0.3)]);
                poly(&[p(0.0, 0.0), p(0.0, 0.65)]);
            }
            Icon::More => {
                for x in [-0.45, 0.0, 0.45] {
                    painter.circle_filled(p(x, 0.0), s * 0.11, color);
                }
            }
        }
    }
}
