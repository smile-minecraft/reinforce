use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::content::{ContentBlock, Pin, QuestionCard};
use crate::primitives::{Error, GradingMode, QuestionType, Result};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NormalizedPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PinClick {
    pub no: u32,
    pub point: NormalizedPoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnswerSubmission {
    Keys { values: Vec<String> },
    Text { value: String },
    PinClicks { clicks: Vec<PinClick> },
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GradingConfig {
    pub pin_radius: f64,
    /// Image width divided by height. `None` treats normalized axes as equal.
    pub pin_aspect_ratio: Option<f64>,
}

impl Default for GradingConfig {
    fn default() -> Self {
        Self {
            pin_radius: 0.05,
            pin_aspect_ratio: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradingOutcome {
    pub is_correct: bool,
    pub scoreable: bool,
    pub voided: bool,
    pub affects_scheduling: bool,
    pub note: String,
}

/// Case-folds, trims and maps full-width ASCII and the ideographic space.
pub fn normalize_term(value: &str) -> String {
    let mut mapped = String::with_capacity(value.len());
    for character in value.chars() {
        let code = u32::from(character);
        if (0xFF01..=0xFF5E).contains(&code) {
            mapped.push(char::from_u32(code - 0xFEE0).unwrap_or(character));
        } else if character == '\u{3000}' {
            mapped.push(' ');
        } else {
            mapped.push(character);
        }
    }
    mapped
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn validate_config(config: &GradingConfig) -> Result<()> {
    if !config.pin_radius.is_finite() || !(0.0..=1.0).contains(&config.pin_radius) {
        return Err(Error::InvalidValue(
            "pin radius",
            config.pin_radius.to_string(),
        ));
    }
    if let Some(aspect) = config.pin_aspect_ratio {
        if !aspect.is_finite() || aspect <= 0.0 {
            return Err(Error::InvalidValue("pin aspect ratio", aspect.to_string()));
        }
    }
    Ok(())
}

fn canonical(values: &[String]) -> Vec<String> {
    let mut result: Vec<String> = values
        .iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect();
    result.sort();
    result
}

fn outcome(correct: bool, voided: bool, affects: bool, note: &str) -> GradingOutcome {
    GradingOutcome {
        is_correct: correct,
        scoreable: !voided,
        voided,
        affects_scheduling: affects && !voided,
        note: note.to_owned(),
    }
}

fn values(submission: &AnswerSubmission) -> Vec<String> {
    match submission {
        AnswerSubmission::Keys { values } => values.clone(),
        AnswerSubmission::Text { value } => vec![value.clone()],
        AnswerSubmission::PinClicks { .. } | AnswerSubmission::TimedOut => Vec::new(),
    }
}

fn grade_keys(
    question: &QuestionCard,
    submission: &AnswerSubmission,
    affects: bool,
) -> GradingOutcome {
    let accepted: Vec<Vec<String>> = match question.grading_mode {
        GradingMode::AnyAccepted => {
            let mut groups = vec![question.answer.clone()];
            groups.extend(question.accepted_answers.iter().cloned());
            groups
        }
        _ => vec![question.answer.clone()],
    };
    let submitted = canonical(&values(submission));
    if submitted.is_empty() {
        return outcome(false, false, affects, "no submitted values");
    }
    if accepted.iter().any(|group| canonical(group) == submitted) {
        outcome(true, false, affects, "answer combination accepted")
    } else {
        outcome(false, false, affects, "answer combination rejected")
    }
}

/// Free-text answers compare after `normalize_term`, so case, surrounding
/// whitespace and full-width forms do not turn a correct answer into a miss.
fn grade_text(question: &QuestionCard, value: &str, affects: bool) -> GradingOutcome {
    let submitted = normalize_term(value);
    if submitted.is_empty() {
        return outcome(false, false, affects, "no submitted values");
    }
    let mut accepted = vec![question.answer.clone()];
    if question.grading_mode == GradingMode::AnyAccepted {
        accepted.extend(question.accepted_answers.iter().cloned());
    }
    let matched = accepted.iter().any(|group| {
        group
            .iter()
            .any(|candidate| normalize_term(candidate) == submitted)
    });
    if matched {
        outcome(true, false, affects, "text answer accepted")
    } else {
        outcome(false, false, affects, "text answer rejected")
    }
}

fn pins(question: &QuestionCard) -> Vec<&Pin> {
    question
        .stem_blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::InteractivePins { pins, .. } => Some(pins),
            _ => None,
        })
        .flat_map(|pins| pins.iter())
        .collect()
}

fn normalized(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn grade_pin_locate(
    question: &QuestionCard,
    submission: &AnswerSubmission,
    config: &GradingConfig,
    affects: bool,
) -> Result<GradingOutcome> {
    let AnswerSubmission::PinClicks { clicks } = submission else {
        return Ok(outcome(false, false, affects, "pin click required"));
    };
    let all_pins = pins(question);
    let expected: Vec<u32> = if question.answer.is_empty() {
        all_pins.iter().map(|pin| pin.no).collect()
    } else {
        question
            .answer
            .iter()
            .map(|answer| {
                answer
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| Error::InvalidValue("pin answer", answer.clone()))
            })
            .collect::<Result<Vec<_>>>()?
    };
    if expected.is_empty() {
        return Err(Error::InvalidValue(
            "pin answer",
            "no pins to grade".to_owned(),
        ));
    }

    let aspect = config.pin_aspect_ratio.unwrap_or(1.0);
    for number in expected {
        let Some(pin) = all_pins.iter().find(|pin| pin.no == number) else {
            return Err(Error::InvalidValue(
                "pin answer",
                format!("pin {number} is missing from the stem"),
            ));
        };
        let click = clicks.iter().find(|click| click.no == number);
        let hit = click.is_some_and(|click| {
            normalized(click.point.x) && normalized(click.point.y) && {
                let dx = (click.point.x - pin.x) * aspect;
                let dy = click.point.y - pin.y;
                dx.hypot(dy) <= config.pin_radius
            }
        });
        if !hit {
            return Ok(outcome(
                false,
                false,
                affects,
                &format!("pin {number} missed"),
            ));
        }
    }
    Ok(outcome(true, false, affects, "all pins hit"))
}

fn grade_pin_identify(
    question: &QuestionCard,
    submission: &AnswerSubmission,
    affects: bool,
) -> Result<GradingOutcome> {
    let all_pins = pins(question);
    let mut expected_groups: Vec<Vec<String>> = Vec::new();
    for answer in &question.answer {
        if let Ok(number) = answer.trim().parse::<u32>() {
            let pin = all_pins
                .iter()
                .find(|pin| pin.no == number)
                .ok_or_else(|| {
                    Error::InvalidValue(
                        "pin answer",
                        format!("pin {number} is missing from the stem"),
                    )
                })?;
            let mut terms = vec![pin.term_primary.clone()];
            terms.extend(pin.term_secondary.iter().cloned());
            expected_groups.push(terms);
        } else {
            expected_groups.push(vec![answer.clone()]);
        }
    }
    if expected_groups.is_empty() {
        return Err(Error::InvalidValue(
            "pin answer",
            "no terms to grade".to_owned(),
        ));
    }

    let submitted: Vec<String> = values(submission)
        .iter()
        .map(|value| normalize_term(value))
        .collect();
    for terms in expected_groups {
        let matched = terms
            .iter()
            .any(|term| submitted.iter().any(|value| value == &normalize_term(term)));
        if !matched {
            return Ok(outcome(false, false, affects, "pin term mismatch"));
        }
    }
    Ok(outcome(true, false, affects, "all pin terms matched"))
}

pub fn grade_question(
    question: &QuestionCard,
    submission: &AnswerSubmission,
    config: &GradingConfig,
    affects_scheduling: bool,
) -> Result<GradingOutcome> {
    validate_config(config)?;
    if question.grading_mode == GradingMode::Voided {
        return Ok(outcome(true, true, affects_scheduling, "voided question"));
    }
    match question.question_type {
        QuestionType::PinLocate => {
            grade_pin_locate(question, submission, config, affects_scheduling)
        }
        QuestionType::PinIdentify => grade_pin_identify(question, submission, affects_scheduling),
        QuestionType::ShortAnswer | QuestionType::Vocab | QuestionType::ImageQa => match submission
        {
            AnswerSubmission::Text { value } => Ok(grade_text(question, value, affects_scheduling)),
            _ => Ok(grade_keys(question, submission, affects_scheduling)),
        },
        _ => Ok(grade_keys(question, submission, affects_scheduling)),
    }
}

/// True when a submission matches the canonical answer or any accepted
/// alternative, independent of ordering.
pub fn matches_any_accepted(submitted: &[String], accepted: &[Vec<String>]) -> bool {
    let submitted = canonical(submitted);
    accepted.iter().any(|group| canonical(group) == submitted)
}

/// Option keys from a wrong submission that are not part of the canonical answer.
pub fn unexpected_keys(submitted: &[String], answer: &[String]) -> Vec<String> {
    let answer: BTreeSet<&str> = answer.iter().map(String::as_str).collect();
    let mut wrong: Vec<String> = submitted
        .iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !answer.contains(value.as_str()))
        .collect();
    wrong.sort();
    wrong.dedup();
    wrong
}

pub fn voided_marker() -> Value {
    serde_json::json!({ "voided": true, "grading_mode": "voided" })
}
