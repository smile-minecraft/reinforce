use crate::content::{ContentBlock, EdgeEndpoint, OutlineNode};
use crate::primitives::{Result, Verification};
use crate::validation::{
    Snapshot, Staged, StoreView, ValidationError, ValidationRule, anchor_exists,
    knowledge_card_exists, question_card_exists, question_card_for, resource_dimensions,
    resource_exists,
};

/// Relative tolerance: base and overlay may differ by at most 1% in width/height ratio.
pub const ASPECT_RATIO_TOLERANCE: f64 = 0.01;

fn push_missing(
    errors: &mut Vec<ValidationError>,
    entry: &Staged<impl Sized>,
    field_path: impl Into<String>,
    kind: &str,
    id: impl std::fmt::Display,
) {
    errors.push(entry.error(
        ValidationRule::ReferentialIntegrity,
        field_path,
        format!("{kind} {id} does not exist"),
    ));
}

fn check_resource<T>(
    snapshot: &Snapshot,
    view: &dyn StoreView,
    entry: &Staged<T>,
    id: &crate::ResourceId,
    field_path: impl Into<String>,
    errors: &mut Vec<ValidationError>,
) -> Result<()> {
    if !resource_exists(snapshot, view, id)? {
        push_missing(errors, entry, field_path, "resource", id);
    }
    Ok(())
}

fn check_anchor<T>(
    snapshot: &Snapshot,
    view: &dyn StoreView,
    entry: &Staged<T>,
    id: &crate::AnchorId,
    field_path: impl Into<String>,
    errors: &mut Vec<ValidationError>,
) -> Result<()> {
    if !anchor_exists(snapshot, view, id)? {
        push_missing(errors, entry, field_path, "anchor", id);
    }
    Ok(())
}

fn check_knowledge_card<T>(
    snapshot: &Snapshot,
    view: &dyn StoreView,
    entry: &Staged<T>,
    id: &crate::KnowledgeCardId,
    field_path: impl Into<String>,
    errors: &mut Vec<ValidationError>,
) -> Result<()> {
    if !knowledge_card_exists(snapshot, view, id)? {
        push_missing(errors, entry, field_path, "linked card", id);
    }
    Ok(())
}

fn aspect_ratio_matches(base: (i64, i64), overlay: (i64, i64)) -> bool {
    if base.1 <= 0 || overlay.1 <= 0 {
        return false;
    }
    let base_ratio = base.0 as f64 / base.1 as f64;
    let overlay_ratio = overlay.0 as f64 / overlay.1 as f64;
    base_ratio > 0.0
        && overlay_ratio > 0.0
        && (base_ratio - overlay_ratio).abs() <= base_ratio * ASPECT_RATIO_TOLERANCE
}

