//! Turns MinerU's JSON summary and markdown output into proposal operations.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use reinforce_plugin_protocol::{Operation, OperationKind};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::protocol::RunError;

/// The job input accepted by this worker.
pub struct Input {
    pub pdf_path: PathBuf,
    pub title: Option<String>,
    pub subject: Option<String>,
}

pub fn parse_input(value: &Value) -> Result<Input, RunError> {
    let path = value
        .get("pdf_path")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            crate::protocol::failure("invalid_input", "input.pdf_path must be a string")
        })?;
    let pdf_path = PathBuf::from(path);
    if !pdf_path.is_absolute() {
        return Err(crate::protocol::failure(
            "invalid_input",
            "input.pdf_path must be an absolute path",
        ));
    }
    if !pdf_path.is_file() {
        return Err(crate::protocol::failure(
            "invalid_input",
            format!("PDF does not exist: {}", pdf_path.display()),
        ));
    }
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|text| !text.trim().is_empty());
    let subject = value
        .get("subject")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|text| !text.trim().is_empty());
    Ok(Input {
        pdf_path,
        title,
        subject,
    })
}

/// Fixed strings handed to the external tool, never assembled from a shell.
pub fn tool_arguments(tool_options: &super::Options, pdf: &Path, output: &Path) -> Vec<String> {
    [
        "parse".to_owned(),
        pdf.display().to_string(),
        "--tier".to_owned(),
        tool_options.tier.clone(),
        "--pages".to_owned(),
        "all".to_owned(),
        "--output".to_owned(),
        output.display().to_string(),
        "--json".to_owned(),
        "--wait".to_owned(),
        tool_options.wait.to_string(),
        "--force".to_owned(),
    ]
    .to_vec()
}

struct Page {
    number: i64,
    markdown: String,
}

fn page_marker(line: &str) -> Option<i64> {
    let inner = line
        .trim()
        .strip_prefix("<!--")?
        .strip_suffix("-->")?
        .trim();
    let (left, right) = inner.split_once(" of ")?;
    let number = left.strip_prefix("page ")?.trim().parse().ok()?;
    let _total: i64 = right.trim().parse().ok()?;
    Some(number)
}

/// MinerU writes `<!-- page N of M -->` markers when `--output` is used. A
/// single unmarked document is still usable, e.g. from a different MinerU
/// version or a fake tool used in tests.
fn split_pages(markdown: &str) -> Vec<Page> {
    let mut pages = Vec::new();
    let mut current: Option<Page> = None;
    let mut marked = false;
    for line in markdown.lines() {
        if let Some(number) = page_marker(line) {
            marked = true;
            if let Some(page) = current.take() {
                if !page.markdown.trim().is_empty() {
                    pages.push(page);
                }
            }
            current = Some(Page {
                number,
                markdown: String::new(),
            });
        } else if let Some(page) = current.as_mut() {
            page.markdown.push_str(line);
            page.markdown.push('\n');
        }
    }
    if let Some(page) = current.take() {
        if !page.markdown.trim().is_empty() {
            pages.push(page);
        }
    }
    if !marked && !markdown.trim().is_empty() {
        return vec![Page {
            number: 1,
            markdown: markdown.trim().to_owned(),
        }];
    }
    pages
}

fn page_title(page: &Page) -> String {
    page.markdown
        .lines()
        .find_map(|line| {
            let line = line.trim();
            for level in 1..=6 {
                let prefix = format!("{} ", "#".repeat(level));
                if let Some(text) = line.strip_prefix(&prefix) {
                    let text = text.trim();
                    if !text.is_empty() {
                        return Some(text.to_owned());
                    }
                }
            }
            None
        })
        .unwrap_or_else(|| format!("Page {}", page.number))
}

fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut text = String::with_capacity(64);
    for byte in digest {
        write!(&mut text, "{byte:02x}").expect("writing to a String cannot fail");
    }
    text
}

fn timestamp() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "2026-01-01T00:00:00Z".to_owned())
}

fn operation(kind: OperationKind, data: Value) -> Operation {
    Operation {
        op: kind,
        temp_ref: None,
        data,
    }
}

fn validate_report(report: &Value) -> Result<(), RunError> {
    if let Some(error) = report.get("error") {
        let code = error
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("MinerU reported an error");
        return Err(crate::protocol::failure(
            "external_tool_failed",
            format!("mineru {code}: {message}"),
        ));
    }
    let parse = report
        .get("parse")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            crate::protocol::failure("unparseable_output", "mineru output has no parse object")
        })?;
    if parse.get("status").and_then(Value::as_str) != Some("done") {
        return Err(crate::protocol::failure(
            "external_tool_failed",
            "mineru did not finish parsing the document",
        ));
    }
    Ok(())
}

