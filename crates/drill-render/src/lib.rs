//! Backend-neutral, deterministic drawing commands for DrillForge.
//!
//! Builders own no GPU or UI state. A display list can therefore be consumed by
//! the live renderer, SVG/PDF exporters, and the offline video rasterizer.

use drill_core::show_heatmap::FieldOccupancy;
use drill_core::transition::{self, TransitionPlan};
use drill_core::{Document, GridStyle, PerformerId, Point, SetId};
use std::cell::RefCell;
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

/// Design note (trails): a trail is represented as a run of plain
/// [`DrawCmd::Line`] segments rather than a dedicated `DrawCmd::Trail { .. }`
/// variant. Adding a new `DrawCmd` variant would force every consumer of this
/// enum (the live egui/wgpu painter, the SVG exporter, and eventually a PDF
/// backend) to grow a matching match-arm before it could render anything
/// again, which is a lot of blast radius for what is, structurally, just a
/// polyline with a color per segment. Reusing `Line` keeps every existing
/// backend working with zero changes -- see [`append_trails`] for how the
/// per-segment color is chosen. If a future need arises for something a
/// sequence of colored lines cannot express (e.g. a single GPU-instanced
/// gradient stroke for very long trails), revisit this decision then.
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
    Heatmap,
    Trail,
    Highlight,
    Dot,
    DotLabel,
    Marker,
    Overlay,
}

