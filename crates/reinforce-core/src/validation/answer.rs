use crate::primitives::{AnswerSourceKind, Result};
use crate::validation::{
    Snapshot, StoreView, ValidationError, ValidationRule, anchor_exists, question_card_exists,
};

fn has_answer(answer: &[String]) -> bool {
    answer.iter().any(|value| !value.trim().is_empty())
}

fn has_basis(basis: Option<&String>) -> bool {
    basis.is_some_and(|value| !value.trim().is_empty())
}

pub fn validate(snapshot: &Snapshot, view: &dyn StoreView) -> Result<Vec<ValidationError>> {
    let mut errors = Vec::new();

    for entry in &snapshot.question_cards {
        let card = &entry.value;
        if !has_answer(&card.answer) {
            continue;
        }

        if card.answer_source_kind == AnswerSourceKind::Unanswered {
            errors.push(entry.error(
                ValidationRule::AnswerProvenance,
                "answer_source_kind",
                "a question with an answer cannot use unanswered as its source",
            ));
        }

        if matches!(
            card.answer_source_kind,
            AnswerSourceKind::Inferred
                | AnswerSourceKind::OriginalAuthored
                | AnswerSourceKind::DuplicateQuestion
        ) && !has_basis(card.answer_basis.as_ref())
        {
            errors.push(entry.error(
                ValidationRule::AnswerProvenance,
                "answer_basis",
                format!(
                    "{} requires a non-empty answer_basis",
                    card.answer_source_kind
                ),
            ));
        }

        if card.answer_source_kind == AnswerSourceKind::Inferred {
            let mut attached = false;
            for (index, id) in card.anchor_ids.iter().enumerate() {
                if anchor_exists(snapshot, view, id)? {
                    attached = true;
                    break;
                }
                errors.push(entry.error(
                    ValidationRule::AnswerProvenance,
                    format!("anchor_ids[{index}]"),
                    format!("anchor {id} does not exist"),
                ));
            }
            if !attached && card.anchor_ids.is_empty() {
                errors.push(entry.error(
                    ValidationRule::AnswerProvenance,
                    "anchor_ids",
                    "an inferred answer requires at least one existing anchor",
                ));
            }
        }

        if card.answer_source_kind == AnswerSourceKind::DuplicateQuestion {
            match &card.duplicate_question_id {
                None => errors.push(entry.error(
                    ValidationRule::AnswerProvenance,
                    "duplicate_question_id",
                    "a duplicate_question answer must reference an existing question",
                )),
                Some(id) if *id == card.id => errors.push(entry.error(
                    ValidationRule::AnswerProvenance,
                    "duplicate_question_id",
                    "a question cannot duplicate itself",
                )),
                Some(id) => {
                    if !question_card_exists(snapshot, view, id)? {
                        errors.push(entry.error(
                            ValidationRule::AnswerProvenance,
                            "duplicate_question_id",
                            format!("duplicate question {id} does not exist"),
                        ));
                    }
                }
            }
        }
    }

    Ok(errors)
}
