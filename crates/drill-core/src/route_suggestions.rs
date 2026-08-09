//! Bounded, deterministic route-rewrite suggestions for Live Clinic.
//!
//! Suggestions are values: generation never mutates the source document.  A
//! selected suggestion can be converted to one atomic, undoable `Edit`.

use crate::clinic::{self, ClinicParams, ScanScratch, StepStyle};
use crate::transition::{self, ChordPoint, RouteShape, RouteTable, SetCounts};
use crate::{Document, Edit, PerformerId};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SuggestionConstraints {
    pub collision_distance: f32,
    pub max_step_per_count: f32,
    pub max_arrival_turn_radians: f32,
    /// `None` permits arrival on the final move count.
    pub latest_arrival_count: Option<f32>,
    pub allow_count_change: bool,
}

impl Default for SuggestionConstraints {
    fn default() -> Self {
        Self {
            collision_distance: 0.75,
            max_step_per_count: 1.0,
            max_arrival_turn_radians: 135.0_f32.to_radians(),
            latest_arrival_count: None,
            allow_count_change: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SuggestionLimits {
    pub max_candidates: usize,
    pub max_diagnostics: usize,
    pub samples: u8,
}

impl Default for SuggestionLimits {
    fn default() -> Self {
        Self {
            max_candidates: 8,
            max_diagnostics: 256,
            samples: 24,
        }
    }
}

impl SuggestionLimits {
    fn bounded(self) -> Self {
        Self {
            max_candidates: self.max_candidates.clamp(1, 16),
            max_diagnostics: self.max_diagnostics.clamp(1, 2_048),
            samples: self.samples.clamp(4, 64),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SuggestionScore {
    pub collisions: u16,
    pub excessive_strides: u16,
    pub sharp_turns: u16,
    pub late_arrivals: u16,
}

impl SuggestionScore {
    pub fn penalty(self) -> u32 {
        u32::from(self.collisions) * 10_000
            + u32::from(self.excessive_strides) * 1_000
            + u32::from(self.sharp_turns) * 100
            + u32::from(self.late_arrivals) * 10
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SuggestionReason {
    AvoidCollision,
    ReduceStride,
    SoftenArrivalTurn,
    MeetArrival,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RouteSuggestion {
    pub counts: SetCounts,
    pub routes: RouteTable,
    pub reasons: Vec<SuggestionReason>,
    pub affected_performers: Vec<PerformerId>,
    pub before: SuggestionScore,
    pub after: SuggestionScore,
    source_counts: SetCounts,
    source_routes: RouteTable,
}

impl RouteSuggestion {
    pub fn improvement(&self) -> u32 {
        self.before.penalty().saturating_sub(self.after.penalty())
    }

    /// Produces one atomic edit. The input document remains untouched.
    pub fn to_edit(&self, document: &Document, set_index: usize) -> Option<Edit> {
        let set = document.sets.get(set_index)?;
        if (SetCounts {
            moves: set.counts,
            hold: set.hold,
        }) != self.source_counts
            || set.routes != self.source_routes
        {
            return None;
        }
        let mut next = document.clone();
        let target = next
            .sets
            .iter_mut()
            .find(|candidate| candidate.id == set.id)?;
        target.counts = self.counts.moves;
        target.hold = self.counts.hold;
        target.routes = self.routes.clone();
        next.validate().ok()?;
        Some(Edit::ReplaceDocument {
            document: Box::new(next),
        })
    }
}

/// Generates ranked rewrite candidates without modifying `document`.
pub fn suggest_routes(
    document: &Document,
    set_index: usize,
    constraints: SuggestionConstraints,
    limits: SuggestionLimits,
) -> Vec<RouteSuggestion> {
    let limits = limits.bounded();
    let Some(from) = document.sets.get(set_index) else {
        return Vec::new();
    };
    if document.sets.get(set_index + 1).is_none() {
        return Vec::new();
    }
    let counts = SetCounts {
        moves: from.counts,
        hold: from.hold,
    };
    let (before, collision_pairs, stride_ids, turn_ids, late_ids) = diagnose(
        document,
        set_index,
        &from.routes,
        counts,
        constraints,
        limits,
        &[],
    );
    let mut candidates = Vec::with_capacity(limits.max_candidates);

    // Count expansion is the least surprising stride fix and preserves paths.
    if constraints.allow_count_change && !stride_ids.is_empty() && from.counts > 0 {
        let required = required_move_counts(document, set_index, &from.routes, constraints)
            .min(transition::MAX_SET_COUNTS) as u16;
        if required > from.counts {
            push_candidate(
                &mut candidates,
                document,
                set_index,
                constraints,
                limits,
                before,
                SetCounts {
                    moves: required,
                    hold: from.hold,
                },
                from.routes.clone(),
                vec![SuggestionReason::ReduceStride],
                stride_ids.clone(),
            );
        }
    }

    // Detour one member of each colliding pair. Alternating signs are stable and
    // make symmetric formations deterministic across runs and platforms.
    for (ordinal, &(a, b)) in collision_pairs.iter().enumerate() {
        if candidates.len() >= limits.max_candidates {
            break;
        }
        let id = if ordinal % 2 == 0 { b } else { a };
        for sign in [1.0_f32, -1.0] {
            if candidates.len() >= limits.max_candidates {
                break;
            }
            let mut routes = from.routes.clone();
            let route = routes.route_for(id).clone();
            let mut rewritten = route;
            rewritten.shape = RouteShape::Curve {
                control: ChordPoint {
                    along: 0.5,
                    lateral: sign * (0.12 + ordinal.min(4) as f32 * 0.025),
                },
            };
            routes.overrides.insert(id, rewritten);
            push_candidate(
                &mut candidates,
                document,
                set_index,
                constraints,
                limits,
                before,
                counts,
                routes,
                vec![SuggestionReason::AvoidCollision],
                vec![id],
            );
        }
    }

    // Aim the final curve tangent along the following move, reducing the turn
    // at the set while retaining both authored endpoints.
    for id in turn_ids
        .into_iter()
        .take(limits.max_candidates.saturating_sub(candidates.len()))
    {
        let mut routes = from.routes.clone();
        let mut route = routes.route_for(id).clone();
        let Some(index) = document.performers.iter().position(|p| p.id == id) else {
            continue;
        };
        let Some(next) = document
            .sets
            .get(set_index + 2)
            .and_then(|s| s.positions.get(index))
        else {
            continue;
        };
        let start = from.positions[index];
        let end = document.sets[set_index + 1].positions[index];
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        let chord_sq = dx * dx + dy * dy;
        let nx = next.x - end.x;
        let ny = next.y - end.y;
        let next_len = (nx * nx + ny * ny).sqrt();
        if chord_sq <= f32::EPSILON || next_len <= f32::EPSILON {
            continue;
        }
        let chord_len = chord_sq.sqrt();
        let control_world = crate::Point {
            x: end.x - nx / next_len * chord_len * 0.35,
            y: end.y - ny / next_len * chord_len * 0.35,
        };
        let px = control_world.x - start.x;
        let py = control_world.y - start.y;
        route.shape = RouteShape::Curve {
            control: ChordPoint {
                along: (px * dx + py * dy) / chord_sq,
                lateral: (-px * dy + py * dx) / chord_sq,
            },
        };
        route.easing = transition::Easing::Smooth;
        routes.overrides.insert(id, route);
        push_candidate(
            &mut candidates,
            document,
            set_index,
            constraints,
            limits,
            before,
            counts,
            routes,
            vec![SuggestionReason::SoftenArrivalTurn],
            vec![id],
        );
    }

    for id in late_ids
        .into_iter()
        .take(limits.max_candidates.saturating_sub(candidates.len()))
    {
        let mut routes = from.routes.clone();
        let mut route = routes.route_for(id).clone();
        route.gate.arrive = constraints.latest_arrival_count;
        routes.overrides.insert(id, route);
        push_candidate(
            &mut candidates,
            document,
            set_index,
            constraints,
            limits,
            before,
            counts,
            routes,
            vec![SuggestionReason::MeetArrival],
            vec![id],
        );
    }

    candidates.retain(|candidate| candidate.improvement() > 0);
    candidates.sort_by(|a, b| {
        b.improvement()
            .cmp(&a.improvement())
            .then_with(|| a.after.penalty().cmp(&b.after.penalty()))
            .then_with(|| a.affected_performers.cmp(&b.affected_performers))
    });
    candidates.truncate(limits.max_candidates);
    candidates
}

#[allow(clippy::too_many_arguments)]
fn push_candidate(
    out: &mut Vec<RouteSuggestion>,
    document: &Document,
    set_index: usize,
    constraints: SuggestionConstraints,
    limits: SuggestionLimits,
    before: SuggestionScore,
    counts: SetCounts,
    routes: RouteTable,
    reasons: Vec<SuggestionReason>,
    affected_performers: Vec<PerformerId>,
) {
    let focus = if reasons.contains(&SuggestionReason::AvoidCollision) {
        affected_performers.as_slice()
    } else {
        &[]
    };
    let (after, ..) = diagnose(
        document,
        set_index,
        &routes,
        counts,
        constraints,
        limits,
        focus,
    );
    let source = &document.sets[set_index];
    out.push(RouteSuggestion {
        counts,
        routes,
        reasons,
        affected_performers,
        before,
        after,
        source_counts: SetCounts {
            moves: source.counts,
            hold: source.hold,
        },
        source_routes: source.routes.clone(),
    });
}

type Diagnostics = (
    SuggestionScore,
    Vec<(PerformerId, PerformerId)>,
    Vec<PerformerId>,
    Vec<PerformerId>,
    Vec<PerformerId>,
);

fn diagnose(
    document: &Document,
    set_index: usize,
    routes: &RouteTable,
    counts: SetCounts,
    c: SuggestionConstraints,
    limits: SuggestionLimits,
    collision_focus: &[PerformerId],
) -> Diagnostics {
    let from = &document.sets[set_index];
    let to = &document.sets[set_index + 1];
    // Collision geometry is invariant under count-only changes, so the broad
    // phase can inspect the source directly without cloning it.
    let mut scratch = ScanScratch::default();
    let report = clinic::scan_transition(
        document,
        set_index,
        ClinicParams {
            style: StepStyle::Custom {
                units_per_step: c.max_step_per_count.max(0.01),
            },
            collision_radius: c.collision_distance.max(0.01),
            danger_radius: c.collision_distance.max(0.01),
            crowded_radius: c.collision_distance.max(0.01),
            max_events: limits.max_diagnostics,
            report_at_least: clinic::Severity::Crowded,
            ..ClinicParams::default()
        },
        &mut scratch,
    );
    let mut broad_pairs = report
        .collisions
        .iter()
        .map(|e| (e.a, e.b))
        .collect::<Vec<_>>();
    // Curving one lane can introduce a new conflict. Inspect that lane against
    // all performers, bounded by `max_diagnostics`, instead of rescanning N².
    let mut seen = broad_pairs.iter().copied().collect::<BTreeSet<_>>();
    'focus: for &id in collision_focus {
        for performer in &document.performers {
            if performer.id == id {
                continue;
            }
            let pair = (id.min(performer.id), id.max(performer.id));
            if seen.insert(pair) {
                broad_pairs.push(pair);
                if broad_pairs.len() >= limits.max_diagnostics {
                    break 'focus;
                }
            }
        }
    }
    let mut collision_pairs = Vec::new();
    for &(a, b) in &broad_pairs {
        let Some(ai) = document.performers.iter().position(|p| p.id == a) else {
            continue;
        };
        let Some(bi) = document.performers.iter().position(|p| p.id == b) else {
            continue;
        };
        let mut closest = f32::INFINITY;
        for sample in 0..=limits.samples {
            let count = f32::from(counts.moves) * f32::from(sample) / f32::from(limits.samples);
            let pa = transition::evaluate(
                routes.route_for(a),
                from.positions[ai],
                to.positions[ai],
                count,
                counts.moves,
            );
            let pb = transition::evaluate(
                routes.route_for(b),
                from.positions[bi],
                to.positions[bi],
                count,
                counts.moves,
            );
            closest = closest.min(((pa.x - pb.x).powi(2) + (pa.y - pb.y).powi(2)).sqrt());
        }
        if closest < c.collision_distance {
            collision_pairs.push((a, b));
        }
    }
    let mut stride_ids = Vec::new();
    let mut turn_ids = Vec::new();
    let mut late_ids = Vec::new();
    for (index, performer) in document
        .performers
        .iter()
        .enumerate()
        .take(from.positions.len().min(to.positions.len()))
    {
        let route = routes.route_for(performer.id);
        let (depart, arrive) = route.gate.resolve(f32::from(counts.moves));
        let active = (arrive - depart).max(f32::EPSILON);
        if transition::route_length(route, from.positions[index], to.positions[index]) / active
            > c.max_step_per_count
        {
            stride_ids.push(performer.id);
        }
        if c.latest_arrival_count.is_some_and(|latest| arrive > latest) {
            late_ids.push(performer.id);
        }
        if let Some(next) = document
            .sets
            .get(set_index + 2)
            .and_then(|s| s.positions.get(index))
        {
            let before = transition::evaluate(
                route,
                from.positions[index],
                to.positions[index],
                (arrive - 0.25).max(depart),
                counts.moves,
            );
            if transition::arrival_turn_angle(before, to.positions[index], *next)
                .is_some_and(|angle| angle > c.max_arrival_turn_radians)
            {
                turn_ids.push(performer.id);
            }
        }
    }
    let score = SuggestionScore {
        collisions: collision_pairs.len().min(u16::MAX as usize) as u16,
        excessive_strides: stride_ids.len().min(u16::MAX as usize) as u16,
        sharp_turns: turn_ids.len().min(u16::MAX as usize) as u16,
        late_arrivals: late_ids.len().min(u16::MAX as usize) as u16,
    };
    (score, collision_pairs, stride_ids, turn_ids, late_ids)
}

fn required_move_counts(
    document: &Document,
    set_index: usize,
    routes: &RouteTable,
    c: SuggestionConstraints,
) -> u32 {
    let from = &document.sets[set_index];
    let to = &document.sets[set_index + 1];
    from.positions
        .iter()
        .zip(&to.positions)
        .zip(&document.performers)
        .map(|((&a, &b), p)| {
            (transition::route_length(routes.route_for(p.id), a, b)
                / c.max_step_per_count.max(0.01))
            .ceil() as u32
        })
        .max()
        .unwrap_or(u32::from(from.counts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Point;

    #[test]
    fn suggestions_are_deterministic_bounded_and_pure() {
        let mut doc = Document::demo(1, 2);
        doc.sets[0].positions = vec![Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 0.0 }];
        doc.sets[1].positions = vec![Point { x: 10.0, y: 0.0 }, Point { x: 0.0, y: 0.0 }];
        let original = doc.clone();
        let limits = SuggestionLimits {
            max_candidates: 3,
            ..Default::default()
        };
        let a = suggest_routes(&doc, 0, SuggestionConstraints::default(), limits);
        let b = suggest_routes(&doc, 0, SuggestionConstraints::default(), limits);
        assert_eq!(a, b);
        assert!(a.len() <= 3);
        assert_eq!(doc, original);
        assert!(a.iter().all(|s| s.improvement() > 0));
    }

    #[test]
    fn stride_candidate_applies_as_one_undoable_edit() {
        let mut doc = Document::demo(1, 1);
        doc.sets[0].counts = 4;
        doc.sets[0].positions[0] = Point { x: 0.0, y: 0.0 };
        doc.sets[1].positions[0] = Point { x: 20.0, y: 0.0 };
        let source = doc.clone();
        let suggestion = suggest_routes(
            &doc,
            0,
            SuggestionConstraints::default(),
            SuggestionLimits::default(),
        )
        .into_iter()
        .find(|s| s.reasons.contains(&SuggestionReason::ReduceStride))
        .unwrap();
        let inverse = suggestion
            .to_edit(&doc, 0)
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        assert!(doc.sets[0].counts >= 20);
        inverse.apply(&mut doc).unwrap();
        assert_eq!(doc, source);
    }

    #[test]
    fn invalid_transition_has_no_candidates() {
        let doc = Document::demo(0, 4);
        assert!(suggest_routes(&doc, 99, Default::default(), Default::default()).is_empty());
    }

    #[test]
    fn sharp_arrival_gets_endpoint_preserving_curve() {
        let mut doc = Document::demo(2, 1);
        doc.sets[0].positions[0] = Point { x: 0.0, y: 0.0 };
        doc.sets[1].positions[0] = Point { x: 10.0, y: 0.0 };
        let mut third = doc.sets[1].clone();
        third.id = crate::SetId::new(3).unwrap();
        doc.sets.push(third);
        doc.sets[2].positions[0] = Point { x: 10.0, y: 10.0 };
        let constraints = SuggestionConstraints {
            max_step_per_count: 100.0,
            max_arrival_turn_radians: 30.0_f32.to_radians(),
            ..Default::default()
        };
        let suggestions = suggest_routes(&doc, 0, constraints, Default::default());
        let candidate = suggestions
            .iter()
            .find(|s| s.reasons.contains(&SuggestionReason::SoftenArrivalTurn))
            .expect("turn rewrite");
        assert_eq!(candidate.before.sharp_turns, 1);
        assert_eq!(candidate.after.sharp_turns, 0);
        let route = candidate.routes.route_for(doc.performers[0].id);
        assert_eq!(
            transition::evaluate(
                route,
                doc.sets[0].positions[0],
                doc.sets[1].positions[0],
                0.0,
                candidate.counts.moves
            ),
            doc.sets[0].positions[0]
        );
        assert_eq!(
            transition::evaluate(
                route,
                doc.sets[0].positions[0],
                doc.sets[1].positions[0],
                f32::from(candidate.counts.moves),
                candidate.counts.moves
            ),
            doc.sets[1].positions[0]
        );
    }

    #[test]
    fn stale_suggestion_refuses_to_build_edit() {
        let mut doc = Document::demo(1, 1);
        doc.sets[0].counts = 4;
        doc.sets[1].positions[0] = Point { x: 20.0, y: 0.0 };
        let candidate = suggest_routes(&doc, 0, Default::default(), Default::default())
            .into_iter()
            .next()
            .unwrap();
        doc.sets[0].counts = 5;
        assert!(candidate.to_edit(&doc, 0).is_none());
    }
}