impl Layer {
    pub const COUNT: usize = 12;
    pub const ALL: [Self; Self::COUNT] = [
        Self::FieldFill,
        Self::GridMinor,
        Self::GridMajor,
        Self::Hash,
        Self::FieldText,
        Self::Heatmap,
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
    /// Commands in semantic paint order. Late-appended analytical overlays
    /// (heatmaps and trails) therefore remain below performers in every
    /// backend, rather than only in the live egui painter.
    pub fn paint_order(&self) -> impl Iterator<Item = &DrawCmd> {
        Layer::ALL
            .iter()
            .flat_map(move |layer| self.layer(*layer).iter())
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

/// Which performers' movement trails a caller wants drawn this frame.
/// Consumed by callers that invoke [`append_trails`] (not by
/// [`build_field_2d`] itself, which does not yet call `append_trails` --
/// see that function's doc comment for why). Lives on `RenderOptions` now so
/// the on/off + scope knob has one obvious home once that wiring lands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrailSelection {
    /// Draw no trails.
    #[default]
    None,
    /// Draw a trail only for the currently-selected performer(s).
    Selected,
    /// Draw a trail for every performer.
    All,
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
    /// Session viewport centre in field units. `None` keeps the complete
    /// field fitted, which is the stable default for exports and previews.
    pub field_center: Option<Point>,
    /// Session-only magnification of the 2D field. This deliberately lives
    /// in render options rather than the document: changing the view must not
    /// create an edit or affect another collaborator's saved drill.
    pub field_zoom: f32,
    /// Trail visibility scope; see [`TrailSelection`]. Defaults to `None`,
    /// so existing callers built via `..RenderOptions::default()` keep
    /// today's behavior (no trails) unchanged.
    pub show_trails_for: TrailSelection,
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
            field_center: None,
            field_zoom: 1.0,
            show_trails_for: TrailSelection::None,
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

/// The single field<->screen mapping every consumer of a 2D field must share:
/// the live egui view, the SVG/PDF exporters, and hit-testing/dragging in the
/// app layer. Computing this independently in more than one place is how the
/// dot the user sees and the dot the app hit-tests against drift apart.
///
/// Aspect ratio is preserved (`scale` is the smaller of the two axis scales)
/// and the field is centered in any letterboxed remainder, matching how
/// [`build_field_2d`] has always rendered it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldMap {
    pub scale: f32,
    pub origin: Vec2,
    pub grid_height: f32,
}

impl FieldMap {
    /// `viewport_size` and `margin` are in the same units as the `Vec2`s this
    /// map produces (typically screen pixels). `grid_width`/`grid_height` are
    /// document units (yards/meters). Falls back to `scale = 1.0` centered at
    /// the origin if the grid dimensions are non-finite or non-positive, so
    /// callers never divide by zero.
    pub fn new(grid_width: f32, grid_height: f32, viewport_size: Vec2, margin: f32) -> Self {
        if !grid_width.is_finite()
            || !grid_height.is_finite()
            || grid_width <= 0.0
            || grid_height <= 0.0
        {
            return Self {
                scale: 1.0,
                origin: Vec2 { x: 0.0, y: 0.0 },
                grid_height: grid_height.max(f32::EPSILON),
            };
        }
        let margin = margin.max(0.0);
        let available = Vec2 {
            x: (viewport_size.x - margin * 2.0).max(1.0),
            y: (viewport_size.y - margin * 2.0).max(1.0),
        };
        let scale = (available.x / grid_width).min(available.y / grid_height);
        let origin = Vec2 {
            x: (viewport_size.x - grid_width * scale) * 0.5,
            y: (viewport_size.y - grid_height * scale) * 0.5,
        };
        Self {
            scale,
            origin,
            grid_height,
        }
    }

    /// Constructs the same aspect-preserving map as [`Self::new`], but pins a
    /// field-space centre to the viewport centre and applies magnification.
    /// This is the one map used by painting and all direct manipulation.
    pub fn with_view(
        grid_width: f32,
        grid_height: f32,
        viewport_size: Vec2,
        margin: f32,
        center: Option<Point>,
        zoom: f32,
    ) -> Self {
        let mut map = Self::new(grid_width, grid_height, viewport_size, margin);
        let zoom = if zoom.is_finite() {
            zoom.clamp(0.25, 8.0)
        } else {
            1.0
        };
        let center = center.unwrap_or(Point {
            x: grid_width * 0.5,
            y: grid_height * 0.5,
        });
        map.scale *= zoom;
        map.origin = Vec2 {
            x: viewport_size.x * 0.5 - center.x * map.scale,
            y: viewport_size.y * 0.5 - (grid_height - center.y) * map.scale,
        };
        map
    }

    /// Field units -> viewport-local pixels. Front sideline (`y = 0`) maps to
    /// the bottom edge of the field rect.
    pub fn map(&self, p: Point) -> Vec2 {
        Vec2 {
            x: self.origin.x + p.x * self.scale,
            y: self.origin.y + (self.grid_height - p.y) * self.scale,
        }
    }

    /// Inverse of [`map`](Self::map): viewport-local pixels -> field units.
    /// Not clamped to the field bounds; callers that need points confined to
    /// the field should clamp/snap the result themselves (e.g. via
    /// `GridConfig::snap`).
    pub fn unmap(&self, screen: Vec2) -> Point {
        let scale = self.scale.max(f32::EPSILON);
        Point {
            x: (screen.x - self.origin.x) / scale,
            y: self.grid_height - (screen.y - self.origin.y) / scale,
        }
    }
}

/// Reusable scratch for [`append_trails`], kept warm across calls in a
/// `thread_local` rather than a caller-supplied parameter so the public
/// function signature stays exactly `(document, set_id, performer_ids,
/// field_map, samples_per_trail, out)`. Every `Vec` here is grown with
/// `reserve`/`resize` against its *current* capacity, never replaced with a
/// fresh `Vec::new()`, so repeated calls with the same performer-count and
/// sample-count settle into zero additional heap traffic.
#[derive(Default)]
struct TrailCache {
    plan: TransitionPlan,
    /// Scratch for one `transition::eval` call: every lane's position at a
    /// single sampled count, index-aligned with `Document::performers`.
    all_positions: Vec<Point>,
    /// `performer_ids[i]` -> its index into `Document::performers`/the
    /// compiled plan's lanes, or `usize::MAX` for an id that no longer
    /// exists. Resolved once per `append_trails` call, not once per sample.
    lane_indices: Vec<usize>,
    /// Column-major `[performer][sample]` matrix of sampled points, sized
    /// `performer_ids.len() * point_count`.
    samples: Vec<Point>,
}

thread_local! {
    static TRAIL_CACHE: RefCell<TrailCache> = RefCell::new(TrailCache::default());
}

/// Cool(slow) -> mid -> warm(fast) color ramp for trail speed shading, hand
/// rolled to avoid pulling in a colormap crate for three lerps. `t` should
/// already be normalized to `0.0..=1.0`; non-finite input is treated as
/// `0.0` (the cool end) so a stray NaN speed never produces a NaN color.
fn trail_speed_color(t: f32) -> Rgba {
    const COOL: Rgba = Rgba(66, 133, 244, 255);
    const MID: Rgba = Rgba(52, 199, 89, 255);
    const WARM: Rgba = Rgba(234, 67, 53, 255);
    let t = if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if t < 0.5 {
        lerp_rgba(COOL, MID, t * 2.0)
    } else {
        lerp_rgba(MID, WARM, (t - 0.5) * 2.0)
    }
}

fn lerp_rgba(a: Rgba, b: Rgba, u: f32) -> Rgba {
    let u = u.clamp(0.0, 1.0);
    let channel = |x: u8, y: u8| {
        (f32::from(x) + (f32::from(y) - f32::from(x)) * u)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Rgba(
        channel(a.0, b.0),
        channel(a.1, b.1),
        channel(a.2, b.2),
        channel(a.3, b.3),
    )
}

/// Samples each requested performer's travel path for the transition leaving
/// `set_id`, and appends it to `out`'s `Trail` layer as speed-shaded
/// [`DrawCmd::Line`] segments (see the `DrawCmd` doc comment for why a new
/// variant was not introduced instead).
///
/// Positions come from [`Document::plan_transition`] +
/// [`transition::eval`], the same compiled-plan path real-time playback
/// uses, so curved/path/arc routes are sampled faithfully rather than
/// re-derived as a straight-line lerp between the two sets' dots. `moves`
/// (the pure-movement count, excluding any ensemble hold) is sampled evenly
/// `samples_per_trail.max(2)` times from `0` to `moves` inclusive, so the
/// trail always includes both the departure and arrival positions.
///
/// `field_map` must be the exact [`FieldMap`] the caller used to place this
/// frame's dots and grid lines -- building an independent one here is
/// exactly the kind of coordinate drift this type exists to prevent.
///
/// Each performer's segment colors are a cool(slow)/warm(fast) gradient
/// normalized against *that performer's own* min/max segment speed within
/// this one trail, not against the whole cast. That is a deliberate
/// trade-off: it makes gate accelerations/decelerations and curve easing
/// pop visually for every performer, including ones whose absolute speed is
/// unremarkable next to the rest of the ensemble. A performer moving at a
/// perfectly constant speed (however fast) therefore renders in a single
/// neutral mid-gradient color rather than pegged to one extreme.
///
/// This function appends to `out` without clearing it first -- call it
/// after [`build_field_2d`] has built the rest of the frame's scene so both
/// share one [`DisplayList`], then it updates the `Trail` layer's span to
/// point at what it just pushed. That span is correct for any layer-aware
/// painter (i.e. one that iterates `out.layer(Layer::X)` per layer, as the
/// GPU backend does) regardless of where in the underlying command vector
/// the bytes physically land. A painter that instead ignores layers and
/// replays `out.commands()` strictly in insertion order will draw these
/// trails *after* (visually: on top of) the dots, since `build_field_2d`
/// already appended its Dot/Label commands earlier in the vector. Fixing
/// that for such a painter means having `build_field_2d` call this
/// function itself while the `Trail` layer slot is still open -- left as
/// follow-up integration work, since that requires plumbing a `SetId` and a
/// performer selection into `Scene`/`RenderOptions`.
///
/// Never panics: an empty `performer_ids`, a `set_id` that names no set (or
/// the last set, which has no "next" to move toward), an unknown performer
/// id, or non-finite/degenerate positions all degrade to skipping that
/// performer/segment rather than trapping.
pub fn append_trails(
    document: &Document,
    set_id: SetId,
    performer_ids: &[PerformerId],
    field_map: &FieldMap,
    samples_per_trail: u16,
    out: &mut DisplayList,
) {
    if performer_ids.is_empty() {
        return;
    }
    let point_count = samples_per_trail.max(2) as usize;

    TRAIL_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let TrailCache {
            plan,
            all_positions,
            lane_indices,
            samples,
        } = &mut *cache;

        document.plan_transition(set_id, plan);
        let lane_count = plan.performer_count();
        if lane_count == 0 {
            return;
        }
        let moves = f32::from(plan.counts().moves);

        lane_indices.clear();
        lane_indices.reserve(performer_ids.len().saturating_sub(lane_indices.capacity()));
        lane_indices.extend(performer_ids.iter().map(|id| {
            document
                .performers
                .iter()
                .position(|performer| performer.id == *id)
                .filter(|&index| index < lane_count)
                .unwrap_or(usize::MAX)
        }));

        let needed = performer_ids.len().saturating_mul(point_count);
        samples.clear();
        samples.reserve(needed.saturating_sub(samples.capacity()));
        samples.resize(
            needed,
            Point {
                x: f32::NAN,
                y: f32::NAN,
            },
        );

        let last_sample = (point_count - 1) as f32;
        for s in 0..point_count {
            let local_count = (s as f32 / last_sample) * moves;
            transition::eval(plan, local_count, all_positions);
            for (local_idx, &lane_index) in lane_indices.iter().enumerate() {
                if lane_index == usize::MAX {
                    continue;
                }
                if let Some(&p) = all_positions.get(lane_index) {
                    samples[local_idx * point_count + s] = p;
                }
            }
        }

        let start = out.commands.len();
        for (local_idx, &lane_index) in lane_indices.iter().enumerate() {
            if lane_index == usize::MAX {
                continue;
            }
            let row = &samples[local_idx * point_count..(local_idx + 1) * point_count];
            let finite_segment = |a: Point, b: Point| {
                a.x.is_finite() && a.y.is_finite() && b.x.is_finite() && b.y.is_finite()
            };
            let mut min_len = f32::INFINITY;
            let mut max_len = f32::NEG_INFINITY;
            for pair in row.windows(2) {
                if finite_segment(pair[0], pair[1]) {
                    let d = distance(pair[0], pair[1]);
                    if d.is_finite() {
                        min_len = min_len.min(d);
                        max_len = max_len.max(d);
                    }
                }
            }
            let span = max_len - min_len;
            for pair in row.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                if !finite_segment(a, b) {
                    continue;
                }
                let d = distance(a, b);
                let t = if span > f32::EPSILON && d.is_finite() {
                    (d - min_len) / span
                } else {
                    0.5
                };
                out.commands.push(DrawCmd::Line {
                    a: field_map.map(a),
                    b: field_map.map(b),
                    width: 2.0,
                    color: trail_speed_color(t),
                });
            }
        }
        out.close_layer(Layer::Trail, start);
    });
}

