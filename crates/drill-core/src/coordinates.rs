//! Human-readable marching coordinate readout and drill-sheet / CSV export.
//! Pure logic, no UI. Japanese user-facing strings.
//!
//! Coordinate convention (standard marching-band "dot"):
//! * Side-to-side: steps inside/outside the nearest yard line, on サイド1 (toward
//!   x=0) or サイド2 (toward x=grid.width). Yard-line numbers run 0 at each
//!   endzone up to `width/2` (the "50") at field center, i.e.
//!   `number = min(pos, width - pos)`.
//! * Front-to-back: steps in front of / behind the nearest reference line — a
//!   hash line (`grid.hashes`) or a sideline (front y=0, back y=grid.height).
//!   y increases away from the audience, so larger y is "behind" (後ろ).

use crate::{Document, GridConfig, Locale, Point};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepRounding {
    Eighth,
    #[default]
    Quarter,
    Half,
    Whole,
}

impl StepRounding {
    const fn denominator(self) -> f32 {
        match self {
            Self::Eighth => 8.0,
            Self::Quarter => 4.0,
            Self::Half => 2.0,
            Self::Whole => 1.0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum FrontBackReference {
    #[default]
    NearestLine,
    NearestHash,
    FixedLabel(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OnLineStyle {
    #[default]
    Explicit,
    Short,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum YardLineInterval {
    #[default]
    Grid,
    Five,
    Ten,
    Custom(f32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepNotationStyle {
    #[default]
    Steps,
    EightToFive,
    SixToFive,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProductionTemplatePreset {
    #[default]
    Standard,
    Compact,
    Rehearsal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CoordinateNotation {
    pub front_back: FrontBackReference,
    pub on_line: OnLineStyle,
    pub rounding: StepRounding,
    pub yard_lines: YardLineInterval,
    pub step_style: StepNotationStyle,
    pub production_template: ProductionTemplatePreset,
}

impl Default for CoordinateNotation {
    fn default() -> Self {
        Self {
            front_back: FrontBackReference::NearestLine,
            on_line: OnLineStyle::Explicit,
            rounding: StepRounding::Quarter,
            yard_lines: YardLineInterval::Grid,
            step_style: StepNotationStyle::Steps,
            production_template: ProductionTemplatePreset::Standard,
        }
    }
}

impl CoordinateNotation {
    pub fn validate(&self) -> bool {
        !matches!(self.yard_lines, YardLineInterval::Custom(v) if !v.is_finite() || !(1.0..=50.0).contains(&v))
            && !matches!(&self.front_back, FrontBackReference::FixedLabel(v) if v.trim().is_empty() || v.len() > crate::MAX_TEXT_BYTES)
    }
    pub fn dci() -> Self {
        Self {
            front_back: FrontBackReference::FixedLabel("フロントハッシュ".into()),
            step_style: StepNotationStyle::EightToFive,
            ..Self::default()
        }
    }
    pub fn indoor() -> Self {
        Self {
            front_back: FrontBackReference::NearestHash,
            on_line: OnLineStyle::Short,
            rounding: StepRounding::Half,
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FieldSide {
    One,
    Two,
    Center,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LateralRelation {
    On,
    Inside,
    Outside,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SideCoordinate {
    pub side: FieldSide,
    pub yard_line: f32,
    pub relation: LateralRelation,
    pub steps: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VerticalRelation {
    On,
    Front,
    Behind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrontCoordinate {
    pub reference: String,
    pub relation: VerticalRelation,
    pub steps: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CoordinateReadout {
    pub side: SideCoordinate,
    pub front: FrontCoordinate,
}

/// Round a step value to the nearest quarter step (standard drill precision).
fn round_steps(v: f32, rounding: StepRounding) -> f32 {
    let denominator = rounding.denominator();
    (v * denominator).round() / denominator
}

/// Format a step/yard value, dropping a redundant fractional part
/// (`2.0 -> "2"`, `1.5 -> "1.5"`, `2.25 -> "2.25"`).
fn fmt_num(v: f32) -> String {
    if (v - v.round()).abs() < 1e-4 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn fmt_steps(v: f32, style: StepNotationStyle) -> String {
    match style {
        StepNotationStyle::Steps => fmt_num(v),
        StepNotationStyle::EightToFive => format!("{} (8-to-5)", fmt_num(v)),
        StepNotationStyle::SixToFive => format!("{} (6-to-5)", fmt_num(v)),
    }
}

/// Quote and escape a CSV field only when it contains a comma, quote or newline.
fn csv_escape(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Side-to-side readout: steps inside/outside the nearest yard line, with side.
///
/// A horizontal step is `horizontal_units / horizontal_steps` field-units
/// (e.g. 8-to-5 => 5/8 yard). Reported to 0.25-step precision.
pub fn side_coordinate(point: Point, grid: &GridConfig) -> SideCoordinate {
    let center = grid.width / 2.0;
    let interval = match grid.coordinate_notation.yard_lines {
        YardLineInterval::Grid => grid.major_line_interval,
        YardLineInterval::Five => 5.0,
        YardLineInterval::Ten => 10.0,
        YardLineInterval::Custom(v) => v,
    };
    let interval = if interval.is_finite() && interval > 0.0 {
        interval
    } else {
        5.0
    };
    let step = grid.horizontal_units / grid.horizontal_steps.max(1) as f32;

    // Nearest yard line (snapped to the major interval) and its number.
    let yard_line = ((point.x / interval).round() * interval).clamp(0.0, grid.width);
    let yard = yard_line.min(grid.width - yard_line);

    // Distance from center shrinks as we move "inside" toward the 50.
    let d_point = (point.x - center).abs();
    let d_line = (yard_line - center).abs();
    let steps = round_steps((d_point - d_line) / step, grid.coordinate_notation.rounding);

    let side = if (yard_line - center).abs() < 1e-4 {
        FieldSide::Center
    } else if point.x < center {
        FieldSide::One
    } else {
        FieldSide::Two
    };
    let on_fifty = (yard_line - center).abs() < 1e-4;

    SideCoordinate {
        side: if on_fifty { FieldSide::Center } else { side },
        yard_line: yard,
        relation: if steps.abs() < 1e-4 {
            LateralRelation::On
        } else if steps > 0.0 {
            LateralRelation::Outside
        } else {
            LateralRelation::Inside
        },
        steps: steps.abs(),
    }
}

pub fn format_side(value: &SideCoordinate, locale: Locale) -> String {
    format_side_with_notation(value, &CoordinateNotation::default(), locale)
}

pub fn format_side_with_notation(
    value: &SideCoordinate,
    notation: &CoordinateNotation,
    locale: Locale,
) -> String {
    let side = match (value.side, locale) {
        (FieldSide::One, Locale::Ja) => "サイド1",
        (FieldSide::Two, Locale::Ja) => "サイド2",
        (FieldSide::One, Locale::En) => "Side 1",
        (FieldSide::Two, Locale::En) => "Side 2",
        _ => "",
    };
    match (value.relation, locale) {
        (LateralRelation::On, Locale::Ja) if notation.on_line == OnLineStyle::Short => {
            format!("{side} {}ちょうど", fmt_num(value.yard_line))
                .trim()
                .to_owned()
        }
        (LateralRelation::On, Locale::En) if notation.on_line == OnLineStyle::Short => {
            format!("On {side} {}", fmt_num(value.yard_line)).replace("  ", " ")
        }
        (LateralRelation::On, Locale::Ja) if value.side == FieldSide::Center => {
            format!("{}ヤードラインちょうど", fmt_num(value.yard_line))
        }
        (LateralRelation::On, Locale::Ja) => {
            format!("{side} {}ヤードラインちょうど", fmt_num(value.yard_line))
        }
        (LateralRelation::On, Locale::En) if value.side == FieldSide::Center => {
            format!("On the {} yard line", fmt_num(value.yard_line))
        }
        (LateralRelation::On, Locale::En) => {
            format!("{side}: on the {} yard line", fmt_num(value.yard_line))
        }
        (relation, Locale::Ja) => format!(
            "{side} {}ヤードラインの{}に{}歩",
            fmt_num(value.yard_line),
            if relation == LateralRelation::Outside {
                "外側"
            } else {
                "内側"
            },
            fmt_steps(value.steps, notation.step_style)
        ),
        (relation, Locale::En) => format!(
            "{side}: {} steps {} the {} yard line",
            fmt_steps(value.steps, notation.step_style),
            if relation == LateralRelation::Outside {
                "outside"
            } else {
                "inside"
            },
            fmt_num(value.yard_line)
        ),
    }
}

pub fn side_to_side_localized(point: Point, grid: &GridConfig, locale: Locale) -> String {
    format_side_with_notation(
        &side_coordinate(point, grid),
        &grid.coordinate_notation,
        locale,
    )
}
pub fn side_to_side(point: Point, grid: &GridConfig) -> String {
    side_to_side_localized(point, grid, Locale::Ja)
}

/// Front-to-back readout: steps in front of / behind the nearest reference line.
///
/// References are the front sideline (y=0), every hash in `grid.hashes`, and the
/// back sideline (y=grid.height). A vertical step is
/// `vertical_units / vertical_steps` field-units. Reported to 0.25-step precision.
pub fn front_coordinate(point: Point, grid: &GridConfig) -> FrontCoordinate {
    let step = grid.vertical_units / grid.vertical_steps.max(1) as f32;

    let mut refs: Vec<(&str, f32)> = Vec::with_capacity(grid.hashes.len() + 2);
    refs.push(("フロントサイドライン", 0.0));
    for h in &grid.hashes {
        refs.push((h.label.as_str(), h.position));
    }
    refs.push(("バックサイドライン", grid.height));

    match &grid.coordinate_notation.front_back {
        FrontBackReference::NearestLine => {}
        FrontBackReference::NearestHash => {
            if grid.hashes.is_empty() { /* sidelines remain as safe fallback */
            } else {
                refs.retain(|(_, position)| *position > 0.0 && *position < grid.height);
            }
        }
        FrontBackReference::FixedLabel(wanted) => {
            let wanted = wanted.to_lowercase();
            refs.retain(|(label, _)| {
                let label = label.to_lowercase();
                label == wanted
                    || (wanted.contains("front") && label.contains("フロント"))
                    || (wanted.contains("back") && label.contains("バック"))
            });
        }
    }

    // Nearest reference; ties resolve to the earlier entry (front sideline first).
    let (label, pos) = refs
        .into_iter()
        .min_by(|a, b| {
            (point.y - a.1)
                .abs()
                .partial_cmp(&(point.y - b.1).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(("フロントサイドライン", 0.0));

    let steps = round_steps(
        (point.y - pos).abs() / step,
        grid.coordinate_notation.rounding,
    );
    FrontCoordinate {
        reference: label.to_owned(),
        relation: if steps.abs() < 1e-4 {
            VerticalRelation::On
        } else if point.y > pos {
            VerticalRelation::Behind
        } else {
            VerticalRelation::Front
        },
        steps,
    }
}

fn localized_reference(label: &str, locale: Locale) -> &str {
    if locale == Locale::Ja {
        return label;
    }
    match label {
        "フロントサイドライン" => "front sideline",
        "バックサイドライン" => "back sideline",
        "フロントハッシュ" => "front hash",
        "バックハッシュ" => "back hash",
        _ => label,
    }
}
pub fn format_front(value: &FrontCoordinate, locale: Locale) -> String {
    format_front_with_notation(value, &CoordinateNotation::default(), locale)
}
pub fn format_front_with_notation(
    value: &FrontCoordinate,
    notation: &CoordinateNotation,
    locale: Locale,
) -> String {
    let label = localized_reference(&value.reference, locale);
    match (value.relation, locale) {
        (VerticalRelation::On, Locale::Ja) => {
            if notation.on_line == OnLineStyle::Short {
                label.to_owned()
            } else {
                format!("{label}ちょうど")
            }
        }
        (VerticalRelation::On, Locale::En) => {
            if notation.on_line == OnLineStyle::Short {
                label.to_owned()
            } else {
                format!("On the {label}")
            }
        }
        (VerticalRelation::Behind, Locale::Ja) => {
            format!(
                "{label}の{}歩後ろ",
                fmt_steps(value.steps, notation.step_style)
            )
        }
        (VerticalRelation::Front, Locale::Ja) => format!(
            "{label}の{}歩前",
            fmt_steps(value.steps, notation.step_style)
        ),
        (VerticalRelation::Behind, Locale::En) => {
            format!(
                "{} steps behind the {label}",
                fmt_steps(value.steps, notation.step_style)
            )
        }
        (VerticalRelation::Front, Locale::En) => {
            format!(
                "{} steps in front of the {label}",
                fmt_steps(value.steps, notation.step_style)
            )
        }
    }
}
pub fn front_to_back_localized(point: Point, grid: &GridConfig, locale: Locale) -> String {
    format_front_with_notation(
        &front_coordinate(point, grid),
        &grid.coordinate_notation,
        locale,
    )
}
pub fn front_to_back(point: Point, grid: &GridConfig) -> String {
    front_to_back_localized(point, grid, Locale::Ja)
}

/// Full readable coordinate: side-to-side and front-to-back, comma-separated.
pub fn readable(point: Point, grid: &GridConfig) -> String {
    readable_localized(point, grid, Locale::Ja)
}
pub fn coordinate_readout(point: Point, grid: &GridConfig) -> CoordinateReadout {
    CoordinateReadout {
        side: side_coordinate(point, grid),
        front: front_coordinate(point, grid),
    }
}
pub fn format_readout(value: &CoordinateReadout, locale: Locale) -> String {
    format_readout_with_notation(value, &CoordinateNotation::default(), locale)
}
pub fn format_readout_with_notation(
    value: &CoordinateReadout,
    notation: &CoordinateNotation,
    locale: Locale,
) -> String {
    let separator = if locale == Locale::Ja { "、" } else { "; " };
    format!(
        "{}{}{}",
        format_side_with_notation(&value.side, notation, locale),
        separator,
        format_front_with_notation(&value.front, notation, locale)
    )
}
pub fn readable_localized(point: Point, grid: &GridConfig, locale: Locale) -> String {
    format_readout_with_notation(
        &coordinate_readout(point, grid),
        &grid.coordinate_notation,
        locale,
    )
}

/// Plaintext per-performer "dot book": one line per set with the performer label,
/// set name, counts and the readable coordinate. Empty string if the index is
/// out of range.
pub fn performer_sheet(doc: &Document, performer_index: usize) -> String {
    performer_sheet_localized(doc, performer_index, Locale::Ja)
}
pub fn performer_sheet_localized(doc: &Document, performer_index: usize, locale: Locale) -> String {
    let Some(performer) = doc.performers.get(performer_index) else {
        return String::new();
    };
    doc.sets
        .iter()
        .map(|set| {
            let coord = set
                .positions
                .get(performer_index)
                .map(|p| readable_localized(*p, &doc.grid, locale))
                .unwrap_or_default();
            if locale == Locale::Ja {
                format!(
                    "{}  {} ({}カウント): {}",
                    performer.label, set.name, set.counts, coord
                )
            } else {
                format!(
                    "{}  {} ({} counts): {}",
                    performer.label, set.name, set.counts, coord
                )
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// CSV of every coordinate: header plus one row per (performer, set).
/// Columns: `performer,label,set,counts,x,y,side_to_side,front_to_back`.
pub fn coordinates_csv(doc: &Document) -> String {
    coordinates_csv_localized(doc, Locale::Ja)
}
pub fn coordinates_csv_localized(doc: &Document, locale: Locale) -> String {
    let mut out = String::from("performer,label,set,counts,x,y,side_to_side,front_to_back");
    for (i, performer) in doc.performers.iter().enumerate() {
        for set in &doc.sets {
            let p = set.positions.get(i).copied().unwrap_or_default();
            let sts = side_to_side_localized(p, &doc.grid, locale);
            let ftb = front_to_back_localized(p, &doc.grid, locale);
            out.push('\n');
            out.push_str(&format!(
                "{},{},{},{},{},{},{},{}",
                performer.id,
                csv_escape(&performer.label),
                csv_escape(&set.name),
                set.counts,
                fmt_num(p.x),
                fmt_num(p.y),
                csv_escape(&sts),
                csv_escape(&ftb),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GridConfig;

    fn grid() -> GridConfig {
        GridConfig::default()
    }

    #[test]
    fn center_reports_on_the_fifty_with_no_side() {
        let s = side_to_side(Point { x: 50.0, y: 25.0 }, &grid());
        assert_eq!(s, "50ヤードラインちょうど");
    }

    #[test]
    fn exactly_on_a_side_yard_line() {
        // x=45 on サイド1 (toward x=0); yard number = min(45,55) = 45.
        assert_eq!(
            side_to_side(Point { x: 45.0, y: 25.0 }, &grid()),
            "サイド1 45ヤードラインちょうど"
        );
        // x=60 on サイド2; yard number = min(60,40) = 40 (decreases toward end).
        assert_eq!(
            side_to_side(Point { x: 60.0, y: 25.0 }, &grid()),
            "サイド2 40ヤードラインちょうど"
        );
    }

    #[test]
    fn steps_inside_a_yard_line() {
        // x=45.625 is one 8-to-5 step (0.625 yd) inside the 45 toward the 50.
        assert_eq!(
            side_to_side(Point { x: 45.625, y: 25.0 }, &grid()),
            "サイド1 45ヤードラインの内側に1歩"
        );
    }

    #[test]
    fn steps_outside_a_yard_line() {
        // x=43.75 is two steps outside the 45 toward the endzone.
        assert_eq!(
            side_to_side(Point { x: 43.75, y: 25.0 }, &grid()),
            "サイド1 45ヤードラインの外側に2歩"
        );
    }

    #[test]
    fn on_a_hash_reports_zero_steps() {
        // Front hash sits at y=20 in the default grid.
        assert_eq!(
            front_to_back(Point { x: 50.0, y: 20.0 }, &grid()),
            "フロントハッシュちょうど"
        );
    }

    #[test]
    fn steps_behind_a_hash() {
        // y=21.25 is two vertical steps (0.625 yd each) behind the front hash.
        assert_eq!(
            front_to_back(Point { x: 50.0, y: 21.25 }, &grid()),
            "フロントハッシュの2歩後ろ"
        );
    }

    #[test]
    fn steps_in_front_of_a_hash() {
        // y=18.75 is two steps toward the audience (in front of) the front hash.
        assert_eq!(
            front_to_back(Point { x: 50.0, y: 18.75 }, &grid()),
            "フロントハッシュの2歩前"
        );
    }

    #[test]
    fn front_sideline_is_a_reference() {
        assert_eq!(
            front_to_back(Point { x: 50.0, y: 0.0 }, &grid()),
            "フロントサイドラインちょうど"
        );
    }

    #[test]
    fn readable_combines_both_axes() {
        let r = readable(Point { x: 50.0, y: 20.0 }, &grid());
        assert_eq!(r, "50ヤードラインちょうど、フロントハッシュちょうど");
        assert!(r.contains('、'));
    }

    #[test]
    fn quarter_step_rounding() {
        // 0.15625 yd past the 45 = 0.25 step exactly.
        assert_eq!(
            side_to_side(
                Point {
                    x: 45.15625,
                    y: 25.0
                },
                &grid()
            ),
            "サイド1 45ヤードラインの内側に0.25歩"
        );
    }

    #[test]
    fn performer_sheet_has_one_line_per_set() {
        let doc = Document::demo(2, 2);
        let sheet = performer_sheet(&doc, 0);
        assert_eq!(sheet.lines().count(), doc.sets.len());
        assert!(sheet.contains("セット 1"));
    }

    #[test]
    fn performer_sheet_out_of_range_is_empty() {
        let doc = Document::demo(1, 1);
        assert_eq!(performer_sheet(&doc, 99), "");
    }

    #[test]
    fn csv_has_header_and_one_row_per_pair() {
        let doc = Document::demo(2, 2);
        let csv = coordinates_csv(&doc);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), doc.performers.len() * doc.sets.len() + 1);
        assert_eq!(
            lines[0],
            "performer,label,set,counts,x,y,side_to_side,front_to_back"
        );
        // Every data row carries all 8 columns' separators intact.
        assert!(lines[1].split(',').count() >= 8);
    }

    #[test]
    fn csv_quotes_fields_with_commas() {
        let mut doc = Document::demo(1, 1);
        doc.performers[0].label = "Smith, Jr".into();
        let csv = coordinates_csv(&doc);
        assert!(csv.contains("\"Smith, Jr\""));
    }

    #[test]
    fn english_readout_has_no_built_in_japanese_glyphs() {
        let text = readable_localized(
            Point {
                x: 45.625,
                y: 21.25,
            },
            &grid(),
            Locale::En,
        );
        assert_eq!(
            text,
            "Side 1: 1 steps inside the 45 yard line; 2 steps behind the front hash"
        );
        assert!(
            !text.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c)
                || ('\u{4e00}'..='\u{9fff}').contains(&c))
        );
    }

    #[test]
    fn coordinate_model_is_locale_independent() {
        let model = coordinate_readout(Point { x: 43.75, y: 18.75 }, &grid());
        assert_eq!(model.side.relation, LateralRelation::Outside);
        assert_eq!(model.front.relation, VerticalRelation::Front);
        assert!(format_readout(&model, Locale::Ja).contains("外側"));
        assert!(format_readout(&model, Locale::En).contains("outside"));
    }

    #[test]
    fn notation_variants_drive_rounding_lines_references_and_step_labels() {
        let mut grid = grid();
        grid.coordinate_notation.rounding = StepRounding::Eighth;
        grid.coordinate_notation.yard_lines = YardLineInterval::Ten;
        grid.coordinate_notation.front_back = FrontBackReference::FixedLabel("front hash".into());
        grid.coordinate_notation.step_style = StepNotationStyle::EightToFive;
        let text = readable_localized(Point { x: 43.2, y: 21.3 }, &grid, Locale::En);
        assert!(text.contains("40 yard line"));
        assert!(text.contains("front hash"));
        assert!(text.contains("8-to-5"));
    }

    #[test]
    fn notation_json_roundtrip_and_legacy_default_are_stable() {
        let mut doc = Document::demo(1, 1);
        doc.grid.coordinate_notation = CoordinateNotation::indoor();
        let loaded = Document::from_json(&doc.to_json().unwrap()).unwrap();
        assert_eq!(
            loaded.grid.coordinate_notation,
            CoordinateNotation::indoor()
        );
        let mut legacy: serde_json::Value = serde_json::from_str(&doc.to_json().unwrap()).unwrap();
        legacy["grid"]
            .as_object_mut()
            .unwrap()
            .remove("coordinate_notation");
        let loaded = Document::from_json(&legacy.to_string()).unwrap();
        assert_eq!(
            loaded.grid.coordinate_notation,
            CoordinateNotation::default()
        );
    }

    #[test]
    fn csv_uses_the_same_advanced_notation_as_screen_readout() {
        let mut doc = Document::demo(1, 1);
        doc.grid.coordinate_notation = CoordinateNotation::dci();
        let expected = readable_localized(doc.sets[0].positions[0], &doc.grid, Locale::En);
        let csv = coordinates_csv_localized(&doc, Locale::En);
        assert!(expected.contains("8-to-5"));
        assert!(csv.contains("8-to-5"));
    }
}
