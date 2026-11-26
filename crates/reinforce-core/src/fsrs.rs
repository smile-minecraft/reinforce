use crate::primitives::{
    AttemptDirection, CardId, Confidence, Error, Result, SchedulingState, UserId,
};
use crate::study::{AttemptLog, CardScheduling, is_voided_attempt};
use crate::timestamp::Timestamp;

const W: [f64; 19] = [
    0.40255, 1.18385, 3.173, 15.69105, 7.1949, 0.5345, 1.4604, 0.0046, 1.54575, 0.1192, 1.01925,
    1.9395, 0.11, 0.29605, 2.2698, 0.2315, 2.9898, 0.51655, 0.6621,
];

const DECAY: f64 = -0.5;
const FACTOR: f64 = 19.0 / 81.0;
const S_MIN: f64 = 0.01;
const S_MAX: f64 = 36_500.0;
const D_MIN: f64 = 1.0;
const D_MAX: f64 = 10.0;

fn clamp(value: f64, minimum: f64, maximum: f64) -> f64 {
    value.clamp(minimum, maximum)
}

fn initial_difficulty(grade: u32) -> f64 {
    W[4] - (W[5] * f64::from(grade - 1)).exp() + 1.0
}

fn retrievability(days: f64, stability: f64) -> f64 {
    (1.0 + FACTOR * days.max(0.0) / stability).powf(DECAY)
}

fn interval_days(stability: f64, desired_retention: f64) -> f64 {
    let target = clamp(desired_retention, 0.001, 0.9999);
    (stability / FACTOR) * (target.powf(1.0 / DECAY) - 1.0)
}

pub fn rating_for_attempt(attempt: &AttemptLog) -> u32 {
    match (attempt.is_correct, attempt.confidence) {
        (false, _) => 1,
        (true, Confidence::Guessed) => 2,
        (true, Confidence::Split) => 3,
        (true, Confidence::Certain) => 4,
    }
}

fn next_state(grade: u32, interval: f64, previous: SchedulingState) -> SchedulingState {
    let was_mature = matches!(
        previous,
        SchedulingState::Review | SchedulingState::Relearning
    );
    if grade == 1 || interval < 1.0 {
        if was_mature {
            SchedulingState::Relearning
        } else {
            SchedulingState::Learning
        }
    } else {
        SchedulingState::Review
    }
}