#[inline]
fn distance(a: Point, b: Point) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    (dx * dx + dy * dy).sqrt()
}

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
    let field_map = FieldMap::with_view(
        grid.width,
        grid.height,
        scene.viewport.size,
        scene.options.margin,
        scene.options.field_center,
        scene.options.field_zoom,
    );
    let map = |p: Point| field_map.map(p);
    let field = Rect {
        min: field_map.origin,
        max: Vec2 {
            x: field_map.origin.x + grid.width * field_map.scale,
            y: field_map.origin.y + grid.height * field_map.scale,
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

/// "Show DNA": renders a `drill_core::show_heatmap::FieldOccupancy` as
/// semi-transparent, color-graded rectangles appended to the display list's
/// `Heatmap` layer (cold/blue = lightly used, hot/red = heavily used). This
/// layer is below trails, selection highlights, performers and labels.
///
/// Analysis and drawing are kept separate on purpose: `drill-core` computes
/// the grid of occupancy values with no knowledge of screen space, and this
/// function is the only place that knows how to turn a cell into a rect
/// through the shared [`FieldMap`] (so it always agrees with where dots and
/// the grid itself are drawn). Cells with zero dwell time are skipped
/// entirely so the field underneath stays fully visible.
///
/// Must be called after the base scene (e.g. [`build_field_2d`]) has built
/// `out`, since it fills the late-appended `Heatmap` layer range.
pub fn append_heatmap(occupancy: &FieldOccupancy, field_map: &FieldMap, out: &mut DisplayList) {
    let start = out.commands.len();
    for cy in 0..occupancy.cells_y {
        for cx in 0..occupancy.cells_x {
            let value = occupancy.get(cx, cy);
            if value <= 0.0 {
                continue;
            }
            let (min_field, max_field) = occupancy.cell_bounds(cx, cy);
            // `FieldMap::map` flips the y axis (field y=0 is the bottom of
            // the screen rect), so the mapped corners must be re-sorted into
            // a proper min/max screen rect rather than assumed to preserve
            // corner order.
            let a = field_map.map(min_field);
            let b = field_map.map(max_field);
            let rect = Rect {
                min: Vec2 {
                    x: a.x.min(b.x),
                    y: a.y.min(b.y),
                },
                max: Vec2 {
                    x: a.x.max(b.x),
                    y: a.y.max(b.y),
                },
            };
            out.commands.push(DrawCmd::FieldFill {
                rect,
                fill: heat_color(occupancy.normalized(cx, cy)),
            });
        }
    }
    out.close_layer(Layer::Heatmap, start);
}

/// Cold (blue) -> warm (red) gradient through cyan/yellow, with alpha rising
/// alongside intensity so lightly-used cells stay faint overlays instead of
/// fully opaque blocks.
fn heat_color(t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    let (r, g, b) = if t < 0.5 {
        let u = t * 2.0;
        (0.0, u, 1.0 - u) // blue -> green
    } else {
        let u = (t - 0.5) * 2.0;
        (u, 1.0 - u, 0.0) // green -> red
    };
    let alpha = 70.0 + t * 130.0; // 70..=200
    Rgba(
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
        alpha.round() as u8,
    )
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
    for command in list.paint_order() {
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
        list.close_layer(Layer::Dot, 0);
        assert_eq!(
            display_list_svg(&list),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"10\" viewBox=\"0 0 20 10\">\n\
<circle cx=\"2.5\" cy=\"3\" r=\"1.25\" fill=\"#ff8000\" stroke=\"#000000\"/>\n\
</svg>"
        );
    }

    fn single_hot_cell_occupancy() -> FieldOccupancy {
        // 100x53.333 field (drill_core::GridConfig default), sampled once so
        // performer at (5, 5) lands in the bottom-left-most cell.
        let doc = Document::demo(1, 1);
        let params = drill_core::show_heatmap::HeatmapParams {
            cells_x: 10,
            cells_y: 10,
            samples_per_count: 1.0,
        };
        drill_core::show_heatmap::analyze_show_occupancy(&doc, &params)
    }

    #[test]
    fn append_heatmap_only_draws_occupied_cells() {
        let occupancy = single_hot_cell_occupancy();
        let occupied_cells = (0..occupancy.cells_y)
            .flat_map(|cy| (0..occupancy.cells_x).map(move |cx| (cx, cy)))
            .filter(|&(cx, cy)| occupancy.get(cx, cy) > 0.0)
            .count();
        assert!(occupied_cells > 0);

        let mut out = DisplayList::new();
        build_demo(&mut out);
        let field_map = FieldMap::new(
            100.0,
            53.333,
            Vec2 {
                x: 1280.0,
                y: 720.0,
            },
            16.0,
        );
        append_heatmap(&occupancy, &field_map, &mut out);

        let overlay_fills = out
            .layer(Layer::Heatmap)
            .iter()
            .filter(|cmd| matches!(cmd, DrawCmd::FieldFill { .. }))
            .count();
        assert_eq!(overlay_fills, occupied_cells);
    }

    #[test]
    fn append_heatmap_rects_stay_within_field_bounds_and_have_alpha() {
        let occupancy = single_hot_cell_occupancy();
        let mut out = DisplayList::new();
        build_demo(&mut out);
        let field_map = FieldMap::new(
            100.0,
            53.333,
            Vec2 {
                x: 1280.0,
                y: 720.0,
            },
            16.0,
        );
        let field_min = field_map.origin;
        let field_max = Vec2 {
            x: field_map.origin.x + 100.0 * field_map.scale,
            y: field_map.origin.y + 53.333 * field_map.scale,
        };
        append_heatmap(&occupancy, &field_map, &mut out);

        for cmd in out.layer(Layer::Heatmap) {
            let DrawCmd::FieldFill { rect, fill } = cmd else {
                continue;
            };
            assert!(rect.min.x >= field_min.x - 0.01 && rect.max.x <= field_max.x + 0.01);
            assert!(rect.min.y >= field_min.y - 0.01 && rect.max.y <= field_max.y + 0.01);
            assert!(rect.min.x <= rect.max.x);
            assert!(rect.min.y <= rect.max.y);
            assert!(fill.3 > 0 && fill.3 < 255);
        }
    }

    #[test]
    fn late_appended_heatmap_paints_below_performers_in_every_backend() {
        let doc = Document::demo(2, 2);
        let positions = doc.sets[0].positions.clone();
        let options = RenderOptions::default();
        let scene = Scene {
            document: &doc,
            positions: &positions,
            viewport: Viewport {
                size: Vec2 { x: 800.0, y: 450.0 },
                ui_scale: 1.0,
            },
            options: &options,
            theme: &Theme::SCREEN_DARK,
        };
        let mut out = DisplayList::new();
        build_field_2d(&scene, &mut BuildScratch, &mut out);
        let map = FieldMap::new(
            doc.grid.width,
            doc.grid.height,
            scene.viewport.size,
            options.margin,
        );
        let occupancy = drill_core::show_heatmap::analyze_show_occupancy(
            &doc,
            &drill_core::show_heatmap::HeatmapParams::default(),
        );
        append_heatmap(&occupancy, &map, &mut out);

        let ordered: Vec<_> = out.paint_order().collect();
        let heatmap = out
            .layer(Layer::Heatmap)
            .first()
            .expect("occupied heatmap cell");
        let dot = out.layer(Layer::Dot).first().expect("performer dot");
        let heatmap_index = ordered
            .iter()
            .position(|command| std::ptr::eq(*command, heatmap))
            .unwrap();
        let dot_index = ordered
            .iter()
            .position(|command| std::ptr::eq(*command, dot))
            .unwrap();
        assert!(
            heatmap_index < dot_index,
            "heatmap must never obscure performers"
        );
    }

    #[test]
    fn heat_color_ranges_from_cold_to_hot() {
        let cold = heat_color(0.0);
        let hot = heat_color(1.0);
        assert!(cold.2 > cold.0); // cold end leans blue
        assert!(hot.0 > hot.2); // hot end leans red
        assert!(hot.3 > cold.3); // more intense cells are less transparent
    }

    fn identity_field_map() -> FieldMap {
        // scale = 1.0, origin = (0, 0), so map() is easy to reason about by
        // hand: (x, y) -> (x, grid_height - y).
        FieldMap::new(100.0, 100.0, Vec2 { x: 100.0, y: 100.0 }, 0.0)
    }

    fn straight_two_set_doc(start: Point, end: Point, counts: u16) -> Document {
        let mut doc = Document::demo(1, 1);
        doc.sets[0].counts = counts;
        doc.sets[0].positions = vec![start];
        doc.sets[1].positions = vec![end];
        doc
    }

    #[test]
    fn trail_includes_endpoints_and_matches_sample_count() {
        let doc = straight_two_set_doc(Point { x: 0.0, y: 0.0 }, Point { x: 8.0, y: 0.0 }, 8);
        let performer_id = doc.performers[0].id;
        let set_id = doc.sets[0].id;
        let field_map = identity_field_map();

        let mut out = DisplayList::new();
        append_trails(&doc, set_id, &[performer_id], &field_map, 5, &mut out);

        let lines = out.layer(Layer::Trail);
        assert_eq!(lines.len(), 4, "5 sampled points make 4 segments");
        let DrawCmd::Line { a, .. } = lines[0] else {
            panic!("expected a Line command");
        };
        let DrawCmd::Line { b, .. } = lines[3] else {
            panic!("expected a Line command");
        };
        assert_eq!(a, field_map.map(Point { x: 0.0, y: 0.0 }));
        assert_eq!(b, field_map.map(Point { x: 8.0, y: 0.0 }));
    }

    #[test]
    fn constant_speed_is_uniform_and_gated_speed_varies() {
        let mut doc = straight_two_set_doc(Point { x: 0.0, y: 0.0 }, Point { x: 16.0, y: 0.0 }, 16);
        let performer_id = doc.performers[0].id;
        let set_id = doc.sets[0].id;
        let field_map = identity_field_map();

        let colors_of = |doc: &Document, out: &DisplayList| -> Vec<Rgba> {
            let _ = doc;
            out.layer(Layer::Trail)
                .iter()
                .map(|cmd| match cmd {
                    DrawCmd::Line { color, .. } => *color,
                    other => panic!("expected a Line command, got {other:?}"),
                })
                .collect()
        };

        let mut uniform = DisplayList::new();
        append_trails(&doc, set_id, &[performer_id], &field_map, 9, &mut uniform);
        let uniform_colors = colors_of(&doc, &uniform);
        assert!(
            uniform_colors.windows(2).all(|pair| pair[0] == pair[1]),
            "constant-speed straight move should color every segment the same: {uniform_colors:?}"
        );

        // A mid-transition gate makes the performer hold, then cover the
        // same ground in less time -- a sharp, unmistakable speed change.
        doc.sets[0].routes.default.gate = transition::Gate {
            depart: 4.0,
            arrive: Some(16.0),
        };
        let mut gated = DisplayList::new();
        append_trails(&doc, set_id, &[performer_id], &field_map, 9, &mut gated);
        let gated_colors = colors_of(&doc, &gated);
        assert!(
            gated_colors.windows(2).any(|pair| pair[0] != pair[1]),
            "gated accel/decel should produce varying segment colors: {gated_colors:?}"
        );
    }

    #[test]
    fn empty_selection_and_unknown_ids_never_panic() {
        let doc = Document::demo(2, 2);
        let field_map = identity_field_map();
        let mut out = DisplayList::new();

        append_trails(&doc, doc.sets[0].id, &[], &field_map, 4, &mut out);
        assert!(out.layer(Layer::Trail).is_empty());

        let bogus_set = SetId::new(999_999).unwrap();
        let all_ids: Vec<PerformerId> = doc.performers.iter().map(|p| p.id).collect();
        append_trails(&doc, bogus_set, &all_ids, &field_map, 4, &mut out);
        assert!(out.layer(Layer::Trail).is_empty());

        let unknown_performer = PerformerId::new(999_999).unwrap();
        append_trails(
            &doc,
            doc.sets[0].id,
            &[unknown_performer],
            &field_map,
            4,
            &mut out,
        );
        assert!(out.layer(Layer::Trail).is_empty());
    }

    #[test]
    fn warm_trails_reuse_allocations() {
        let mut doc = Document::demo(4, 10);
        doc.sets[0].counts = 16;
        let ids: Vec<PerformerId> = doc.performers.iter().map(|p| p.id).collect();
        let set_id = doc.sets[0].id;
        let field_map = identity_field_map();
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
        let mut scratch = BuildScratch;
        let mut out = DisplayList::new();

        let mut run = |out: &mut DisplayList| {
            build_field_2d(&scene, &mut scratch, out);
            append_trails(&doc, set_id, &ids, &field_map, 12, out);
        };

        run(&mut out);
        let capacity = out.capacities();
        for _ in 0..50 {
            run(&mut out);
            assert_eq!(out.capacities(), capacity);
        }
    }
}