fn check_blocks<T>(
    snapshot: &Snapshot,
    view: &dyn StoreView,
    entry: &Staged<T>,
    blocks: &[ContentBlock],
    prefix: &str,
    errors: &mut Vec<ValidationError>,
) -> Result<()> {
    for (index, block) in blocks.iter().enumerate() {
        match block {
            ContentBlock::Diagram { node_bindings, .. } => {
                for (binding_index, binding) in node_bindings.iter().enumerate() {
                    check_knowledge_card(
                        snapshot,
                        view,
                        entry,
                        &binding.card_id,
                        format!("{prefix}[{index}].node_bindings[{binding_index}].card_id"),
                        errors,
                    )?;
                }
            }
            ContentBlock::ImageOcclusion { resource_id, .. } => {
                check_resource(
                    snapshot,
                    view,
                    entry,
                    resource_id,
                    format!("{prefix}[{index}].resource_id"),
                    errors,
                )?;
            }
            ContentBlock::InteractivePins {
                resource_id,
                overlay_resource_id,
                pins,
            } => {
                check_resource(
                    snapshot,
                    view,
                    entry,
                    resource_id,
                    format!("{prefix}[{index}].resource_id"),
                    errors,
                )?;
                if let Some(overlay_id) = overlay_resource_id {
                    check_resource(
                        snapshot,
                        view,
                        entry,
                        overlay_id,
                        format!("{prefix}[{index}].overlay_resource_id"),
                        errors,
                    )?;

                    let base_dimensions = resource_dimensions(snapshot, view, resource_id)?;
                    let overlay_dimensions = resource_dimensions(snapshot, view, overlay_id)?;
                    match (base_dimensions, overlay_dimensions) {
                        (Some(base), Some(overlay)) => {
                            if !aspect_ratio_matches(base, overlay) {
                                errors.push(entry.error(
                                    ValidationRule::ReferentialIntegrity,
                                    format!("{prefix}[{index}].overlay_resource_id"),
                                    format!(
                                        "overlay aspect ratio {:?} does not match base {:?} within {:.1}%",
                                        overlay, base,
                                        ASPECT_RATIO_TOLERANCE * 100.0
                                    ),
                                ));
                            }
                        }
                        (None, _) => errors.push(entry.error(
                            ValidationRule::ReferentialIntegrity,
                            format!("{prefix}[{index}].resource_id"),
                            "base image needs width and height for aspect ratio validation",
                        )),
                        (_, None) => errors.push(entry.error(
                            ValidationRule::ReferentialIntegrity,
                            format!("{prefix}[{index}].overlay_resource_id"),
                            "overlay image needs width and height for aspect ratio validation",
                        )),
                    }
                }
                for (pin_index, pin) in pins.iter().enumerate() {
                    if let Some(linked_id) = &pin.linked_card_id {
                        check_knowledge_card(
                            snapshot,
                            view,
                            entry,
                            linked_id,
                            format!("{prefix}[{index}].pins[{pin_index}].linked_card_id"),
                            errors,
                        )?;
                    }
                }
            }
            ContentBlock::Markdown { .. } | ContentBlock::Math { .. } => {}
        }
    }
    Ok(())
}

fn check_outline<T>(
    snapshot: &Snapshot,
    view: &dyn StoreView,
    entry: &Staged<T>,
    nodes: &[OutlineNode],
    prefix: &str,
    errors: &mut Vec<ValidationError>,
) -> Result<()> {
    for (index, node) in nodes.iter().enumerate() {
        match node {
            OutlineNode::Section { children, .. } => {
                check_outline(
                    snapshot,
                    view,
                    entry,
                    children,
                    &format!("{prefix}[{index}].children"),
                    errors,
                )?;
            }
            OutlineNode::CardRef { card_id } => check_knowledge_card(
                snapshot,
                view,
                entry,
                card_id,
                format!("{prefix}[{index}].card_id"),
                errors,
            )?,
        }
    }
    Ok(())
}

