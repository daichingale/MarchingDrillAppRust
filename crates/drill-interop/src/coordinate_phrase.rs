//! Bounded parser for public, human-readable marching coordinate phrases.
//! It deliberately rejects missing field-side information instead of guessing.

use drill_core::{
    GridConfig, Point,
    coordinates::{
        CoordinateReadout, FieldSide, FrontCoordinate, LateralRelation, SideCoordinate,
        VerticalRelation,
    },
};

const MAX_PHRASE_BYTES: usize = 256;
const MAX_TOKENS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhraseAxis {
    Lateral,
    Depth,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CoordinatePhraseError {
    Empty,
    Limit { what: &'static str, limit: usize },
    Unsupported { axis: PhraseAxis },
    AmbiguousSide { yard_line: f32 },
    UnknownReference,
    OutOfField { value: f32 },
    InvalidGrid,
}

impl std::fmt::Display for CoordinatePhraseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("coordinate phrase is empty"),
            Self::Limit { what, limit } => write!(f, "{what} exceeds safety limit ({limit})"),
            Self::Unsupported {
                axis: PhraseAxis::Lateral,
            } => f.write_str("unsupported side-to-side coordinate phrase"),
            Self::Unsupported {
                axis: PhraseAxis::Depth,
            } => f.write_str("unsupported front-to-back coordinate phrase"),
            Self::AmbiguousSide { yard_line } => write!(
                f,
                "yard line {yard_line} exists on both sides; specify Side 1 or Side 2"
            ),
            Self::UnknownReference => f.write_str("unknown hash or sideline reference"),
            Self::OutOfField { value } => write!(f, "coordinate {value} is outside the field"),
            Self::InvalidGrid => f.write_str("grid step configuration is invalid"),
        }
    }
}

impl std::error::Error for CoordinatePhraseError {}

fn bounded_words(input: &str) -> Result<String, CoordinatePhraseError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(CoordinatePhraseError::Empty);
    }
    if input.len() > MAX_PHRASE_BYTES {
        return Err(CoordinatePhraseError::Limit {
            what: "coordinate phrase bytes",
            limit: MAX_PHRASE_BYTES,
        });
    }
    let normalized = input
        .to_lowercase()
        .replace([':', ',', ';', '\u{3001}'], " ")
        .replace("yard line", "yardline")
        .replace("ヤードライン", "yardline")
        .replace("サイド", "side")
        .replace("ステップ", "歩")
        .replace(['の', 'に'], " ")
        .replace('歩', " 歩 ");
    let count = normalized.split_whitespace().count();
    if count > MAX_TOKENS {
        return Err(CoordinatePhraseError::Limit {
            what: "coordinate phrase tokens",
            limit: MAX_TOKENS,
        });
    }
    // The returned slices borrow `normalized`; callers cannot receive both safely.
    // Keep this helper only for validation and split again in each parser.
    Ok(normalized)
}

fn number_before(words: &[&str], marker: &str) -> Option<f32> {
    words
        .iter()
        .position(|word| *word == marker)
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| words[index].parse::<f32>().ok())
}

fn first_number(words: &[&str]) -> Option<f32> {
    words.iter().find_map(|word| word.parse::<f32>().ok())
}

pub fn parse_lateral_phrase(
    input: &str,
    grid: &GridConfig,
) -> Result<(SideCoordinate, f32), CoordinatePhraseError> {
    let normalized = bounded_words(input)?;
    let words = normalized.split_whitespace().collect::<Vec<_>>();
    let side = if normalized.contains("side 1") || normalized.contains("side1") {
        Some(FieldSide::One)
    } else if normalized.contains("side 2") || normalized.contains("side2") {
        Some(FieldSide::Two)
    } else {
        None
    };
    let relation = if words.iter().any(|v| matches!(*v, "inside" | "内側" | "内")) {
        LateralRelation::Inside
    } else if words
        .iter()
        .any(|v| matches!(*v, "outside" | "外側" | "外"))
    {
        LateralRelation::Outside
    } else {
        LateralRelation::On
    };
    let yard = words
        .iter()
        .find_map(|word| word.strip_suffix("yardline").and_then(|v| v.parse().ok()))
        .or_else(|| number_before(&words, "yardline"))
        .or_else(|| {
            if relation == LateralRelation::On {
                first_number(&words)
            } else {
                words.iter().rev().find_map(|word| word.parse().ok())
            }
        })
        .ok_or(CoordinatePhraseError::Unsupported {
            axis: PhraseAxis::Lateral,
        })?;
    if !yard.is_finite() || yard < 0.0 || yard > grid.width / 2.0 {
        return Err(CoordinatePhraseError::OutOfField { value: yard });
    }
    let center = grid.width / 2.0;
    let side = if (yard - center).abs() < 1e-4 {
        FieldSide::Center
    } else {
        side.ok_or(CoordinatePhraseError::AmbiguousSide { yard_line: yard })?
    };
    let steps = if relation == LateralRelation::On {
        0.0
    } else {
        number_before(&words, "steps")
            .or_else(|| number_before(&words, "step"))
            .or_else(|| number_before(&words, "歩"))
            .ok_or(CoordinatePhraseError::Unsupported {
                axis: PhraseAxis::Lateral,
            })?
    };
    let step_size = grid.horizontal_units / f32::from(grid.horizontal_steps);
    if !step_size.is_finite() || step_size <= 0.0 || !steps.is_finite() || steps < 0.0 {
        return Err(CoordinatePhraseError::InvalidGrid);
    }
    let line_x = match side {
        FieldSide::One => yard,
        FieldSide::Two => grid.width - yard,
        FieldSide::Center => center,
    };
    let direction =
        match (side, relation) {
            (_, LateralRelation::On) => 0.0,
            (FieldSide::One, LateralRelation::Inside)
            | (FieldSide::Two, LateralRelation::Outside) => 1.0,
            (FieldSide::One, LateralRelation::Outside)
            | (FieldSide::Two, LateralRelation::Inside) => -1.0,
            (FieldSide::Center, _) => {
                return Err(CoordinatePhraseError::Unsupported {
                    axis: PhraseAxis::Lateral,
                });
            }
        };
    let x = line_x + direction * steps * step_size;
    if !(0.0..=grid.width).contains(&x) {
        return Err(CoordinatePhraseError::OutOfField { value: x });
    }
    Ok((
        SideCoordinate {
            side,
            yard_line: yard,
            relation,
            steps,
        },
        x,
    ))
}

