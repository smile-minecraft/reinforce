use std::collections::HashSet;

use crate::content::QuestionCard;
use crate::primitives::{AnswerSourceKind, GradingMode, QuestionType, Result};
use crate::validation::{Snapshot, ValidationError, ValidationRule};

fn is_choice(question_type: QuestionType) -> bool {
    matches!(
        question_type,
        QuestionType::SingleChoice | QuestionType::MultipleChoice
    )
}

fn validate_card(
    card: &QuestionCard,
    errors: &mut Vec<ValidationError>,
    entry: &crate::validation::Staged<QuestionCard>,
) {
    if card.grading_mode == GradingMode::Voided {
        if !card.answer.is_empty() {
            errors.push(entry.error(
                ValidationRule::TypeGradingConsistency,
                "answer",
                "a voided question cannot carry an answer",
            ));
        }
        if !card.accepted_answers.is_empty() {
            errors.push(entry.error(
                ValidationRule::TypeGradingConsistency,
                "accepted_answers",
                "a voided question cannot carry accepted answers",
            ));
        }
        if card.answer_source_kind != AnswerSourceKind::Unanswered {
            errors.push(entry.error(
                ValidationRule::TypeGradingConsistency,
                "answer_source_kind",
                "a voided question must be marked unanswered",
            ));
        }
        if card.duplicate_question_id.is_some() {
            errors.push(entry.error(
                ValidationRule::TypeGradingConsistency,
                "duplicate_question_id",
                "a voided question cannot duplicate another question",
            ));
        }
        return;
    }

    if card.question_type == QuestionType::SingleChoice {
        match card.grading_mode {
            GradingMode::Exact if card.answer.len() != 1 => errors.push(entry.error(
                ValidationRule::TypeGradingConsistency,
                "answer",
                "single_choice with exact grading requires exactly one answer; use any_accepted for multi-key items",
            )),
            GradingMode::AnyAccepted if card.answer.is_empty() => errors.push(entry.error(
                ValidationRule::TypeGradingConsistency,
                "answer",
                "single_choice requires at least one accepted answer",
            )),
            _ => {}
        }
    }

    if card.grading_mode == GradingMode::Exact && !card.accepted_answers.is_empty() {
        errors.push(entry.error(
            ValidationRule::TypeGradingConsistency,
            "accepted_answers",
            "alternative answers require any_accepted grading",
        ));
    }

    for (index, accepted) in card.accepted_answers.iter().enumerate() {
        if accepted.is_empty() {
            errors.push(entry.error(
                ValidationRule::TypeGradingConsistency,
                format!("accepted_answers[{index}]"),
                "an accepted answer cannot be empty",
            ));
        }
        if card.question_type == QuestionType::SingleChoice && accepted.len() != 1 {
            errors.push(entry.error(
                ValidationRule::TypeGradingConsistency,
                format!("accepted_answers[{index}]"),
                "single_choice accepts exactly one key per combination",
            ));
        }
    }

    if is_choice(card.question_type) && card.options.is_empty() {
        errors.push(entry.error(
            ValidationRule::TypeGradingConsistency,
            "options",
            "choice questions must declare their options",
        ));
    }

    let mut keys = HashSet::new();
    for (index, option) in card.options.iter().enumerate() {
        if option.key.trim().is_empty() {
            errors.push(entry.error(
                ValidationRule::TypeGradingConsistency,
                format!("options[{index}].key"),
                "an option key cannot be empty",
            ));
        }
        if !keys.insert(option.key.as_str()) {
            errors.push(entry.error(
                ValidationRule::TypeGradingConsistency,
                format!("options[{index}].key"),
                format!("duplicate option key {}", option.key),
            ));
        }
    }

    if keys.is_empty() {
        return;
    }

    for (index, answer) in card.answer.iter().enumerate() {
        if !keys.contains(answer.as_str()) {
            errors.push(entry.error(
                ValidationRule::TypeGradingConsistency,
                format!("answer[{index}]"),
                format!("answer key {answer} is not declared in options"),
            ));
        }
    }
    for (index, accepted) in card.accepted_answers.iter().enumerate() {
        for (key_index, answer) in accepted.iter().enumerate() {
            if !keys.contains(answer.as_str()) {
                errors.push(entry.error(
                    ValidationRule::TypeGradingConsistency,
                    format!("accepted_answers[{index}][{key_index}]"),
                    format!("answer key {answer} is not declared in options"),
                ));
            }
        }
    }
}

pub fn validate(snapshot: &Snapshot) -> Result<Vec<ValidationError>> {
    let mut errors = Vec::new();
    for entry in &snapshot.question_cards {
        validate_card(&entry.value, &mut errors, entry);
    }
    Ok(errors)
}
