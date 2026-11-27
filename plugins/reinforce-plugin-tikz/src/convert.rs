//! Turns LaTeX/dvisvgm output into an SVG resource and a diagram draft card.

use std::fmt::Write as _;

use reinforce_plugin_protocol::{Operation, OperationKind};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use crate::protocol::RunError;

/// The job input accepted by this worker.
pub struct Input {
    pub source: String,
    pub preamble: Option<String>,
    pub title: Option<String>,
    pub subject: Option<String>,
}

pub fn parse_input(value: &Value) -> Result<Input, RunError> {
    let source = value.get("source").and_then(Value::as_str).ok_or_else(|| {
        crate::protocol::failure("invalid_input", "input.source must be a string")
    })?;
    if source.trim().is_empty() {
        return Err(crate::protocol::failure(
            "invalid_input",
            "input.source must not be empty",
        ));
    }
    let optional = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .filter(|text| !text.trim().is_empty())
    };
    Ok(Input {
        source: source.to_owned(),
        preamble: optional("preamble"),
        title: optional("title"),
        subject: optional("subject"),
    })
}

/// The job directory is the tools' working directory, so every path handed to
/// them is relative to it. That keeps TeX's `openin_any=p` confinement tight.
pub const TEX_FILE: &str = "diagram.tex";
pub const DVI_FILE: &str = "diagram.dvi";
pub const SVG_FILE: &str = "diagram.svg";

/// Fixed strings handed to the external tools, never assembled from a shell.
pub fn latex_arguments() -> Vec<String> {
    [
        "-interaction=nonstopmode",
        "-halt-on-error",
        "-no-shell-escape",
        "-output-directory=.",
        TEX_FILE,
    ]
    .map(str::to_owned)
    .to_vec()
}

pub fn dvisvgm_arguments() -> Vec<String> {
    ["-o", SVG_FILE, DVI_FILE].map(str::to_owned).to_vec()
}

pub fn tex_source(input: &Input) -> String {
    format!(
        "\\documentclass{{standalone}}\n\\usepackage{{tikz}}\n{}\n\\begin{{document}}\n{}\n\\end{{document}}\n",
        input.preamble.as_deref().unwrap_or_default(),
        input.source
    )
}

/// The quoted value of `name` in an SVG start tag. The attribute must start
/// after whitespace so `stroke-width` never matches `width`.
fn attribute<'a>(attributes: &'a str, name: &str) -> Option<&'a str> {
    let mut search = 0;
    while let Some(relative) = attributes[search..].find(name) {
        let start = search + relative;
        search = start + name.len();
        let preceded_by_space = attributes[..start]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace);
        let Some(rest) = attributes[search..].strip_prefix('=') else {
            continue;
        };
        let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            continue;
        };
        if !preceded_by_space {
            continue;
        }
        let value = &rest[1..];
        return value.find(quote).map(|end| &value[..end]);
    }
    None
}

/// Rounds a length to a whole number the resource table accepts (`> 0`).
fn positive_length(value: f64) -> Option<i64> {
    let rounded = value.round();
    (rounded.is_finite() && rounded >= 1.0 && rounded <= f64::from(i32::MAX))
        .then_some(rounded as i64)
}

/// A length such as `28.746032pt`: the leading number, units ignored.
fn length(value: &str) -> Option<i64> {
    let number: String = value
        .trim()
        .chars()
        .take_while(|character| character.is_ascii_digit() || *character == '.')
        .collect();
    positive_length(number.parse().ok()?)
}

fn view_box_dimensions(attributes: &str) -> Option<(i64, i64)> {
    let values: Vec<f64> = attribute(attributes, "viewBox")?
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|part| !part.is_empty())
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    if values.len() != 4 {
        return None;
    }
    Some((positive_length(values[2])?, positive_length(values[3])?))
}

