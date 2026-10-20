//! A Blender project bundle: `drill_scene.py` plus `drill_scene.json`.
//!
//! The JSON is the drill (people, colors, counts, tempo). The script builds
//! the field, figures, cameras, and lights, and can render headless:
//! `blender -b -P drill_scene.py -- --render out.mp4`.

use drill_core::tempo::TempoMap;
use drill_core::transition::{Easing, RouteShape};
use drill_core::{Document, PerformerKind, Set};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const SCENE_SCRIPT: &str = include_str!("blender_scene.py");
pub const MARCHER_SCRIPT: &str = include_str!("drill_marcher.py");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlenderCamera {
    PressBox,
    EndZone,
    Orbit,
    Follow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlenderEngine {
    Eevee,
    Cycles,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeOfDay {
    Day,
    Night,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FigureStyle {
    Capsule,
    Person,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderQuality {
    Draft,
    Fine,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlenderOptions {
    pub fps: u16,
    pub width: u32,
    pub height: u32,
    pub engine: BlenderEngine,
    pub quality: RenderQuality,
    pub time_of_day: TimeOfDay,
    pub camera: BlenderCamera,
    pub figures: FigureStyle,
    pub audio: bool,
    /// Burn the show name, set, and count into the picture.
    pub title_card: bool,
    /// Jump the render camera between presets at each set.
    pub cuts: bool,
    /// `None` is the whole show. `Some` is one set's move, in set index.
    pub set_index: Option<usize>,
}

impl Default for BlenderOptions {
    fn default() -> Self {
        Self {
            fps: 30,
            engine: BlenderEngine::Eevee,
            quality: RenderQuality::Draft,
            time_of_day: TimeOfDay::Day,
            camera: BlenderCamera::PressBox,
            figures: FigureStyle::Person,
            audio: true,
            title_card: true,
            cuts: false,
            set_index: None,
            width: 1280,
            height: 720,
        }
    }
}

impl BlenderOptions {
    pub fn samples(&self) -> u16 {
        match (self.engine, self.quality) {
            (BlenderEngine::Cycles, RenderQuality::Fine) => 64,
            (BlenderEngine::Cycles, RenderQuality::Draft) => 8,
            (BlenderEngine::Eevee, RenderQuality::Fine) => 32,
            (BlenderEngine::Eevee, RenderQuality::Draft) => 8,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlenderScene {
    pub schema: u16,
    pub title: String,
    pub fps: u16,
    pub width: u32,
    pub height: u32,
    pub engine: BlenderEngine,
    pub samples: u16,
    pub time_of_day: TimeOfDay,
    pub camera: BlenderCamera,
    pub figures: FigureStyle,
    pub quality: RenderQuality,
    pub title_card: bool,
    pub cuts: bool,
    pub cues: Vec<BlenderCue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_section: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_path: Option<String>,
    pub audio_offset_seconds: f32,
    pub field: BlenderField,
    pub performers: Vec<BlenderPerformer>,
    pub keyframes: Vec<BlenderKeyframe>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlenderField {
    pub width: f32,
    pub height: f32,
    pub unit: String,
    /// `football`, `floor`, `indoor`, or `pitch`.
    pub kind: String,
    pub major_x: Vec<f32>,
    pub major_y: Vec<f32>,
    pub hashes: Vec<f32>,
    /// Each item is `(min_x, min_y, max_x, max_y)` in field meters.
    pub frames: Vec<[f32; 4]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlenderPerformer {
    pub index: usize,
    pub label: String,
    pub section: String,
    pub color: [u8; 3],
    pub kind: String,
    pub prop: String,
    pub height_m: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlenderKeyframe {
    pub performer: usize,
    pub frame: u32,
    pub x: f32,
    pub y: f32,
    /// Yaw in radians. `0` faces the audience.
    pub facing: f32,
    /// `0` or `1`, alternating marching step. The plant is on the count.
    pub step: u8,
    pub holding: bool,
    /// `forward`, `back`, `mark`, `attention`, or `horn`.
    pub gait: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlenderCue {
    pub frame: u32,
    /// 1-based set number.
    pub set: u16,
    pub count: u16,
}

/// Last sampled frame, or 1 when the show has no dots.
pub fn last_frame(scene: &BlenderScene) -> u32 {
    scene
        .keyframes
        .iter()
        .map(|key| key.frame)
        .max()
        .unwrap_or(1)
}

/// Blender frame at a global count. Frame 1 is count 0.
pub fn blender_frame(tempo: &TempoMap, global_count: f64, fps: u16) -> u32 {
    let seconds = tempo.seconds_at_f64(global_count);
    let frames = (seconds * f64::from(fps.max(1))).round();
    1 + frames.max(0.0) as u32
}

/// Counts sampled for the bundle, one per count across the show or one set.
pub fn sample_counts(document: &Document, set_index: Option<usize>) -> Vec<f32> {
    let last = document.sets.len().saturating_sub(1);
    let (start, end) = match set_index {
        Some(index) => {
            let start = document.global_count(index, 0.0);
            let outgoing = index < last;
            let length = if outgoing {
                document
                    .sets
                    .get(index)
                    .map_or(0.0, |set| f32::from(set.counts) + f32::from(set.hold))
            } else {
                0.0
            };
            (start, start + length)
        }
        None => (0.0, document.timeline_counts() as f32),
    };
    let mut counts = Vec::new();
    let mut count = start;
    while count < end - 0.001 {
        counts.push(count);
        count += 1.0;
    }
    counts.push(end);
    counts
}

pub fn build_scene(document: &Document, options: &BlenderOptions) -> BlenderScene {
    let follow = document
        .sections
        .first()
        .map(|section| section.short.clone())
        .filter(|short| !short.is_empty());
    let audio_path = if options.audio {
        document
            .audio
            .as_ref()
            .map(|track| track.path.clone())
            .filter(|path| !path.is_empty())
    } else {
        None
    };
    let audio_offset_seconds = document
        .audio
        .as_ref()
        .map(|track| track.offset_seconds)
        .unwrap_or(0.0);
    let performers = document
        .performers
        .iter()
        .enumerate()
        .map(|(index, performer)| BlenderPerformer {
            index,
            label: performer.label.clone(),
            section: document
                .sections
                .iter()
                .find(|section| section.id == performer.section)
                .map(|section| {
                    if section.short.is_empty() {
                        section.name.clone()
                    } else {
                        section.short.clone()
                    }
                })
                .unwrap_or_default(),
            color: performer.resolved_color(&document.sections),
            kind: match performer.kind {
                PerformerKind::Wind => "wind",
                PerformerKind::Percussion => "percussion",
                PerformerKind::Guard => "guard",
                PerformerKind::Prop => "prop",
            }
            .to_owned(),
            prop: prop_for(
                document
                    .sections
                    .iter()
                    .find(|section| section.id == performer.section)
                    .map(|section| section.name.as_str())
                    .unwrap_or(""),
                document
                    .sections
                    .iter()
                    .find(|section| section.id == performer.section)
                    .map(|section| section.short.as_str())
                    .unwrap_or(""),
                performer.kind,
            )
            .to_owned(),
            height_m: performer.height_m,
        })
        .collect();
    let mut keyframes = Vec::new();
    let mut cues = Vec::new();
    let mut positions = Vec::new();
    let mut previous = vec![None; document.performers.len()];
    for count in motion_counts(document, options.set_index) {
        let (set_index, local) = document.locate_count(count);
        let move_counts = document
            .sets
            .get(set_index)
            .map(|set| f32::from(set.counts))
            .unwrap_or(0.0);
        let holding = local + 0.001 >= move_counts;
        document.positions_at_count(set_index, local, &mut positions);
        let frame = blender_frame(&document.tempo, f64::from(count), options.fps);
        if (count - count.round()).abs() < 0.001
            && cues
                .last()
                .is_none_or(|cue: &BlenderCue| cue.frame != frame)
        {
            cues.push(BlenderCue {
                frame,
                set: (set_index + 1) as u16,
                count: local.round().max(0.0) as u16,
            });
        }
        for (index, point) in positions.iter().enumerate() {
            if !point.x.is_finite() || !point.y.is_finite() {
                continue;
            }
            let (dx, dy) = match previous.get(index).and_then(|prior| *prior) {
                Some((px, py)) => (point.x - px, point.y - py),
                None => (0.0, 0.0),
            };
            let gait = march_gait(dx, dy, holding);
            let facing = if gait == "forward" {
                travel_yaw(dx, dy)
            } else {
                0.0
            };
            if let Some(slot) = previous.get_mut(index) {
                *slot = Some((point.x, point.y));
            }
            keyframes.push(BlenderKeyframe {
                performer: index,
                frame,
                x: point.x,
                y: point.y,
                facing,
                step: (local.floor() as u32 % 2) as u8,
                holding,
                gait: gait.to_owned(),
            });
        }
    }
    BlenderScene {
        schema: 1,
        title: document.title.clone(),
        fps: options.fps,
        width: options.width,
        height: options.height,
        engine: options.engine,
        samples: options.samples(),
        time_of_day: options.time_of_day,
        camera: options.camera,
        figures: options.figures,
        quality: options.quality,
        title_card: options.title_card,
        cuts: options.cuts,
        cues,
        follow_section: follow,
        audio_path,
        audio_offset_seconds,
        field: BlenderField {
            width: document.grid.width,
            height: document.grid.height,
            unit: match document.grid.unit {
                drill_core::Unit::Meters => "m",
                drill_core::Unit::Yards => "yd",
            }
            .to_owned(),
            kind: field_kind(&document.grid).to_owned(),
            major_x: document.grid.horizontal_major_positions(),
            major_y: document.grid.vertical_major_positions(),
            hashes: document
                .grid
                .hashes
                .iter()
                .map(|line| line.position)
                .collect(),
            frames: document
                .grid
                .reference_frame_bounds()
                .into_iter()
                .map(|(min_x, min_y, max_x, max_y)| [min_x, min_y, max_x, max_y])
                .collect(),
        },
        performers,
        keyframes,
    }
}

fn motion_counts(document: &Document, set_index: Option<usize>) -> Vec<f32> {
    let base = sample_counts(document, set_index);
    let mut counts = Vec::new();
    for pair in base.windows(2) {
        counts.push(pair[0]);
        let (index, _) = document.locate_count(pair[0]);
        if document
            .sets
            .get(index)
            .is_some_and(set_needs_dense_samples)
        {
            counts.push((pair[0] + pair[1]) * 0.5);
        }
    }
    if let Some(last) = base.last() {
        counts.push(*last);
    }
    if counts.is_empty() {
        counts.push(0.0);
    }
    counts
}

fn set_needs_dense_samples(set: &Set) -> bool {
    std::iter::once(&set.routes.default)
        .chain(set.routes.overrides.values())
        .any(|route| route.easing != Easing::Linear || !matches!(route.shape, RouteShape::Straight))
}

/// `forward` faces travel. `back` walks upfield while still facing the audience.
/// A hold brings the horn up. Standing still is attention.
fn march_gait(dx: f32, dy: f32, holding: bool) -> &'static str {
    if holding {
        "horn"
    } else if dx.hypot(dy) < 0.04 {
        "attention"
    } else if dy > 0.05 && dy.abs() >= dx.abs() {
        "back"
    } else {
        "forward"
    }
}

/// `0` faces the audience (−Y). Moving toward +X is a quarter turn.
fn travel_yaw(dx: f32, dy: f32) -> f32 {
    if dx.hypot(dy) < 0.04 {
        0.0
    } else {
        dx.atan2(-dy)
    }
}

fn field_kind(grid: &drill_core::GridConfig) -> &'static str {
    if !grid.reference_frames.is_empty() && grid.unit == drill_core::Unit::Meters {
        "floor"
    } else if grid.width >= 100.0 && grid.height >= 70.0 {
        "pitch"
    } else if grid.hashes.is_empty() && grid.unit == drill_core::Unit::Yards && grid.width < 100.0 {
        "indoor"
    } else if grid.unit == drill_core::Unit::Yards {
        "football"
    } else {
        "floor"
    }
}

fn prop_hit(name: &str, short: &str, japanese: &[&str], english: &[&str]) -> bool {
    let lower_name = name.to_ascii_lowercase();
    let lower_short = short.to_ascii_lowercase();
    japanese.iter().any(|needle| name.contains(needle))
        || english.iter().any(|needle| {
            lower_short == *needle
                || lower_name
                    .split(|ch: char| !ch.is_ascii_alphanumeric())
                    .any(|word| word == *needle)
        })
}

fn prop_for(name: &str, short: &str, kind: PerformerKind) -> &'static str {
    if prop_hit(name, short, &["ライフル"], &["rifle"]) {
        return "rifle";
    }
    if prop_hit(
        name,
        short,
        &["マレット", "マリンバ", "シロフォン", "木琴"],
        &["mallet"],
    ) {
        return "mallet";
    }
    if prop_hit(name, short, &["スネア"], &["snare"]) {
        return "snare";
    }
    if prop_hit(name, short, &["テナー"], &["tenor"]) {
        return "tenor";
    }
    if prop_hit(name, short, &["大太鼓", "バスドラム"], &["bass"]) {
        return "bass";
    }
    if prop_hit(name, short, &["フルート"], &["flute", "fl"]) {
        return "flute";
    }
    if prop_hit(name, short, &["クラリ"], &["clarinet", "cl"]) {
        return "clarinet";
    }
    if prop_hit(name, short, &["サックス"], &["sax", "sx"]) {
        return "sax";
    }
    if prop_hit(
        name,
        short,
        &["チューバ", "スーザ"],
        &["tuba", "tu", "sousaphone"],
    ) {
        return "tuba";
    }
    if prop_hit(
        name,
        short,
        &["ホルン", "メロフォン"],
        &["horn", "hr", "mellophone"],
    ) {
        return "mellophone";
    }
    if prop_hit(name, short, &["トロンボーン"], &["trombone", "tb"]) {
        return "trombone";
    }
    if prop_hit(
        name,
        short,
        &["ユーフォ", "バリトン"],
        &["euphonium", "baritone", "eu"],
    ) {
        return "baritone";
    }
    if prop_hit(name, short, &["トランペット"], &["trumpet", "tp"]) {
        return "trumpet";
    }
    match kind {
        PerformerKind::Guard => "flag",
        PerformerKind::Percussion => "drum",
        PerformerKind::Prop => "none",
        PerformerKind::Wind => {
            if prop_hit(name, short, &["木管", "オーボエ", "ファゴット"], &["oboe"]) {
                "woodwind"
            } else {
                "brass"
            }
        }
    }
}

pub fn scene_json(scene: &BlenderScene) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(scene)
}

pub fn parse_scene(json: &str) -> Result<BlenderScene, serde_json::Error> {
    serde_json::from_str(json)
}

/// Writes `drill_scene.py`, `drill_marcher.py`, and `drill_scene.json` into `directory`.
pub fn write_bundle(directory: &Path, scene: &BlenderScene) -> std::io::Result<(PathBuf, PathBuf)> {
    fs::create_dir_all(directory)?;
    let script = directory.join("drill_scene.py");
    let data = directory.join("drill_scene.json");
    fs::write(&script, SCENE_SCRIPT)?;
    fs::write(directory.join("drill_marcher.py"), MARCHER_SCRIPT)?;
    fs::write(&data, scene_json(scene).map_err(std::io::Error::other)?)?;
    Ok((script, data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use drill_core::{Document, Point};

    fn two_step_show() -> Document {
        let mut document = Document::blank(2);
        document.title = "test".into();
        document.sets[0].counts = 8;
        document.sets[0].positions = vec![Point { x: 1.0, y: 2.0 }, Point { x: 3.0, y: 4.0 }];
        let mut next = document.sets[0].clone();
        next.positions = vec![Point { x: 5.0, y: 2.0 }, Point { x: 7.0, y: 6.0 }];
        document.sets.push(next);
        document
    }

    #[test]
    fn frames_follow_tempo() {
        let tempo = TempoMap::default();
        assert_eq!(blender_frame(&tempo, 0.0, 30), 1);
        // 120 bpm is two counts a second, so count 2 is one second, frame 31.
        assert_eq!(blender_frame(&tempo, 2.0, 30), 31);
        assert_eq!(blender_frame(&tempo, 2.0, 60), 61);
        assert_eq!(blender_frame(&tempo, 4.0, 24), 1 + 48);
    }

    #[test]
    fn scene_json_round_trips_and_keyframes_span_the_move() {
        let scene = build_scene(&two_step_show(), &BlenderOptions::default());
        let parsed = parse_scene(&scene_json(&scene).unwrap()).unwrap();
        assert_eq!(parsed, scene);
        assert_eq!(scene.schema, 1);
        assert_eq!(scene.performers.len(), 2);
        assert!(scene.field.width > 1.0);
        let frames: Vec<u32> = scene.keyframes.iter().map(|key| key.frame).collect();
        assert_eq!(*frames.iter().min().unwrap(), 1);
        // 8 counts at 120 bpm is 4 seconds, 30 fps, frame 1 + 120.
        assert_eq!(*frames.iter().max().unwrap(), 121);
        let start = scene
            .keyframes
            .iter()
            .find(|key| key.performer == 0 && key.frame == 1)
            .unwrap();
        assert!((start.x - 1.0).abs() < 1e-3);
        let end = scene
            .keyframes
            .iter()
            .filter(|key| key.performer == 0)
            .max_by_key(|key| key.frame)
            .unwrap();
        assert!((end.x - 5.0).abs() < 1e-3);
        let first = build_scene(
            &two_step_show(),
            &BlenderOptions {
                set_index: Some(0),
                ..BlenderOptions::default()
            },
        );
        assert_eq!(first.keyframes.iter().map(|key| key.frame).max(), Some(121));
        let last = build_scene(
            &two_step_show(),
            &BlenderOptions {
                set_index: Some(1),
                ..BlenderOptions::default()
            },
        );
        assert_eq!(last.keyframes.len(), 2);
        assert!(last.keyframes.iter().all(|key| key.frame == 121));
        assert_eq!(scene.field.kind, "floor");
        assert!(scene.performers.iter().all(|person| person.prop == "brass"));
        assert!(scene.keyframes.iter().any(|key| key.facing > 1.0));
        assert!(
            scene
                .keyframes
                .iter()
                .any(|key| key.gait == "forward" && key.facing > 1.0)
        );
        assert!(
            scene
                .keyframes
                .iter()
                .any(|key| key.frame == 1 && key.gait == "attention")
        );
        assert!(!scene.cues.is_empty());
    }

    #[test]
    fn bundle_script_is_the_headless_scene_builder() {
        let directory =
            std::env::temp_dir().join(format!("drill-blender-bundle-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        let scene = build_scene(&two_step_show(), &BlenderOptions::default());
        let (script, data) = write_bundle(&directory, &scene).unwrap();
        let text = fs::read_to_string(&script).unwrap();
        assert_eq!(text, SCENE_SCRIPT);
        assert!(text.contains("blender -b -P drill_scene.py -- --render out.mp4"));
        assert!(text.contains("press_box"));
        assert!(text.contains("end_zone"));
        assert!(text.contains("orbit"));
        assert!(text.contains("follow"));
        assert!(text.contains("BLENDER_EEVEE"));
        assert!(text.contains("H264"));
        assert!(text.contains("MPEG4"));
        assert!(text.contains("use_gtao"));
        assert!(text.contains("shako"));
        assert!(text.contains("drill_marcher"));
        assert!(text.contains("heroes-only"));
        let marcher = fs::read_to_string(directory.join("drill_marcher.py")).unwrap();
        assert_eq!(marcher, MARCHER_SCRIPT);
        assert!(marcher.contains("MarcherRig"));
        assert!(marcher.contains("march.forward"));
        assert!(marcher.contains("march.back"));
        assert!(marcher.contains("march.mark"));
        assert!(marcher.contains("march.attention"));
        assert!(marcher.contains("march.horn"));
        let loaded = parse_scene(&fs::read_to_string(&data).unwrap()).unwrap();
        assert_eq!(loaded, scene);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn hold_counts_face_the_audience() {
        let mut document = two_step_show();
        document.sets[0].hold = 4;
        let scene = build_scene(&document, &BlenderOptions::default());
        assert!(
            scene
                .keyframes
                .iter()
                .any(|key| key.holding && key.facing == 0.0 && key.gait == "horn")
        );
        assert!(
            scene
                .keyframes
                .iter()
                .any(|key| !key.holding && key.facing > 1.0)
        );
    }

    #[test]
    fn field_kind_and_props_follow_the_show() {
        let mut document = Document::blank(1);
        document.grid = drill_core::GridConfig::default();
        let football = build_scene(&document, &BlenderOptions::default());
        assert_eq!(football.field.kind, "football");
        assert!(football.field.hashes.len() >= 2);
        document.grid = drill_core::GridConfig::indoor();
        assert_eq!(
            build_scene(&document, &BlenderOptions::default())
                .field
                .kind,
            "indoor"
        );
        document.grid = drill_core::GridConfig::soccer();
        assert_eq!(
            build_scene(&document, &BlenderOptions::default())
                .field
                .kind,
            "pitch"
        );
        document.grid = drill_core::GridConfig::japan_floor();
        document.sections[0].name = "カラーガード".into();
        document.performers[0].kind = PerformerKind::Guard;
        assert_eq!(
            build_scene(&document, &BlenderOptions::default()).performers[0].prop,
            "flag"
        );
        document.sections[0].name = "ライフル".into();
        assert_eq!(
            build_scene(&document, &BlenderOptions::default()).performers[0].prop,
            "rifle"
        );
        document.sections[0].name = "ドラム".into();
        document.performers[0].kind = PerformerKind::Percussion;
        assert_eq!(
            build_scene(&document, &BlenderOptions::default()).performers[0].prop,
            "drum"
        );
        document.sections[0].name = "フルート".into();
        document.performers[0].kind = PerformerKind::Wind;
        assert_eq!(
            build_scene(&document, &BlenderOptions::default()).performers[0].prop,
            "flute"
        );
        document.sections[0].name = "トランペット".into();
        assert_eq!(
            build_scene(&document, &BlenderOptions::default()).performers[0].prop,
            "trumpet"
        );
        document.sections[0].name = "ホルン".into();
        assert_eq!(
            build_scene(&document, &BlenderOptions::default()).performers[0].prop,
            "mellophone"
        );
        document.sections[0].name = "トロンボーン".into();
        assert_eq!(
            build_scene(&document, &BlenderOptions::default()).performers[0].prop,
            "trombone"
        );
        document.sections[0].name = "ユーフォニアム".into();
        assert_eq!(
            build_scene(&document, &BlenderOptions::default()).performers[0].prop,
            "baritone"
        );
        document.sections[0].name = "チューバ".into();
        assert_eq!(
            build_scene(&document, &BlenderOptions::default()).performers[0].prop,
            "tuba"
        );
        document.sections[0].name = "サックス".into();
        assert_eq!(
            build_scene(&document, &BlenderOptions::default()).performers[0].prop,
            "sax"
        );
        document.sections[0].name = "クラリネット".into();
        assert_eq!(
            build_scene(&document, &BlenderOptions::default()).performers[0].prop,
            "clarinet"
        );
        document.sections[0].name = "スネア".into();
        document.performers[0].kind = PerformerKind::Percussion;
        assert_eq!(
            build_scene(&document, &BlenderOptions::default()).performers[0].prop,
            "snare"
        );
        document.sections[0].name = "マレット".into();
        assert_eq!(
            build_scene(&document, &BlenderOptions::default()).performers[0].prop,
            "mallet"
        );
    }

    #[test]
    fn backward_march_faces_the_audience() {
        let mut document = two_step_show();
        document.sets[0].positions = vec![Point { x: 2.0, y: 2.0 }, Point { x: 4.0, y: 2.0 }];
        document.sets[1].positions = vec![Point { x: 2.0, y: 8.0 }, Point { x: 4.0, y: 8.0 }];
        let scene = build_scene(&document, &BlenderOptions::default());
        assert!(
            scene
                .keyframes
                .iter()
                .any(|key| key.gait == "back" && key.facing == 0.0)
        );
        assert!(scene.keyframes.iter().all(|key| key.facing == 0.0));
    }
}
