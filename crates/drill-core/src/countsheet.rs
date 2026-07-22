//! Count sheet generation: per-set timing with spreadsheet-style rehearsal
//! marks, count offsets, musical position (measure:beat), and wall-clock time.
//! Pure logic, no UI.
//!
//! A *count sheet* is the performer-facing summary of a show: for every set it
//! lists where the set begins on the count timeline, which measure/beat that
//! lands on, and the elapsed real time. Timing is derived from the document's
//! [`TempoMap`](crate::tempo::TempoMap), so variable tempo is handled correctly.

use crate::Document;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

/// Spreadsheet-style rehearsal mark for a zero-based set index.
///
/// Uses bijective base-26 (the same scheme as spreadsheet column names):
/// `0 -> "A"`, `25 -> "Z"`, `26 -> "AA"`, `27 -> "AB"`, and so on.
pub fn rehearsal_mark(set_index: usize) -> String {
    let mut n = set_index;
    let mut letters = Vec::new();
    loop {
        letters.push(b'A' + (n % 26) as u8);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    letters.reverse();
    String::from_utf8(letters).expect("ASCII letters are valid UTF-8")
}

/// Timing summary for a single set on the count timeline.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SetTiming {
    /// Zero-based index of the set within the document.
    pub index: usize,
    /// Set name, copied from the document.
    pub name: String,
    /// Spreadsheet-style rehearsal mark (see [`rehearsal_mark`]).
    pub rehearsal_mark: String,
    /// Cumulative counts before this set begins (the set's global start count).
    pub start_count: u32,
    /// Counts from this set to the next.
    pub counts: u16,
    /// Measure the set starts on (measures start at 1).
    pub start_measure: u32,
    /// Beat within `start_measure` the set starts on (beats start at 1.0).
    pub start_beat: f32,
    /// Elapsed real time, in seconds, from count 0 to this set's start.
    pub start_seconds: f32,
}

/// Build a count sheet: one [`SetTiming`] per set, in order.
///
/// `beats_per_measure` is the time signature numerator used to derive the
/// measure/beat of each set start. It is clamped to at least 1 by the tempo map.
pub fn count_sheet(doc: &Document, beats_per_measure: u16) -> Vec<SetTiming> {
    let mut start_count: u32 = 0;
    let mut timings = Vec::with_capacity(doc.sets.len());
    for (index, set) in doc.sets.iter().enumerate() {
        let (start_measure, start_beat) =
            doc.tempo.measure_beat(start_count as f32, beats_per_measure);
        let start_seconds = doc.tempo.seconds_at(start_count as f32);
        timings.push(SetTiming {
            index,
            name: set.name.clone(),
            rehearsal_mark: rehearsal_mark(index),
            start_count,
            counts: set.counts,
            start_measure,
            start_beat,
            start_seconds,
        });
        start_count += u32::from(set.counts);
    }
    timings
}

/// Format a duration in seconds as `M:SS.ss` (e.g. `8.0 -> "0:08.00"`).
fn format_seconds(seconds: f32) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor();
    let rest = seconds - minutes * 60.0;
    format!("{}:{:05.2}", minutes as u64, rest)
}

/// Render a count sheet as a plaintext, column-aligned table: a header line
/// followed by one row per set.
pub fn count_sheet_text(doc: &Document, beats_per_measure: u16) -> String {
    let rows = count_sheet(doc, beats_per_measure);

    // Column widths grow to fit the widest cell (including the header labels).
    let mark_w = rows
        .iter()
        .map(|r| r.rehearsal_mark.len())
        .max()
        .unwrap_or(0)
        .max("Mark".len());
    let name_w = rows
        .iter()
        .map(|r| r.name.chars().count())
        .max()
        .unwrap_or(0)
        .max("Name".len());

    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:<mark_w$}  {:<name_w$}  {:>7}  {:>6}  {:>9}  {:>8}",
        "Mark", "Name", "Count", "Counts", "Meas:Beat", "Time",
    );
    for r in &rows {
        let meas_beat = format!("{}:{:.1}", r.start_measure, r.start_beat);
        let _ = writeln!(
            out,
            "{:<mark_w$}  {:<name_w$}  {:>7}  {:>6}  {:>9}  {:>8}",
            r.rehearsal_mark,
            r.name,
            r.start_count,
            r.counts,
            meas_beat,
            format_seconds(r.start_seconds),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rehearsal_marks_are_bijective_base26() {
        assert_eq!(rehearsal_mark(0), "A");
        assert_eq!(rehearsal_mark(25), "Z");
        assert_eq!(rehearsal_mark(26), "AA");
        assert_eq!(rehearsal_mark(27), "AB");
    }

    #[test]
    fn count_sheet_places_two_sets_on_the_timeline() {
        let doc = Document::demo(2, 2); // two 16-count sets, 4/4 at 120 bpm
        let sheet = count_sheet(&doc, 4);
        assert_eq!(sheet.len(), 2);

        let first = &sheet[0];
        assert_eq!(first.index, 0);
        assert_eq!(first.rehearsal_mark, "A");
        assert_eq!(first.start_count, 0);
        assert_eq!(first.counts, 16);
        assert_eq!(first.start_measure, 1);
        assert!((first.start_beat - 1.0).abs() < 1e-3);
        assert!((first.start_seconds - 0.0).abs() < 1e-3);

        let second = &sheet[1];
        assert_eq!(second.index, 1);
        assert_eq!(second.rehearsal_mark, "B");
        assert_eq!(second.start_count, 16);
        assert_eq!(second.start_measure, 5); // 16 counts / 4 = 4 measures later
        assert!((second.start_beat - 1.0).abs() < 1e-3);
        assert!((second.start_seconds - 8.0).abs() < 1e-3); // 16 * 60 / 120
    }

    #[test]
    fn format_seconds_uses_minutes_and_hundredths() {
        assert_eq!(format_seconds(0.0), "0:00.00");
        assert_eq!(format_seconds(8.0), "0:08.00");
        assert_eq!(format_seconds(65.5), "1:05.50");
    }

    #[test]
    fn text_has_header_plus_one_line_per_set() {
        let doc = Document::demo(2, 2);
        let text = count_sheet_text(&doc, 4);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 1 + doc.sets.len());
        assert!(lines[0].contains("Mark"));
        assert!(lines[1].contains('A'));
        assert!(lines[2].contains('B'));
    }
}
