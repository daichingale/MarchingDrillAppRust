//! Rhythm synchronization analysis: how closely the show's departs and
//! arrivals line up with the musical beat.
//!
//! Marching drill reads more sharply to the eye (and to a musician's ear)
//! when a performer departs or arrives exactly on a strong beat; it reads
//! differently — not worse, just differently — when the same motion lands
//! off the beat. Pyware 3D has no equivalent analysis: it can tell you where
//! performers are, but not how their motion sits against the music. This
//! module answers that question using machinery that already exists
//! elsewhere in the crate:
//!
//! - [`crate::tempo::TempoMap::measure_beat`] for the count-to-beat mapping.
//! - [`crate::transition::RouteTable::route_for`] /
//!   [`crate::transition::Gate::resolve`] for each performer's own
//!   depart/arrive counts within a transition (routes may be overridden per
//!   performer, so this module always resolves through `route_for` rather
//!   than assuming the set's default route applies to everyone).
//!
//! It follows the params/report shape used by [`crate::clinic`]: a plain
//! configuration struct ([`RhythmSyncParams`]) and a report whose event list
//! is allocated once, with an exact upfront capacity, rather than grown
//! incrementally.

use crate::{Document, PerformerId};

/// A moment's position within a measure, in the same `(measure, beat)`
/// convention as [`crate::tempo::TempoMap::measure_beat`]: `beat_in_measure`
/// runs `1.0..beats_per_measure + 1.0`, where `1.0` is the downbeat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeatAlignment {
    pub measure: u32,
    pub beat_in_measure: f32,
    /// Signed distance, in counts, from the nearest whole beat. Negative
    /// means early (the event happens before the beat lands), positive means
    /// late (after it). Always within `-0.5..=0.5`.
    pub offset_from_nearest_beat: f32,
}

impl BeatAlignment {
    fn new(measure: u32, beat_in_measure: f32) -> Self {
        let nearest = beat_in_measure.round();
        Self {
            measure,
            beat_in_measure,
            offset_from_nearest_beat: beat_in_measure - nearest,
        }
    }

    /// The nearest whole beat number within the measure, normalized back into
    /// `1..=beats_per_measure` (rounding can walk one step past the open end
    /// of `measure_beat`'s range, onto the next measure's downbeat).
    fn nearest_beat_in_measure(self, beats_per_measure: u16) -> f32 {
        let per_measure = f32::from(beats_per_measure.max(1));
        let nearest = self.beat_in_measure.round();
        if nearest >= per_measure + 1.0 {
            nearest - per_measure
        } else {
            nearest
        }
    }
}

/// Whether a performer is departing a formation or arriving at one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RhythmEventKind {
    Depart,
    Arrive,
}

/// Musical character of a single depart/arrive moment, classified from its
/// [`BeatAlignment`] against `on_beat_tolerance_counts`. This is the
/// vocabulary a music-literate reader cares about, not just a raw offset:
///
/// - [`OnDownbeat`](Self::OnDownbeat): within tolerance of beat 1 — the
///   strongest pulse in the measure. Reads as a clean "hit".
/// - [`OnBackbeat`](Self::OnBackbeat): within tolerance of any other whole
///   beat (2, 3, 4, ...). Still rhythmically locked, just not the heaviest
///   downbeat — the classic "on beat 2/4" backbeat feel, generalized to any
///   meter.
/// - [`Syncopated`](Self::Syncopated): not within tolerance of any whole
///   beat, but within tolerance of the exact halfway point between two beats
///   (the "and"/upbeat). A deliberate off-the-beat placement, not noise.
/// - [`Free`](Self::Free): neither cleanly on a beat nor cleanly on the
///   half-beat — motion with no clear rhythmic anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RhythmicIntent {
    OnDownbeat,
    OnBackbeat,
    Syncopated,
    Free,
}

impl RhythmicIntent {
    fn classify(alignment: BeatAlignment, beats_per_measure: u16, tolerance: f32) -> Self {
        let offset = alignment.offset_from_nearest_beat;
        if offset.abs() <= tolerance {
            let nearest_beat = alignment.nearest_beat_in_measure(beats_per_measure);
            return if (nearest_beat - 1.0).abs() < 0.5 {
                Self::OnDownbeat
            } else {
                Self::OnBackbeat
            };
        }
        if (offset.abs() - 0.5).abs() <= tolerance {
            return Self::Syncopated;
        }
        Self::Free
    }
}

