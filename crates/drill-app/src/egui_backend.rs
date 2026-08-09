//! Thin egui consumer for backend-neutral display lists.

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

pub(crate) fn paint(painter: &egui::Painter, origin: Pos2, list: &DisplayList) {
    paint_filtered(painter, origin, list, true);
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

fn paint_filtered(painter: &egui::Painter, origin: Pos2, list: &DisplayList, dots: bool) {
    for command in list.commands() {
        if !dots && matches!(command, DrawCmd::Dot { .. }) {
            continue;
        }
        paint_command(painter, origin, list, command);
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
        } => {
            let center = position(origin, center);
            painter.circle_filled(center, radius, color(fill));
            painter.circle_stroke(center, radius, Stroke::new(1.0, color(stroke)));
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
