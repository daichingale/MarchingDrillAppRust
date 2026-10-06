//! UI-independent marching drill document model.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, VecDeque};

pub mod aesthetics;
pub mod audio;
pub mod camera;
pub mod clinic;
pub mod constraint_solver;
pub mod continuity;
pub mod coordinates;
pub mod countsheet;
pub mod editing;
pub mod error;
pub mod ids;
pub mod pathing;
pub mod playback;
pub mod production;
pub mod rhythm_sync;
pub mod roster;
pub mod route_suggestions;
pub mod shapes;
pub mod show_heatmap;
pub mod snapshot;
pub mod stadium;
pub mod svg;
pub mod tempo;
pub mod transition;
pub mod underlay;
pub mod video;
pub mod visibility;

pub use error::{DrillError, Locale};
pub use ids::{
    CameraId, GeneratorId, IdAllocator, PerformerId, ProductionMarkerId, SectionId, SetId, SubsetId,
};
pub use production::{ProductionMarker, ProductionMarkerKind, SetAnnotation};
pub use roster::{OptionalColor, PerformerKind, PerformerMetadata, Section, Subset, Symbol};
pub use transition::{
    ArcTable, ChordPoint, Easing, Gate, PathVia, Route, RouteShape, RouteTable, SetCounts,
    TransitionPlan, eval as eval_transition,
};