/// A single performer's depart or arrival moment, resolved to the show's
/// global count timeline and classified against the beat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PerformerRhythmEvent {
    pub performer_id: PerformerId,
    /// Index of the transition this event belongs to: the performer moves
    /// from `document.sets[set_index]` toward `document.sets[set_index + 1]`.
    pub set_index: usize,
    pub kind: RhythmEventKind,
    pub global_count: f32,
    pub alignment: BeatAlignment,
    pub intent: RhythmicIntent,
}

/// Configuration for a rhythm-sync scan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RhythmSyncParams {
    pub beats_per_measure: u16,
    /// How close (in counts) a depart/arrive must land to a beat (or
    /// half-beat) to count as "on" it.
    pub on_beat_tolerance_counts: f32,
}

impl Default for RhythmSyncParams {
    fn default() -> Self {
        Self {
            beats_per_measure: 4,
            on_beat_tolerance_counts: 0.15,
        }
    }
}

impl RhythmSyncParams {
    fn sanitized(self) -> Self {
        Self {
            beats_per_measure: self.beats_per_measure.max(1),
            on_beat_tolerance_counts: if self.on_beat_tolerance_counts.is_finite() {
                self.on_beat_tolerance_counts.max(0.0)
            } else {
                Self::default().on_beat_tolerance_counts
            },
        }
    }
}

/// Show-wide (or, via [`set_rhythm_summary`], per-set) rhythm sync result.
#[derive(Clone, Debug, PartialEq)]
pub struct RhythmSyncReport {
    pub events: Vec<PerformerRhythmEvent>,
    /// Fraction of events (depart + arrive) that landed on a beat
    /// ([`RhythmicIntent::OnDownbeat`] or [`RhythmicIntent::OnBackbeat`]),
    /// in `0.0..=1.0`.
    pub on_beat_ratio: f32,
    /// `on_beat_ratio` rescaled to `0.0..=100.0`, matching the normalization
    /// used by the step-style difficulty score.
    pub show_score: f32,
}

/// Coarse, UI-friendly summary of one transition's rhythm sync.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SetRhythmSummary {
    pub set_index: usize,
    pub event_count: usize,
    pub on_beat_ratio: f32,
    pub average_offset_counts: f32,
    /// Fraction of events classified as [`RhythmicIntent::Syncopated`].
    pub syncopation_ratio: f32,
}

/// Analyzes every transition in the show: for each performer, the depart and
/// arrival moments of each transition are resolved to the global count
/// timeline and classified against the beat.
///
/// `O(performers * transitions)`, one allocation for the event list (sized
/// exactly up front), no per-pair heap traffic. Never panics: an empty
/// document, zero performers, an empty tempo map, or non-finite gate values
/// all degrade to an empty or all-`Free` report rather than crashing.
pub fn analyze_show(document: &Document, params: &RhythmSyncParams) -> RhythmSyncReport {
    let params = params.sanitized();
    let transitions = document.sets.len().saturating_sub(1);
    let mut events = Vec::with_capacity(
        document
            .performers
            .len()
            .saturating_mul(transitions)
            .saturating_mul(2),
    );
    for set_index in 0..transitions {
        push_transition_events(document, set_index, params, &mut events);
    }
    finish_report(events)
}

/// Coarse per-set summary, e.g. "this set arrives on-beat 80% of the time",
/// suitable for a compact UI readout. Scans only the one transition
/// (`O(performers)`), not the whole show.
pub fn set_rhythm_summary(
    document: &Document,
    set_index: usize,
    params: &RhythmSyncParams,
) -> SetRhythmSummary {
    let params = params.sanitized();
    let mut events = Vec::with_capacity(document.performers.len().saturating_mul(2));
    push_transition_events(document, set_index, params, &mut events);
    let event_count = events.len();
    if event_count == 0 {
        return SetRhythmSummary {
            set_index,
            event_count: 0,
            on_beat_ratio: 0.0,
            average_offset_counts: 0.0,
            syncopation_ratio: 0.0,
        };
    }
    let on_beat = events
        .iter()
        .filter(|event| event.intent.is_on_beat())
        .count();
    let syncopated = events
        .iter()
        .filter(|event| event.intent == RhythmicIntent::Syncopated)
        .count();
    let offset_sum: f32 = events
        .iter()
        .map(|event| event.alignment.offset_from_nearest_beat.abs())
        .sum();
    SetRhythmSummary {
        set_index,
        event_count,
        on_beat_ratio: on_beat as f32 / event_count as f32,
        average_offset_counts: offset_sum / event_count as f32,
        syncopation_ratio: syncopated as f32 / event_count as f32,
    }
}

