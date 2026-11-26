use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::post;
use reinforce_export::PdfPreset;
use serde::Deserialize;

use crate::auth::Principal;
use crate::error::{ApiError, ApiResult, JsonBody};
use crate::export::{ExportJob, empty_export, has_content, render, safe_filename};
use crate::export_scope::ExportScope;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/export", post(export))
}

#[derive(Debug, Deserialize)]
struct ExportBody {
    format: String,
    preset: Option<String>,
    scope: ScopeBody,
    web_base_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ScopeBody {
    Note { note_id: String },
    Subject { subject: String },
    Weakness { threshold: f64 },
}

fn parse_preset(value: Option<&str>) -> ApiResult<PdfPreset> {
    match value {
        Some("standard") => Ok(PdfPreset::Standard),
        Some("goodnotes") => Ok(PdfPreset::GoodNotes),
        Some("worksheet") => Ok(PdfPreset::Worksheet),
        Some(other) => Err(ApiError::bad_request(
            "preset",
            format!("unknown preset {other}"),
        )),
        None => Err(ApiError::bad_request("preset", "required for pdf exports")),
    }
}

fn web_base_url(value: Option<String>) -> ApiResult<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.starts_with("https://") || value.starts_with("http://") {
        Ok(Some(value))
    } else {
        Err(ApiError::bad_request(
            "web_base_url",
            "must be an http or https URL",
        ))
    }
}

fn scope(body: &ExportBody, user_id: reinforce_core::UserId) -> ApiResult<(ExportScope, String)> {
    match &body.scope {
        ScopeBody::Note { note_id } => {
            let id = note_id
                .parse()
                .map_err(|_| ApiError::bad_request("scope.note_id", "invalid note id"))?;
            Ok((ExportScope::Note { note_id: id }, note_id.clone()))
        }
        ScopeBody::Subject { subject } => {
            if subject.trim().is_empty() {
                return Err(ApiError::bad_request("scope.subject", "cannot be empty"));
            }
            Ok((
                ExportScope::Subject {
                    subject: subject.clone(),
                },
                subject.clone(),
            ))
        }
        ScopeBody::Weakness { threshold } => {
            if !threshold.is_finite() || !(0.0..=1.0).contains(threshold) {
                return Err(ApiError::bad_request(
                    "scope.threshold",
                    "must be between 0 and 1",
                ));
            }
            Ok((
                ExportScope::Weakness {
                    threshold: *threshold,
                    user_id,
                },
                "weakness".to_owned(),
            ))
        }
    }
}

async fn export(
    State(state): State<AppState>,
    principal: Principal,
    JsonBody(body): JsonBody<ExportBody>,
) -> ApiResult<Response> {
    let owner = principal.is_owner();
    let (scope, scope_name) = scope(&body, principal.user_id.clone())?;
    let web_base_url = web_base_url(body.web_base_url.clone())?;
    let job = match body.format.as_str() {
        "pdf" => ExportJob::Pdf {
            preset: parse_preset(body.preset.as_deref())?,
            fonts_dir: state.fonts.clone(),
            web_base_url,
            viewer_is_owner: owner,
        },
        "markdown" => ExportJob::Markdown {
            viewer_is_owner: owner,
        },
        other => {
            return Err(ApiError::bad_request(
                "format",
                format!("unknown format {other}"),
            ));
        }
    };

    let blobs = state.blobs.clone();
    let input = state
        .with_db(move |database| {
            crate::export_scope::select(database.connection(), &scope, owner, &blobs)
        })
        .await?;
    if !has_content(&input) {
        return Err(empty_export());
    }

    let bytes = render(input, job.clone()).await?;
    let disposition = match job {
        ExportJob::Pdf { .. } => "inline",
        ExportJob::Markdown { .. } => "attachment",
    };
    let filename = safe_filename(&scope_name, job.extension());
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, job.content_type())
        .header(
            header::CONTENT_DISPOSITION,
            format!("{disposition}; filename=\"{filename}\""),
        )
        .header(header::CONTENT_LENGTH, bytes.len().to_string())
        .body(Body::from(bytes))
        .map_err(|error| ApiError::internal(format!("cannot build export response: {error}")))?)
}
