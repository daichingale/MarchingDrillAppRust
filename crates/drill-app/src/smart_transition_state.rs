//! Background collision-aware assignment suggestions ("smart transition").
//!
//! The refinement in `drill_core::pathing` runs a swept clinic scan per
//! candidate swap, so it is far too heavy for the frame thread at ensemble
//! scale. It runs on the `AssignmentOptimize` job thread, is gated on the
//! document revision plus the set being inspected, and lands in the UI as a
//! *suggestion* -- nothing is written to the document until the designer
//! presses apply.

use drill_core::pathing::{self, CollisionAwareAssignment};
use drill_core::{Document, Revision, clinic};
use drill_jobs::{Job, JobErrorCode, JobFailure, JobKind, JobMsg};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Clinic thresholds shared with the inspector's live scan.
///
/// Both surfaces must agree: the "before" number in a suggestion summary sits
/// one line below the collision count the designer is already reading, and two
/// different radii there would read as a bug.
pub(crate) fn clinic_params() -> clinic::ClinicParams {
    clinic::ClinicParams {
        style: clinic::StepStyle::Custom {
            units_per_step: 1.0,
        },
        collision_radius: 0.75,
        danger_radius: 0.75,
        crowded_radius: 0.75,
        aggressive_above: 1.0,
        impossible_above: f32::MAX,
        ..clinic::ClinicParams::default()
    }
}

/// Identifies exactly which transition a suggestion belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SmartTransitionKey {
    pub revision: Revision,
    pub set_index: usize,
}

pub(crate) struct SmartTransitionSuggestion {
    pub key: SmartTransitionKey,
    pub outcome: CollisionAwareAssignment,
}

#[derive(Default)]
pub(crate) struct SmartTransitionState {
    job: Option<Job<SmartTransitionSuggestion>>,
    running: Option<SmartTransitionKey>,
    suggestion: Option<SmartTransitionSuggestion>,
    last_poll: Option<Instant>,
}

impl SmartTransitionState {
    /// Starts a refinement from an owned snapshot. Explicitly user-triggered
    /// rather than revision-driven: this is a suggestion the designer asks
    /// for, not an analysis that should follow every edit.
    pub(crate) fn request(&mut self, document: &Document, key: SmartTransitionKey) {
        if self.running.is_some() {
            return;
        }
        self.suggestion = None;
        let snapshot = document.clone();
        self.running = Some(key);
        self.job = Some(Job::spawn_typed(
            JobKind::AssignmentOptimize,
            move |progress| {
                if progress.is_cancelled() {
                    return Err(JobFailure::new(JobErrorCode::Cancelled));
                }
                let (Some(from), Some(to)) = (
                    snapshot.sets.get(key.set_index),
                    snapshot.sets.get(key.set_index + 1),
                ) else {
                    return Err(JobFailure::new(JobErrorCode::InvalidInput));
                };
                if from.positions.is_empty() || from.positions.len() != to.positions.len() {
                    return Err(JobFailure::new(JobErrorCode::InvalidInput));
                }
                progress.set(0.1);
                let outcome = pathing::optimal_assignment_collision_aware(
                    &from.positions,
                    &to.positions,
                    from.counts,
                    &snapshot.grid,
                    clinic_params(),
                );
                progress.set(1.0);
                Ok(SmartTransitionSuggestion { key, outcome })
            },
        ));
    }

    /// At most one lock-free channel poll every 50 ms. A result whose key no
    /// longer matches the document the designer is looking at is dropped: a
    /// stale permutation would silently re-seat the wrong transition.
    pub(crate) fn poll(&mut self, current: SmartTransitionKey) {
        if self
            .last_poll
            .is_some_and(|last| last.elapsed() < POLL_INTERVAL)
        {
            return;
        }
        self.last_poll = Some(Instant::now());
        let Some(message) = self.job.as_mut().and_then(Job::poll) else {
            return;
        };
        self.job = None;
        self.running = None;
        if let JobMsg::Done(suggestion) = message
            && suggestion.key == current
        {
            self.suggestion = Some(suggestion);
        }
    }

    pub(crate) fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// The pending suggestion, but only while it still describes `key`.
    pub(crate) fn suggestion(&self, key: SmartTransitionKey) -> Option<&SmartTransitionSuggestion> {
        self.suggestion
            .as_ref()
            .filter(|suggestion| suggestion.key == key)
    }

    /// Consumes the suggestion for `key`, leaving the state empty.
    pub(crate) fn take(&mut self, key: SmartTransitionKey) -> Option<SmartTransitionSuggestion> {
        self.suggestion(key)?;
        self.suggestion.take()
    }

    pub(crate) fn discard(&mut self) {
        self.suggestion = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(state: &mut SmartTransitionState, key: SmartTransitionKey) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while state.is_running() && Instant::now() < deadline {
            state.last_poll = None;
            let started = Instant::now();
            state.poll(key);
            assert!(
                started.elapsed() < Duration::from_millis(2),
                "poll must never block the frame thread"
            );
            std::thread::yield_now();
        }
        assert!(!state.is_running(), "job did not terminate");
    }

    fn key(revision: u64, set_index: usize) -> SmartTransitionKey {
        SmartTransitionKey {
            revision: Revision(revision),
            set_index,
        }
    }

    #[test]
    fn suggestion_is_produced_without_touching_the_source_document() {
        let document = Document::demo(4, 6);
        let before = document.clone();
        let mut state = SmartTransitionState::default();
        let current = key(0, 0);
        state.request(&document, current);
        wait(&mut state, current);
        let suggestion = state.suggestion(current).expect("suggestion arrived");
        assert_eq!(suggestion.outcome.assignment.len(), document.performers.len());
        assert_eq!(document, before);
    }

    #[test]
    fn a_result_for_a_superseded_revision_is_discarded() {
        let document = Document::demo(4, 6);
        let mut state = SmartTransitionState::default();
        state.request(&document, key(0, 0));
        wait(&mut state, key(1, 0));
        assert!(state.suggestion(key(0, 0)).is_none());
        assert!(state.suggestion(key(1, 0)).is_none());
    }

    #[test]
    fn take_only_releases_a_suggestion_matching_the_requested_key() {
        let document = Document::demo(3, 4);
        let mut state = SmartTransitionState::default();
        let current = key(0, 0);
        state.request(&document, current);
        wait(&mut state, current);
        assert!(state.take(key(0, 1)).is_none());
        assert!(state.suggestion(current).is_some());
        assert!(state.take(current).is_some());
        assert!(state.suggestion(current).is_none());
    }

    #[test]
    fn a_transition_without_a_following_set_fails_instead_of_suggesting() {
        let document = Document::demo(2, 3);
        let last = document.sets.len() - 1;
        let mut state = SmartTransitionState::default();
        let current = key(0, last);
        state.request(&document, current);
        wait(&mut state, current);
        assert!(state.suggestion(current).is_none());
    }
}