fn reference_position(input: &str, grid: &GridConfig) -> Option<(String, f32)> {
    let aliases = [
        ("front sideline", "フロントサイドライン", 0.0),
        ("back sideline", "バックサイドライン", grid.height),
        ("front hash", "フロントハッシュ", f32::NAN),
        ("back hash", "バックハッシュ", f32::NAN),
    ];
    for (en, ja, fixed) in aliases {
        if input.contains(en) || input.contains(ja) {
            let position = if fixed.is_nan() {
                grid.hashes.iter().find_map(|line| {
                    let label = line.label.to_lowercase();
                    ((en == "front hash"
                        && (label.contains("front") || label.contains("フロント")))
                        || (en == "back hash"
                            && (label.contains("back") || label.contains("バック"))))
                    .then_some(line.position)
                })?
            } else {
                fixed
            };
            return Some((ja.to_owned(), position));
        }
    }
    None
}

pub fn parse_depth_phrase(
    input: &str,
    grid: &GridConfig,
) -> Result<(FrontCoordinate, f32), CoordinatePhraseError> {
    let normalized = bounded_words(input)?;
    let words = normalized.split_whitespace().collect::<Vec<_>>();
    let (reference, reference_y) =
        reference_position(&normalized, grid).ok_or(CoordinatePhraseError::UnknownReference)?;
    let relation = if normalized.contains("behind") || normalized.contains("後ろ") {
        VerticalRelation::Behind
    } else if normalized.contains("in front")
        || normalized.contains("front of")
        || normalized.contains("前")
    {
        VerticalRelation::Front
    } else {
        VerticalRelation::On
    };
    let steps = if relation == VerticalRelation::On {
        0.0
    } else {
        number_before(&words, "steps")
            .or_else(|| number_before(&words, "step"))
            .or_else(|| number_before(&words, "歩"))
            .ok_or(CoordinatePhraseError::Unsupported {
                axis: PhraseAxis::Depth,
            })?
    };
    let step_size = grid.vertical_units / f32::from(grid.vertical_steps);
    if !step_size.is_finite() || step_size <= 0.0 || !steps.is_finite() || steps < 0.0 {
        return Err(CoordinatePhraseError::InvalidGrid);
    }
    let y = reference_y
        + match relation {
            VerticalRelation::On => 0.0,
            VerticalRelation::Behind => steps * step_size,
            VerticalRelation::Front => -steps * step_size,
        };
    if !(0.0..=grid.height).contains(&y) {
        return Err(CoordinatePhraseError::OutOfField { value: y });
    }
    Ok((
        FrontCoordinate {
            reference,
            relation,
            steps,
        },
        y,
    ))
}

pub fn parse_coordinate_phrases(
    lateral: &str,
    depth: &str,
    grid: &GridConfig,
) -> Result<(CoordinateReadout, Point), CoordinatePhraseError> {
    let (side, x) = parse_lateral_phrase(lateral, grid)?;
    let (front, y) = parse_depth_phrase(depth, grid)?;
    Ok((CoordinateReadout { side, front }, Point { x, y }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use drill_core::Locale;
    use drill_core::coordinates::format_readout;

    #[test]
    fn parses_english_and_japanese_and_round_trips_formatter() {
        let grid = GridConfig::default();
        let (_, en) = parse_coordinate_phrases(
            "Side 1: 2 steps inside the 45 yard line",
            "4 steps behind the front hash",
            &grid,
        )
        .unwrap();
        assert_eq!(en, Point { x: 46.25, y: 22.5 });
        let (readout, ja) = parse_coordinate_phrases(
            "side1 45yardlineの内側に2歩",
            "フロントハッシュの4歩後ろ",
            &grid,
        )
        .unwrap();
        assert_eq!(ja, en);
        for locale in [Locale::Ja, Locale::En] {
            let formatted = format_readout(&readout, locale);
            assert!(!formatted.is_empty());
        }
    }

    #[test]
    fn fifty_and_named_lines_are_unambiguous() {
        let grid = GridConfig::default();
        let (_, point) = parse_coordinate_phrases("50 yard line", "back sideline", &grid).unwrap();
        assert_eq!(
            point,
            Point {
                x: 50.0,
                y: grid.height
            }
        );
    }

    #[test]
    fn rejects_ambiguous_and_hostile_input_without_guessing() {
        let grid = GridConfig::default();
        assert!(matches!(
            parse_lateral_phrase("2 inside the 45", &grid),
            Err(CoordinatePhraseError::AmbiguousSide { yard_line: 45.0 })
        ));
        assert!(matches!(
            parse_depth_phrase(&"word ".repeat(40), &grid),
            Err(CoordinatePhraseError::Limit { .. })
        ));
        assert!(parse_depth_phrase("NaN behind front hash", &grid).is_err());
        assert!(parse_depth_phrase("999 behind front hash", &grid).is_err());
    }
}
