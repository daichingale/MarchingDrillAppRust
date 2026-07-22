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

use crate::{Document, GridConfig, Point};

/// Round a step value to the nearest quarter step (standard drill precision).
fn round_quarter(v: f32) -> f32 {
    (v * 4.0).round() / 4.0
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
pub fn side_to_side(point: Point, grid: &GridConfig) -> String {
    let center = grid.width / 2.0;
    let interval = if grid.major_line_interval > 0.0 {
        grid.major_line_interval
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
    let steps = round_quarter((d_point - d_line) / step);

    let side = if point.x < center { "サイド1" } else { "サイド2" };
    let on_fifty = (yard_line - center).abs() < 1e-4;

    if steps.abs() < 1e-4 {
        if on_fifty {
            format!("{}ヤードラインちょうど", fmt_num(yard))
        } else {
            format!("{} {}ヤードラインちょうど", side, fmt_num(yard))
        }
    } else {
        let dir = if steps > 0.0 { "外側" } else { "内側" };
        format!(
            "{} {}ヤードラインの{}に{}歩",
            side,
            fmt_num(yard),
            dir,
            fmt_num(steps.abs())
        )
    }
}

/// Front-to-back readout: steps in front of / behind the nearest reference line.
///
/// References are the front sideline (y=0), every hash in `grid.hashes`, and the
/// back sideline (y=grid.height). A vertical step is
/// `vertical_units / vertical_steps` field-units. Reported to 0.25-step precision.
pub fn front_to_back(point: Point, grid: &GridConfig) -> String {
    let step = grid.vertical_units / grid.vertical_steps.max(1) as f32;

    let mut refs: Vec<(&str, f32)> = Vec::with_capacity(grid.hashes.len() + 2);
    refs.push(("フロントサイドライン", 0.0));
    for h in &grid.hashes {
        refs.push((h.label.as_str(), h.position));
    }
    refs.push(("バックサイドライン", grid.height));

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

    let steps = round_quarter((point.y - pos).abs() / step);
    if steps.abs() < 1e-4 {
        format!("{label}ちょうど")
    } else {
        let dir = if point.y > pos { "後ろ" } else { "前" };
        format!("{}の{}歩{}", label, fmt_num(steps), dir)
    }
}

/// Full readable coordinate: side-to-side and front-to-back, comma-separated.
pub fn readable(point: Point, grid: &GridConfig) -> String {
    format!(
        "{}、{}",
        side_to_side(point, grid),
        front_to_back(point, grid)
    )
}

/// Plaintext per-performer "dot book": one line per set with the performer label,
/// set name, counts and the readable coordinate. Empty string if the index is
/// out of range.
pub fn performer_sheet(doc: &Document, performer_index: usize) -> String {
    let Some(performer) = doc.performers.get(performer_index) else {
        return String::new();
    };
    doc.sets
        .iter()
        .map(|set| {
            let coord = set
                .positions
                .get(performer_index)
                .map(|&p| readable(p, &doc.grid))
                .unwrap_or_default();
            format!(
                "{}  {} ({}カウント): {}",
                performer.label, set.name, set.counts, coord
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// CSV of every coordinate: header plus one row per (performer, set).
/// Columns: `performer,label,set,counts,x,y,side_to_side,front_to_back`.
pub fn coordinates_csv(doc: &Document) -> String {
    let mut out =
        String::from("performer,label,set,counts,x,y,side_to_side,front_to_back");
    for (i, performer) in doc.performers.iter().enumerate() {
        for set in &doc.sets {
            let p = set.positions.get(i).copied().unwrap_or_default();
            let sts = side_to_side(p, &doc.grid);
            let ftb = front_to_back(p, &doc.grid);
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
            side_to_side(Point { x: 45.15625, y: 25.0 }, &grid()),
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
}