/// Builds the PDF resource, one page anchor per parsed page and a draft card
/// for each page. Ids are generated here so no `$alias` appears in the batch.
pub fn operations(
    input: &Input,
    report: &Value,
    markdown: &str,
) -> Result<Vec<Operation>, RunError> {
    validate_report(report)?;
    let pages = split_pages(markdown);
    if pages.is_empty() {
        return Err(crate::protocol::failure(
            "unparseable_output",
            "mineru produced no page content",
        ));
    }
    let bytes = fs::read(&input.pdf_path).map_err(|error| {
        crate::protocol::failure(
            "invalid_input",
            format!("cannot read PDF {}: {error}", input.pdf_path.display()),
        )
    })?;
    let now = timestamp();
    let resource_id = Uuid::now_v7().to_string();
    let subject = input
        .subject
        .clone()
        .unwrap_or_else(|| "general".to_owned());
    let title = input.title.clone().unwrap_or_else(|| {
        input.pdf_path.file_name().map_or_else(
            || "PDF".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        )
    });

    let mut operations = Vec::with_capacity(1 + pages.len() * 2);
    operations.push(operation(
        OperationKind::Resource,
        json!({
            "id": resource_id,
            "kind": "pdf",
            "title": title,
            "sha256": hex_digest(&bytes),
            "byte_size": bytes.len(),
            "page_count": null,
            "width": null,
            "height": null,
            "license": null,
            "attribution_url": null,
            "attribution_notice": null,
            "sensitivity": "normal",
            "export_policy": "allow",
            "created_at": now,
        }),
    ));

    for page in &pages {
        let anchor_id = Uuid::now_v7().to_string();
        let card_id = Uuid::now_v7().to_string();
        operations.push(operation(
            OperationKind::Anchor,
            json!({
                "id": anchor_id,
                "resource_id": resource_id,
                "anchor_kind": "pdf_page",
                "pdf_page": page.number,
                "printed_page": null,
                "legend_pdf_page": null,
                "rect": null,
                "media_span": null,
            }),
        ));
        operations.push(operation(
            OperationKind::KnowledgeCard,
            json!({
                "id": card_id,
                "subject": subject,
                "topics": [],
                "title": page_title(page),
                "blocks": [{"type": "markdown", "text": page.markdown.trim()}],
                "anchor_ids": [anchor_id],
                "status": "draft",
                "verification": "unchecked",
                "unsure_count": 0,
                "metadata": {},
                "created_at": now,
                "updated_at": now,
            }),
        ));
    }
    Ok(operations)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> Value {
        json!({"parse": {"status": "done", "sha256": "abc"}})
    }

    #[test]
    fn page_markers_split_the_markdown_into_pages() {
        let pages = split_pages(
            "<!-- page 1 of 2 -->\n# One\nfirst\n<!-- page 2 of 2 -->\n# Two\nsecond\n",
        );
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].number, 1);
        assert_eq!(pages[0].markdown.trim(), "# One\nfirst");
        assert_eq!(pages[1].number, 2);
        assert_eq!(page_title(&pages[1]), "Two");
    }

    #[test]
    fn unmarked_markdown_is_one_page() {
        let pages = split_pages("# Only page\nbody\n");
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].number, 1);
    }

    #[test]
    fn operations_link_cards_to_their_page_anchors() {
        let directory = tempfile::tempdir().expect("temp dir");
        let pdf = directory.path().join("paper.pdf");
        fs::write(&pdf, b"pdf bytes").expect("write pdf");
        let input = Input {
            pdf_path: pdf,
            title: None,
            subject: None,
        };
        let markdown = "<!-- page 1 of 1 -->\n# Findings\nResult\n";
        let operations = operations(&input, &report(), markdown).expect("operations");
        assert_eq!(operations.len(), 3);
        assert_eq!(operations[0].op, OperationKind::Resource);
        assert_eq!(operations[0].data["kind"], "pdf");
        assert_eq!(operations[1].op, OperationKind::Anchor);
        assert_eq!(operations[1].data["pdf_page"], 1);
        assert_eq!(operations[2].op, OperationKind::KnowledgeCard);
        assert_eq!(
            operations[2].data["anchor_ids"][0],
            operations[1].data["id"]
        );
        assert_eq!(operations[2].data["verification"], "unchecked");
        assert!(
            operations
                .iter()
                .all(|operation| operation.temp_ref.is_none())
        );
    }

    #[test]
    fn mineru_error_reports_become_structured_failures() {
        let directory = tempfile::tempdir().expect("temp dir");
        let pdf = directory.path().join("paper.pdf");
        fs::write(&pdf, b"pdf bytes").expect("write pdf");
        let input = Input {
            pdf_path: pdf,
            title: None,
            subject: None,
        };
        let error = operations(
            &input,
            &json!({"error": {"code": "engine_error", "message": "no engine"}}),
            "ignored",
        )
        .expect_err("mineru errors must fail");
        match error {
            RunError::Failure(failure) => {
                assert_eq!(failure.code, "external_tool_failed");
                assert!(failure.message.contains("engine_error"));
            }
            RunError::Cancelled => panic!("an error report is not a cancellation"),
        }
    }
}