fn svg_dimensions(svg: &str) -> (Option<i64>, Option<i64>) {
    let Some(start) = svg.find("<svg") else {
        return (None, None);
    };
    let Some(relative_end) = svg[start..].find('>') else {
        return (None, None);
    };
    let attributes = &svg[start..start + relative_end];
    let width = attribute(attributes, "width").and_then(length);
    let height = attribute(attributes, "height").and_then(length);
    match (width, height) {
        (Some(width), Some(height)) => (Some(width), Some(height)),
        _ => view_box_dimensions(attributes)
            .map_or((None, None), |(width, height)| (Some(width), Some(height))),
    }
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

/// Builds the SVG resource and the diagram card that keeps the original TikZ
/// source. Ids are generated here so no `$alias` appears in the batch.
pub fn operations(input: &Input, svg_bytes: &[u8]) -> Result<Vec<Operation>, RunError> {
    let svg = std::str::from_utf8(svg_bytes).map_err(|error| {
        crate::protocol::failure(
            "unparseable_output",
            format!("dvisvgm output is not UTF-8: {error}"),
        )
    })?;
    if !svg.contains("<svg") {
        return Err(crate::protocol::failure(
            "unparseable_output",
            "dvisvgm output does not contain an SVG element",
        ));
    }
    let (width, height) = svg_dimensions(svg);
    let now = timestamp();
    let resource_id = Uuid::now_v7().to_string();
    let card_id = Uuid::now_v7().to_string();
    let title = input
        .title
        .clone()
        .unwrap_or_else(|| "TikZ diagram".to_owned());
    let subject = input
        .subject
        .clone()
        .unwrap_or_else(|| "general".to_owned());
    Ok(vec![
        operation(
            OperationKind::Resource,
            json!({
                "id": resource_id,
                "kind": "image",
                "title": title,
                "sha256": hex_digest(svg_bytes),
                "byte_size": svg_bytes.len(),
                "page_count": null,
                "width": width,
                "height": height,
                "license": null,
                "attribution_url": null,
                "attribution_notice": null,
                "sensitivity": "normal",
                "export_policy": "allow",
                "created_at": now,
            }),
        ),
        operation(
            OperationKind::KnowledgeCard,
            json!({
                "id": card_id,
                "subject": subject,
                "topics": [],
                "title": title,
                "blocks": [{
                    "type": "diagram",
                    "source": input.source,
                    "syntax": "tikz",
                    "svg_cache": svg,
                    "node_bindings": [],
                }],
                "anchor_ids": [],
                "status": "draft",
                "verification": "unchecked",
                "unsure_count": 0,
                "metadata": {},
                "created_at": now,
                "updated_at": now,
            }),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> Input {
        Input {
            source: "\\begin{tikzpicture}\\draw (0,0) -- (1,1);\\end{tikzpicture}".to_owned(),
            preamble: None,
            title: None,
            subject: None,
        }
    }

    #[test]
    fn dimensions_come_from_svg_attributes() {
        let svg =
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="120.4pt" height="80.6pt"></svg>"#;
        assert_eq!(svg_dimensions(svg), (Some(120), Some(81)));
    }

    #[test]
    fn dimensions_fall_back_to_the_view_box() {
        let svg = r#"<svg viewBox="0 0 200 100"></svg>"#;
        assert_eq!(svg_dimensions(svg), (Some(200), Some(100)));
    }

    #[test]
    fn dimensions_parse_real_dvisvgm_output() {
        let svg = "<?xml version='1.0' encoding='UTF-8'?>\n<svg version='1.1' xmlns='http://www.w3.org/2000/svg' width='28.746032pt' height='19.5pt' viewBox='-72.0 -72.0 28.746032 19.5'>";
        assert_eq!(svg_dimensions(svg), (Some(29), Some(20)));
    }

    #[test]
    fn view_box_lengths_may_be_fractional() {
        let svg = "<svg viewBox='-72.0 -72.0 28.7 19.5'>";
        assert_eq!(svg_dimensions(svg), (Some(29), Some(20)));
    }

    #[test]
    fn prefixed_attributes_are_not_mistaken_for_dimensions() {
        let svg = r#"<svg stroke-width="3" line-height="4" viewBox="0 0 50 40">"#;
        assert_eq!(svg_dimensions(svg), (Some(50), Some(40)));
    }

    #[test]
    fn zero_or_negative_sizes_are_dropped() {
        assert_eq!(
            svg_dimensions(r#"<svg width="0.2pt" height="10pt">"#),
            (None, None)
        );
        assert_eq!(svg_dimensions(r#"<svg viewBox="0 0 -5 10">"#), (None, None));
    }

    #[test]
    fn tex_source_keeps_the_source_as_a_single_document() {
        let tex = tex_source(&input());
        assert!(tex.contains("\\documentclass{standalone}"));
        assert!(tex.contains("\\begin{tikzpicture}"));
        assert!(tex.trim_end().ends_with("\\end{document}"));
    }

    #[test]
    fn operations_emit_an_svg_resource_and_a_diagram_card() {
        let svg = br#"<svg width="10pt" height="10pt"></svg>"#;
        let operations = operations(&input(), svg).expect("operations");
        assert_eq!(operations.len(), 2);
        assert_eq!(operations[0].op, OperationKind::Resource);
        assert_eq!(operations[0].data["kind"], "image");
        assert_eq!(operations[0].data["width"], 10);
        assert_eq!(operations[1].op, OperationKind::KnowledgeCard);
        assert_eq!(operations[1].data["blocks"][0]["syntax"], "tikz");
        assert_eq!(operations[1].data["verification"], "unchecked");
        assert!(
            operations
                .iter()
                .all(|operation| operation.temp_ref.is_none())
        );
    }

    #[test]
    fn malformed_svg_is_reported() {
        let error = operations(&input(), b"not an svg").expect_err("svg must be valid");
        match error {
            RunError::Failure(failure) => assert_eq!(failure.code, "unparseable_output"),
            RunError::Cancelled => panic!("bad output is not a cancellation"),
        }
    }
}
