use crate::primitives::{AnswerSourceKind, Result, SourceKind};
use crate::validation::{Snapshot, StoreView, ValidationError, ValidationRule, exam_set_for};

pub fn validate(snapshot: &Snapshot, view: &dyn StoreView) -> Result<Vec<ValidationError>> {
    let mut errors = Vec::new();

    for entry in &snapshot.exam_sets {
        if entry.value.source_kind == SourceKind::Original && entry.value.exam_year.is_some() {
            errors.push(entry.error(
                ValidationRule::OriginalAntiFabrication,
                "exam_year",
                "an original exam set must not carry an exam year",
            ));
        }
    }

    for entry in &snapshot.question_cards {
        let card = &entry.value;
        let Some(exam_set_id) = &card.exam_set_id else {
            continue;
        };
        let Some(exam_set) = exam_set_for(snapshot, view, exam_set_id)? else {
            continue;
        };

        let authored = card.answer_source_kind == AnswerSourceKind::OriginalAuthored
            || exam_set.source_kind == SourceKind::Original;
        if !authored {
            continue;
        }

        if exam_set.source_kind == SourceKind::PastExam {
            errors.push(entry.error(
                ValidationRule::OriginalAntiFabrication,
                "exam_set_id",
                "original content cannot belong to a past_exam set",
            ));
            if card.question_no.is_some() {
                errors.push(entry.error(
                    ValidationRule::OriginalAntiFabrication,
                    "question_no",
                    "original content cannot impersonate a numbered past exam question",
                ));
            }
        }
    }

    Ok(errors)
}
