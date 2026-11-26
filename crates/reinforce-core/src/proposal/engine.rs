use rusqlite::Connection;

use crate::Timestamp;
use crate::content::Proposal;
use crate::db::content as store;
use crate::primitives::{Error, ProposalId, ProposalStatus, Result, Verification};
use crate::validation::{Snapshot, ValidationReport, validate_snapshot};

use super::batch::{self, BatchPayload};

#[derive(Debug, Clone)]
pub enum SubmitOutcome {
    Accepted { proposal: Proposal },
    Rejected { errors: ValidationReport },
}

#[derive(Debug, Clone)]
pub struct ApprovalOptions {
    pub reviewer: crate::validation::Actor,
    pub promote_verified: bool,
}

#[derive(Debug, Clone)]
pub enum ApprovalOutcome {
    Applied {
        proposal: Proposal,
        applied: usize,
    },
    Rejected {
        proposal: Proposal,
        errors: ValidationReport,
    },
}

fn next_sync_cursor(connection: &Connection) -> Result<i64> {
    let cursor: i64 = connection.query_row(
        "SELECT COALESCE(MAX(sync_cursor), 0) + 1 FROM proposals",
        [],
        |row| row.get(0),
    )?;
    Ok(cursor)
}

fn apply_snapshot(connection: &Connection, snapshot: &Snapshot) -> Result<usize> {
    let mut applied = 0;
    for entry in &snapshot.resources {
        store::upsert_resource(connection, &entry.value)?;
        applied += 1;
    }
    for entry in &snapshot.anchors {
        store::upsert_anchor(connection, &entry.value)?;
        applied += 1;
    }
    for entry in &snapshot.exam_sets {
        store::upsert_exam_set(connection, &entry.value)?;
        applied += 1;
    }
    for entry in &snapshot.question_groups {
        store::upsert_question_group(connection, &entry.value)?;
        applied += 1;
    }
    for entry in &snapshot.notes {
        store::upsert_note(connection, &entry.value)?;
        applied += 1;
    }
    for entry in &snapshot.knowledge_cards {
        store::upsert_knowledge_card(connection, &entry.value)?;
        applied += 1;
    }
    for entry in &snapshot.question_cards {
        store::upsert_question_card(connection, &entry.value)?;
        applied += 1;
    }
    for entry in &snapshot.edges {
        store::upsert_edge(connection, &entry.value)?;
        applied += 1;
    }
    Ok(applied)
}

fn promote_verification(snapshot: &mut Snapshot) {
    for entry in &mut snapshot.knowledge_cards {
        entry.value.verification = Verification::Verified;
    }
    for entry in &mut snapshot.question_cards {
        entry.value.verification = Verification::Verified;
    }
}

/// Validates and stages the whole batch in one proposal row. Validation
/// errors are returned field by field and no row is written on rejection.
pub fn submit_batch(
    connection: &mut Connection,
    actor: crate::validation::Actor,
    operations: Vec<batch::BatchOperation>,
) -> Result<SubmitOutcome> {
    // Validation reads and the proposal write share one transaction so the
    // store cannot change between the check and the insert.
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let prepared = match batch::prepare(&transaction, actor.clone(), operations)? {
        batch::Preparation::Ready(prepared) => *prepared,
        batch::Preparation::Invalid(errors) => return Ok(SubmitOutcome::Rejected { errors }),
    };
    let errors = validate_snapshot(&prepared.snapshot, &*transaction)?;
    if !errors.is_ok() {
        return Ok(SubmitOutcome::Rejected { errors });
    }

    let now = Timestamp::now();
    let proposal = Proposal {
        id: ProposalId::generate(),
        target: batch::batch_target(&prepared.payload)?,
        payload: serde_json::to_value(&prepared.payload)?,
        submitted_by: batch::submitted_by(&actor),
        status: ProposalStatus::Pending,
        review_note: None,
        created_at: now,
        updated_at: now,
        sync_cursor: next_sync_cursor(&transaction)?,
    };
    store::insert_proposal(&transaction, &proposal)?;
    transaction.commit()?;
    Ok(SubmitOutcome::Accepted { proposal })
}

/// Applies the stored snapshot with full replacement semantics. Every entity
/// and the proposal status change live in one transaction.
pub fn approve_batch(
    connection: &mut Connection,
    proposal_id: &ProposalId,
    options: ApprovalOptions,
) -> Result<ApprovalOutcome> {
    // Re-validation and the apply share one transaction so a concurrent writer
    // cannot invalidate the snapshot between the check and the write.
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let proposal = store::get_proposal(&transaction, proposal_id)?
        .ok_or_else(|| Error::InvalidValue("proposal", "proposal does not exist".to_owned()))?;
    if proposal.status != ProposalStatus::Pending {
        return Err(Error::InvalidValue(
            "proposal",
            "only a pending proposal can be approved".to_owned(),
        ));
    }
    if options.promote_verified && !options.reviewer.is_owner() {
        return Err(Error::InvalidValue(
            "reviewer",
            "only the owner can promote a proposal to verified".to_owned(),
        ));
    }

    let payload = batch::batch_from_proposal(&proposal)?;
    let mut snapshot = batch::snapshot_from_payload(&payload)?;
    snapshot.actor = options.reviewer.clone();
    if options.promote_verified {
        promote_verification(&mut snapshot);
    }

    let errors = validate_snapshot(&snapshot, &*transaction)?;
    if !errors.is_ok() {
        return Ok(ApprovalOutcome::Rejected { proposal, errors });
    }

    let mut approved = proposal;
    approved.status = ProposalStatus::Approved;
    approved.updated_at = Timestamp::now();
    let applied = apply_snapshot(&transaction, &snapshot)?;
    store::update_proposal(&transaction, &approved)?;
    transaction.commit()?;
    Ok(ApprovalOutcome::Applied {
        proposal: approved,
        applied,
    })
}

pub fn reject_batch(
    connection: &Connection,
    proposal_id: &ProposalId,
    review_note: &str,
) -> Result<Proposal> {
    if review_note.trim().is_empty() {
        return Err(Error::InvalidValue(
            "review_note",
            "a rejection needs a review note".to_owned(),
        ));
    }
    let mut proposal = store::get_proposal(connection, proposal_id)?
        .ok_or_else(|| Error::InvalidValue("proposal", "proposal does not exist".to_owned()))?;
    if proposal.status != ProposalStatus::Pending {
        return Err(Error::InvalidValue(
            "proposal",
            "only a pending proposal can be rejected".to_owned(),
        ));
    }
    proposal.status = ProposalStatus::Rejected;
    proposal.review_note = Some(review_note.to_owned());
    proposal.updated_at = Timestamp::now();
    store::update_proposal(connection, &proposal)?;
    Ok(proposal)
}

/// Rejected batches keep their resolved operations so an agent can read the
/// review note, revise the JSON and resubmit it.
pub fn rejected_batches(connection: &Connection) -> Result<Vec<(Proposal, BatchPayload)>> {
    store::list_proposals(connection, ProposalStatus::Rejected)?
        .into_iter()
        .map(|proposal| {
            let payload = batch::batch_from_proposal(&proposal)?;
            Ok((proposal, payload))
        })
        .collect()
}
