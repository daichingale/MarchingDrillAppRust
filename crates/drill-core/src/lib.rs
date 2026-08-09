//! UI-independent marching drill document model.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub mod audio;
pub mod camera;
pub mod continuity;
pub mod coordinates;
pub mod countsheet;
pub mod editing;
pub mod pathing;
pub mod playback;
pub mod shapes;
pub mod svg;
pub mod tempo;
pub mod video;

pub type PerformerId = u32;

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
        }
    }
}

impl GridConfig {
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

    pub fn snap(&self, point: Point) -> Point {
        if !self.snap_enabled {
            return point;
        }
        let dx = self.horizontal_units / self.horizontal_steps.max(1) as f32;
        let dy = self.vertical_units / self.vertical_steps.max(1) as f32;
        Point {
            x: (point.x / dx).round() * dx,
            y: (point.y / dy).round() * dy,
        }
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
    let Some(from) = document.sets.get(set_index) else {
        return TransitionAnalysis::default();
    };
    let Some(to) = document.sets.get(set_index + 1) else {
        return TransitionAnalysis::default();
    };
    let mut collisions = 0;
    for i in 0..to.positions.len() {
        for j in (i + 1)..to.positions.len() {
            let dx = to.positions[i].x - to.positions[j].x;
            let dy = to.positions[i].y - to.positions[j].y;
            if dx * dx + dy * dy < collision_distance * collision_distance {
                collisions += 1;
            }
        }
    }
    let counts = f32::from(from.counts.max(1));
    let excessive_strides = from
        .positions
        .iter()
        .zip(&to.positions)
        .filter(|(a, b)| {
            let dx = a.x - b.x;
            let dy = a.y - b.y;
            (dx * dx + dy * dy).sqrt() / counts > max_step_per_count
        })
        .count();
    TransitionAnalysis {
        collisions,
        excessive_strides,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Performer {
    pub id: PerformerId,
    pub label: String,
    pub color: [u8; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Set {
    pub name: String,
    pub counts: u16,
    /// Dense and index-aligned with `Document::performers` for cache-friendly playback.
    pub positions: Vec<Point>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub schema_version: u16,
    pub title: String,
    #[serde(default)]
    pub grid: GridConfig,
    #[serde(default)]
    pub tempo: tempo::TempoMap,
    #[serde(default)]
    pub audio: Option<audio::AudioTrack>,
    pub performers: Vec<Performer>,
    pub sets: Vec<Set>,
}

impl Document {
    pub fn demo(rows: usize, columns: usize) -> Self {
        let count = rows * columns;
        let performers = (0..count)
            .map(|i| Performer {
                id: i as PerformerId,
                label: format!("{}{}", (b'A' + (i / 10).min(25) as u8) as char, i % 10 + 1),
                color: [245, 197, 66],
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
            schema_version: 1,
            title: "新しいドリル".into(),
            grid: GridConfig::default(),
            tempo: tempo::TempoMap::constant(120.0),
            audio: None,
            performers,
            sets: vec![
                Set {
                    name: "セット 1".into(),
                    counts: 16,
                    positions: block,
                },
                Set {
                    name: "セット 2".into(),
                    counts: 16,
                    positions: arc,
                },
            ],
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "未対応のファイルバージョンです: {}",
                self.schema_version
            ));
        }
        if self.sets.is_empty() {
            return Err("セットがありません".into());
        }
        if self.grid.width <= 0.0 || self.grid.height <= 0.0 {
            return Err("グリッド寸法は正数である必要があります".into());
        }
        let expected = self.performers.len();
        if let Some((i, _)) = self
            .sets
            .iter()
            .enumerate()
            .find(|(_, s)| s.positions.len() != expected)
        {
            return Err(format!("セット {} の演者数が一致しません", i + 1));
        }
        let unique = self
            .performers
            .iter()
            .map(|p| p.id)
            .collect::<BTreeSet<_>>();
        if unique.len() != expected {
            return Err("演者IDが重複しています".into());
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
            .map(|set| u32::from(set.counts))
            .sum()
    }

    pub fn global_count(&self, set_index: usize, local_count: f32) -> f32 {
        let prior = self
            .sets
            .iter()
            .take(set_index)
            .map(|set| u32::from(set.counts))
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
            let counts = f32::from(set.counts);
            if remaining < counts {
                return (index, remaining);
            }
            remaining -= counts;
        }
        (self.sets.len().saturating_sub(1), 0.0)
    }

    pub fn positions_at(&self, set_index: usize, progress: f32, out: &mut Vec<Point>) {
        let from = &self.sets[set_index.min(self.sets.len() - 1)].positions;
        let to = self.sets.get(set_index + 1).map_or(from, |s| &s.positions);
        out.clear();
        out.reserve(from.len().saturating_sub(out.capacity()));
        out.extend(
            from.iter()
                .zip(to)
                .map(|(&a, &b)| a.lerp(b, progress.clamp(0.0, 1.0))),
        );
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
    pub fn from_json(json: &str) -> Result<Self, String> {
        let doc: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        doc.validate()?;
        Ok(doc)
    }
}

#[derive(Clone, Debug)]
pub struct MoveCommand {
    pub set_index: usize,
    pub performer_indices: Vec<usize>,
    pub before: Vec<Point>,
    pub after: Vec<Point>,
}

#[derive(Debug, Default)]
pub struct History {
    commands: Vec<MoveCommand>,
    cursor: usize,
    limit: usize,
}

impl History {
    pub fn with_limit(limit: usize) -> Self {
        Self {
            commands: Vec::new(),
            cursor: 0,
            limit,
        }
    }

    pub fn push(&mut self, command: MoveCommand) {
        self.commands.truncate(self.cursor);
        self.commands.push(command);
        if self.commands.len() > self.limit {
            self.commands.remove(0);
        }
        self.cursor = self.commands.len();
    }

    pub fn undo(&mut self, document: &mut Document) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        self.commands[self.cursor].apply(document, false);
        true
    }

    pub fn redo(&mut self, document: &mut Document) -> bool {
        if self.cursor == self.commands.len() {
            return false;
        }
        self.commands[self.cursor].apply(document, true);
        self.cursor += 1;
        true
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }
    pub fn can_redo(&self) -> bool {
        self.cursor < self.commands.len()
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
    fn malformed_and_future_documents_are_rejected() {
        assert!(Document::from_json("{not json").is_err());
        let mut document = Document::demo(2, 2);
        document.schema_version = u16::MAX;
        let error = Document::from_json(&document.to_json().unwrap()).unwrap_err();
        assert!(error.contains("未対応"));
    }

    #[test]
    fn mismatched_set_size_is_rejected() {
        let mut document = Document::demo(2, 2);
        document.sets[1].positions.pop();
        let error = Document::from_json(&document.to_json().unwrap()).unwrap_err();
        assert!(error.contains("演者数"));
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
    fn transition_analysis_finds_collisions_and_long_strides() {
        let mut doc = Document::demo(1, 2);
        doc.sets[1].positions[0] = Point { x: 90.0, y: 40.0 };
        doc.sets[1].positions[1] = Point { x: 90.1, y: 40.0 };
        let result = analyze_transition(&doc, 0, 0.5, 1.0);
        assert_eq!(result.collisions, 1);
        assert!(result.excessive_strides > 0);
    }
}