impl RhythmicIntent {
    fn is_on_beat(self) -> bool {
        matches!(self, Self::OnDownbeat | Self::OnBackbeat)
    }
}

fn finish_report(events: Vec<PerformerRhythmEvent>) -> RhythmSyncReport {
    let total = events.len();
    let on_beat = events
        .iter()
        .filter(|event| event.intent.is_on_beat())
        .count();
    let on_beat_ratio = if total == 0 {
        0.0
    } else {
        on_beat as f32 / total as f32
    };
    let show_score = (on_beat_ratio * 100.0).clamp(0.0, 100.0);
    RhythmSyncReport {
        events,
        on_beat_ratio,
        show_score,
    }
}

/// Pushes both events (depart, arrive) for every performer of the transition
/// starting at `set_index`, or does nothing if there is no such transition
/// (out of range, or the last set with nowhere left to go).
fn push_transition_events(
    document: &Document,
    set_index: usize,
    params: RhythmSyncParams,
    out: &mut Vec<PerformerRhythmEvent>,
) {
    let Some(from) = document.sets.get(set_index) else {
        return;
    };
    if document.sets.get(set_index + 1).is_none() {
        return;
    }
    let moves = f32::from(from.counts);
    for performer in &document.performers {
        let route = from.routes.route_for(performer.id);
        let (depart_local, arrive_local) = route.gate.resolve(moves);
        push_event(
            document,
            out,
            performer.id,
            set_index,
            RhythmEventKind::Depart,
            document.global_count(set_index, depart_local),
            params,
        );
        push_event(
            document,
            out,
            performer.id,
            set_index,
            RhythmEventKind::Arrive,
            document.global_count(set_index, arrive_local),
            params,
        );
    }
}

