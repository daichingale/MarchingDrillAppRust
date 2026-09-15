//! Thin egui consumer for backend-neutral display lists.

use drill_core::Symbol;
use drill_render::{DisplayList, DrawCmd, Layer, Rgba, Vec2};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke};

fn color(value: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(value.0, value.1, value.2, value.3)
}

fn position(origin: Pos2, value: Vec2) -> Pos2 {
    Pos2::new(origin.x + value.x, origin.y + value.y)
}

fn rectangle(origin: Pos2, value: drill_render::Rect) -> Rect {
    Rect::from_min_max(position(origin, value.min), position(origin, value.max))
}

/// Paints the whole display list for the CPU-only path (no GPU dot
/// instancing). Iterates layer-by-layer in `Layer::ALL`'s semantic order
/// rather than `list.commands()`'s raw insertion order, so a layer that was
/// appended late in the frame (e.g. `drill_render::append_trails`, called
/// after `build_field_2d` has already emitted the `Dot`/`DotLabel` commands)
/// still paints underneath layers that are semantically "above" it, matching
/// what the GPU path already does via `paint_gpu_background` /
/// `paint_gpu_foreground`. See `append_trails`'s doc comment for the
/// insertion-order-vs-layer-order distinction this works around.
pub(crate) fn paint(painter: &egui::Painter, origin: Pos2, list: &DisplayList) {
    paint_layers(painter, origin, list, &Layer::ALL);
}

pub(crate) fn paint_gpu_background(painter: &egui::Painter, origin: Pos2, list: &DisplayList) {
    paint_layers(
        painter,
        origin,
        list,
        &[
            Layer::FieldFill,
            Layer::GridMinor,
            Layer::GridMajor,
            Layer::Hash,
            Layer::FieldText,
            Layer::Heatmap,
            Layer::Trail,
            Layer::Highlight,
        ],
    );
}

pub(crate) fn paint_gpu_foreground(painter: &egui::Painter, origin: Pos2, list: &DisplayList) {
    paint_layers(
        painter,
        origin,
        list,
        &[Layer::DotLabel, Layer::Marker, Layer::Overlay],
    );
}

fn paint_layers(painter: &egui::Painter, origin: Pos2, list: &DisplayList, layers: &[Layer]) {
    for command in layers.iter().flat_map(|layer| list.layer(*layer)) {
        paint_command(painter, origin, list, command);
    }
}

/// Reference implementation of the shared marker geometry documented on
/// `drill_render::DrawCmd::Dot::symbol`. `center` is already in screen space
/// (the caller has applied the paint origin). Polygon shapes reuse
/// `drill_render::symbol_points` so this stays bit-identical to what the SVG
/// exporter draws, just rasterized instead of pathed.
fn paint_symbol(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    fill: Color32,
    stroke: Color32,
    symbol: Symbol,
) {
    match symbol {
        Symbol::Circle => {
            painter.circle_filled(center, radius, fill);
            painter.circle_stroke(center, radius, Stroke::new(1.0, stroke));
        }
        Symbol::Square => {
            let half = radius * 0.8;
            let rect = Rect::from_center_size(center, egui::Vec2::splat(half * 2.0));
            painter.rect_filled(rect, 0.0, fill);
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(1.0, stroke),
                egui::StrokeKind::Inside,
            );
        }
        Symbol::Cross => {
            let arm = radius;
            let width = (radius * 0.4).max(1.0);
            painter.line_segment(
                [
                    Pos2::new(center.x - arm, center.y - arm),
                    Pos2::new(center.x + arm, center.y + arm),
                ],
                Stroke::new(width, stroke),
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - arm, center.y + arm),
                    Pos2::new(center.x + arm, center.y - arm),
                ],
                Stroke::new(width, stroke),
            );
        }
        Symbol::Triangle | Symbol::Diamond | Symbol::Star => {
            let points = drill_render::symbol_points(
                Vec2 {
                    x: center.x,
                    y: center.y,
                },
                radius,
                symbol,
            )
            .into_iter()
            .map(|p| Pos2::new(p.x, p.y))
            .collect();
            painter.add(egui::Shape::convex_polygon(
                points,
                fill,
                Stroke::new(1.0, stroke),
            ));
        }
    }
}

fn paint_command(painter: &egui::Painter, origin: Pos2, list: &DisplayList, command: &DrawCmd) {
    match *command {
        DrawCmd::FieldFill { rect, fill } => {
            painter.rect_filled(rectangle(origin, rect), 4.0, color(fill));
        }
        DrawCmd::Line {
            a,
            b,
            width,
            color: line,
        } => {
            painter.line_segment(
                [position(origin, a), position(origin, b)],
                Stroke::new(width, color(line)),
            );
        }
        DrawCmd::Dot {
            center,
            radius,
            fill,
            stroke,
            symbol,
        } => {
            paint_symbol(
                painter,
                position(origin, center),
                radius,
                color(fill),
                color(stroke),
                symbol,
            );
        }
        DrawCmd::Text {
            at,
            text,
            size,
            color: text_color,
        } => {
            painter.text(
                position(origin, at),
                egui::Align2::CENTER_BOTTOM,
                list.text(text),
                egui::FontId::monospace(size),
                color(text_color),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinates_are_offset_by_paint_origin() {
        assert_eq!(
            position(Pos2::new(100.0, 20.0), Vec2 { x: 4.0, y: 7.0 }),
            Pos2::new(104.0, 27.0)
        );
    }

    #[test]
    fn rgba_conversion_preserves_channels() {
        assert_eq!(
            color(Rgba(1, 2, 3, 4)),
            Color32::from_rgba_unmultiplied(1, 2, 3, 4)
        );
    }
}