/// Replays attempts in chronological order and returns the derived FSRS-5 state.
/// No randomness, wall-clock reads or external state are used, so the same
/// attempts always produce the same scheduling row.
pub fn recompute_scheduling(
    user_id: UserId,
    card_id: CardId,
    direction: AttemptDirection,
    attempts: &[AttemptLog],
    desired_retention: f64,
) -> Result<CardScheduling> {
    if !(0.0..=1.0).contains(&desired_retention)
        || desired_retention == 0.0
        || desired_retention == 1.0
    {
        return Err(Error::InvalidValue(
            "desired retention",
            desired_retention.to_string(),
        ));
    }

    let mut ordered: Vec<&AttemptLog> = attempts.iter().collect();
    ordered.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.as_str().cmp(&right.id.as_str()))
    });

    let mut stability = 0.0_f64;
    let mut difficulty = 0.0_f64;
    let mut reps = 0_u32;
    let mut lapses = 0_u32;
    let mut state = SchedulingState::New;
    let mut last_review_at: Option<Timestamp> = None;
    let mut due_at: Option<Timestamp> = None;

    for attempt in ordered {
        if attempt.user_id != user_id
            || attempt.card_id != card_id
            || attempt.direction != direction
        {
            return Err(Error::InvalidValue(
                "attempt batch",
                "all attempts must belong to the same user, card and direction".to_owned(),
            ));
        }

        if !attempt.affects_scheduling || is_voided_attempt(attempt) {
            continue;
        }

        let grade = rating_for_attempt(attempt);
        if reps == 0 {
            stability = W[(grade - 1) as usize];
            difficulty = clamp(initial_difficulty(grade), D_MIN, D_MAX);
        } else {
            let previous_review = last_review_at.expect("a review implies a previous timestamp");
            let elapsed = (attempt.created_at.days_since(&previous_review)).max(0.0);
            let recall = retrievability(elapsed, stability);

            if elapsed < 1e-9 {
                stability *= (W[17] * (f64::from(grade) - 3.0 + W[18])).exp();
            } else if grade == 1 {
                let post_lapse = W[11]
                    * difficulty.powf(-W[12])
                    * ((stability + 1.0).powf(W[13]) - 1.0)
                    * (W[14] * (1.0 - recall)).exp();
                stability = stability.min(post_lapse);
            } else {
                let hard_multiplier = if grade == 2 { W[15] } else { 1.0 };
                let easy_multiplier = if grade == 4 { W[16] } else { 1.0 };
                stability *= 1.0
                    + W[8].exp()
                        * (11.0 - difficulty)
                        * stability.powf(-W[9])
                        * ((W[10] * (1.0 - recall)).exp() - 1.0)
                        * hard_multiplier
                        * easy_multiplier;
            }

            stability = clamp(stability, S_MIN, S_MAX);
            let delta = -W[6] * f64::from(grade as i32 - 3) * (10.0 - difficulty) / 9.0;
            let reverted = W[7] * initial_difficulty(4) + (1.0 - W[7]) * (difficulty + delta);
            difficulty = clamp(reverted, D_MIN, D_MAX);
        }

        reps += 1;
        if grade == 1 {
            lapses += 1;
        }
        let interval = interval_days(stability, desired_retention).max(1.0 / 86_400.0);
        state = next_state(grade, interval, state);
        last_review_at = Some(attempt.created_at);
        due_at = Some(attempt.created_at.plus_days(interval));
    }

    Ok(CardScheduling {
        user_id,
        card_id,
        direction,
        stability,
        difficulty,
        reps,
        lapses,
        state,
        last_review_at,
        due_at,
        desired_retention,
        updated_at: last_review_at.unwrap_or_else(Timestamp::unix_epoch),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{AttemptId, SessionMode};
    use crate::study::AttemptLog;
    use serde_json::json;

    fn attempt(seconds: i64, correct: bool, confidence: Confidence) -> AttemptLog {
        let created_at = Timestamp::unix_epoch().plus_days(seconds as f64 / 86_400.0);
        AttemptLog {
            id: AttemptId::generate(),
            user_id: UserId::generate(),
            card_id: CardId::generate(),
            direction: AttemptDirection::Default,
            session_mode: SessionMode::DailyDue,
            submitted_payload: json!({}),
            is_correct: correct,
            duration_ms: 1_000,
            confidence,
            error_tags: vec![],
            created_at,
            sync_cursor: seconds,
            affects_scheduling: true,
        }
    }

    #[test]
    fn ratings_are_derived_from_answer_and_confidence() {
        assert_eq!(
            rating_for_attempt(&attempt(0, false, Confidence::Certain)),
            1
        );
        assert_eq!(
            rating_for_attempt(&attempt(0, true, Confidence::Guessed)),
            2
        );
        assert_eq!(rating_for_attempt(&attempt(0, true, Confidence::Split)), 3);
        assert_eq!(
            rating_for_attempt(&attempt(0, true, Confidence::Certain)),
            4
        );
    }

    #[test]
    fn review_interval_grows_with_successful_reviews() {
        let user_id = UserId::generate();
        let card_id = CardId::generate();
        let logs: Vec<AttemptLog> = vec![
            attempt(0, true, Confidence::Split),
            attempt(3 * 86_400, true, Confidence::Split),
        ]
        .into_iter()
        .map(|mut log| {
            log.user_id = user_id.clone();
            log.card_id = card_id.clone();
            log
        })
        .collect();
        let first = recompute_scheduling(
            user_id.clone(),
            card_id.clone(),
            AttemptDirection::Default,
            &logs[..1],
            0.9,
        )
        .unwrap();
        let second =
            recompute_scheduling(user_id, card_id, AttemptDirection::Default, &logs, 0.9).unwrap();
        assert!(second.stability > first.stability);
        assert_eq!(second.reps, 2);
    }

    #[test]
    fn a_lapse_increments_lapses_and_lowers_state() {
        let user_id = UserId::generate();
        let card_id = CardId::generate();
        let logs: Vec<AttemptLog> = vec![
            attempt(0, true, Confidence::Certain),
            attempt(10 * 86_400, false, Confidence::Certain),
        ]
        .into_iter()
        .map(|mut log| {
            log.user_id = user_id.clone();
            log.card_id = card_id.clone();
            log
        })
        .collect();
        let scheduling =
            recompute_scheduling(user_id, card_id, AttemptDirection::Default, &logs, 0.9).unwrap();
        assert_eq!(scheduling.lapses, 1);
        assert_eq!(scheduling.state, SchedulingState::Relearning);
    }
}
