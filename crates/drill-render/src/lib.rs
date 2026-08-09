//! Backend-neutral, deterministic drawing commands for DrillForge.
//!
//! Builders own no GPU or UI state. A display list can therefore be consumed by
//! the live renderer, SVG/PDF exporters, and the offline video rasterizer.

use drill_core::{Document, GridStyle, Point};
use std::fmt::Write as _;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub min: Vec2,
    pub max: Vec2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextSpan {
    pub start: u32,
    pub len: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrawCmd {
    FieldFill {
        rect: Rect,
        fill: Rgba,
    },
    Line {
        a: Vec2,
        b: Vec2,
        width: f32,
        color: Rgba,
    },
    Dot {
        center: Vec2,
        radius: f32,
        fill: Rgba,
        stroke: Rgba,
    },
    Text {
        at: Vec2,
        text: TextSpan,
        size: f32,
        color: Rgba,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Layer {
    FieldFill,
    GridMinor,
    GridMajor,
    Hash,
    FieldText,
    Trail,
    Highlight,
    Dot,
    DotLabel,
    Marker,
    Overlay,
}

impl Layer {
    pub const COUNT: usize = 11;
    pub const ALL: [Self; Self::COUNT] = [
        Self::FieldFill,
        Self::GridMinor,
        Self::GridMajor,
        Self::Hash,
        Self::FieldText,
        Self::Trail,
        Self::Highlight,
        Self::Dot,
        Self::DotLabel,
        Self::Marker,
        Self::Overlay,
    ];
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Viewport {
    pub size: Vec2,
    pub ui_scale: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuildStats {
    pub performers_total: u32,
    pub dots_emitted: u32,
    pub labels_emitted: u32,
    pub dropped_nonfinite: u32,
    pub truncated: bool,
}

#[derive(Debug, Default)]
pub struct DisplayList {
    commands: Vec<DrawCmd>,
    text: String,
    layer_ranges: [Span; Layer::COUNT],
    viewport: Viewport,
    stats: BuildStats,
}

impl DisplayList {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn clear(&mut self) {
        self.commands.clear();
        self.text.clear();
        self.layer_ranges = [Span::default(); Layer::COUNT];
        self.stats = BuildStats::default();
    }
    pub fn commands(&self) -> &[DrawCmd] {
        &self.commands
    }
    pub fn layer(&self, layer: Layer) -> &[DrawCmd] {
        let span = self.layer_ranges[layer as usize];
        &self.commands[span.start as usize..span.end as usize]
    }
    pub fn text(&self, span: TextSpan) -> &str {
        let end = span.start.saturating_add(span.len) as usize;
        self.text.get(span.start as usize..end).unwrap_or("")
    }
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }
    pub fn stats(&self) -> BuildStats {
        self.stats
    }
    pub fn capacities(&self) -> (usize, usize) {
        (self.commands.capacity(), self.text.capacity())
    }
    fn close_layer(&mut self, layer: Layer, start: usize) {
        self.layer_ranges[layer as usize] = Span {
            start: start as u32,
            end: self.commands.len() as u32,
        };
    }
    fn push_text(&mut self, value: &str) -> TextSpan {
        let start = self.text.len();
        self.text.push_str(value);
        TextSpan {
            start: start as u32,
            len: (self.text.len() - start) as u32,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub turf: Rgba,
    pub sideline: Rgba,
    pub major: Rgba,
    pub minor: Rgba,
    pub hash: Rgba,
    pub text: Rgba,
    pub dot_stroke: Rgba,
}

impl Theme {
    pub const SCREEN_DARK: Self = Self {
        turf: Rgba(17, 74, 42, 255),
        sideline: Rgba(225, 232, 236, 255),
        major: Rgba(210, 220, 215, 150),
        minor: Rgba(190, 210, 200, 48),
        hash: Rgba(225, 232, 236, 130),
        text: Rgba(240, 244, 242, 255),
        dot_stroke: Rgba(20, 24, 29, 255),
    };
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderOptions {
    pub margin: f32,
    pub show_step_grid: bool,
    pub show_hashes: bool,
    pub show_labels: bool,
    pub dot_radius: f32,
    pub label_size: f32,
    pub max_minor_lines: u32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            margin: 16.0,
            show_step_grid: true,
            show_hashes: true,
            show_labels: true,
            dot_radius: 5.0,
            label_size: 11.0,
            max_minor_lines: 240,
        }
    }
}

pub struct Scene<'a> {
    pub document: &'a Document,
    pub positions: &'a [Point],
    pub viewport: Viewport,
    pub options: &'a RenderOptions,
    pub theme: &'a Theme,
}

#[derive(Debug, Default)]
pub struct BuildScratch;

/// Builds a 2D field using index multiplication, never cumulative floating-point stepping.
pub fn build_field_2d(scene: &Scene<'_>, _scratch: &mut BuildScratch, out: &mut DisplayList) {
    out.clear();
    out.viewport = scene.viewport;
    out.stats.performers_total = scene.document.performers.len().min(u32::MAX as usize) as u32;
    let grid = &scene.document.grid;
    if !grid.width.is_finite()
        || !grid.height.is_finite()
        || grid.width <= 0.0
        || grid.height <= 0.0
    {
        out.stats.dropped_nonfinite = 1;
        return;
    }
    let margin = scene.options.margin.max(0.0);
    let available = Vec2 {
        x: (scene.viewport.size.x - margin * 2.0).max(1.0),
        y: (scene.viewport.size.y - margin * 2.0).max(1.0),
    };
    let scale = (available.x / grid.width).min(available.y / grid.height);
    let origin = Vec2 {
        x: (scene.viewport.size.x - grid.width * scale) * 0.5,
        y: (scene.viewport.size.y - grid.height * scale) * 0.5,
    };
    let map = |p: Point| Vec2 {
        x: origin.x + p.x * scale,
        y: origin.y + (grid.height - p.y) * scale,
    };
    let field = Rect {
        min: origin,
        max: Vec2 {
            x: origin.x + grid.width * scale,
            y: origin.y + grid.height * scale,
        },
    };

    let mut start = out.commands.len();
    out.commands.push(DrawCmd::FieldFill {
        rect: field,
        fill: scene.theme.turf,
    });
    out.close_layer(Layer::FieldFill, start);

    start = out.commands.len();
    if scene.options.show_step_grid && grid.show_step_grid {
        let dx = grid.horizontal_units / f32::from(grid.horizontal_steps.max(1));
        let dy = grid.vertical_units / f32::from(grid.vertical_steps.max(1));
        let nx = (grid.width / dx).floor().max(0.0) as u32;
        let ny = (grid.height / dy).floor().max(0.0) as u32;
        let total = nx.saturating_add(ny).max(1);
        let stride = total.div_ceil(scene.options.max_minor_lines.max(1)).max(1);
        let mut i = stride;
        while i < nx {
            let x = i as f32 * dx;
            out.commands.push(DrawCmd::Line {
                a: map(Point { x, y: 0.0 }),
                b: map(Point { x, y: grid.height }),
                width: 0.5,
                color: scene.theme.minor,
            });
            i = i.saturating_add(stride);
        }
        let mut i = stride;
        while i < ny {
            let y = i as f32 * dy;
            out.commands.push(DrawCmd::Line {
                a: map(Point { x: 0.0, y }),
                b: map(Point { x: grid.width, y }),
                width: 0.5,
                color: scene.theme.minor,
            });
            i = i.saturating_add(stride);
        }
    }
    out.close_layer(Layer::GridMinor, start);

    start = out.commands.len();
    let major_count = (grid.width / grid.major_line_interval.max(0.001)).floor() as u32;
    for i in 0..=major_count {
        let x = (i as f32 * grid.major_line_interval).min(grid.width);
        out.commands.push(DrawCmd::Line {
            a: map(Point { x, y: 0.0 }),
            b: map(Point { x, y: grid.height }),
            width: 1.0,
            color: scene.theme.major,
        });
    }
    for (a, b) in [
        (
            Point { x: 0.0, y: 0.0 },
            Point {
                x: grid.width,
                y: 0.0,
            },
        ),
        (
            Point {
                x: 0.0,
                y: grid.height,
            },
            Point {
                x: grid.width,
                y: grid.height,
            },
        ),
    ] {
        out.commands.push(DrawCmd::Line {
            a: map(a),
            b: map(b),
            width: 1.5,
            color: scene.theme.sideline,
        });
    }
    out.close_layer(Layer::GridMajor, start);

    start = out.commands.len();
    if scene.options.show_hashes {
        for hash in &grid.hashes {
            if hash.position.is_finite() {
                let y = hash.position.clamp(0.0, grid.height);
                out.commands.push(DrawCmd::Line {
                    a: map(Point { x: 0.0, y }),
                    b: map(Point { x: grid.width, y }),
                    width: hash.weight.max(0.25),
                    color: scene.theme.hash,
                });
            }
        }
    }
    out.close_layer(Layer::Hash, start);
    for layer in [Layer::FieldText, Layer::Trail, Layer::Highlight] {
        out.close_layer(layer, out.commands.len());
    }

    start = out.commands.len();
    for (performer, point) in scene.document.performers.iter().zip(scene.positions) {
        if point.x.is_finite() && point.y.is_finite() {
            out.commands.push(DrawCmd::Dot {
                center: map(*point),
                radius: scene.options.dot_radius * scene.viewport.ui_scale.max(0.1),
                fill: {
                    let color = performer.resolved_color(&scene.document.sections);
                    Rgba(color[0], color[1], color[2], 255)
                },
                stroke: scene.theme.dot_stroke,
            });
            out.stats.dots_emitted += 1;
        } else {
            out.stats.dropped_nonfinite += 1;
        }
    }
    if scene.positions.len() != scene.document.performers.len() {
        out.stats.truncated = true;
    }
    out.close_layer(Layer::Dot, start);

    start = out.commands.len();
    if scene.options.show_labels {
        for (performer, point) in scene.document.performers.iter().zip(scene.positions) {
            if point.x.is_finite() && point.y.is_finite() {
                let text = out.push_text(&performer.label);
                let at = map(*point);
                out.commands.push(DrawCmd::Text {
                    at: Vec2 {
                        x: at.x,
                        y: at.y - scene.options.dot_radius - 2.0,
                    },
                    text,
                    size: scene.options.label_size * scene.viewport.ui_scale.max(0.1),
                    color: scene.theme.text,
                });
                out.stats.labels_emitted += 1;
            }
        }
    }
    out.close_layer(Layer::DotLabel, start);
    out.close_layer(Layer::Marker, out.commands.len());
    out.close_layer(Layer::Overlay, out.commands.len());
    let _ = grid.style == GridStyle::Dots;
}

/// Builds the same drill through a saved camera pose. This CPU path is used by
/// offline export so every frame is reproducible and independent of the UI/GPU.
pub fn build_field_camera(
    scene: &Scene<'_>,
    camera: &drill_core::camera::CameraPose,
    out: &mut DisplayList,
) {
    out.clear();
    out.viewport = scene.viewport;
    let size = scene.viewport.size;
    let project = |p: Point| {
        camera
            .project(drill_core::camera::field_to_world(p, 0.0), size.x, size.y)
            .map(|p| Vec2 { x: p[0], y: p[1] })
    };
    let mut start = out.commands.len();
    out.commands.push(DrawCmd::FieldFill {
        rect: Rect {
            min: Vec2 { x: 0.0, y: 0.0 },
            max: size,
        },
        fill: Rgba(9, 17, 26, 255),
    });
    out.close_layer(Layer::FieldFill, start);

    start = out.commands.len();
    let grid = &scene.document.grid;
    let line = |out: &mut DisplayList, a: Point, b: Point, width: f32, color: Rgba| {
        if let (Some(a), Some(b)) = (project(a), project(b)) {
            out.commands.push(DrawCmd::Line { a, b, width, color });
        }
    };
    let x_lines = (grid.width / grid.major_line_interval.max(0.001)).floor() as u32;
    for index in 0..=x_lines {
        let x = (index as f32 * grid.major_line_interval).min(grid.width);
        line(
            out,
            Point { x, y: 0.0 },
            Point { x, y: grid.height },
            1.0,
            scene.theme.major,
        );
    }
    for y in [0.0, grid.height] {
        line(
            out,
            Point { x: 0.0, y },
            Point { x: grid.width, y },
            1.5,
            scene.theme.sideline,
        );
    }
    out.close_layer(Layer::GridMajor, start);
    for layer in [
        Layer::GridMinor,
        Layer::Hash,
        Layer::FieldText,
        Layer::Trail,
        Layer::Highlight,
    ] {
        out.close_layer(layer, out.commands.len());
    }

    start = out.commands.len();
    for (performer, point) in scene.document.performers.iter().zip(scene.positions) {
        if let Some(center) = project(*point) {
            let color = performer.resolved_color(&scene.document.sections);
            out.commands.push(DrawCmd::Dot {
                center,
                radius: scene.options.dot_radius * scene.viewport.ui_scale.max(0.1),
                fill: Rgba(color[0], color[1], color[2], 255),
                stroke: scene.theme.dot_stroke,
            });
            out.stats.dots_emitted += 1;
        } else {
            out.stats.dropped_nonfinite += 1;
        }
    }
    out.close_layer(Layer::Dot, start);
    for layer in [Layer::DotLabel, Layer::Marker, Layer::Overlay] {
        out.close_layer(layer, out.commands.len());
    }
}

/// Serialize a backend-neutral display list as standalone SVG. This is the
/// canonical vector export path: it applies no independent field transform.
pub fn display_list_svg(list: &DisplayList) -> String {
    let size = list.viewport().size;
    let width = size.x.max(1.0);
    let height = size.y.max(1.0);
    let mut out = String::with_capacity(list.commands().len() * 96 + 128);
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">",
        svg_number(width),
        svg_number(height),
        svg_number(width),
        svg_number(height)
    );
    for command in list.commands() {
        match *command {
            DrawCmd::FieldFill { rect, fill } => {
                let _ = writeln!(
                    out,
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
                    svg_number(rect.min.x),
                    svg_number(rect.min.y),
                    svg_number(rect.max.x - rect.min.x),
                    svg_number(rect.max.y - rect.min.y),
                    svg_color(fill)
                );
            }
            DrawCmd::Line { a, b, width, color } => {
                let _ = writeln!(
                    out,
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{}\"/>",
                    svg_number(a.x),
                    svg_number(a.y),
                    svg_number(b.x),
                    svg_number(b.y),
                    svg_color(color),
                    svg_number(width)
                );
            }
            DrawCmd::Dot {
                center,
                radius,
                fill,
                stroke,
            } => {
                let _ = writeln!(
                    out,
                    "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\" stroke=\"{}\"/>",
                    svg_number(center.x),
                    svg_number(center.y),
                    svg_number(radius),
                    svg_color(fill),
                    svg_color(stroke)
                );
            }
            DrawCmd::Text {
                at,
                text,
                size,
                color,
            } => {
                let _ = writeln!(
                    out,
                    "<text x=\"{}\" y=\"{}\" fill=\"{}\" font-family=\"sans-serif\" font-size=\"{}\" text-anchor=\"middle\">{}</text>",
                    svg_number(at.x),
                    svg_number(at.y),
                    svg_color(color),
                    svg_number(size),
                    svg_escape(list.text(text))
                );
            }
        }
    }
    out.push_str("</svg>");
    out
}

/// Build and serialize one set through the same DisplayList used on screen.
pub fn set_svg(document: &Document, set_index: usize) -> String {
    let width = 1000.0;
    let height = if document.grid.width > 0.0 {
        (width * document.grid.height / document.grid.width).max(1.0)
    } else {
        500.0
    };
    field_svg(document, set_index, width, height)
}

pub fn field_svg(document: &Document, set_index: usize, width: f32, height: f32) -> String {
    let mut list = DisplayList::new();
    if let Some(set) = document.sets.get(set_index) {
        build_field_2d(
            &Scene {
                document,
                positions: &set.positions,
                viewport: Viewport {
                    size: Vec2 {
                        x: width.max(1.0),
                        y: height.max(1.0),
                    },
                    ui_scale: 1.0,
                },
                options: &RenderOptions::default(),
                theme: &Theme::SCREEN_DARK,
            },
            &mut BuildScratch,
            &mut list,
        );
    } else {
        list.viewport = Viewport {
            size: Vec2 {
                x: width.max(1.0),
                y: height.max(1.0),
            },
            ui_scale: 1.0,
        };
    }
    display_list_svg(&list)
}

fn svg_number(value: f32) -> String {
    if !value.is_finite() {
        return "0".into();
    }
    format!("{value:.3}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn svg_color(color: Rgba) -> String {
    if color.3 == 255 {
        format!("#{:02x}{:02x}{:02x}", color.0, color.1, color.2)
    } else {
        format!(
            "rgba({},{},{},{:.3})",
            color.0,
            color.1,
            color.2,
            f32::from(color.3) / 255.0
        )
    }
}

fn svg_escape(value: &str) -> String {
    value.chars().fold(String::new(), |mut out, c| {
        out.push_str(match c {
            '&' => "&amp;",
            '<' => "&lt;",
            '>' => "&gt;",
            '\"' => "&quot;",
            '\'' => "&#39;",
            _ => {
                out.push(c);
                return out;
            }
        });
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_demo(out: &mut DisplayList) {
        let doc = Document::demo(4, 10);
        let options = RenderOptions::default();
        let scene = Scene {
            positions: &doc.sets[0].positions,
            document: &doc,
            viewport: Viewport {
                size: Vec2 {
                    x: 1280.0,
                    y: 720.0,
                },
                ui_scale: 1.0,
            },
            options: &options,
            theme: &Theme::SCREEN_DARK,
        };
        build_field_2d(&scene, &mut BuildScratch, out);
    }

    #[test]
    fn emits_field_and_every_performer() {
        let mut out = DisplayList::new();
        build_demo(&mut out);
        assert_eq!(out.layer(Layer::FieldFill).len(), 1);
        assert_eq!(out.stats().dots_emitted, 40);
        assert_eq!(out.stats().labels_emitted, 40);
    }

    #[test]
    fn output_is_deterministic() {
        let mut a = DisplayList::new();
        let mut b = DisplayList::new();
        build_demo(&mut a);
        build_demo(&mut b);
        assert_eq!(a.commands(), b.commands());
    }

    #[test]
    fn warm_build_reuses_allocations() {
        let mut out = DisplayList::new();
        build_demo(&mut out);
        let capacity = out.capacities();
        for _ in 0..100 {
            build_demo(&mut out);
            assert_eq!(out.capacities(), capacity);
        }
    }

    #[test]
    fn invalid_text_span_is_safe() {
        let out = DisplayList::new();
        assert_eq!(out.text(TextSpan { start: 99, len: 4 }), "");
    }

    #[test]
    fn svg_backend_is_deterministic_and_uses_display_list_coordinates() {
        let mut list = DisplayList::new();
        build_demo(&mut list);
        let a = display_list_svg(&list);
        let b = display_list_svg(&list);
        assert_eq!(a, b);
        let first = list.layer(Layer::Dot).first().expect("demo dot");
        let DrawCmd::Dot { center, .. } = first else {
            panic!("dot layer invariant")
        };
        assert!(a.contains(&format!(
            "cx=\"{}\" cy=\"{}\"",
            svg_number(center.x),
            svg_number(center.y)
        )));
        assert_eq!(
            a.matches("<circle").count(),
            list.stats().dots_emitted as usize
        );
    }

    #[test]
    fn svg_backend_escapes_labels() {
        let mut doc = Document::demo(1, 1);
        doc.performers[0].label = "A&B<1>".into();
        let svg = field_svg(&doc, 0, 400.0, 200.0);
        assert!(svg.contains("A&amp;B&lt;1&gt;"));
    }

    #[test]
    fn svg_backend_golden_minimal_document() {
        let mut list = DisplayList::new();
        list.viewport = Viewport {
            size: Vec2 { x: 20.0, y: 10.0 },
            ui_scale: 1.0,
        };
        list.commands.push(DrawCmd::Dot {
            center: Vec2 { x: 2.5, y: 3.0 },
            radius: 1.25,
            fill: Rgba(255, 128, 0, 255),
            stroke: Rgba(0, 0, 0, 255),
        });
        assert_eq!(
            display_list_svg(&list),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"10\" viewBox=\"0 0 20 10\">\n\
<circle cx=\"2.5\" cy=\"3\" r=\"1.25\" fill=\"#ff8000\" stroke=\"#000000\"/>\n\
</svg>"
        );
    }
}
