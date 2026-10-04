//! Rules of the automatic checks, kept apart from the network and the
//! database so they can be tested on their own.

/// Consecutive failed checks before a service shows an outage.
pub const FAILURES_BEFORE_OUTAGE: u8 = 3;
/// Rounds in a row skipped as Statup's own failure before they count again.
pub const MAX_SKIPPED_ROUNDS: u8 = 5;
/// Fewest services falling together that make a round look like Statup's
/// own failure.
const MIN_SIMULTANEOUS_FALLS: usize = 3;

/// What one check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Answered,
    Failed,
}

/// What a round asks of a service's outage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    Stay,
    Down,
    Up,
}

/// One watched service in a round: what it had before, and what the check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observation {
    pub failures: u8,
    pub detected: bool,
    pub outcome: Outcome,
}

/// Returns the new failure count and the transition for one service.
///
/// While `silenced` (maintenance), failures are counted but never raise an
/// outage, so a service still failing afterwards falls at once. An answer
/// clears a detected outage even during a maintenance.
pub fn next_state(observation: Observation, silenced: bool) -> (u8, Transition) {
    if observation.outcome == Outcome::Answered {
        let transition = if observation.detected {
            Transition::Up
        } else {
            Transition::Stay
        };
        return (0, transition);
    }
    let failures = observation.failures.saturating_add(1);
    let falls = !observation.detected && !silenced && failures >= FAILURES_BEFORE_OUTAGE;
    let transition = if falls {
        Transition::Down
    } else {
        Transition::Stay
    };
    (failures, transition)
}

/// Tells whether enough healthy services fell at once to blame Statup's own
/// connection.
///
/// Services already failing or in outage are ignored, so a new fall beside
/// outages that are already shown is not mistaken for Statup's own failure.
pub fn looks_like_own_failure(observations: &[Observation]) -> bool {
    let answering_before = || {
        observations
            .iter()
            .filter(|observation| observation.failures == 0 && !observation.detected)
    };
    answering_before().count() >= MIN_SIMULTANEOUS_FALLS
        && answering_before().all(|observation| observation.outcome == Outcome::Failed)
}

/// Returns whether to ignore this round and the new count of rounds ignored in a row.
///
/// The cap lets a real outage of the whole site show up after a few rounds.
pub fn skip_round(own_failure: bool, skipped: u8) -> (bool, u8) {
    if own_failure && skipped < MAX_SKIPPED_ROUNDS {
        return (true, skipped + 1);
    }
    (false, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(failures: u8, detected: bool, outcome: Outcome) -> Observation {
        Observation {
            failures,
            detected,
            outcome,
        }
    }

    #[test]
    fn outage_needs_three_failures_in_a_row() {
        assert_eq!(
            next_state(obs(0, false, Outcome::Failed), false),
            (1, Transition::Stay)
        );
        assert_eq!(
            next_state(obs(1, false, Outcome::Failed), false),
            (2, Transition::Stay)
        );
        assert_eq!(
            next_state(obs(2, false, Outcome::Failed), false),
            (3, Transition::Down)
        );
    }

    #[test]
    fn service_already_in_outage_does_not_fall_twice() {
        assert_eq!(
            next_state(obs(3, true, Outcome::Failed), false),
            (4, Transition::Stay)
        );
    }

    #[test]
    fn answer_clears_the_count_and_lifts_a_detected_outage() {
        assert_eq!(
            next_state(obs(5, true, Outcome::Answered), false),
            (0, Transition::Up)
        );
        assert_eq!(
            next_state(obs(2, false, Outcome::Answered), false),
            (0, Transition::Stay)
        );
    }

    #[test]
    fn maintenance_counts_failures_without_falling() {
        let mut failures = 0;
        for _ in 0..3 {
            let (next, transition) = next_state(obs(failures, false, Outcome::Failed), true);
            assert_eq!(transition, Transition::Stay);
            failures = next;
        }
        assert_eq!(failures, 3);
        assert_eq!(
            next_state(obs(failures, false, Outcome::Failed), false),
            (4, Transition::Down)
        );
    }

    #[test]
    fn answer_during_maintenance_lifts_a_detected_outage() {
        assert_eq!(
            next_state(obs(3, true, Outcome::Answered), true),
            (0, Transition::Up)
        );
    }

    #[test]
    fn failure_count_saturates() {
        assert_eq!(
            next_state(obs(u8::MAX, true, Outcome::Failed), false),
            (u8::MAX, Transition::Stay)
        );
    }

    #[test]
    fn three_healthy_services_falling_together_look_like_own_failure() {
        let round = [
            obs(0, false, Outcome::Failed),
            obs(0, false, Outcome::Failed),
            obs(0, false, Outcome::Failed),
            obs(4, true, Outcome::Failed),
        ];
        assert!(looks_like_own_failure(&round));
    }

    #[test]
    fn one_service_answering_clears_the_suspicion() {
        let round = [
            obs(0, false, Outcome::Failed),
            obs(0, false, Outcome::Failed),
            obs(0, false, Outcome::Answered),
        ];
        assert!(!looks_like_own_failure(&round));
    }

    #[test]
    fn two_services_falling_are_not_enough() {
        let round = [
            obs(0, false, Outcome::Failed),
            obs(0, false, Outcome::Failed),
            obs(4, true, Outcome::Failed),
            obs(5, true, Outcome::Failed),
        ];
        assert!(!looks_like_own_failure(&round));
    }

    #[test]
    fn service_already_failing_is_not_counted_as_answering_before() {
        let round = [
            obs(0, false, Outcome::Failed),
            obs(0, false, Outcome::Failed),
            obs(1, false, Outcome::Failed),
        ];
        assert!(!looks_like_own_failure(&round));
    }

    #[test]
    fn skipping_stops_after_the_cap() {
        let mut skipped = 0;
        for expected in 1..=MAX_SKIPPED_ROUNDS {
            assert_eq!(skip_round(true, skipped), (true, expected));
            skipped = expected;
        }
        assert_eq!(skip_round(true, skipped), (false, 0));
        assert_eq!(skip_round(false, 3), (false, 0));
    }
}
