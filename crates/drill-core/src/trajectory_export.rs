//! Count-accurate, read-only CSV export of one transition for selected performers.
//! Suitable for rehearsal diagnostics and comparison with other drill editors.
//! Uses the same route evaluator as live playback (including gates and holds).

use crate::{Document, Unit, transition};
use std::{collections::BTreeSet, fmt::Write as _};

// CSV escaping is deliberately local: it is an export-only detail and does
// not need to expose the coordinates module's private implementation.
fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

/// Bound synchronous clipboard exports to avoid allocating huge strings on the UI thread.
pub const MAX_TRAJECTORY_CSV_ROWS: usize = 50_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrajectoryCsvError {
    NoTransition,
    NoSelection,
    InvalidSelection,
    TooManyRows,
}

/// Samples integer local counts, including the starting dot (count 0), arrival,
/// and each hold count, for a transition between consecutive sets.
///
/// The columns are:
/// from_set,to_set,local_count,global_count,phase,performer,label,x,y,unit
///
/// Counts are zero-based and positions are field units (meters or yards as
/// specified by the unit column). Output order is count-major, then input
/// performer order. This never modifies the document or its history.
///
/// This intentionally does not parse or emulate proprietary Pyware formats.
pub fn transition_trajectory_csv(
    document: &Document,
    from_set_index: usize,
    performer_indices: &[usize],
) -> Result<String, TrajectoryCsvError> {
    let Some(from) = document.sets.get(from_set_index) else {
        return Err(TrajectoryCsvError::NoTransition);
    };
    let Some(to) = document.sets.get(from_set_index + 1) else {
        return Err(TrajectoryCsvError::NoTransition);
    };
    if performer_indices.is_empty() {
        return Err(TrajectoryCsvError::NoSelection);
    }

    let mut seen = BTreeSet::new();
    let mut performers = Vec::with_capacity(performer_indices.len());
    for &index in performer_indices {
        if !seen.insert(index) {
            return Err(TrajectoryCsvError::InvalidSelection);
        }
        let Some(performer) = document.performers.get(index) else {
            return Err(TrajectoryCsvError::InvalidSelection);
        };
        let (Some(&start), Some(&end)) = (from.positions.get(index), to.positions.get(index))
        else {
            return Err(TrajectoryCsvError::InvalidSelection);
        };
        performers.push((
            performer.id,
            performer.id.to_string(),
            csv_escape(&performer.label),
            start,
            end,
        ));
    }

    let moves = u32::from(from.counts);
    let total = moves + u32::from(from.hold);
    let rows = (total as usize + 1)
        .checked_mul(performers.len())
        .ok_or(TrajectoryCsvError::TooManyRows)?;
    if rows > MAX_TRAJECTORY_CSV_ROWS {
        return Err(TrajectoryCsvError::TooManyRows);
    }

    let from_name = csv_escape(&from.name);
    let to_name = csv_escape(&to.name);
    let unit = match document.grid.unit {
        Unit::Meters => "meters",
        Unit::Yards => "yards",
    };
    let mut csv = String::with_capacity((rows * 110).min(6_000_000));
    csv.push_str("from_set,to_set,local_count,global_count,phase,performer,label,x,y,unit");
    let first_global = document.global_count(from_set_index, 0.0) as u32;

    for local_count in 0..=total {
        let phase = if local_count < moves {
            "move"
        } else if local_count == moves {
            "arrival"
        } else {
            "hold"
        };
        let global_count = first_global + local_count;
        for &(performer_id, ref id, ref label, start, end) in &performers {
            let point = if local_count >= moves {
                end
            } else {
                transition::evaluate(
                    from.routes.route_for(performer_id),
                    start,
                    end,
                    local_count as f32,
                    from.counts,
                )
            };
            write!(
                &mut csv,
                "\n{from_name},{to_name},{local_count},{global_count},{phase},{id},{label},{:.6},{:.6},{unit}",
                point.x, point.y,
            )
            .expect("formatting into a String cannot fail");
        }
    }
    Ok(csv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Point,
        transition::{ChordPoint, Easing, Gate, Route, RouteShape},
    };

    #[test]
    fn start_arrival_and_hold_are_included_once() {
        let mut document = Document::demo(1, 1);
        document.sets[0].counts = 4;
        document.sets[0].hold = 2;
        document.sets[0].positions[0] = Point { x: 0.0, y: 0.0 };
        document.sets[1].positions[0] = Point { x: 8.0, y: 0.0 };
        let before = document.clone();

        let csv = transition_trajectory_csv(&document, 0, &[0]).unwrap();
        let rows: Vec<Vec<&str>> = csv
            .lines()
            .skip(1)
            .map(|row| row.split(',').collect())
            .collect();
        assert_eq!(rows.len(), 7);
        assert_eq!(rows[0][2], "0");
        assert_eq!(rows[0][4], "move");
        assert_eq!(rows[0][7], "0.000000");
        assert_eq!(rows[1][7], "2.000000");
        assert_eq!(rows[4][4], "arrival");
        assert_eq!(rows[4][7], "8.000000");
        assert_eq!(rows[5][4], "hold");
        assert_eq!(rows[6][7], "8.000000");
        assert_eq!(rows[6][3], "6");
        assert_eq!(rows[6][9], "yards");
        assert_eq!(document, before, "CSV export must never mutate a document");
    }

    #[test]
    fn gated_curved_route_matches_live_playback_at_each_count() {
        let mut document = Document::demo(1, 2);
        document.sets[0].counts = 4;
        document.sets[0].hold = 1;
        document.sets[0].routes.overrides.insert(
            document.performers[1].id,
            Route {
                shape: RouteShape::Curve {
                    control: ChordPoint {
                        along: 0.5,
                        lateral: 0.3,
                    },
                },
                gate: Gate {
                    depart: 1.0,
                    arrive: Some(3.0),
                },
                easing: Easing::Smooth,
            },
        );

        // Nonzero indices prove the CSV respects the selected performer,
        // not the first dot in the dense position array.
        let csv = transition_trajectory_csv(&document, 0, &[1]).unwrap();
        let mut evaluated = Vec::new();
        for (count, row) in csv.lines().skip(1).enumerate() {
            document.positions_at_count(0, count as f32, &mut evaluated);
            let values: Vec<&str> = row.split(',').collect();
            let csv_x = values[7].parse::<f32>().unwrap();
            let csv_y = values[8].parse::<f32>().unwrap();
            assert!(
                (csv_x - evaluated[1].x).abs() < 0.00002,
                "x at count {count}"
            );
            assert!(
                (csv_y - evaluated[1].y).abs() < 0.00002,
                "y at count {count}"
            );
        }
    }

    #[test]
    fn escapes_labels_set_names_and_identifies_meter_units() {
        let mut document = Document::demo(1, 1);
        document.sets[0].name = "Intro, A".into();
        document.sets[1].name = "Final \"B\"".into();
        document.performers[0].label = "木管,\"A\"".into();
        document.grid.unit = Unit::Meters;
        let csv = transition_trajectory_csv(&document, 0, &[0]).unwrap();
        assert!(csv.contains("\"Intro, A\""));
        assert!(csv.contains("\"Final \"\"B\"\"\""));
        assert!(csv.contains("\"木管,\"\"A\"\"\""));
        assert!(csv.lines().skip(1).all(|row| row.ends_with(",meters")));
    }

    #[test]
    fn rejects_invalid_or_unbounded_exports() {
        let document = Document::demo(1, 1);
        assert_eq!(
            transition_trajectory_csv(&document, 1, &[0]),
            Err(TrajectoryCsvError::NoTransition)
        );
        assert_eq!(
            transition_trajectory_csv(&document, 0, &[]),
            Err(TrajectoryCsvError::NoSelection)
        );
        assert_eq!(
            transition_trajectory_csv(&document, 0, &[99]),
            Err(TrajectoryCsvError::InvalidSelection)
        );
        assert_eq!(
            transition_trajectory_csv(&document, 0, &[0, 0]),
            Err(TrajectoryCsvError::InvalidSelection)
        );

        let mut large = Document::demo(1, 20);
        large.sets[0].counts = 4_096;
        assert_eq!(
            transition_trajectory_csv(&large, 0, &(0..20).collect::<Vec<_>>()),
            Err(TrajectoryCsvError::TooManyRows)
        );
    }
}
