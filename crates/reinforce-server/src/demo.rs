use reinforce_core::auth::{get_setting, set_setting};
use reinforce_core::proposal::{
    ApprovalOutcome, BatchOperation, SubmitOutcome, approve_batch, submit_batch,
};
use reinforce_core::validation::Actor;
use reinforce_core::ContentHash;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

const BATCH: &str = include_str!("../../../fixtures/demo/batch.jsonl");
const ASSETS: &[(&str, &[u8])] = &[
    (
        "phase-diagram.svg",
        include_bytes!("../../../fixtures/demo/phase-diagram.svg"),
    ),
    (
        "phase-labels.svg",
        include_bytes!("../../../fixtures/demo/phase-labels.svg"),
    ),
];

const SEEDED_SETTING: &str = "demo_seeded";

pub async fn seed(state: &AppState) -> ApiResult<()> {
    let already_seeded = state
        .with_db(move |database| Ok(get_setting(database.connection(), SEEDED_SETTING)?))
        .await?
        .is_some();
    if already_seeded {
        return Ok(());
    }

    for (name, bytes) in ASSETS {
        let hash = ContentHash::from_bytes(bytes);
        let target = state.blob_path(hash.as_str());
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if !target.exists() {
            tokio::fs::write(&target, bytes).await?;
        }
        tracing::info!("demo asset {name} stored as {hash}");
    }

    let operations = BATCH
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str::<BatchOperation>)
        .collect::<serde_json::Result<Vec<_>>>()
        .map_err(|error| ApiError::internal(format!("invalid demo fixture: {error}")))?;

    state
        .with_db(move |database| {
            let proposal = match submit_batch(database.connection_mut(), Actor::Owner, operations)? {
                SubmitOutcome::Accepted { proposal } => proposal,
                SubmitOutcome::Rejected { errors } => return Err(ApiError::validation(errors)),
            };
            match approve_batch(
                database.connection_mut(),
                &proposal.id,
                reinforce_core::ApprovalOptions {
                    reviewer: Actor::Owner,
                    promote_verified: true,
                },
            )? {
                ApprovalOutcome::Applied { .. } => {}
                ApprovalOutcome::Rejected { errors, .. } => {
                    return Err(ApiError::validation(errors));
                }
            }
            set_setting(database.connection(), SEEDED_SETTING, "1")?;
            Ok(())
        })
        .await?;
    Ok(())
}