fn push_event(
    document: &Document,
    out: &mut Vec<PerformerRhythmEvent>,
    performer_id: PerformerId,
    set_index: usize,
    kind: RhythmEventKind,
    global_count: f32,
    params: RhythmSyncParams,
) {
    let global_count = if global_count.is_finite() {
        global_count.max(0.0)
    } else {
        0.0
    };
    let (measure, beat) = document
        .tempo
        .measure_beat(global_count, params.beats_per_measure);
    let alignment = BeatAlignment::new(measure, beat);
    let intent = RhythmicIntent::classify(
        alignment,
        params.beats_per_measure,
        params.on_beat_tolerance_counts,
    );
    out.push(PerformerRhythmEvent {
        performer_id,
        set_index,
        kind,
        global_count,
        alignment,
        intent,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tempo::TempoMap;
    use crate::transition::{Gate, Route};

    /// 120 bpm, 1 count = 1 beat, so counts and beats coincide 1:1: count 8 in
    /// 4/4 is measure 3, beat 1 (the downbeat).
    fn doc_with_gate(gate: Gate, counts: u16) -> Document {
        let mut doc = Document::demo(1, 1);
        doc.tempo = TempoMap::constant(120.0);
        doc.sets[0].counts = counts;
        doc.sets[0].routes.default = Route {
            gate,
            ..Route::default()
        };
        doc
    }

    #[test]
    fn single_performer_hand_computed_downbeat_arrival() {
        // Arrives at local count 8 of an 8-count transition starting at
        // global count 0: global count 8, which in 4/4 is measure 3 beat 1 —
        // squarely the downbeat, so offset should be ~0 and intent OnDownbeat.
        let doc = doc_with_gate(
            Gate {
                depart: 0.0,
                arrive: Some(8.0),
            },
            8,
        );
        let params = RhythmSyncParams {
            beats_per_measure: 4,
            on_beat_tolerance_counts: 0.15,
        };
        let report = analyze_show(&doc, &params);
        assert_eq!(report.events.len(), 2); // depart + arrive, 1 performer
        let arrive = report
            .events
            .iter()
            .find(|e| e.kind == RhythmEventKind::Arrive)
            .unwrap();
        assert_eq!(arrive.global_count, 8.0);
        assert_eq!(arrive.alignment.measure, 3);
        assert!((arrive.alignment.beat_in_measure - 1.0).abs() < 1e-5);
        assert!((arrive.alignment.offset_from_nearest_beat).abs() < 1e-5);
        assert_eq!(arrive.intent, RhythmicIntent::OnDownbeat);

        let depart = report
            .events
            .iter()
            .find(|e| e.kind == RhythmEventKind::Depart)
            .unwrap();
        assert_eq!(depart.global_count, 0.0);
        assert_eq!(depart.alignment.measure, 1);
        assert!((depart.alignment.beat_in_measure - 1.0).abs() < 1e-5);
        assert_eq!(depart.intent, RhythmicIntent::OnDownbeat);
    }

    #[test]
    fn hand_computed_backbeat_and_offbeat_alignment() {
        // Arrives at global count 5 in 4/4: measure 2, beat 2 -- a backbeat.
        let doc = doc_with_gate(
            Gate {
                depart: 0.0,
                arrive: Some(5.0),
            },
            5,
        );
        let params = RhythmSyncParams::default();
        let summary = set_rhythm_summary(&doc, 0, &params);
        assert_eq!(summary.event_count, 2);
        let report = analyze_show(&doc, &params);
        let arrive = report
            .events
            .iter()
            .find(|e| e.kind == RhythmEventKind::Arrive)
            .unwrap();
        assert_eq!(arrive.alignment.measure, 2);
        assert!((arrive.alignment.beat_in_measure - 2.0).abs() < 1e-5);
        assert_eq!(arrive.intent, RhythmicIntent::OnBackbeat);
    }

    #[test]
    fn all_arrivals_on_downbeat_score_high() {
        // 4 performers, all arriving at local count 8 (global count 8 == beat
        // 1 of measure 3): every arrival is a clean downbeat hit. Departs are
        // all at count 0 (also a downbeat), so the whole show should score
        // at or near 100.
        let mut doc = Document::demo(2, 2);
        doc.tempo = TempoMap::constant(120.0);
        doc.sets[0].counts = 8;
        doc.sets[0].routes.default = Route {
            gate: Gate {
                depart: 0.0,
                arrive: Some(8.0),
            },
            ..Route::default()
        };
        let params = RhythmSyncParams::default();
        let report = analyze_show(&doc, &params);
        assert_eq!(report.events.len(), doc.performers.len() * 2);
        assert!(
            report.on_beat_ratio > 0.99,
            "ratio={}",
            report.on_beat_ratio
        );
        assert!(report.show_score > 99.0, "score={}", report.show_score);
    }

    #[test]
    fn all_arrivals_off_beat_score_low() {
        // Depart/arrive at local counts 0.3 / 8.3: each sits 0.3 counts off
        // the nearest beat (0 and 8 respectively), which is outside the
        // default 0.15 on-beat tolerance *and* outside 0.15 of the 0.5
        // half-beat mark (|0.3 - 0.5| = 0.2), so this should read as Free /
        // low-scoring rather than Syncopated.
        let mut doc = Document::demo(2, 2);
        doc.tempo = TempoMap::constant(120.0);
        doc.sets[0].counts = 9; // u16 field; gate below clamps within it
        doc.sets[0].routes.default = Route {
            gate: Gate {
                depart: 0.3,
                arrive: Some(8.3),
            },
            ..Route::default()
        };
        let params = RhythmSyncParams::default();
        let report = analyze_show(&doc, &params);
        assert!(
            report.on_beat_ratio < 0.01,
            "ratio={}",
            report.on_beat_ratio
        );
        assert!(report.show_score < 1.0, "score={}", report.show_score);
        assert!(
            report
                .events
                .iter()
                .all(|event| event.intent == RhythmicIntent::Free)
        );
    }

    #[test]
    fn syncopated_events_land_near_the_half_beat() {
        let mut doc = Document::demo(1, 1);
        doc.tempo = TempoMap::constant(120.0);
        doc.sets[0].counts = 8;
        doc.sets[0].routes.default = Route {
            gate: Gate {
                depart: 4.5,
                arrive: Some(8.0),
            },
            ..Route::default()
        };
        let params = RhythmSyncParams::default();
        let report = analyze_show(&doc, &params);
        let depart = report
            .events
            .iter()
            .find(|e| e.kind == RhythmEventKind::Depart)
            .unwrap();
        assert_eq!(depart.global_count, 4.5);
        assert_eq!(depart.intent, RhythmicIntent::Syncopated);
    }

    #[test]
    fn per_performer_route_overrides_are_respected() {
        // Two performers in the same set with different gates must be
        // resolved through `route_for`, not the set's default route.
        let mut doc = Document::demo(1, 2);
        doc.tempo = TempoMap::constant(120.0);
        doc.sets[0].counts = 8;
        doc.sets[0].routes.default = Route {
            gate: Gate {
                depart: 0.0,
                arrive: Some(8.0),
            },
            ..Route::default()
        };
        let overridden_id = doc.performers[1].id;
        doc.sets[0].routes.overrides.insert(
            overridden_id,
            Route {
                gate: Gate {
                    depart: 4.5,
                    arrive: Some(8.0),
                },
                ..Route::default()
            },
        );
        let params = RhythmSyncParams::default();
        let report = analyze_show(&doc, &params);
        let default_performer_depart = report
            .events
            .iter()
            .find(|e| e.performer_id != overridden_id && e.kind == RhythmEventKind::Depart)
            .unwrap();
        assert_eq!(default_performer_depart.global_count, 0.0);
        let overridden_depart = report
            .events
            .iter()
            .find(|e| e.performer_id == overridden_id && e.kind == RhythmEventKind::Depart)
            .unwrap();
        assert_eq!(overridden_depart.global_count, 4.5);
        assert_eq!(overridden_depart.intent, RhythmicIntent::Syncopated);
    }

    #[test]
    fn empty_and_degenerate_documents_never_panic() {
        let mut empty = Document::demo(0, 0);
        empty.sets.clear();
        let params = RhythmSyncParams::default();
        let report = analyze_show(&empty, &params);
        assert!(report.events.is_empty());
        assert_eq!(report.on_beat_ratio, 0.0);
        assert_eq!(report.show_score, 0.0);

        let no_performers = Document::demo(0, 0);
        let report = analyze_show(&no_performers, &params);
        assert!(report.events.is_empty());

        // Single set: no transitions to analyze.
        let mut single_set = Document::demo(1, 1);
        single_set.sets.truncate(1);
        let report = analyze_show(&single_set, &params);
        assert!(report.events.is_empty());

        // Non-finite gate values must not propagate into a panic or NaN
        // global counts.
        let mut nan_gate = Document::demo(1, 1);
        nan_gate.sets[0].routes.default = Route {
            gate: Gate {
                depart: f32::NAN,
                arrive: Some(f32::NAN),
            },
            ..Route::default()
        };
        let report = analyze_show(&nan_gate, &params);
        for event in &report.events {
            assert!(event.global_count.is_finite());
            assert!(event.alignment.beat_in_measure.is_finite());
            assert!(event.alignment.offset_from_nearest_beat.is_finite());
        }

        // Non-finite beats_per_measure-adjacent input (zero) is clamped, not
        // a division by zero.
        let degenerate_params = RhythmSyncParams {
            beats_per_measure: 0,
            on_beat_tolerance_counts: f32::NAN,
        };
        let doc = Document::demo(1, 1);
        let report = analyze_show(&doc, &degenerate_params);
        for event in &report.events {
            assert!(event.alignment.beat_in_measure.is_finite());
        }

        // set_rhythm_summary on an out-of-range index degrades to zeroed
        // output rather than panicking.
        let summary = set_rhythm_summary(&doc, 99, &params);
        assert_eq!(summary.event_count, 0);
        assert_eq!(summary.on_beat_ratio, 0.0);
    }

    #[test]
    fn thousand_performers_sixty_four_sets_completes() {
        // Scale check: not a strict timing assertion (CI hardware varies),
        // just confirms the O(performers * sets) path completes cleanly at
        // the documented scale without panicking or looping.
        let mut doc = Document::demo(40, 25); // 1000 performers
        assert_eq!(doc.performers.len(), 1000);
        let mut sets = Vec::with_capacity(64);
        for i in 0..64 {
            let mut set = doc.sets[i % doc.sets.len()].clone();
            set.id = crate::SetId::new(i as u32 + 1).unwrap();
            set.counts = 8 + (i % 5) as u16;
            sets.push(set);
        }
        doc.sets = sets;
        let params = RhythmSyncParams::default();
        let report = analyze_show(&doc, &params);
        assert_eq!(report.events.len(), 1000 * 63 * 2);
        assert!(report.on_beat_ratio.is_finite());
        assert!(report.show_score.is_finite());
    }

    #[test]
    fn analysis_is_deterministic() {
        let doc = Document::demo(3, 3);
        let params = RhythmSyncParams::default();
        let first = analyze_show(&doc, &params);
        let second = analyze_show(&doc, &params);
        assert_eq!(first, second);
    }
}