fn check_endpoint<T>(
    snapshot: &Snapshot,
    view: &dyn StoreView,
    entry: &Staged<T>,
    endpoint: &EdgeEndpoint,
    field_path: &str,
    errors: &mut Vec<ValidationError>,
) -> Result<()> {
    match endpoint {
        EdgeEndpoint::Card { id } => check_knowledge_card(
            snapshot,
            view,
            entry,
            id,
            format!("{field_path}.card_id"),
            errors,
        )?,
        EdgeEndpoint::Question { id } => check_question(
            snapshot,
            view,
            entry,
            id,
            format!("{field_path}.question_id"),
            errors,
        )?,
        EdgeEndpoint::Option { id, key } => {
            check_question(
                snapshot,
                view,
                entry,
                id,
                format!("{field_path}.question_id"),
                errors,
            )?;
            if let Some(question) = question_card_for(snapshot, view, id)? {
                if !question.options.iter().any(|option| option.key == *key) {
                    errors.push(entry.error(
                        ValidationRule::ReferentialIntegrity,
                        format!("{field_path}.option_key"),
                        format!("option key {key} does not exist on question {id}"),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn check_question<T>(
    snapshot: &Snapshot,
    view: &dyn StoreView,
    entry: &Staged<T>,
    id: &crate::QuestionCardId,
    field_path: impl Into<String>,
    errors: &mut Vec<ValidationError>,
) -> Result<()> {
    if !question_card_exists(snapshot, view, id)? {
        push_missing(errors, entry, field_path, "question", id);
    }
    Ok(())
}

pub fn validate(snapshot: &Snapshot, view: &dyn StoreView) -> Result<Vec<ValidationError>> {
    let mut errors = Vec::new();

    for entry in &snapshot.anchors {
        check_resource(
            snapshot,
            view,
            entry,
            &entry.value.resource_id,
            "resource_id",
            &mut errors,
        )?;
    }

    for entry in &snapshot.knowledge_cards {
        for (index, id) in entry.value.anchor_ids.iter().enumerate() {
            check_anchor(
                snapshot,
                view,
                entry,
                id,
                format!("anchor_ids[{index}]"),
                &mut errors,
            )?;
        }
        check_blocks(
            snapshot,
            view,
            entry,
            &entry.value.blocks,
            "blocks",
            &mut errors,
        )?;
        if !snapshot.actor.is_owner() && entry.value.verification == Verification::Verified {
            errors.push(entry.error(
                ValidationRule::VerificationLock,
                "verification",
                "only the owner may submit verified knowledge cards",
            ));
        }
    }

    for entry in &snapshot.notes {
        check_outline(
            snapshot,
            view,
            entry,
            &entry.value.outline,
            "outline",
            &mut errors,
        )?;
    }

    for entry in &snapshot.exam_sets {
        if let Some(id) = &entry.value.source_resource_id {
            check_resource(snapshot, view, entry, id, "source_resource_id", &mut errors)?;
        }
    }

    for entry in &snapshot.question_groups {
        if let Some(id) = &entry.value.exam_set_id {
            if crate::validation::exam_set_for(snapshot, view, id)?.is_none() {
                push_missing(&mut errors, entry, "exam_set_id", "exam set", id);
            }
        }
        check_blocks(
            snapshot,
            view,
            entry,
            &entry.value.shared_stem_blocks,
            "shared_stem_blocks",
            &mut errors,
        )?;
    }

    for entry in &snapshot.question_cards {
        let card = &entry.value;
        if let Some(id) = &card.exam_set_id {
            if crate::validation::exam_set_for(snapshot, view, id)?.is_none() {
                push_missing(&mut errors, entry, "exam_set_id", "exam set", id);
            }
        }
        if let Some(id) = &card.question_group_id {
            if view.question_group(id)?.is_none()
                && !snapshot
                    .question_groups
                    .iter()
                    .any(|group| group.value.id == *id)
            {
                push_missing(
                    &mut errors,
                    entry,
                    "question_group_id",
                    "question group",
                    id,
                );
            }
        }
        for (index, id) in card.anchor_ids.iter().enumerate() {
            check_anchor(
                snapshot,
                view,
                entry,
                id,
                format!("anchor_ids[{index}]"),
                &mut errors,
            )?;
        }
        for (index, option) in card.options.iter().enumerate() {
            for (linked_index, id) in option.linked_card_ids.iter().enumerate() {
                check_knowledge_card(
                    snapshot,
                    view,
                    entry,
                    id,
                    format!("options[{index}].linked_card_ids[{linked_index}]"),
                    &mut errors,
                )?;
            }
        }
        check_blocks(
            snapshot,
            view,
            entry,
            &card.stem_blocks,
            "stem_blocks",
            &mut errors,
        )?;
        check_blocks(
            snapshot,
            view,
            entry,
            &card.explanation_blocks,
            "explanation_blocks",
            &mut errors,
        )?;
        if !snapshot.actor.is_owner() && card.verification == Verification::Verified {
            errors.push(entry.error(
                ValidationRule::VerificationLock,
                "verification",
                "only the owner may submit verified question cards",
            ));
        }
    }

    for entry in &snapshot.edges {
        check_endpoint(
            snapshot,
            view,
            entry,
            &entry.value.from,
            "from",
            &mut errors,
        )?;
        check_endpoint(snapshot, view, entry, &entry.value.to, "to", &mut errors)?;
    }

    Ok(errors)
}