pub const SCHEMA_VERSION: u16 = 5;
pub const MAX_PERFORMERS: usize = 4_000;
pub const MAX_SETS: usize = 256;
pub const MAX_SUBSETS: usize = 4_000;
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_PROJECT_JSON_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unit {
    Yards,
    Meters,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GridStyle {
    Lines,
    Dots,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridLine {
    pub position: f32,
    pub label: String,
    pub weight: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridConfig {
    pub width: f32,
    pub height: f32,
    pub unit: Unit,
    pub horizontal_steps: u16,
    pub horizontal_units: f32,
    pub vertical_steps: u16,
    pub vertical_units: f32,
    pub major_line_interval: f32,
    pub resolution: u8,
    pub style: GridStyle,
    pub show_step_grid: bool,
    pub snap_enabled: bool,
    pub hashes: Vec<GridLine>,
    #[serde(default)]
    pub coordinate_notation: coordinates::CoordinateNotation,
    /// Side lengths (in grid units) of centered square reference frames
    /// drawn as bold nested boundaries -- e.g. a 30m contest-floor boundary
    /// inside a larger practice canvas, with a 20m inner reference frame.
    /// Each frame is centered on the grid's own midpoint, independent of
    /// `width`/`height`, which describe the full canvas a performer may be
    /// placed on (deliberately larger than the contest floor itself, to
    /// leave staging/rehearsal room around the marked boundary).
    #[serde(default)]
    pub reference_frames: Vec<f32>,
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            width: 100.0,
            height: 53.333,
            unit: Unit::Yards,
            horizontal_steps: 8,
            horizontal_units: 5.0,
            vertical_steps: 8,
            vertical_units: 5.0,
            major_line_interval: 5.0,
            resolution: 4,
            style: GridStyle::Lines,
            show_step_grid: true,
            snap_enabled: true,
            hashes: vec![
                GridLine {
                    position: 20.0,
                    label: "フロントハッシュ".into(),
                    weight: 1.0,
                },
                GridLine {
                    position: 28.0,
                    label: "バックハッシュ".into(),
                    weight: 1.0,
                },
            ],
            coordinate_notation: coordinates::CoordinateNotation::default(),
            reference_frames: Vec::new(),
        }
    }
}

impl GridConfig {
    /// Creates the standard football grid with localized built-in line names.
    /// `Default` remains Japanese for file/UI compatibility.
    pub fn default_for_locale(locale: Locale) -> Self {
        let mut grid = Self::default();
        if locale == Locale::En {
            grid.hashes[0].label = "Front hash".into();
            grid.hashes[1].label = "Back hash".into();
        }
        grid
    }

    pub fn indoor() -> Self {
        Self {
            width: 90.0,
            height: 60.0,
            major_line_interval: 10.0,
            hashes: Vec::new(),
            ..Self::default()
        }
    }

    pub fn soccer() -> Self {
        Self {
            width: 120.0,
            height: 75.0,
            horizontal_steps: 10,
            horizontal_units: 10.0,
            vertical_steps: 10,
            vertical_units: 10.0,
            major_line_interval: 10.0,
            hashes: vec![GridLine {
                position: 37.5,
                label: "センターライン".into(),
                weight: 1.5,
            }],
            ..Self::default()
        }
    }

    /// 全日本マーチングコンテストで一般的なフロアドリル会場（体育館アリーナ等）
    /// の規格: 30m四方の競技エリア、メートル法。ハッシュマーク（アメフト場の
    /// ヤードライン基準線）は屋内フロアには存在しないため空にする。
    ///
    /// キャンバス自体（`width`/`height`、演者を配置できる範囲）は競技エリア
    /// ちょうどではなく、周囲にリハーサル・待機スペースの余白を持たせて
    /// 横46m×縦40mとする（横を偶数にして中心座標を割り切れる値にし、左右
    /// 対称に見えるようにする）。実際の30m競技エリアと、その内側の目安と
    /// して20mのラインを`reference_frames`で中央基準の太枠として描画する。
    ///
    /// ステップ幅は世界的に広く使われる「8 to 5」（5ヤードを8歩で移動 = 1歩
    /// 22.5インチ）をメートル換算してそのまま維持する: 5yd = 4.572m。主要線
    /// の間隔は視認性を優先し、切りの良い5m刻みにする（ステップ細分化とは
    /// 独立した値なので、8-to-5の歩幅計算には影響しない）。
    pub fn japan_floor() -> Self {
        const FIVE_YARDS_IN_METERS: f32 = 4.572;
        Self {
            width: 46.0,
            height: 40.0,
            unit: Unit::Meters,
            horizontal_steps: 8,
            horizontal_units: FIVE_YARDS_IN_METERS,
            vertical_steps: 8,
            vertical_units: FIVE_YARDS_IN_METERS,
            major_line_interval: 5.0,
            hashes: Vec::new(),
            reference_frames: vec![30.0, 20.0],
            ..Self::default()
        }
    }

    /// The largest x coordinate that is both `<= width` and exactly on the
    /// fine step grid. `width` itself is generally not a whole multiple of
    /// the step size (e.g. `japan_floor`'s 45m canvas vs. its `4.572/8`m
    /// step), so clamping a snapped point straight to `width` can land it
    /// back on an off-step value -- clamp to this instead when a clamped
    /// position also needs to satisfy `point == grid.snap(point)`.
    pub fn max_x(&self) -> f32 {
        Self::max_on_step(self.width, self.horizontal_units, self.horizontal_steps)
    }

    /// The vertical-axis counterpart of [`Self::max_x`].
    pub fn max_y(&self) -> f32 {
        Self::max_on_step(self.height, self.vertical_units, self.vertical_steps)
    }

    fn max_on_step(axis_len: f32, step_units: f32, steps: u16) -> f32 {
        let step = step_units / f32::from(steps.max(1));
        if step > 0.0 {
            (axis_len / step).floor() * step
        } else {
            axis_len
        }
    }

    pub fn snap(&self, point: Point) -> Point {
        if !self.snap_enabled {
            return point;
        }
        self.snap_to_step(point)
    }

    /// Snap onto the step lattice even when [`Self::snap_enabled`] is off.
    ///
    /// Dragging honors the snap toggle. "Put these dots on the grid" does not:
    /// a formation that was nudged with snap disabled can still be dressed
    /// back onto the step intersections.
    pub fn snap_to_step(&self, point: Point) -> Point {
        let dx = self.horizontal_units / self.horizontal_steps.max(1) as f32;
        let dy = self.vertical_units / self.vertical_steps.max(1) as f32;
        if !(dx.is_finite() && dy.is_finite() && dx > 0.0 && dy > 0.0) {
            return point;
        }
        Point {
            x: (point.x / dx).round() * dx,
            y: (point.y / dy).round() * dy,
        }
    }

    /// Snaps an ordered sequence of points (e.g. performers sampled along a
    /// shape/curve, in the shape's own parametric order) to the grid using
    /// per-axis error diffusion instead of independently rounding each
    /// point.
    ///
    /// Rounding is monotonic, so a plain per-point `snap` can never reverse
    /// direction -- but it still rounds each point's leftover fraction
    /// independently, so the *spacing* between consecutive snapped points
    /// comes out noisy (anywhere from zero to a full extra grid step),
    /// which reads as a bumpy/uneven curve even though it never doubles
    /// back. Carrying each point's rounding remainder into the next
    /// point's target (the same idea Bresenham's line algorithm and image
    /// dithering use) keeps the quantized sequence's step sizes close to
    /// uniform, so the snapped curve visually tracks the smooth input
    /// instead of looking jagged, while every output point still lands
    /// exactly on a grid multiple.
    pub fn snap_sequence(&self, points: &[Point]) -> Vec<Point> {
        if !self.snap_enabled {
            return points.to_vec();
        }
        let dx = self.horizontal_units / self.horizontal_steps.max(1) as f32;
        let dy = self.vertical_units / self.vertical_steps.max(1) as f32;
        let mut carry = Point { x: 0.0, y: 0.0 };
        points
            .iter()
            .map(|point| {
                let target_x = point.x + carry.x;
                let target_y = point.y + carry.y;
                let snapped = Point {
                    x: (target_x / dx).round() * dx,
                    y: (target_y / dy).round() * dy,
                };
                carry = Point {
                    x: target_x - snapped.x,
                    y: target_y - snapped.y,
                };
                snapped
            })
            .collect()
    }

    /// Bold major-gridline positions along the horizontal axis, snapped onto
    /// the fine step grid so the two always coincide.
    pub fn horizontal_major_positions(&self) -> Vec<f32> {
        Self::major_positions(
            self.width,
            self.major_line_interval,
            self.horizontal_units,
            self.horizontal_steps,
        )
    }

    /// Bold major-gridline positions along the vertical axis, snapped onto
    /// the fine step grid so the two always coincide.
    pub fn vertical_major_positions(&self) -> Vec<f32> {
        Self::major_positions(
            self.height,
            self.major_line_interval,
            self.vertical_units,
            self.vertical_steps,
        )
    }

    /// `major_line_interval` is chosen for readability (a round number of
    /// yards/meters) and is independent of the fine step grid's own spacing
    /// (e.g. `japan_floor`'s round 5m major interval vs. its `4.572/8`m
    /// marching-step size) -- the two periods are generally not whole
    /// multiples of each other, so drawing major lines at raw multiples of
    /// `interval` leaves them a fraction of a step off from the nearest fine
    /// gridline almost everywhere except the origin. Snapping each major
    /// line to its nearest step-grid line keeps every bold line sitting
    /// exactly on a fine line, at the cost of a sub-step (at most half a
    /// step) shift from the "ideal" round-number position, which is
    /// imperceptible next to a visibly misaligned line.
    ///
    /// Positions radiate outward from the axis's own midpoint (`k = 0` is
    /// the center line itself) rather than counting up from one edge: an
    /// edge-anchored count puts every line's position at the mercy of
    /// wherever counting from that one edge happens to land, which reads as
    /// visibly off-center whenever the axis length isn't itself an exact
    /// multiple of `2 * interval` (e.g. a 45m canvas isn't a multiple of
    /// 10m) -- exactly the "counted from the corner, not the middle"
    /// mismatch this fixes. Centering also means any lines that don't reach
    /// evenly to the true edge leave a symmetric gap on *both* sides instead
    /// of accumulating all the leftover space on whichever edge counting
    /// happened to end on.
    fn major_positions(axis_len: f32, interval: f32, step_units: f32, steps: u16) -> Vec<f32> {
        let interval = interval.max(0.001);
        let step = step_units / f32::from(steps.max(1));
        let center = axis_len * 0.5;
        let max_k = (center / interval).floor() as i32;
        let mut positions: Vec<f32> = (-max_k..=max_k)
            .map(|k| {
                let raw = (center + k as f32 * interval).clamp(0.0, axis_len);
                if step > 0.0 {
                    (raw / step).round() * step
                } else {
                    raw
                }
            })
            .collect();
        positions.sort_by(f32::total_cmp);
        positions.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        positions
    }

    /// Bounds of each centered square in `reference_frames`, as
    /// `(min_x, min_y, max_x, max_y)`, snapped onto the fine step grid (see
    /// `major_positions` for why raw round-number positions don't line up
    /// with it). Each frame is centered on the grid's own midpoint
    /// regardless of `width`/`height`, which may deliberately extend beyond
    /// the marked competition boundary to leave staging room.
    pub fn reference_frame_bounds(&self) -> Vec<(f32, f32, f32, f32)> {
        let dx = self.horizontal_units / f32::from(self.horizontal_steps.max(1));
        let dy = self.vertical_units / f32::from(self.vertical_steps.max(1));
        let snap_axis = |value: f32, step: f32| {
            if step > 0.0 {
                (value / step).round() * step
            } else {
                value
            }
        };
        let center_x = self.width * 0.5;
        let center_y = self.height * 0.5;
        self.reference_frames
            .iter()
            .map(|&side| {
                let half = side * 0.5;
                (
                    snap_axis(center_x - half, dx),
                    snap_axis(center_y - half, dy),
                    snap_axis(center_x + half, dx),
                    snap_axis(center_y + half, dy),
                )
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
        }
    }
}

pub fn evenly_spaced_line(start: Point, end: Point, count: usize) -> Vec<Point> {
    match count {
        0 => Vec::new(),
        1 => vec![start.lerp(end, 0.5)],
        _ => (0..count)
            .map(|i| start.lerp(end, i as f32 / (count - 1) as f32))
            .collect(),
    }
}

pub fn evenly_spaced_arc(
    center: Point,
    radius: f32,
    start_angle: f32,
    end_angle: f32,
    count: usize,
) -> Vec<Point> {
    match count {
        0 => Vec::new(),
        1 => vec![Point {
            x: center.x + radius * ((start_angle + end_angle) * 0.5).cos(),
            y: center.y + radius * ((start_angle + end_angle) * 0.5).sin(),
        }],
        _ => (0..count)
            .map(|i| {
                let angle = start_angle + (end_angle - start_angle) * i as f32 / (count - 1) as f32;
                Point {
                    x: center.x + radius * angle.cos(),
                    y: center.y + radius * angle.sin(),
                }
            })
            .collect(),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransitionAnalysis {
    pub collisions: usize,
    pub excessive_strides: usize,
}

pub fn analyze_transition(
    document: &Document,
    set_index: usize,
    collision_distance: f32,
    max_step_per_count: f32,
) -> TransitionAnalysis {
    let mut scratch = clinic::ScanScratch::default();
    let report = clinic::scan_transition(
        document,
        set_index,
        clinic::ClinicParams {
            style: clinic::StepStyle::Custom {
                units_per_step: max_step_per_count,
            },
            collision_radius: collision_distance,
            danger_radius: collision_distance,
            crowded_radius: collision_distance,
            aggressive_above: 1.0,
            impossible_above: f32::MAX,
            ..clinic::ClinicParams::default()
        },
        &mut scratch,
    );
    TransitionAnalysis {
        collisions: report.collisions.len(),
        excessive_strides: report
            .strides
            .iter()
            .filter(|stride| stride.rating > clinic::StrideRating::Comfortable)
            .count(),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Performer {
    pub id: PerformerId,
    pub label: String,
    pub section: SectionId,
    /// Documents saved before `Symbol::Cross` became the default marker
    /// carry no `symbol` field at all; back-fill those with the marker they
    /// were actually drawn with (`Circle`) so loading an old project doesn't
    /// silently change what's on screen, or mismatch a chart already
    /// printed from it. Callers constructing a brand-new `Performer` should
    /// set `symbol: Symbol::default()` (`Cross`) explicitly instead of
    /// relying on this legacy-only fallback.
    #[serde(default = "legacy_default_symbol")]
    pub symbol: Symbol,
    #[serde(default)]
    pub color: OptionalColor,
    #[serde(default = "default_height_m")]
    pub height_m: f32,
    #[serde(default)]
    pub kind: PerformerKind,
}

fn default_height_m() -> f32 {
    1.7
}

fn legacy_default_symbol() -> Symbol {
    Symbol::Circle
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Set {
    pub id: SetId,
    pub name: String,
    #[serde(default)]
    pub annotation: SetAnnotation,
    pub counts: u16,
    #[serde(default)]
    pub hold: u16,
    #[serde(default, skip_serializing_if = "RouteTable::is_trivial")]
    pub routes: RouteTable,
    /// Editable source geometry when this formation was generated by the designer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<shapes::ShapeSpec>,
    /// Set by a live generator (currently only Follow the Leader) that still
    /// owns this set's participant positions. `None` means hand-authored, or
    /// baked/detached, and nothing will ever rewrite it again. Additive and
    /// optional so documents saved before generators existed keep loading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<GeneratorId>,
    /// Dense and index-aligned with `Document::performers` for cache-friendly playback.
    pub positions: Vec<Point>,
}

/// A live, re-editable Follow the Leader run.
///
/// The generated sets stay ordinary `Set`s — nothing downstream needs to know
/// a generator exists — but keeping the path and step count next to the list
/// of sets they produced means the designer can change either and have the
/// range recomputed, instead of undoing and redrawing from scratch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FollowTheLeaderGenerator {
    pub id: GeneratorId,
    pub path: shapes::ShapeSpec,
    pub steps: u32,
    /// Stable identities, not indices: the participant list has to survive
    /// performers being added, removed, or reordered between regenerations.
    pub group: Vec<PerformerId>,
    /// The set the run was inserted after; its positions seed any set the
    /// generator has to create when the step count grows.
    pub source_set: SetId,
    /// The sets this generator produced, in show order.
    pub owns: Vec<SetId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub schema_version: u16,
    pub title: String,
    #[serde(default)]
    pub grid: GridConfig,
    #[serde(default)]
    pub tempo: tempo::TempoMap,
    #[serde(default)]
    pub audio: Option<audio::AudioTrack>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underlay: Option<underlay::ImageUnderlay>,
    #[serde(default)]
    pub camera_program: camera::CameraProgram,
    #[serde(default)]
    pub sections: Vec<Section>,
    /// Reusable overlapping selections; independent from exclusive sections.
    #[serde(default)]
    pub subsets: Vec<Subset>,
    pub performers: Vec<Performer>,
    pub sets: Vec<Set>,
    /// Rehearsal and production landmarks addressed by stable IDs.
    #[serde(default)]
    pub production_markers: Vec<ProductionMarker>,
    /// Live generators that still own a range of `sets`. Additive and
    /// optional: older documents simply load with none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generators: Vec<FollowTheLeaderGenerator>,
}

impl Document {
    pub fn demo(rows: usize, columns: usize) -> Self {
        let count = rows * columns;
        let performers = (0..count)
            .map(|i| Performer {
                id: PerformerId::new(i as u32 + 1).expect("demo performer id is non-zero"),
                label: format!("{}{}", (b'A' + (i / 10).min(25) as u8) as char, i % 10 + 1),
                section: SectionId::new(1).expect("demo section id is non-zero"),
                symbol: Symbol::Cross,
                color: Some([245, 197, 66]).into(),
                height_m: default_height_m(),
                kind: PerformerKind::Wind,
            })
            .collect::<Vec<_>>();
        let block = (0..count)
            .map(|i| Point {
                x: 15.0 + (i % columns) as f32 * 5.0,
                y: 10.0 + (i / columns) as f32 * 5.0,
            })
            .collect::<Vec<_>>();
        let arc = (0..count)
            .map(|i| {
                let angle =
                    std::f32::consts::PI * i as f32 / (count.saturating_sub(1).max(1)) as f32;
                Point {
                    x: 50.0 - angle.cos() * 35.0,
                    y: 42.0 - angle.sin() * 28.0,
                }
            })
            .collect();
        Self {
            schema_version: SCHEMA_VERSION,
            title: "新しいドリル".into(),
            grid: GridConfig::default(),
            tempo: tempo::TempoMap::constant(120.0),
            audio: None,
            underlay: None,
            camera_program: camera::CameraProgram::default_for_grid(&GridConfig::default()),
            sections: vec![Section {
                id: SectionId::new(1).expect("demo section id is non-zero"),
                name: "Ensemble".into(),
                short: "Ens".into(),
                color: [245, 197, 66],
                order: 0,
            }],
            subsets: Vec::new(),
            performers,
            sets: vec![
                Set {
                    id: SetId::new(1).expect("demo set id is non-zero"),
                    name: "セット 1".into(),
                    annotation: SetAnnotation::default(),
                    counts: 16,
                    hold: 0,
                    routes: RouteTable::default(),
                    shape: None,
                    generated_by: None,
                    positions: block,
                },
                Set {
                    id: SetId::new(2).expect("demo set id is non-zero"),
                    name: "セット 2".into(),
                    annotation: SetAnnotation::default(),
                    counts: 16,
                    hold: 0,
                    routes: RouteTable::default(),
                    shape: None,
                    generated_by: None,
                    positions: arc,
                },
            ],
            production_markers: Vec::new(),
            generators: Vec::new(),
        }
    }

    /// A one-set show on the Japanese floor-drill grid, with `count`
    /// performers in a centered block. This is File → New: a writable
    /// starting roster, not a two-set demo of transitions.
    pub fn blank(count: usize) -> Self {
        let count = count.clamp(1, MAX_PERFORMERS);
        let grid = GridConfig::japan_floor();
        let section = Section {
            id: SectionId::new(1).expect("blank section id is non-zero"),
            name: "Ensemble".into(),
            short: "Ens".into(),
            color: [245, 197, 66],
            order: 0,
        };
        let cols = (count as f32).sqrt().ceil().max(1.0) as usize;
        let rows = count.div_ceil(cols);
        let step = grid.horizontal_units / f32::from(grid.horizontal_steps.max(1));
        let gap = step * 4.0;
        let origin_x = (grid.width - cols.saturating_sub(1) as f32 * gap) * 0.5;
        let origin_y = (grid.height - rows.saturating_sub(1) as f32 * gap) * 0.5;
        let performers = (0..count)
            .map(|i| Performer {
                id: PerformerId::new(i as u32 + 1).expect("blank performer id is non-zero"),
                label: format!("P{}", i + 1),
                section: section.id,
                symbol: Symbol::Cross,
                color: Some([245, 197, 66]).into(),
                height_m: default_height_m(),
                kind: PerformerKind::Wind,
            })
            .collect::<Vec<_>>();
        let positions = (0..count)
            .map(|i| {
                grid.snap(Point {
                    x: origin_x + (i % cols) as f32 * gap,
                    y: origin_y + (i / cols) as f32 * gap,
                })
            })
            .collect::<Vec<_>>();
        Self {
            schema_version: SCHEMA_VERSION,
            title: "新しいドリル".into(),
            grid: grid.clone(),
            tempo: tempo::TempoMap::constant(120.0),
            audio: None,
            underlay: None,
            camera_program: camera::CameraProgram::default_for_grid(&grid),
            sections: vec![section],
            subsets: Vec::new(),
            performers,
            sets: vec![Set {
                id: SetId::new(1).expect("blank set id is non-zero"),
                name: "セット 1".into(),
                annotation: SetAnnotation::default(),
                counts: 16,
                hold: 0,
                routes: RouteTable::default(),
                shape: None,
                generated_by: None,
                positions,
            }],
            production_markers: Vec::new(),
            generators: Vec::new(),
        }
    }

    pub fn next_performer_id(&self) -> Option<PerformerId> {
        let max = self
            .performers
            .iter()
            .map(|performer| performer.id.get())
            .max()
            .unwrap_or(0);
        PerformerId::new(max.checked_add(1)?)
    }

    pub fn next_set_id(&self) -> Option<SetId> {
        let max = self.sets.iter().map(|set| set.id.get()).max().unwrap_or(0);
        SetId::new(max.checked_add(1)?)
    }

    /// Appends one performer, copying `position` onto every set so the
    /// roster stays index-aligned. Route overrides and subsets are left
    /// alone: the newcomer uses the default route and belongs to no subset.
    pub fn add_performer(
        &mut self,
        performer: Performer,
        position: Point,
    ) -> Result<(), DrillError> {
        if self.performers.len() >= MAX_PERFORMERS {
            return Err(DrillError::LimitExceeded {
                field: "performers",
                limit: MAX_PERFORMERS,
            });
        }
        if !position.x.is_finite() || !position.y.is_finite() {
            return Err(DrillError::InvalidNumber { field: "positions" });
        }
        if self
            .performers
            .iter()
            .any(|existing| existing.id == performer.id)
        {
            return Err(DrillError::DuplicatePerformerId);
        }
        if !self
            .sections
            .iter()
            .any(|section| section.id == performer.section)
        {
            return Err(DrillError::InvalidEdit);
        }
        let position = self.grid.snap(Point {
            x: position.x.clamp(0.0, self.grid.max_x()),
            y: position.y.clamp(0.0, self.grid.max_y()),
        });
        self.performers.push(performer);
        for set in &mut self.sets {
            set.positions.push(position);
        }
        self.validate()
    }

    /// Drops the named performers from the roster, every set's position
    /// vector, subset membership, and route overrides. A Follow the Leader
    /// generator that listed any of them is detached rather than left with a
    /// hole in its group.
    pub fn remove_performers(&mut self, ids: &[PerformerId]) -> Result<(), DrillError> {
        if ids.is_empty() {
            return Ok(());
        }
        let drop: BTreeSet<_> = ids.iter().copied().collect();
        if drop
            .iter()
            .any(|id| self.performers.iter().all(|p| p.id != *id))
        {
            return Err(DrillError::MissingPerformer);
        }
        let keep: Vec<usize> = self
            .performers
            .iter()
            .enumerate()
            .filter(|(_, performer)| !drop.contains(&performer.id))
            .map(|(index, _)| index)
            .collect();
        if keep.is_empty() {
            return Err(DrillError::InvalidEdit);
        }
        self.performers = keep
            .iter()
            .map(|&index| self.performers[index].clone())
            .collect();
        for set in &mut self.sets {
            set.positions = keep.iter().map(|&index| set.positions[index]).collect();
            set.routes.overrides.retain(|id, _| !drop.contains(id));
        }
        for subset in &mut self.subsets {
            subset.members.retain(|id| !drop.contains(id));
        }
        let affected_generators: Vec<GeneratorId> = self
            .generators
            .iter()
            .filter(|generator| generator.group.iter().any(|id| drop.contains(id)))
            .map(|generator| generator.id)
            .collect();
        for id in affected_generators {
            self.detach_generator(id);
        }
        self.validate()
    }

    /// Removes the set at `index`. The last remaining set cannot be deleted
    /// (`EmptySets`). A generator that owned the set is detached.
    pub fn remove_set_at(&mut self, index: usize) -> Result<(), DrillError> {
        if self.sets.len() <= 1 {
            return Err(DrillError::EmptySets);
        }
        let Some(set) = self.sets.get(index) else {
            return Err(DrillError::MissingSet);
        };
        if let Some(generator_id) = set.generated_by {
            self.detach_generator(generator_id);
        } else {
            let owned: Vec<GeneratorId> = self
                .generators
                .iter()
                .filter(|generator| {
                    generator.owns.contains(&set.id) || generator.source_set == set.id
                })
                .map(|generator| generator.id)
                .collect();
            for id in owned {
                self.detach_generator(id);
            }
        }
        self.sets.remove(index);
        self.validate()
    }

    pub fn generator(&self, id: GeneratorId) -> Option<&FollowTheLeaderGenerator> {
        self.generators.iter().find(|entry| entry.id == id)
    }

    /// Bake a generated range into ordinary sets: drop the generator and
    /// clear the tag on every set it owned. Deliberately touches no
    /// positions, so the sets are byte-identical to hand-authored ones
    /// afterwards. Also the detach path for a manual edit inside the range.
    pub fn detach_generator(&mut self, id: GeneratorId) -> bool {
        let existed = self.generators.iter().any(|entry| entry.id == id);
        self.generators.retain(|entry| entry.id != id);
        for set in &mut self.sets {
            if set.generated_by == Some(id) {
                set.generated_by = None;
            }
        }
        existed
    }

    /// The next free generator identity (running max plus one), mirroring
    /// how set IDs are allocated when duplicating a set.
    pub fn next_generator_id(&self) -> Option<GeneratorId> {
        let max = self
            .generators
            .iter()
            .map(|entry| entry.id.get())
            .max()
            .unwrap_or(0);
        GeneratorId::new(max.checked_add(1)?)
    }

    pub fn validate(&self) -> Result<(), DrillError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(DrillError::UnsupportedSchema {
                found: self.schema_version,
                supported: SCHEMA_VERSION,
            });
        }
        if self.sets.is_empty() {
            return Err(DrillError::EmptySets);
        }
        if self.performers.len() > MAX_PERFORMERS {
            return Err(DrillError::LimitExceeded {
                field: "performers",
                limit: MAX_PERFORMERS,
            });
        }
        if self.sets.len() > MAX_SETS {
            return Err(DrillError::LimitExceeded {
                field: "sets",
                limit: MAX_SETS,
            });
        }
        let mut marker_ids = BTreeSet::new();
        for marker in &self.production_markers {
            if !marker_ids.insert(marker.id) || !marker.validate(self.timeline_counts()) {
                return Err(DrillError::InvalidEdit);
            }
        }
        if self.subsets.len() > MAX_SUBSETS {
            return Err(DrillError::LimitExceeded {
                field: "subsets",
                limit: MAX_SUBSETS,
            });
        }
        if self.title.len() > MAX_TEXT_BYTES {
            return Err(DrillError::LimitExceeded {
                field: "title",
                limit: MAX_TEXT_BYTES,
            });
        }
        if !self.grid.width.is_finite()
            || !self.grid.height.is_finite()
            || self.grid.width <= 0.0
            || self.grid.height <= 0.0
            || !self.grid.coordinate_notation.validate()
        {
            return Err(DrillError::InvalidGrid);
        }
        if let coordinates::FrontBackReference::FixedLabel(label) =
            &self.grid.coordinate_notation.front_back
        {
            let normalized = label.to_lowercase();
            let canonical = normalized.contains("sideline")
                || normalized.contains("サイドライン")
                || (normalized.contains("front") && normalized.contains("hash"))
                || (normalized.contains("back") && normalized.contains("hash"))
                || normalized.contains("フロントハッシュ")
                || normalized.contains("バックハッシュ");
            if !canonical
                && !self
                    .grid
                    .hashes
                    .iter()
                    .any(|line| line.label.eq_ignore_ascii_case(label))
            {
                return Err(DrillError::InvalidGrid);
            }
        }
        let expected = self.performers.len();
        for set in &self.sets {
            if !set.annotation.validate() || set.name.len() > MAX_TEXT_BYTES {
                return Err(DrillError::InvalidEdit);
            }
            if let Some(shape) = &set.shape {
                shape.validate()?;
            }
            set.routes.validate(
                &self.performers,
                SetCounts {
                    moves: set.counts,
                    hold: set.hold,
                },
            )?;
        }
        if let Some((i, set)) = self
            .sets
            .iter()
            .enumerate()
            .find(|(_, s)| s.positions.len() != expected)
        {
            return Err(DrillError::SetSizeMismatch {
                set_index: i,
                expected,
                found: set.positions.len(),
            });
        }
        if self
            .sets
            .iter()
            .flat_map(|set| &set.positions)
            .any(|point| !point.x.is_finite() || !point.y.is_finite())
        {
            return Err(DrillError::InvalidNumber { field: "positions" });
        }
        let unique = self
            .performers
            .iter()
            .map(|p| p.id)
            .collect::<BTreeSet<_>>();
        if unique.len() != expected {
            return Err(DrillError::DuplicatePerformerId);
        }
        let section_ids = self
            .sections
            .iter()
            .map(|section| section.id)
            .collect::<BTreeSet<_>>();
        if section_ids.len() != self.sections.len()
            || self.sections.iter().any(|section| {
                section.name.len() > MAX_TEXT_BYTES || section.short.len() > MAX_TEXT_BYTES
            })
        {
            return Err(DrillError::InvalidEdit);
        }
        self.camera_program
            .validate(self.timeline_counts() as f32)?;
        if self.performers.iter().any(|performer| {
            !section_ids.contains(&performer.section)
                || !performer.height_m.is_finite()
                || !(0.2..=3.0).contains(&performer.height_m)
        }) {
            return Err(DrillError::InvalidEdit);
        }
        let unique_sets = self.sets.iter().map(|set| set.id).collect::<BTreeSet<_>>();
        if unique_sets.len() != self.sets.len() {
            return Err(DrillError::DuplicateSetId);
        }
        let subset_ids = self
            .subsets
            .iter()
            .map(|subset| subset.id)
            .collect::<BTreeSet<_>>();
        let performer_ids = self
            .performers
            .iter()
            .map(|performer| performer.id)
            .collect::<BTreeSet<_>>();
        if subset_ids.len() != self.subsets.len() {
            return Err(DrillError::DuplicateSubsetId);
        }
        if self.subsets.iter().any(|subset| {
            subset.name.is_empty()
                || subset.name.len() > MAX_TEXT_BYTES
                || subset.members.windows(2).any(|pair| pair[0] >= pair[1])
                || subset.members.iter().any(|id| !performer_ids.contains(id))
        }) {
            return Err(DrillError::InvalidEdit);
        }
        if self
            .underlay
            .as_ref()
            .is_some_and(|value| !value.validate())
        {
            return Err(DrillError::InvalidEdit);
        }
        Ok(())
    }

    pub fn replace_grid(&mut self, grid: GridConfig, scale_positions: bool) {
        if scale_positions {
            let sx = grid.width / self.grid.width.max(f32::EPSILON);
            let sy = grid.height / self.grid.height.max(f32::EPSILON);
            for set in &mut self.sets {
                for point in &mut set.positions {
                    point.x *= sx;
                    point.y *= sy;
                }
            }
        }
        self.grid = grid;
    }

    pub fn timeline_counts(&self) -> u32 {
        self.sets
            .iter()
            .take(self.sets.len().saturating_sub(1))
            .map(|set| u32::from(set.counts) + u32::from(set.hold))
            .sum()
    }

    pub fn global_count(&self, set_index: usize, local_count: f32) -> f32 {
        let prior = self
            .sets
            .iter()
            .take(set_index)
            .map(|set| u32::from(set.counts) + u32::from(set.hold))
            .sum::<u32>();
        prior as f32 + local_count
    }

    pub fn locate_count(&self, global_count: f32) -> (usize, f32) {
        let mut remaining = global_count.clamp(0.0, self.timeline_counts() as f32);
        for (index, set) in self
            .sets
            .iter()
            .enumerate()
            .take(self.sets.len().saturating_sub(1))
        {
            let counts = f32::from(set.counts) + f32::from(set.hold);
            if remaining < counts {
                return (index, remaining);
            }
            remaining -= counts;
        }
        (self.sets.len().saturating_sub(1), 0.0)
    }

    pub fn positions_at(&self, set_index: usize, progress: f32, out: &mut Vec<Point>) {
        let moves = self
            .sets
            .get(set_index)
            .map_or(0.0, |set| f32::from(set.counts));
        self.positions_at_count(set_index, progress.clamp(0.0, 1.0) * moves, out);
    }

    /// Evaluates a transition directly in set-local counts. During the
    /// ensemble hold all performers remain exactly on the destination dots.
    pub fn positions_at_count(&self, set_index: usize, local_count: f32, out: &mut Vec<Point>) {
        let Some(last_index) = self.sets.len().checked_sub(1) else {
            out.clear();
            return;
        };
        let index = set_index.min(last_index);
        let from_set = &self.sets[index];
        let to = self
            .sets
            .get(index + 1)
            .map_or(&from_set.positions, |s| &s.positions);
        out.clear();
        out.reserve(from_set.positions.len().saturating_sub(out.capacity()));
        if index == last_index || local_count >= f32::from(from_set.counts) {
            out.extend_from_slice(to);
            return;
        }
        out.extend(
            from_set
                .positions
                .iter()
                .zip(to)
                .zip(&self.performers)
                .map(|((&a, &b), p)| {
                    transition::evaluate(
                        from_set.routes.route_for(p.id),
                        a,
                        b,
                        local_count,
                        from_set.counts,
                    )
                }),
        );
    }

    /// Field-unit length of the selected performer's authored route.
    pub fn route_length(&self, set_index: usize, performer_index: usize) -> f32 {
        let Some(from) = self.sets.get(set_index) else {
            return 0.0;
        };
        let Some(to) = self.sets.get(set_index + 1) else {
            return 0.0;
        };
        let (Some(&start), Some(&end), Some(performer)) = (
            from.positions.get(performer_index),
            to.positions.get(performer_index),
            self.performers.get(performer_index),
        ) else {
            return 0.0;
        };
        transition::route_length(from.routes.route_for(performer.id), start, end)
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
    pub fn from_json(json: &str) -> Result<Self, DrillError> {
        if json.len() > MAX_PROJECT_JSON_BYTES {
            return Err(DrillError::LimitExceeded {
                field: "project bytes",
                limit: MAX_PROJECT_JSON_BYTES,
            });
        }
        let mut value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| DrillError::InvalidJson(e.to_string()))?;
        let version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        match version {
            1 => {
                value["schema_version"] = serde_json::Value::from(SCHEMA_VERSION);
                if let Some(performers) = value
                    .get_mut("performers")
                    .and_then(serde_json::Value::as_array_mut)
                {
                    for performer in performers {
                        if let Some(raw) = performer.get("id").and_then(serde_json::Value::as_u64) {
                            performer["id"] = serde_json::Value::from(raw.saturating_add(1));
                        }
                    }
                }
            }
            2..=4 => value["schema_version"] = serde_json::Value::from(SCHEMA_VERSION),
            v if v == u64::from(SCHEMA_VERSION) => {}
            v => {
                return Err(DrillError::UnsupportedSchema {
                    found: u16::try_from(v).unwrap_or(u16::MAX),
                    supported: SCHEMA_VERSION,
                });
            }
        }
        if let Some(sets) = value
            .get_mut("sets")
            .and_then(serde_json::Value::as_array_mut)
        {
            let mut used = sets
                .iter()
                .filter_map(|set| set.get("id")?.as_u64())
                .collect::<BTreeSet<_>>();
            let mut candidate = 1_u64;
            for set in sets {
                if set.get("id").is_none() {
                    while used.contains(&candidate) {
                        candidate = candidate.saturating_add(1);
                    }
                    set["id"] = serde_json::Value::from(candidate);
                    used.insert(candidate);
                }
            }
        }
        let needs_default_section = value
            .get("sections")
            .and_then(serde_json::Value::as_array)
            .is_none_or(Vec::is_empty);
        if needs_default_section {
            value["sections"] = serde_json::json!([{
                "id": 1,
                "name": "Ensemble",
                "short": "Ens",
                "color": [128, 128, 128],
                "order": 0
            }]);
        }
        let default_section = value["sections"]
            .as_array()
            .and_then(|sections| sections.first())
            .and_then(|section| section.get("id"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(1);
        if let Some(performers) = value
            .get_mut("performers")
            .and_then(serde_json::Value::as_array_mut)
        {
            for performer in performers {
                if performer.get("section").is_none() {
                    performer["section"] = serde_json::Value::from(default_section);
                }
            }
        }
        let mut doc: Self =
            serde_json::from_value(value).map_err(|e| DrillError::InvalidJson(e.to_string()))?;
        if doc.camera_program.tracks.is_empty() {
            doc.camera_program = camera::CameraProgram::default_for_grid(&doc.grid);
        }
        doc.validate()?;
        Ok(doc)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Edit {
    /// Atomically replaces the complete document. Intended for import and
    /// migration transactions that must be undone as one user action.
    ReplaceDocument {
        document: Box<Document>,
    },
    MovePerformers {
        set_id: SetId,
        performer_ids: Vec<PerformerId>,
        positions: Vec<Point>,
    },
    SetCounts {
        set_id: SetId,
        counts: SetCounts,
    },
    SetRoutes {
        set_id: SetId,
        routes: RouteTable,
    },
    SetShape {
        set_id: SetId,
        shape: Option<shapes::ShapeSpec>,
    },
    SetAnnotation {
        set_id: SetId,
        annotation: SetAnnotation,
    },
    InsertProductionMarker {
        marker: ProductionMarker,
    },
    SetProductionMarker {
        marker: ProductionMarker,
    },
    RemoveProductionMarker {
        id: ProductionMarkerId,
    },
    ReplaceGrid {
        grid: GridConfig,
        scale_positions: bool,
    },
    SetTempoMap {
        tempo: tempo::TempoMap,
    },
    SetAudioTrack {
        audio: Option<audio::AudioTrack>,
    },
    SetImageUnderlay {
        underlay: Option<underlay::ImageUnderlay>,
    },
    RenameDocument {
        title: String,
    },
    AddSyncAnchor {
        id: Option<audio::AnchorId>,
        anchor: audio::SyncAnchor,
    },
    MoveSyncAnchor {
        id: audio::AnchorId,
        anchor: audio::SyncAnchor,
    },
    RemoveSyncAnchor {
        id: audio::AnchorId,
    },
    AddSection {
        section: Section,
        at: Option<usize>,
    },
    RenameSection {
        id: SectionId,
        name: String,
        short: String,
    },
    RemoveSection {
        id: SectionId,
        reassign_to: SectionId,
    },
    RestoreSection {
        at: usize,
        section: Section,
        assignments: Vec<PerformerId>,
        reassign_to: SectionId,
    },
    AssignPerformersToSection {
        assignments: Vec<(PerformerId, SectionId)>,
    },
    AddSubset {
        subset: Subset,
        at: Option<usize>,
    },
    RenameSubset {
        id: SubsetId,
        name: String,
    },
    SetSubsetMembers {
        id: SubsetId,
        members: Vec<PerformerId>,
    },
    RemoveSubset {
        id: SubsetId,
    },
    SetPerformerMetadata {
        performer: PerformerId,
        metadata: PerformerMetadata,
    },
    InsertCameraKeyframe {
        camera_id: CameraId,
        keyframe: camera::CameraKeyframe,
    },
    RemoveCameraKeyframe {
        camera_id: CameraId,
        count: f32,
    },
    InsertCameraCut {
        cut: camera::CameraCut,
    },
    RemoveCameraCut {
        count: f32,
    },
}

/// Coarse name of an edit, for undo/redo captions. The stored history entry
/// is the inverse, but the variant still says what kind of change it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    Document,
    Positions,
    Counts,
    Routes,
    Shape,
    Annotation,
    Marker,
    Grid,
    Tempo,
    Audio,
    Underlay,
    Title,
    Sync,
    Section,
    Subset,
    Performer,
    Camera,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EditCoalesceKey {
    MovePerformers(SetId),
    Routes(SetId),
    Shape(SetId),
    Annotation(SetId),
    ProductionMarker(ProductionMarkerId),
    MoveSyncAnchor(audio::AnchorId),
    DocumentTitle,
    Grid,
    Tempo,
    Audio,
    Underlay,
    Section(SectionId),
    Subset(SubsetId),
    PerformerMetadata(PerformerId),
    Camera(CameraId),
}

impl Edit {
    /// Stable key a drag controller can use to collapse many transient edits
    /// into one history entry without relying on a mutable vector index.
    pub fn coalesce_key(&self) -> Option<EditCoalesceKey> {
        match self {
            Self::ReplaceDocument { .. } => None,
            Self::MovePerformers { set_id, .. } => Some(EditCoalesceKey::MovePerformers(*set_id)),
            Self::SetRoutes { set_id, .. } => Some(EditCoalesceKey::Routes(*set_id)),
            Self::SetShape { set_id, .. } => Some(EditCoalesceKey::Shape(*set_id)),
            Self::SetAnnotation { set_id, .. } => Some(EditCoalesceKey::Annotation(*set_id)),
            Self::SetProductionMarker { marker } => {
                Some(EditCoalesceKey::ProductionMarker(marker.id))
            }
            Self::SetCounts { .. } => None,
            Self::MoveSyncAnchor { id, .. } => Some(EditCoalesceKey::MoveSyncAnchor(*id)),
            Self::RenameDocument { .. } => Some(EditCoalesceKey::DocumentTitle),
            Self::ReplaceGrid { .. } => Some(EditCoalesceKey::Grid),
            Self::SetTempoMap { .. } => Some(EditCoalesceKey::Tempo),
            Self::SetAudioTrack { .. } => Some(EditCoalesceKey::Audio),
            Self::SetImageUnderlay { .. } => Some(EditCoalesceKey::Underlay),
            Self::RenameSection { id, .. } => Some(EditCoalesceKey::Section(*id)),
            Self::RenameSubset { id, .. } | Self::SetSubsetMembers { id, .. } => {
                Some(EditCoalesceKey::Subset(*id))
            }
            Self::SetPerformerMetadata { performer, .. } => {
                Some(EditCoalesceKey::PerformerMetadata(*performer))
            }
            Self::InsertCameraKeyframe { camera_id, .. }
            | Self::RemoveCameraKeyframe { camera_id, .. } => {
                Some(EditCoalesceKey::Camera(*camera_id))
            }
            Self::AddSyncAnchor { .. }
            | Self::RemoveSyncAnchor { .. }
            | Self::AddSection { .. }
            | Self::RemoveSection { .. }
            | Self::RestoreSection { .. }
            | Self::AssignPerformersToSection { .. }
            | Self::AddSubset { .. }
            | Self::RemoveSubset { .. }
            | Self::InsertCameraCut { .. }
            | Self::RemoveCameraCut { .. } => None,
            Self::InsertProductionMarker { .. } | Self::RemoveProductionMarker { .. } => None,
        }
    }

    /// What an undo or redo button should say this edit changes.
    pub fn kind(&self) -> EditKind {
        match self {
            Self::ReplaceDocument { .. } => EditKind::Document,
            Self::MovePerformers { .. } => EditKind::Positions,
            Self::SetCounts { .. } => EditKind::Counts,
            Self::SetRoutes { .. } => EditKind::Routes,
            Self::SetShape { .. } => EditKind::Shape,
            Self::SetAnnotation { .. } => EditKind::Annotation,
            Self::InsertProductionMarker { .. }
            | Self::SetProductionMarker { .. }
            | Self::RemoveProductionMarker { .. } => EditKind::Marker,
            Self::ReplaceGrid { .. } => EditKind::Grid,
            Self::SetTempoMap { .. } => EditKind::Tempo,
            Self::SetAudioTrack { .. } => EditKind::Audio,
            Self::SetImageUnderlay { .. } => EditKind::Underlay,
            Self::RenameDocument { .. } => EditKind::Title,
            Self::AddSyncAnchor { .. }
            | Self::MoveSyncAnchor { .. }
            | Self::RemoveSyncAnchor { .. } => EditKind::Sync,
            Self::AddSection { .. }
            | Self::RenameSection { .. }
            | Self::RemoveSection { .. }
            | Self::RestoreSection { .. }
            | Self::AssignPerformersToSection { .. } => EditKind::Section,
            Self::AddSubset { .. }
            | Self::RenameSubset { .. }
            | Self::SetSubsetMembers { .. }
            | Self::RemoveSubset { .. } => EditKind::Subset,
            Self::SetPerformerMetadata { .. } => EditKind::Performer,
            Self::InsertCameraKeyframe { .. }
            | Self::RemoveCameraKeyframe { .. }
            | Self::InsertCameraCut { .. }
            | Self::RemoveCameraCut { .. } => EditKind::Camera,
        }
    }

    /// Applies this edit atomically and returns the inverse operation.
    pub fn apply(self, document: &mut Document) -> Result<Self, DrillError> {
        match self {
            Self::ReplaceDocument { document: next } => {
                next.validate()?;
                let previous = std::mem::replace(document, *next);
                Ok(Self::ReplaceDocument {
                    document: Box::new(previous),
                })
            }
            Self::MovePerformers {
                set_id,
                performer_ids,
                positions,
            } => {
                if performer_ids.len() != positions.len() {
                    return Err(DrillError::InvalidEdit);
                }
                let set_index = document
                    .sets
                    .iter()
                    .position(|set| set.id == set_id)
                    .ok_or(DrillError::MissingSet)?;
                let indices = performer_ids
                    .iter()
                    .map(|id| {
                        document
                            .performers
                            .iter()
                            .position(|performer| performer.id == *id)
                            .ok_or(DrillError::MissingPerformer)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let set = &mut document.sets[set_index];
                let mut inverse_positions = Vec::with_capacity(indices.len());
                for &index in &indices {
                    inverse_positions.push(
                        set.positions
                            .get(index)
                            .copied()
                            .ok_or(DrillError::InvalidEdit)?,
                    );
                }
                for (&index, position) in indices.iter().zip(positions) {
                    set.positions[index] = position;
                }
                Ok(Self::MovePerformers {
                    set_id,
                    performer_ids,
                    positions: inverse_positions,
                })
            }
            Self::SetCounts { set_id, counts } => {
                if counts.total() > transition::MAX_SET_COUNTS {
                    return Err(DrillError::InvalidTransition);
                }
                let set = document
                    .sets
                    .iter_mut()
                    .find(|s| s.id == set_id)
                    .ok_or(DrillError::MissingSet)?;
                let previous = SetCounts {
                    moves: set.counts,
                    hold: set.hold,
                };
                set.routes.validate(&document.performers, counts)?;
                set.counts = counts.moves;
                set.hold = counts.hold;
                Ok(Self::SetCounts {
                    set_id,
                    counts: previous,
                })
            }
            Self::SetRoutes { set_id, routes } => {
                let set = document
                    .sets
                    .iter_mut()
                    .find(|s| s.id == set_id)
                    .ok_or(DrillError::MissingSet)?;
                routes.validate(
                    &document.performers,
                    SetCounts {
                        moves: set.counts,
                        hold: set.hold,
                    },
                )?;
                let previous = std::mem::replace(&mut set.routes, routes);
                Ok(Self::SetRoutes {
                    set_id,
                    routes: previous,
                })
            }
            Self::SetShape { set_id, shape } => {
                if let Some(spec) = &shape {
                    spec.validate()?;
                }
                let set = document
                    .sets
                    .iter_mut()
                    .find(|s| s.id == set_id)
                    .ok_or(DrillError::MissingSet)?;
                let previous = std::mem::replace(&mut set.shape, shape);
                Ok(Self::SetShape {
                    set_id,
                    shape: previous,
                })
            }
            Self::SetAnnotation { set_id, annotation } => {
                if !annotation.validate() {
                    return Err(DrillError::InvalidEdit);
                }
                let set = document
                    .sets
                    .iter_mut()
                    .find(|s| s.id == set_id)
                    .ok_or(DrillError::MissingSet)?;
                let previous = std::mem::replace(&mut set.annotation, annotation);
                Ok(Self::SetAnnotation {
                    set_id,
                    annotation: previous,
                })
            }
            Self::InsertProductionMarker { marker } => {
                if !marker.validate(document.timeline_counts())
                    || document
                        .production_markers
                        .iter()
                        .any(|existing| existing.id == marker.id)
                {
                    return Err(DrillError::InvalidEdit);
                }
                document.production_markers.push(marker.clone());
                Ok(Self::RemoveProductionMarker { id: marker.id })
            }
            Self::SetProductionMarker { marker } => {
                if !marker.validate(document.timeline_counts()) {
                    return Err(DrillError::InvalidEdit);
                }
                let existing = document
                    .production_markers
                    .iter_mut()
                    .find(|existing| existing.id == marker.id)
                    .ok_or(DrillError::InvalidEdit)?;
                let previous = std::mem::replace(existing, marker);
                Ok(Self::SetProductionMarker { marker: previous })
            }
            Self::RemoveProductionMarker { id } => {
                let index = document
                    .production_markers
                    .iter()
                    .position(|marker| marker.id == id)
                    .ok_or(DrillError::InvalidEdit)?;
                let marker = document.production_markers.remove(index);
                Ok(Self::InsertProductionMarker { marker })
            }
            Self::ReplaceGrid {
                grid,
                scale_positions,
            } => {
                if !grid.width.is_finite()
                    || !grid.height.is_finite()
                    || grid.width <= 0.0
                    || grid.height <= 0.0
                {
                    return Err(DrillError::InvalidGrid);
                }
                let previous = document.grid.clone();
                document.replace_grid(grid, scale_positions);
                Ok(Self::ReplaceGrid {
                    grid: previous,
                    scale_positions,
                })
            }
            Self::SetTempoMap { tempo } => {
                if tempo.events().iter().any(|event| {
                    !event.count.is_finite() || !event.bpm.is_finite() || event.bpm <= 0.0
                }) {
                    return Err(DrillError::InvalidNumber { field: "tempo" });
                }
                let previous = std::mem::replace(&mut document.tempo, tempo);
                Ok(Self::SetTempoMap { tempo: previous })
            }
            Self::SetAudioTrack { audio } => {
                if audio
                    .as_ref()
                    .is_some_and(|track| track.validate().is_err())
                {
                    return Err(DrillError::InvalidNumber { field: "audio" });
                }
                let previous = std::mem::replace(&mut document.audio, audio);
                Ok(Self::SetAudioTrack { audio: previous })
            }
            Self::SetImageUnderlay { underlay } => {
                if underlay.as_ref().is_some_and(|value| !value.validate()) {
                    return Err(DrillError::InvalidEdit);
                }
                let previous = std::mem::replace(&mut document.underlay, underlay);
                Ok(Self::SetImageUnderlay { underlay: previous })
            }
            Self::RenameDocument { title } => {
                if title.len() > MAX_TEXT_BYTES {
                    return Err(DrillError::LimitExceeded {
                        field: "title",
                        limit: MAX_TEXT_BYTES,
                    });
                }
                let previous = std::mem::replace(&mut document.title, title);
                Ok(Self::RenameDocument { title: previous })
            }
            Self::AddSyncAnchor { id, anchor } => {
                let track = document.audio.as_mut().ok_or(DrillError::InvalidEdit)?;
                let mut anchors = track.anchors.clone();
                let id = if let Some(id) = id {
                    anchors
                        .add_with_id(id, anchor)
                        .map_err(|_| DrillError::InvalidEdit)?;
                    id
                } else {
                    anchors.add(anchor).map_err(|_| DrillError::InvalidEdit)?
                };
                track.anchors = anchors;
                Ok(Self::RemoveSyncAnchor { id })
            }
            Self::MoveSyncAnchor { id, anchor } => {
                let track = document.audio.as_mut().ok_or(DrillError::InvalidEdit)?;
                let mut anchors = track.anchors.clone();
                let previous = anchors
                    .move_anchor(id, anchor)
                    .map_err(|_| DrillError::InvalidEdit)?;
                track.anchors = anchors;
                Ok(Self::MoveSyncAnchor {
                    id,
                    anchor: previous,
                })
            }
            Self::RemoveSyncAnchor { id } => {
                let track = document.audio.as_mut().ok_or(DrillError::InvalidEdit)?;
                let mut anchors = track.anchors.clone();
                let anchor = anchors.remove_id(id).ok_or(DrillError::InvalidEdit)?;
                track.anchors = anchors;
                Ok(Self::AddSyncAnchor {
                    id: Some(id),
                    anchor,
                })
            }
            Self::AddSection { section, at } => {
                if document.sections.iter().any(|item| item.id == section.id)
                    || section.name.len() > MAX_TEXT_BYTES
                    || section.short.len() > MAX_TEXT_BYTES
                {
                    return Err(DrillError::InvalidEdit);
                }
                let at = at
                    .unwrap_or(document.sections.len())
                    .min(document.sections.len());
                let id = section.id;
                document.sections.insert(at, section);
                let reassign_to = document
                    .sections
                    .iter()
                    .find(|item| item.id != id)
                    .map_or(id, |item| item.id);
                Ok(Self::RemoveSection { id, reassign_to })
            }
            Self::RenameSection { id, name, short } => {
                if name.len() > MAX_TEXT_BYTES || short.len() > MAX_TEXT_BYTES {
                    return Err(DrillError::InvalidEdit);
                }
                let section = document
                    .sections
                    .iter_mut()
                    .find(|section| section.id == id)
                    .ok_or(DrillError::InvalidEdit)?;
                let previous_name = std::mem::replace(&mut section.name, name);
                let previous_short = std::mem::replace(&mut section.short, short);
                Ok(Self::RenameSection {
                    id,
                    name: previous_name,
                    short: previous_short,
                })
            }
            Self::RemoveSection { id, reassign_to } => {
                if id == reassign_to || !document.sections.iter().any(|item| item.id == reassign_to)
                {
                    return Err(DrillError::InvalidEdit);
                }
                let at = document
                    .sections
                    .iter()
                    .position(|section| section.id == id)
                    .ok_or(DrillError::InvalidEdit)?;
                let assignments = document
                    .performers
                    .iter()
                    .filter(|performer| performer.section == id)
                    .map(|performer| performer.id)
                    .collect::<Vec<_>>();
                let section = document.sections.remove(at);
                for performer in &mut document.performers {
                    if performer.section == id {
                        performer.section = reassign_to;
                    }
                }
                Ok(Self::RestoreSection {
                    at,
                    section,
                    assignments,
                    reassign_to,
                })
            }
            Self::RestoreSection {
                at,
                section,
                assignments,
                reassign_to,
            } => {
                if document.sections.iter().any(|item| item.id == section.id)
                    || !document.sections.iter().any(|item| item.id == reassign_to)
                    || assignments.iter().any(|id| {
                        !document
                            .performers
                            .iter()
                            .any(|performer| performer.id == *id)
                    })
                {
                    return Err(DrillError::InvalidEdit);
                }
                let id = section.id;
                document
                    .sections
                    .insert(at.min(document.sections.len()), section);
                for performer in &mut document.performers {
                    if assignments.contains(&performer.id) {
                        performer.section = id;
                    }
                }
                Ok(Self::RemoveSection { id, reassign_to })
            }
            Self::AssignPerformersToSection { assignments } => {
                let unique_performers = assignments
                    .iter()
                    .map(|(performer, _)| *performer)
                    .collect::<BTreeSet<_>>();
                if unique_performers.len() != assignments.len()
                    || assignments.iter().any(|(performer, section)| {
                        !document.performers.iter().any(|item| item.id == *performer)
                            || !document.sections.iter().any(|item| item.id == *section)
                    })
                {
                    return Err(DrillError::InvalidEdit);
                }
                let mut inverse = Vec::with_capacity(assignments.len());
                for (performer_id, section_id) in assignments {
                    let performer = document
                        .performers
                        .iter_mut()
                        .find(|item| item.id == performer_id)
                        .ok_or(DrillError::InvalidEdit)?;
                    inverse.push((performer_id, performer.section));
                    performer.section = section_id;
                }
                Ok(Self::AssignPerformersToSection {
                    assignments: inverse,
                })
            }
            Self::AddSubset { mut subset, at } => {
                if document.subsets.len() >= MAX_SUBSETS
                    || document.subsets.iter().any(|item| item.id == subset.id)
                    || subset.name.is_empty()
                    || subset.name.len() > MAX_TEXT_BYTES
                {
                    return Err(DrillError::InvalidEdit);
                }
                subset.members.sort_unstable();
                subset.members.dedup();
                if subset.members.iter().any(|id| {
                    !document
                        .performers
                        .iter()
                        .any(|performer| performer.id == *id)
                }) {
                    return Err(DrillError::InvalidEdit);
                }
                let id = subset.id;
                let at = at
                    .unwrap_or(document.subsets.len())
                    .min(document.subsets.len());
                document.subsets.insert(at, subset);
                Ok(Self::RemoveSubset { id })
            }
            Self::RenameSubset { id, name } => {
                if name.is_empty() || name.len() > MAX_TEXT_BYTES {
                    return Err(DrillError::InvalidEdit);
                }
                let subset = document
                    .subsets
                    .iter_mut()
                    .find(|subset| subset.id == id)
                    .ok_or(DrillError::MissingSubset)?;
                let previous = std::mem::replace(&mut subset.name, name);
                Ok(Self::RenameSubset { id, name: previous })
            }
            Self::SetSubsetMembers { id, mut members } => {
                members.sort_unstable();
                members.dedup();
                if members.iter().any(|id| {
                    !document
                        .performers
                        .iter()
                        .any(|performer| performer.id == *id)
                }) {
                    return Err(DrillError::InvalidEdit);
                }
                let subset = document
                    .subsets
                    .iter_mut()
                    .find(|subset| subset.id == id)
                    .ok_or(DrillError::MissingSubset)?;
                let previous = std::mem::replace(&mut subset.members, members);
                Ok(Self::SetSubsetMembers {
                    id,
                    members: previous,
                })
            }
            Self::RemoveSubset { id } => {
                let at = document
                    .subsets
                    .iter()
                    .position(|subset| subset.id == id)
                    .ok_or(DrillError::MissingSubset)?;
                let subset = document.subsets.remove(at);
                Ok(Self::AddSubset {
                    subset,
                    at: Some(at),
                })
            }
            Self::SetPerformerMetadata {
                performer,
                metadata,
            } => {
                if metadata.label.len() > MAX_TEXT_BYTES
                    || !metadata.height_m.is_finite()
                    || !(0.2..=3.0).contains(&metadata.height_m)
                {
                    return Err(DrillError::InvalidEdit);
                }
                let target = document
                    .performers
                    .iter_mut()
                    .find(|item| item.id == performer)
                    .ok_or(DrillError::InvalidEdit)?;
                let previous = target.metadata();
                target.label = metadata.label;
                target.symbol = metadata.symbol;
                target.color = metadata.color.into();
                target.height_m = metadata.height_m;
                target.kind = metadata.kind;
                Ok(Self::SetPerformerMetadata {
                    performer,
                    metadata: previous,
                })
            }
            Self::InsertCameraKeyframe {
                camera_id,
                keyframe,
            } => {
                let track = document
                    .camera_program
                    .tracks
                    .iter_mut()
                    .find(|track| track.id == camera_id)
                    .ok_or(DrillError::InvalidEdit)?;
                let count = keyframe.count;
                match track.insert_keyframe(keyframe)? {
                    Some(previous) => Ok(Self::InsertCameraKeyframe {
                        camera_id,
                        keyframe: previous,
                    }),
                    None => Ok(Self::RemoveCameraKeyframe { camera_id, count }),
                }
            }
            Self::RemoveCameraKeyframe { camera_id, count } => {
                let track = document
                    .camera_program
                    .tracks
                    .iter_mut()
                    .find(|track| track.id == camera_id)
                    .ok_or(DrillError::InvalidEdit)?;
                let keyframe = track
                    .remove_keyframe(count)
                    .ok_or(DrillError::InvalidEdit)?;
                Ok(Self::InsertCameraKeyframe {
                    camera_id,
                    keyframe,
                })
            }
            Self::InsertCameraCut { cut } => {
                let count = cut.count;
                match document.camera_program.insert_cut(cut)? {
                    Some(previous) => Ok(Self::InsertCameraCut { cut: previous }),
                    None => Ok(Self::RemoveCameraCut { count }),
                }
            }
            Self::RemoveCameraCut { count } => {
                let cut = document
                    .camera_program
                    .remove_cut(count)
                    .ok_or(DrillError::InvalidEdit)?;
                Ok(Self::InsertCameraCut { cut })
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct MoveCommand {
    pub set_index: usize,
    pub performer_indices: Vec<usize>,
    pub before: Vec<Point>,
    pub after: Vec<Point>,
}

#[derive(Clone, Debug)]
enum HistoryEntry {
    Stable(Edit),
    Legacy(MoveCommand),
}

impl HistoryEntry {
    fn kind(&self) -> EditKind {
        match self {
            Self::Legacy(_) => EditKind::Positions,
            Self::Stable(edit) => edit.kind(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Revision(pub u64);

#[derive(Debug, Default)]
pub struct History {
    commands: VecDeque<HistoryEntry>,
    cursor: usize,
    limit: usize,
    savepoint: Option<usize>,
    revision: Revision,
}

impl History {
    pub fn with_limit(limit: usize) -> Self {
        Self {
            commands: VecDeque::new(),
            cursor: 0,
            limit,
            savepoint: Some(0),
            revision: Revision::default(),
        }
    }

    pub fn push(&mut self, command: MoveCommand) {
        self.prepare_push();
        self.commands.push_back(HistoryEntry::Legacy(command));
        self.finish_push();
    }

    /// Applies and records a stable-ID edit. New code should use this instead
    /// of the index-based [`MoveCommand`] adapter.
    pub fn execute(&mut self, document: &mut Document, edit: Edit) -> Result<(), DrillError> {
        let inverse = edit.apply(document)?;
        self.prepare_push();
        self.commands.push_back(HistoryEntry::Stable(inverse));
        self.finish_push();
        Ok(())
    }

    fn finish_push(&mut self) {
        if self.commands.len() > self.limit {
            self.commands.pop_front();
            self.savepoint = self.savepoint.and_then(|position| position.checked_sub(1));
        }
        self.cursor = self.commands.len();
        self.revision.0 = self.revision.0.wrapping_add(1);
    }

    fn prepare_push(&mut self) {
        if self
            .savepoint
            .is_some_and(|position| position > self.cursor)
        {
            self.savepoint = None;
        }
        self.commands.truncate(self.cursor);
    }

    pub fn undo(&mut self, document: &mut Document) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        let success = match self.commands.get(self.cursor).cloned() {
            Some(HistoryEntry::Stable(edit)) => match edit.apply(document) {
                Ok(inverse) => {
                    self.commands[self.cursor] = HistoryEntry::Stable(inverse);
                    true
                }
                Err(_) => false,
            },
            Some(HistoryEntry::Legacy(command)) => {
                command.apply(document, false);
                true
            }
            None => false,
        };
        if success {
            self.revision.0 = self.revision.0.wrapping_add(1);
        } else {
            self.cursor += 1;
        }
        success
    }

    pub fn redo(&mut self, document: &mut Document) -> bool {
        if self.cursor == self.commands.len() {
            return false;
        }
        let success = match self.commands.get(self.cursor).cloned() {
            Some(HistoryEntry::Stable(edit)) => match edit.apply(document) {
                Ok(inverse) => {
                    self.commands[self.cursor] = HistoryEntry::Stable(inverse);
                    true
                }
                Err(_) => false,
            },
            Some(HistoryEntry::Legacy(command)) => {
                command.apply(document, true);
                true
            }
            None => false,
        };
        if success {
            self.cursor += 1;
            self.revision.0 = self.revision.0.wrapping_add(1);
        }
        success
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }
    pub fn can_redo(&self) -> bool {
        self.cursor < self.commands.len()
    }

    /// Kind of change `undo` would apply. `None` when the stack is empty.
    pub fn undo_kind(&self) -> Option<EditKind> {
        self.cursor
            .checked_sub(1)
            .and_then(|index| self.commands.get(index).map(HistoryEntry::kind))
    }

    /// Kind of change `redo` would apply. `None` when there is nothing to redo.
    pub fn redo_kind(&self) -> Option<EditKind> {
        self.commands.get(self.cursor).map(HistoryEntry::kind)
    }

    /// How many edits `undo` would walk back. Session UI uses this to name
    /// the next undo without storing a second history.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn redo_len(&self) -> usize {
        self.commands.len() - self.cursor
    }

    pub fn mark_saved(&mut self) {
        self.savepoint = Some(self.cursor);
    }

    pub fn is_dirty(&self) -> bool {
        self.savepoint != Some(self.cursor)
    }

    pub fn revision(&self) -> Revision {
        self.revision
    }
}

impl MoveCommand {
    pub fn apply(&self, document: &mut Document, forward: bool) {
        let values = if forward { &self.after } else { &self.before };
        for (&index, &point) in self.performer_indices.iter().zip(values) {
            if let Some(slot) = document
                .sets
                .get_mut(self.set_index)
                .and_then(|s| s.positions.get_mut(index))
            {
                *slot = point;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolation_reuses_output_allocation() {
        let doc = Document::demo(10, 10);
        let mut output = Vec::with_capacity(100);
        let pointer = output.as_ptr();
        doc.positions_at(0, 0.5, &mut output);
        assert_eq!(output.len(), 100);
        assert_eq!(pointer, output.as_ptr());
        assert_eq!(output[0], Point { x: 15.0, y: 26.0 });
    }

    #[test]
    fn json_round_trip_is_valid() {
        let doc = Document::demo(4, 4);
        let loaded = Document::from_json(&doc.to_json().unwrap()).unwrap();
        assert_eq!(loaded.performers.len(), 16);
        assert!(loaded.validate().is_ok());
    }

    #[test]
    fn blank_show_is_a_valid_single_set_roster() {
        let doc = Document::blank(16);
        assert!(doc.validate().is_ok());
        assert_eq!(doc.performers.len(), 16);
        assert_eq!(doc.sets.len(), 1);
        assert_eq!(doc.sets[0].positions.len(), 16);
        assert_eq!(doc.grid, GridConfig::japan_floor());
        assert!(
            doc.sets[0]
                .positions
                .iter()
                .all(|point| *point == doc.grid.snap(*point))
        );
    }

    #[test]
    fn add_and_remove_performer_keep_every_set_aligned() {
        let mut doc = Document::demo(2, 2);
        let id = doc.next_performer_id().unwrap();
        let section = doc.sections[0].id;
        doc.add_performer(
            Performer {
                id,
                label: "P5".into(),
                section,
                symbol: Symbol::Cross,
                color: None.into(),
                height_m: default_height_m(),
                kind: PerformerKind::Wind,
            },
            Point { x: 10.0, y: 10.0 },
        )
        .unwrap();
        assert_eq!(doc.performers.len(), 5);
        assert!(doc.sets.iter().all(|set| set.positions.len() == 5));
        assert_eq!(
            doc.sets[0].positions[4],
            doc.grid.snap(Point { x: 10.0, y: 10.0 })
        );

        doc.remove_performers(&[id]).unwrap();
        assert_eq!(doc.performers.len(), 4);
        assert!(doc.sets.iter().all(|set| set.positions.len() == 4));
        assert!(doc.validate().is_ok());

        let everyone: Vec<_> = doc.performers.iter().map(|p| p.id).collect();
        assert_eq!(
            doc.remove_performers(&everyone),
            Err(DrillError::InvalidEdit)
        );
        assert_eq!(doc.performers.len(), 4);
    }

    #[test]
    fn removing_the_last_set_is_rejected() {
        let mut doc = Document::blank(4);
        assert_eq!(doc.remove_set_at(0), Err(DrillError::EmptySets));
        let extra = doc.sets[0].clone();
        let mut extra = extra;
        extra.id = doc.next_set_id().unwrap();
        extra.name = "セット 2".into();
        extra.generated_by = None;
        doc.sets.push(extra);
        assert!(doc.validate().is_ok());
        doc.remove_set_at(1).unwrap();
        assert_eq!(doc.sets.len(), 1);
        assert_eq!(doc.remove_set_at(0), Err(DrillError::EmptySets));
    }

    #[test]
    fn malformed_and_future_documents_are_rejected() {
        assert!(Document::from_json("{not json").is_err());
        let mut document = Document::demo(2, 2);
        document.schema_version = u16::MAX;
        let error = Document::from_json(&document.to_json().unwrap()).unwrap_err();
        assert!(matches!(error, DrillError::UnsupportedSchema { .. }));
    }

    #[test]
    fn mismatched_set_size_is_rejected() {
        let mut document = Document::demo(2, 2);
        document.sets[1].positions.pop();
        let error = Document::from_json(&document.to_json().unwrap()).unwrap_err();
        assert!(matches!(error, DrillError::SetSizeMismatch { .. }));
    }

    #[test]
    fn migrates_v1_without_changing_drill_content() {
        let mut document = Document::demo(2, 2);
        document.schema_version = 1;
        let loaded = Document::from_json(&document.to_json().unwrap()).unwrap();
        assert_eq!(loaded.schema_version, SCHEMA_VERSION);
        assert_eq!(loaded.performers.len(), document.performers.len());
        assert_eq!(loaded.sets[0].positions, document.sets[0].positions);
    }

    #[test]
    fn rejects_non_finite_domain_values() {
        let mut document = Document::demo(2, 2);
        document.sets[0].positions[0].x = f32::NAN;
        assert!(matches!(
            document.validate(),
            Err(DrillError::InvalidNumber { .. })
        ));
        document.sets[0].positions[0].x = 1.0;
        document.grid.width = f32::INFINITY;
        assert_eq!(document.validate(), Err(DrillError::InvalidGrid));
    }

    #[test]
    fn empty_document_positions_are_safe() {
        let mut document = Document::demo(1, 1);
        document.sets.clear();
        let mut output = vec![Point { x: 1.0, y: 1.0 }];
        document.positions_at(0, 0.5, &mut output);
        assert!(output.is_empty());
    }

    #[test]
    fn move_command_is_reversible() {
        let mut doc = Document::demo(2, 2);
        let before = doc.sets[0].positions[0];
        let command = MoveCommand {
            set_index: 0,
            performer_indices: vec![0],
            before: vec![before],
            after: vec![Point { x: 20.0, y: 20.0 }],
        };
        command.apply(&mut doc, true);
        command.apply(&mut doc, false);
        assert_eq!(doc.sets[0].positions[0], before);
    }

    #[test]
    fn history_discards_redo_branch() {
        let mut doc = Document::demo(2, 2);
        let original = doc.sets[0].positions[0];
        let mut history = History::with_limit(100);
        let first = MoveCommand {
            set_index: 0,
            performer_indices: vec![0],
            before: vec![original],
            after: vec![Point { x: 1.0, y: 1.0 }],
        };
        first.apply(&mut doc, true);
        history.push(first);
        assert!(history.undo(&mut doc));
        let second = MoveCommand {
            set_index: 0,
            performer_indices: vec![0],
            before: vec![original],
            after: vec![Point { x: 2.0, y: 2.0 }],
        };
        second.apply(&mut doc, true);
        history.push(second);
        assert!(!history.can_redo());
    }

    #[test]
    fn replace_document_is_atomic_and_undoable() {
        let mut current = Document::demo(2, 2);
        let before = current.clone();
        let mut imported = Document::demo(3, 3);
        imported.title = "Imported selection".into();
        let after = imported.clone();
        let mut history = History::with_limit(10);
        history
            .execute(
                &mut current,
                Edit::ReplaceDocument {
                    document: Box::new(imported),
                },
            )
            .unwrap();
        assert_eq!(current, after);
        assert!(history.undo(&mut current));
        assert_eq!(current, before);
        assert!(history.redo(&mut current));
        assert_eq!(current, after);
    }

    #[test]
    fn stable_edit_survives_performer_and_set_reordering() {
        let mut doc = Document::demo(2, 2);
        let set_id = doc.sets[0].id;
        let performer_id = doc.performers[0].id;
        let original = doc.sets[0].positions[0];
        let mut history = History::with_limit(10);
        history
            .execute(
                &mut doc,
                Edit::MovePerformers {
                    set_id,
                    performer_ids: vec![performer_id],
                    positions: vec![Point { x: 99.0, y: 98.0 }],
                },
            )
            .unwrap();

        doc.sets.swap(0, 1);
        doc.performers.swap(0, 3);
        for set in &mut doc.sets {
            set.positions.swap(0, 3);
        }
        assert!(history.undo(&mut doc));
        let set = doc.sets.iter().find(|set| set.id == set_id).unwrap();
        let performer_index = doc
            .performers
            .iter()
            .position(|performer| performer.id == performer_id)
            .unwrap();
        assert_eq!(set.positions[performer_index], original);
        assert!(history.redo(&mut doc));
        assert_eq!(
            doc.sets[1].positions[performer_index],
            Point { x: 99.0, y: 98.0 }
        );
    }

    #[test]
    fn stable_edit_fails_atomically_after_target_deletion() {
        let mut doc = Document::demo(1, 2);
        let set_id = doc.sets[0].id;
        let deleted_id = doc.performers.remove(0).id;
        for set in &mut doc.sets {
            set.positions.remove(0);
        }
        let before = doc.sets[0].positions.clone();
        let result = Edit::MovePerformers {
            set_id,
            performer_ids: vec![deleted_id],
            positions: vec![Point { x: 9.0, y: 9.0 }],
        }
        .apply(&mut doc);
        assert_eq!(result, Err(DrillError::MissingPerformer));
        assert_eq!(doc.sets[0].positions, before);
    }

    #[test]
    fn ten_thousand_stable_undo_redo_round_trips() {
        let mut doc = Document::demo(1, 1);
        let set_id = doc.sets[0].id;
        let performer_id = doc.performers[0].id;
        let original = doc.sets[0].positions[0];
        let mut history = History::with_limit(10_000);
        for i in 0..10_000 {
            history
                .execute(
                    &mut doc,
                    Edit::MovePerformers {
                        set_id,
                        performer_ids: vec![performer_id],
                        positions: vec![Point {
                            x: i as f32,
                            y: 1.0,
                        }],
                    },
                )
                .unwrap();
        }
        history.mark_saved();
        assert!(!history.is_dirty());
        for _ in 0..10_000 {
            assert!(history.undo(&mut doc));
        }
        assert_eq!(doc.sets[0].positions[0], original);
        assert!(history.is_dirty());
        for _ in 0..10_000 {
            assert!(history.redo(&mut doc));
        }
        assert_eq!(doc.sets[0].positions[0], Point { x: 9_999.0, y: 1.0 });
        assert!(!history.is_dirty());
        assert!(history.revision().0 >= 30_000);
    }

    #[test]
    fn migration_assigns_stable_set_ids_when_absent() {
        let doc = Document::demo(1, 1);
        let mut value = serde_json::to_value(doc).unwrap();
        for set in value["sets"].as_array_mut().unwrap() {
            set.as_object_mut().unwrap().remove("id");
        }
        let loaded = Document::from_json(&serde_json::to_string(&value).unwrap()).unwrap();
        assert_ne!(loaded.sets[0].id, loaded.sets[1].id);
    }

    #[test]
    fn document_setting_edits_are_reversible() {
        let mut doc = Document::demo(1, 1);
        let original_grid = doc.grid.clone();
        let original_tempo = doc.tempo.events().to_vec();
        let mut history = History::with_limit(20);

        history
            .execute(
                &mut doc,
                Edit::RenameDocument {
                    title: "Renamed".into(),
                },
            )
            .unwrap();
        history
            .execute(
                &mut doc,
                Edit::ReplaceGrid {
                    grid: GridConfig::indoor(),
                    scale_positions: false,
                },
            )
            .unwrap();
        history
            .execute(
                &mut doc,
                Edit::SetTempoMap {
                    tempo: tempo::TempoMap::constant(90.0),
                },
            )
            .unwrap();
        history
            .execute(
                &mut doc,
                Edit::SetAudioTrack {
                    audio: Some(audio::AudioTrack {
                        path: "audio.wav".into(),
                        duration_seconds: 10.0,
                        ..audio::AudioTrack::default()
                    }),
                },
            )
            .unwrap();
        for _ in 0..4 {
            assert!(history.undo(&mut doc));
        }
        assert_eq!(doc.title, "新しいドリル");
        assert_eq!(doc.grid, original_grid);
        assert_eq!(doc.tempo.events(), original_tempo);
        assert!(doc.audio.is_none());
        for _ in 0..4 {
            assert!(history.redo(&mut doc));
        }
        assert_eq!(doc.title, "Renamed");
        assert_eq!(doc.tempo.bpm_at(0.0), 90.0);
        assert!(doc.audio.is_some());
    }

    #[test]
    fn invalid_setting_edit_is_atomic() {
        let mut doc = Document::demo(1, 1);
        let before = doc.grid.clone();
        let mut invalid = before.clone();
        invalid.width = f32::NAN;
        assert_eq!(
            Edit::ReplaceGrid {
                grid: invalid,
                scale_positions: true,
            }
            .apply(&mut doc),
            Err(DrillError::InvalidGrid)
        );
        assert_eq!(doc.grid, before);
    }

    #[test]
    fn anchor_edits_are_id_stable_and_reversible() {
        let mut doc = Document::demo(1, 1);
        doc.audio = Some(audio::AudioTrack {
            duration_seconds: 30.0,
            ..audio::AudioTrack::default()
        });
        let mut history = History::with_limit(20);
        for anchor in [
            audio::SyncAnchor {
                count: 0.0,
                seconds: 0.0,
            },
            audio::SyncAnchor {
                count: 8.0,
                seconds: 4.0,
            },
        ] {
            history
                .execute(&mut doc, Edit::AddSyncAnchor { id: None, anchor })
                .unwrap();
        }
        let second_id = doc.audio.as_ref().unwrap().anchors.id_at(1).unwrap();
        history
            .execute(
                &mut doc,
                Edit::MoveSyncAnchor {
                    id: second_id,
                    anchor: audio::SyncAnchor {
                        count: 10.0,
                        seconds: 5.0,
                    },
                },
            )
            .unwrap();
        assert_eq!(
            doc.audio
                .as_ref()
                .unwrap()
                .anchors
                .get(second_id)
                .unwrap()
                .count,
            10.0
        );
        assert!(history.undo(&mut doc));
        assert_eq!(
            doc.audio
                .as_ref()
                .unwrap()
                .anchors
                .get(second_id)
                .unwrap()
                .count,
            8.0
        );
        assert!(history.redo(&mut doc));
        assert_eq!(
            doc.audio
                .as_ref()
                .unwrap()
                .anchors
                .get(second_id)
                .unwrap()
                .count,
            10.0
        );
        history
            .execute(&mut doc, Edit::RemoveSyncAnchor { id: second_id })
            .unwrap();
        assert!(doc.audio.as_ref().unwrap().anchors.get(second_id).is_none());
        assert!(history.undo(&mut doc));
        assert_eq!(
            doc.audio
                .as_ref()
                .unwrap()
                .anchors
                .get(second_id)
                .unwrap()
                .count,
            10.0
        );
    }

    #[test]
    fn ten_thousand_anchor_drag_edits_share_a_stable_coalesce_key() {
        let anchors = audio::AnchorMap::try_from_anchors([audio::SyncAnchor {
            count: 0.0,
            seconds: 0.0,
        }])
        .unwrap();
        let id = anchors.id_at(0).unwrap();
        let expected = EditCoalesceKey::MoveSyncAnchor(id);
        for i in 0..10_000 {
            let edit = Edit::MoveSyncAnchor {
                id,
                anchor: audio::SyncAnchor {
                    count: i as f64 / 100.0,
                    seconds: i as f64 / 200.0,
                },
            };
            assert_eq!(edit.coalesce_key(), Some(expected));
        }
    }

    #[test]
    fn section_and_performer_metadata_edits_round_trip() {
        let mut doc = Document::demo(1, 2);
        let ensemble = doc.sections[0].id;
        let guard = SectionId::new(2).unwrap();
        let performer = doc.performers[0].id;
        let original_metadata = doc.performers[0].metadata();
        let mut history = History::with_limit(20);
        history
            .execute(
                &mut doc,
                Edit::AddSection {
                    section: Section {
                        id: guard,
                        name: "Guard".into(),
                        short: "CG".into(),
                        color: [255, 0, 128],
                        order: 1,
                    },
                    at: None,
                },
            )
            .unwrap();
        history
            .execute(
                &mut doc,
                Edit::AssignPerformersToSection {
                    assignments: vec![(performer, guard)],
                },
            )
            .unwrap();
        history
            .execute(
                &mut doc,
                Edit::RenameSection {
                    id: guard,
                    name: "Color Guard".into(),
                    short: "Guard".into(),
                },
            )
            .unwrap();
        history
            .execute(
                &mut doc,
                Edit::SetPerformerMetadata {
                    performer,
                    metadata: PerformerMetadata {
                        label: "G1".into(),
                        symbol: Symbol::Diamond,
                        color: None,
                        height_m: 1.8,
                        kind: PerformerKind::Guard,
                    },
                },
            )
            .unwrap();
        history
            .execute(
                &mut doc,
                Edit::RemoveSection {
                    id: guard,
                    reassign_to: ensemble,
                },
            )
            .unwrap();
        assert_eq!(doc.performers[0].section, ensemble);
        assert!(history.undo(&mut doc));
        assert_eq!(doc.performers[0].section, guard);
        for _ in 0..4 {
            assert!(history.undo(&mut doc));
        }
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.performers[0].section, ensemble);
        assert_eq!(doc.performers[0].metadata(), original_metadata);
        for _ in 0..5 {
            assert!(history.redo(&mut doc));
        }
        assert_eq!(doc.performers[0].section, ensemble);
        assert_eq!(doc.performers[0].kind, PerformerKind::Guard);
    }

    #[test]
    fn invalid_section_assignment_is_atomic() {
        let mut doc = Document::demo(1, 2);
        let before = doc.performers.iter().map(|p| p.section).collect::<Vec<_>>();
        let missing = SectionId::new(99).unwrap();
        let result = Edit::AssignPerformersToSection {
            assignments: vec![(doc.performers[0].id, missing)],
        }
        .apply(&mut doc);
        assert_eq!(result, Err(DrillError::InvalidEdit));
        assert_eq!(
            doc.performers.iter().map(|p| p.section).collect::<Vec<_>>(),
            before
        );
    }

    #[test]
    fn legacy_roster_json_migrates_without_visual_color_change() {
        let doc = Document::demo(1, 2);
        let expected = doc.performers[0].resolved_color(&doc.sections);
        let mut value = serde_json::to_value(doc).unwrap();
        value.as_object_mut().unwrap().remove("sections");
        for performer in value["performers"].as_array_mut().unwrap() {
            let object = performer.as_object_mut().unwrap();
            object.remove("section");
            object.remove("symbol");
            object.remove("height_m");
            object.remove("kind");
        }
        let loaded = Document::from_json(&serde_json::to_string(&value).unwrap()).unwrap();
        assert_eq!(loaded.sections.len(), 1);
        assert!(
            loaded
                .performers
                .iter()
                .all(|p| p.section == loaded.sections[0].id)
        );
        assert_eq!(
            loaded.performers[0].resolved_color(&loaded.sections),
            expected
        );
        assert_eq!(loaded.performers[0].height_m, 1.7);
        assert_eq!(loaded.performers[0].symbol, Symbol::Circle);
    }

    #[test]
    fn grid_snap_uses_independent_step_sizes() {
        let grid = GridConfig {
            horizontal_steps: 8,
            horizontal_units: 5.0,
            vertical_steps: 4,
            vertical_units: 5.0,
            ..GridConfig::default()
        };
        assert_eq!(
            grid.snap(Point { x: 1.1, y: 1.1 }),
            Point { x: 1.25, y: 1.25 }
        );
    }

    #[test]
    fn major_line_positions_always_land_on_the_fine_step_grid() {
        // japan_floor's round 5m major interval and its 4.572/8m step size
        // are not whole multiples of each other -- the exact scenario that
        // used to leave bold lines visibly off the fine grid everywhere but
        // the origin.
        let grid = GridConfig::japan_floor();
        let dx = grid.horizontal_units / f32::from(grid.horizontal_steps);
        let dy = grid.vertical_units / f32::from(grid.vertical_steps);
        assert!(
            (5.0_f32 / dx).fract() > 1e-3,
            "test needs an incommensurate interval/step pair"
        );

        for x in grid.horizontal_major_positions() {
            let steps = x / dx;
            assert!(
                (steps - steps.round()).abs() < 1e-3,
                "major x={x} is not on the fine step grid"
            );
        }
        for y in grid.vertical_major_positions() {
            let steps = y / dy;
            assert!(
                (steps - steps.round()).abs() < 1e-3,
                "major y={y} is not on the fine step grid"
            );
        }
    }

    #[test]
    fn major_line_positions_stay_close_to_the_round_number_interval() {
        // Positions radiate outward from the axis midpoint, so the "ideal"
        // round-number position for each line is the nearest multiple of
        // `interval` measured from the center, not from x=0.
        let grid = GridConfig::japan_floor();
        let interval = grid.major_line_interval;
        let dx = grid.horizontal_units / f32::from(grid.horizontal_steps);
        let center = grid.width * 0.5;
        for x in grid.horizontal_major_positions() {
            let offset_in_intervals = (x - center) / interval;
            let ideal = center + offset_in_intervals.round() * interval;
            assert!(
                (x - ideal).abs() <= dx * 0.5 + 1e-3,
                "major line drifted too far from its round-number position: ideal={ideal} actual={x}"
            );
        }
    }

    #[test]
    fn major_line_positions_are_ordered_in_bounds_and_centered() {
        let grid = GridConfig::default();
        for (positions, axis_len) in [
            (grid.horizontal_major_positions(), grid.width),
            (grid.vertical_major_positions(), grid.height),
        ] {
            assert!(positions.windows(2).all(|w| w[0] < w[1]));
            assert!(positions.iter().all(|&p| (0.0..=axis_len).contains(&p)));
            // Centered: the outermost lines on each side sit roughly the
            // same distance from the midpoint (within one major interval --
            // the axis length need not be an exact multiple of it).
            let center = axis_len * 0.5;
            let first_gap = center - positions.first().copied().unwrap_or(center);
            let last_gap = positions.last().copied().unwrap_or(center) - center;
            assert!(
                (first_gap - last_gap).abs() <= grid.major_line_interval,
                "positions aren't centered: first_gap={first_gap} last_gap={last_gap}"
            );
        }
    }

    #[test]
    fn japan_floor_canvas_is_larger_than_its_reference_frames() {
        let grid = GridConfig::japan_floor();
        assert_eq!(grid.width, 46.0);
        assert_eq!(grid.height, 40.0);
        assert_eq!(grid.reference_frames, vec![30.0, 20.0]);
        // The frames must actually fit inside the canvas with room to
        // spare, or "staging margin around the marked boundary" is a lie.
        for &side in &grid.reference_frames {
            assert!(side < grid.width);
            assert!(side < grid.height);
        }
    }

    #[test]
    fn reference_frame_bounds_are_centered_and_on_the_fine_grid() {
        let grid = GridConfig::japan_floor();
        let dx = grid.horizontal_units / f32::from(grid.horizontal_steps);
        let dy = grid.vertical_units / f32::from(grid.vertical_steps);
        let bounds = grid.reference_frame_bounds();
        assert_eq!(bounds.len(), grid.reference_frames.len());
        for (&side, (min_x, min_y, max_x, max_y)) in grid.reference_frames.iter().zip(&bounds) {
            // Centered: the frame's own midpoint matches the canvas midpoint,
            // within one step (the snap can nudge each edge independently).
            assert!(((min_x + max_x) * 0.5 - grid.width * 0.5).abs() <= dx);
            assert!(((min_y + max_y) * 0.5 - grid.height * 0.5).abs() <= dy);
            // Close to the requested side length, within a step on each edge.
            assert!((max_x - min_x - side).abs() <= dx);
            assert!((max_y - min_y - side).abs() <= dy);
            // On the fine step grid.
            for value in [min_x, max_x] {
                let steps = value / dx;
                assert!((steps - steps.round()).abs() < 1e-3);
            }
            for value in [min_y, max_y] {
                let steps = value / dy;
                assert!((steps - steps.round()).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn reference_frame_bounds_is_empty_for_presets_without_frames() {
        assert!(GridConfig::default().reference_frame_bounds().is_empty());
        assert!(GridConfig::indoor().reference_frame_bounds().is_empty());
        assert!(GridConfig::soccer().reference_frame_bounds().is_empty());
    }

    #[test]
    fn max_x_and_max_y_are_on_the_fine_grid_and_at_most_the_canvas_size() {
        let grid = GridConfig::japan_floor();
        let dx = grid.horizontal_units / f32::from(grid.horizontal_steps);
        let dy = grid.vertical_units / f32::from(grid.vertical_steps);
        assert!(grid.max_x() <= grid.width);
        assert!(grid.max_y() <= grid.height);
        assert!(grid.width - grid.max_x() < dx);
        assert!(grid.height - grid.max_y() < dy);
        let corner = Point {
            x: grid.max_x(),
            y: grid.max_y(),
        };
        assert_eq!(grid.snap(corner), corner);
    }

    #[test]
    fn snap_sequence_matches_disabled_grid_unchanged() {
        let grid = GridConfig {
            snap_enabled: false,
            ..GridConfig::default()
        };
        let points = vec![Point { x: 1.23, y: 4.56 }, Point { x: 7.89, y: 0.12 }];
        assert_eq!(grid.snap_sequence(&points), points);
    }

    #[test]
    fn snap_sequence_keeps_every_point_on_the_grid() {
        let grid = GridConfig {
            horizontal_steps: 8,
            horizontal_units: 5.0,
            vertical_steps: 8,
            vertical_units: 5.0,
            ..GridConfig::default()
        };
        let dx = grid.horizontal_units / grid.horizontal_steps as f32;
        let dy = grid.vertical_units / grid.vertical_steps as f32;
        // A shallow arc: x advances briskly, y drifts by less than half a
        // grid step per sample -- exactly the case that makes independent
        // per-point rounding look noisy.
        let points: Vec<Point> = (0..40)
            .map(|i| Point {
                x: i as f32 * 0.3,
                y: 10.0 + (i as f32 * 0.05).sin() * 2.0,
            })
            .collect();
        let snapped = grid.snap_sequence(&points);
        assert_eq!(snapped.len(), points.len());
        for point in &snapped {
            let steps_x = point.x / dx;
            let steps_y = point.y / dy;
            assert!(
                (steps_x - steps_x.round()).abs() < 1e-3,
                "x={} is not on the grid",
                point.x
            );
            assert!(
                (steps_y - steps_y.round()).abs() < 1e-3,
                "y={} is not on the grid",
                point.y
            );
        }
    }

    #[test]
    fn snap_sequence_never_drifts_more_than_one_step_from_the_true_curve() {
        // Unlike a single independent `snap` (which is always within half a
        // step of its input by definition of round-to-nearest), the carried
        // remainder in `snap_sequence` means one sample's deviation can add
        // to the *previous* sample's carried error: `raw - snapped` works
        // out to `carry_new - carry_prev`, each individually within half a
        // step, so their difference is bounded by a full step in the worst
        // case. That's the expected, known trade-off of error diffusion
        // (same as image dithering): a single sample can land up to one
        // step from its true position, in exchange for the *sequence*
        // tracking the curve far more evenly than independent rounding
        // does. This test proves that trade-off stays bounded -- error
        // never compounds past one step, however long the curve runs.
        let grid = GridConfig {
            horizontal_steps: 8,
            horizontal_units: 5.0,
            vertical_steps: 8,
            vertical_units: 5.0,
            ..GridConfig::default()
        };
        let dx = grid.horizontal_units / grid.horizontal_steps as f32;
        let dy = grid.vertical_units / grid.vertical_steps as f32;
        // A real arc, not a straight line: x and y both curve.
        let radius = 12.0_f32;
        let points: Vec<Point> = (0..200)
            .map(|i| {
                let angle = i as f32 * 0.05;
                Point {
                    x: radius * angle.cos(),
                    y: radius * angle.sin(),
                }
            })
            .collect();
        let snapped = grid.snap_sequence(&points);
        for (raw, snapped) in points.iter().zip(&snapped) {
            assert!(
                (raw.x - snapped.x).abs() <= dx + 1e-3,
                "x drifted too far from the source curve: raw={} snapped={}",
                raw.x,
                snapped.x
            );
            assert!(
                (raw.y - snapped.y).abs() <= dy + 1e-3,
                "y drifted too far from the source curve: raw={} snapped={}",
                raw.y,
                snapped.y
            );
        }
    }

    #[test]
    fn coordinate_notation_change_is_a_single_undoable_grid_edit() {
        let mut doc = Document::demo(1, 1);
        let mut grid = doc.grid.clone();
        grid.coordinate_notation = coordinates::CoordinateNotation::dci();
        let mut history = History::with_limit(4);
        history
            .execute(
                &mut doc,
                Edit::ReplaceGrid {
                    grid,
                    scale_positions: false,
                },
            )
            .unwrap();
        assert_eq!(
            doc.grid.coordinate_notation,
            coordinates::CoordinateNotation::dci()
        );
        assert!(history.undo(&mut doc));
        assert_eq!(
            doc.grid.coordinate_notation,
            coordinates::CoordinateNotation::default()
        );
    }

    #[test]
    fn replacing_grid_can_preserve_relative_positions() {
        let mut doc = Document::demo(1, 1);
        let before = doc.sets[0].positions[0];
        let mut grid = GridConfig::default();
        grid.width *= 2.0;
        grid.height *= 2.0;
        doc.replace_grid(grid, true);
        assert_eq!(
            doc.sets[0].positions[0],
            Point {
                x: before.x * 2.0,
                y: before.y * 2.0
            }
        );
    }

    #[test]
    fn line_layout_preserves_endpoints() {
        let points = evenly_spaced_line(Point { x: 0.0, y: 5.0 }, Point { x: 10.0, y: 5.0 }, 3);
        assert_eq!(points[1], Point { x: 5.0, y: 5.0 });
    }

    #[test]
    fn arc_layout_preserves_radius() {
        let points = evenly_spaced_arc(
            Point { x: 10.0, y: 10.0 },
            5.0,
            0.0,
            std::f32::consts::PI,
            5,
        );
        for point in points {
            let radius = ((point.x - 10.0).powi(2) + (point.y - 10.0).powi(2)).sqrt();
            assert!((radius - 5.0).abs() < 0.001);
        }
    }

    #[test]
    fn count_timeline_maps_across_sets() {
        let doc = Document::demo(2, 2);
        assert_eq!(doc.timeline_counts(), 16);
        assert_eq!(doc.locate_count(8.0), (0, 8.0));
        assert_eq!(doc.locate_count(16.0), (1, 0.0));
        assert_eq!(doc.global_count(0, 7.0), 7.0);
    }

    #[test]
    fn camera_keyframe_edit_is_stable_and_reversible() {
        let mut doc = Document::demo(1, 2);
        let camera_id = doc.camera_program.tracks[0].id;
        let before = doc.camera_program.clone();
        let keyframe =
            camera::CameraKeyframe::from_camera(8.0, camera::Camera::overhead(&doc.grid));
        let inverse = Edit::InsertCameraKeyframe {
            camera_id,
            keyframe,
        }
        .apply(&mut doc)
        .unwrap();
        assert_eq!(doc.camera_program.tracks[0].keyframes().len(), 2);
        inverse.apply(&mut doc).unwrap();
        assert_eq!(doc.camera_program, before);
    }

    #[test]
    fn camera_cut_edit_is_reversible() {
        let mut doc = Document::demo(2, 2);
        let before = doc.camera_program.clone();
        let camera = doc.camera_program.tracks[0].id;
        let inverse = Edit::InsertCameraCut {
            cut: camera::CameraCut { count: 1.0, camera },
        }
        .apply(&mut doc)
        .unwrap();
        assert_eq!(doc.camera_program.cuts.len(), before.cuts.len() + 1);
        inverse.apply(&mut doc).unwrap();
        assert_eq!(doc.camera_program, before);
    }

    #[test]
    fn transition_analysis_finds_collisions_and_long_strides() {
        let mut doc = Document::demo(1, 2);
        doc.sets[1].positions[0] = Point { x: 90.0, y: 40.0 };
        doc.sets[1].positions[1] = Point { x: 90.1, y: 40.0 };
        let result = analyze_transition(&doc, 0, 0.5, 1.0);
        assert_eq!(result.collisions, 1);
        assert!(result.excessive_strides > 0);
    }

    #[test]
    fn subset_edits_are_canonical_and_fully_reversible() {
        let mut doc = Document::demo(1, 3);
        let original = doc.clone();
        let id = SubsetId::new(7).unwrap();
        let inverse = Edit::AddSubset {
            subset: Subset {
                id,
                name: "Soloists".into(),
                members: vec![
                    doc.performers[2].id,
                    doc.performers[0].id,
                    doc.performers[0].id,
                ],
            },
            at: None,
        }
        .apply(&mut doc)
        .unwrap();
        assert_eq!(
            doc.subsets[0].members,
            vec![doc.performers[0].id, doc.performers[2].id]
        );
        assert!(doc.validate().is_ok());
        inverse.apply(&mut doc).unwrap();
        assert_eq!(doc, original);
    }

    #[test]
    fn subset_rename_members_and_remove_round_trip_through_history() {
        let mut doc = Document::demo(1, 3);
        let id = SubsetId::new(4).unwrap();
        Edit::AddSubset {
            subset: Subset {
                id,
                name: "A".into(),
                members: vec![],
            },
            at: None,
        }
        .apply(&mut doc)
        .unwrap();
        let before = doc.clone();
        let mut history = History::with_limit(20);
        let member = doc.performers[1].id;
        history
            .execute(
                &mut doc,
                Edit::RenameSubset {
                    id,
                    name: "Featured".into(),
                },
            )
            .unwrap();
        history
            .execute(
                &mut doc,
                Edit::SetSubsetMembers {
                    id,
                    members: vec![member],
                },
            )
            .unwrap();
        history
            .execute(&mut doc, Edit::RemoveSubset { id })
            .unwrap();
        assert!(doc.subsets.is_empty());
        assert!(history.undo(&mut doc));
        assert!(history.undo(&mut doc));
        assert!(history.undo(&mut doc));
        assert_eq!(doc, before);
        assert!(history.redo(&mut doc));
        assert!(history.redo(&mut doc));
        assert_eq!(doc.subsets[0].name, "Featured");
        assert_eq!(doc.subsets[0].members, vec![doc.performers[1].id]);
    }

    #[test]
    fn schema_two_migrates_with_empty_subsets() {
        let doc = Document::demo(1, 1);
        let mut value = serde_json::to_value(&doc).unwrap();
        value["schema_version"] = serde_json::json!(2);
        value.as_object_mut().unwrap().remove("subsets");
        let loaded = Document::from_json(&serde_json::to_string(&value).unwrap()).unwrap();
        assert_eq!(loaded.schema_version, SCHEMA_VERSION);
        assert!(loaded.subsets.is_empty());
    }

    #[test]
    fn snapshots_compare_by_stable_identity() {
        let left = snapshot::DocumentSnapshot::capture("main", &Document::demo(1, 2)).unwrap();
        let mut changed = left.document.clone();
        let performer = changed.performers[0].id;
        changed.performers[0].label = "Lead".into();
        changed.sets[0].positions.swap(0, 1);
        let right = snapshot::DocumentSnapshot::capture("idea", &changed).unwrap();
        let diff = left.compare(&right);
        assert_eq!(diff.changed_performers, vec![performer]);
        assert_eq!(diff.changed_sets, vec![changed.sets[0].id]);
        assert!(!diff.is_empty());
    }

    #[test]
    fn set_annotation_edit_is_persistent_and_reversible() {
        let mut doc = Document::demo(1, 1);
        let set_id = doc.sets[0].id;
        let annotation = SetAnnotation {
            title: "Finale".into(),
            notes: "Lights: blue".into(),
            rehearsal_mark: "Z".into(),
            tempo_bpm: Some(168.0),
            sync_time_seconds: Some(42.25),
            transition_duration_seconds: Some(7.5),
        };
        let mut history = History::with_limit(10);
        history
            .execute(
                &mut doc,
                Edit::SetAnnotation {
                    set_id,
                    annotation: annotation.clone(),
                },
            )
            .unwrap();
        assert_eq!(doc.sets[0].annotation, annotation);
        assert!(history.undo(&mut doc));
        assert_eq!(doc.sets[0].annotation, SetAnnotation::default());
        assert!(history.redo(&mut doc));
        assert_eq!(doc.sets[0].annotation, annotation);
        let loaded = Document::from_json(&doc.to_json().unwrap()).unwrap();
        assert_eq!(loaded.sets[0].annotation, doc.sets[0].annotation);
    }

    #[test]
    fn production_markers_are_stable_persistent_and_undoable() {
        let mut doc = Document::demo(1, 1);
        let marker = ProductionMarker {
            id: ProductionMarkerId::new(41).unwrap(),
            count: 7,
            kind: ProductionMarkerKind::Rehearsal,
            label: "Letter B".into(),
            detail: "Reset interval".into(),
        };
        let mut history = History::with_limit(8);
        history
            .execute(
                &mut doc,
                Edit::InsertProductionMarker {
                    marker: marker.clone(),
                },
            )
            .unwrap();
        assert_eq!(doc.production_markers, vec![marker.clone()]);
        history
            .execute(
                &mut doc,
                Edit::SetProductionMarker {
                    marker: ProductionMarker {
                        label: "B".into(),
                        ..marker.clone()
                    },
                },
            )
            .unwrap();
        assert_eq!(doc.production_markers[0].id, marker.id);
        assert!(history.undo(&mut doc));
        assert_eq!(doc.production_markers[0], marker);
        assert!(history.undo(&mut doc));
        assert!(doc.production_markers.is_empty());
        assert!(history.redo(&mut doc));
        let loaded = Document::from_json(&doc.to_json().unwrap()).unwrap();
        assert_eq!(loaded.production_markers, doc.production_markers);

        let mut legacy = serde_json::to_value(&doc).unwrap();
        legacy["schema_version"] = serde_json::json!(4);
        legacy.as_object_mut().unwrap().remove("production_markers");
        let migrated = Document::from_json(&serde_json::to_string(&legacy).unwrap()).unwrap();
        assert!(migrated.production_markers.is_empty());
        assert_eq!(migrated.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn invalid_set_annotation_is_rejected_atomically() {
        let mut doc = Document::demo(1, 1);
        let before = doc.clone();
        let result = Edit::SetAnnotation {
            set_id: doc.sets[0].id,
            annotation: SetAnnotation {
                tempo_bpm: Some(f32::NAN),
                ..SetAnnotation::default()
            },
        }
        .apply(&mut doc);
        assert_eq!(result, Err(DrillError::InvalidEdit));
        assert_eq!(doc, before);
    }

    #[test]
    fn parametric_shape_is_persistent_validated_and_reversible() {
        let mut doc = Document::demo(2, 3);
        let set_id = doc.sets[0].id;
        let shape = shapes::ShapeSpec::Ellipse {
            center: Point { x: 50.0, y: 42.0 },
            radius_x: 20.0,
            radius_y: 8.0,
            rotation: 0.25,
        };
        let mut history = History::with_limit(8);
        history
            .execute(
                &mut doc,
                Edit::SetShape {
                    set_id,
                    shape: Some(shape.clone()),
                },
            )
            .unwrap();
        assert_eq!(doc.sets[0].shape.as_ref(), Some(&shape));
        assert!(history.undo(&mut doc));
        assert!(doc.sets[0].shape.is_none());
        assert!(history.redo(&mut doc));
        let loaded = Document::from_json(&doc.to_json().unwrap()).unwrap();
        assert_eq!(loaded.sets[0].shape, Some(shape));

        let before = doc.clone();
        let invalid = shapes::ShapeSpec::Circle {
            center: Point::default(),
            radius: -1.0,
        };
        assert!(
            Edit::SetShape {
                set_id,
                shape: Some(invalid)
            }
            .apply(&mut doc)
            .is_err()
        );
        assert_eq!(doc, before);
    }

    #[test]
    fn image_underlay_is_schema_migrated_persistent_and_undoable() {
        let mut doc = Document::demo(1, 1);
        let underlay = underlay::ImageUnderlay {
            content_hash: "ab".repeat(32),
            byte_len: 1_024,
            original_name: "reference.png".into(),
            external_path: Some("reference.png".into()),
            placement: underlay::UnderlayPlacement {
                x: 4.0,
                y: 5.0,
                scale_x: 0.8,
                scale_y: 0.7,
                rotation_radians: 0.2,
                opacity: 0.4,
                visible: true,
                render_policy: underlay::UnderlayRenderPolicy::Editor2dOnly,
            },
        };
        let mut history = History::with_limit(4);
        history
            .execute(
                &mut doc,
                Edit::SetImageUnderlay {
                    underlay: Some(underlay.clone()),
                },
            )
            .unwrap();
        assert_eq!(doc.underlay.as_ref(), Some(&underlay));
        assert!(history.undo(&mut doc));
        assert!(doc.underlay.is_none());
        assert!(history.redo(&mut doc));
        assert_eq!(
            Document::from_json(&doc.to_json().unwrap())
                .unwrap()
                .underlay,
            Some(underlay)
        );

        let mut legacy: serde_json::Value = serde_json::from_str(&doc.to_json().unwrap()).unwrap();
        legacy["schema_version"] = serde_json::json!(3);
        legacy.as_object_mut().unwrap().remove("underlay");
        assert!(
            Document::from_json(&legacy.to_string())
                .unwrap()
                .underlay
                .is_none()
        );
    }

    #[test]
    fn snap_to_step_ignores_the_snap_toggle() {
        let off = GridConfig {
            snap_enabled: false,
            ..GridConfig::default()
        };
        let raw = Point { x: 1.2, y: 3.4 };
        assert_eq!(off.snap(raw), raw);
        let stepped = off.snap_to_step(raw);
        assert_ne!(stepped, raw);
        let on = GridConfig {
            snap_enabled: true,
            ..off.clone()
        };
        assert_eq!(on.snap(raw), stepped);
        assert_eq!(on.snap_to_step(stepped), stepped);
    }

    #[test]
    fn undo_kind_names_the_position_edit_then_the_redo() {
        let mut document = Document::demo(1, 1);
        let set_id = document.sets[0].id;
        let performer_id = document.performers[0].id;
        let mut history = History::with_limit(4);
        history
            .execute(
                &mut document,
                Edit::MovePerformers {
                    set_id,
                    performer_ids: vec![performer_id],
                    positions: vec![Point { x: 4.0, y: 6.0 }],
                },
            )
            .unwrap();
        assert_eq!(history.undo_kind(), Some(EditKind::Positions));
        assert_eq!(history.redo_kind(), None);
        assert!(history.undo(&mut document));
        assert_eq!(history.undo_kind(), None);
        assert_eq!(history.redo_kind(), Some(EditKind::Positions));
    }
}
