use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use reinforce_core::content::ContentBlock;
use reinforce_core::primitives::ExportPolicy;
use reinforce_core::{KnowledgeCard, Note, QuestionCard, Resource};
use serde::{Deserialize, Serialize};

use crate::model::{Result, visible_blocks};
use crate::{ExportInput, MarkdownOptions};

const FORMAT: &str = "reinforce-export/v1";
const MANIFEST: &str = "manifest.md";
const ASSETS: &str = "assets";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FrontMatter {
    format: String,
    kind: String,
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    verification: Option<String>,
    record: String,
    #[serde(default)]
    resources: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    blob_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BundleManifest {
    blob_hashes: Vec<String>,
    note_ids: Vec<String>,
    card_ids: Vec<String>,
    question_ids: Vec<String>,
}

fn front_matter(matter: &FrontMatter) -> Result<String> {
    Ok(format!(
        "---\n{}\n---\n",
        serde_json::to_string_pretty(matter)?
    ))
}

struct EntityFile<'a, T: Serialize> {
    directory: &'a str,
    entity: &'a T,
    kind: &'a str,
    id: &'a str,
    status: Option<&'a str>,
    verification: Option<&'a str>,
    resources: &'a [Resource],
    body: &'a str,
}

fn write_entity<T: Serialize>(root: &Path, file: &EntityFile<'_, T>) -> Result<()> {
    let folder = root.join(file.directory);
    std::fs::create_dir_all(&folder)?;
    let matter = FrontMatter {
        format: FORMAT.to_owned(),
        kind: file.kind.to_owned(),
        id: file.id.to_owned(),
        status: file.status.map(str::to_owned),
        verification: file.verification.map(str::to_owned),
        record: serde_json::to_string(file.entity)?,
        resources: file
            .resources
            .iter()
            .map(serde_json::to_string)
            .collect::<serde_json::Result<Vec<_>>>()?,
        blob_dir: None,
    };
    let mut text = front_matter(&matter)?;
    text.push('\n');
    text.push_str(file.body);
    text.push('\n');
    std::fs::write(folder.join(format!("{}.md", file.id)), text)?;
    Ok(())
}

fn blocked_resources(input: &ExportInput, blocks: &[ContentBlock]) -> Vec<Resource> {
    let mut ids = Vec::new();
    for block in blocks {
        let resource_ids: Vec<&reinforce_core::ResourceId> = match block {
            ContentBlock::ImageOcclusion { resource_id, .. } => vec![resource_id],
            ContentBlock::InteractivePins {
                resource_id,
                overlay_resource_id,
                ..
            } => {
                let mut result = vec![resource_id];
                if let Some(id) = overlay_resource_id {
                    result.push(id);
                }
                result
            }
            _ => Vec::new(),
        };
        ids.extend(resource_ids);
    }
    ids.sort();
    ids.dedup();
    input
        .resources
        .iter()
        .filter(|resource| ids.contains(&&resource.id))
        .cloned()
        .collect()
}

fn block_to_markdown(block: &ContentBlock) -> String {
    match block {
        ContentBlock::Markdown { text } => format!("{text}\n\n"),
        ContentBlock::Math { latex, .. } => format!("```latex\n{latex}\n```\n\n"),
        ContentBlock::Diagram {
            source,
            syntax,
            svg_cache,
            node_bindings,
        } => {
            let mut out = format!("```{}\n{}\n```\n\n", syntax.as_str(), source);
            if let Some(svg) = svg_cache {
                out.push_str(&format!("```svg\n{svg}\n```\n\n"));
            }
            if !node_bindings.is_empty() {
                let rows = node_bindings
                    .iter()
                    .map(|binding| {
                        format!(
                            "{{ \"node_id\": {}, \"card_id\": {} }}",
                            serde_json::to_string(&binding.node_id).unwrap_or_default(),
                            serde_json::to_string(&binding.card_id.to_string()).unwrap_or_default()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",\n");
                out.push_str(&format!("```json\n[{rows}]\n```\n\n"));
            }
            out
        }
        ContentBlock::ImageOcclusion { resource_id, masks } => format!(
            "```json\n{}\n```\n\n",
            serde_json::to_string_pretty(&serde_json::json!({
                "resource_id": resource_id,
                "masks": masks,
            }))
            .unwrap_or_default()
        ),
        ContentBlock::InteractivePins {
            resource_id,
            overlay_resource_id,
            pins,
        } => format!(
            "```json\n{}\n```\n\n",
            serde_json::to_string_pretty(&serde_json::json!({
                "resource_id": resource_id,
                "overlay_resource_id": overlay_resource_id,
                "pins": pins,
            }))
            .unwrap_or_default()
        ),
    }
}

fn note_to_markdown(note: &Note, depth: usize) -> String {
    let mut out = String::new();
    for node in &note.outline {
        match node {
            reinforce_core::content::OutlineNode::Section {
                heading,
                intro_markdown,
                children,
            } => {
                let level = depth.min(6);
                out.push_str(&format!("{} {heading}\n\n", "#".repeat(level)));
                if let Some(intro) = intro_markdown.as_ref() {
                    out.push_str(intro);
                    out.push_str("\n\n");
                }
                out.push_str(&note_to_markdown(
                    &Note {
                        id: note.id.clone(),
                        subject: note.subject.clone(),
                        title: note.title.clone(),
                        outline: children.clone(),
                        view_settings: note.view_settings.clone(),
                        status: note.status,
                        created_at: note.created_at,
                        updated_at: note.updated_at,
                    },
                    level + 1,
                ));
            }
            reinforce_core::content::OutlineNode::CardRef { card_id } => {
                out.push_str(&format!("- Card: `{card_id}`\n"));
            }
        }
    }
    out
}

fn card_to_markdown(
    card: &KnowledgeCard,
    options: &MarkdownOptions,
    input: &ExportInput,
) -> String {
    let mut out = format!("# {}\n\n", card.title);
    for block in visible_blocks(input, &card.blocks, options.viewer_is_owner) {
        out.push_str(&block_to_markdown(&block));
    }
    out
}

fn question_to_markdown(
    question: &QuestionCard,
    options: &MarkdownOptions,
    input: &ExportInput,
) -> String {
    let mut out = format!("# {}\n\n", question.title_for_export());
    out.push_str("## Stem\n\n");
    for block in visible_blocks(input, &question.stem_blocks, options.viewer_is_owner) {
        out.push_str(&block_to_markdown(&block));
    }
    if !question.options.is_empty() {
        out.push_str("## Options\n\n");
        for option in &question.options {
            out.push_str(&format!("- {}. {}", option.key, option.content));
            if let Some(explanation) = &option.explanation {
                out.push_str(&format!(" — {explanation}"));
            }
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str("## Explanation\n\n");
    for block in visible_blocks(input, &question.explanation_blocks, options.viewer_is_owner) {
        out.push_str(&block_to_markdown(&block));
    }
    out
}

fn asset_allowed(resource: &Resource, options: &MarkdownOptions) -> bool {
    options.viewer_is_owner || resource.export_policy == ExportPolicy::Allow
}

fn sanitized_resource_policy(resource: &Resource, options: &MarkdownOptions) -> bool {
    if options.viewer_is_owner {
        return true;
    }
    match resource.export_policy {
        ExportPolicy::Allow | ExportPolicy::Redact => true,
        ExportPolicy::OwnerOnly => false,
    }
}

/// Export an input as a Markdown bundle with content-addressed assets.
pub fn export_markdown(input: &ExportInput, options: &MarkdownOptions, out: &Path) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let assets = out.join(ASSETS);
    std::fs::create_dir_all(&assets)?;

    let mut blob_hashes = BTreeSet::new();
    for resource in &input.resources {
        if !asset_allowed(resource, options) {
            continue;
        }
        if let Some(bytes) = input.blob(&resource.sha256)? {
            std::fs::write(assets.join(resource.sha256.as_str()), bytes)?;
            blob_hashes.insert(resource.sha256.to_string());
        }
    }
    for blob in &input.blobs {
        let allowed = input
            .resources
            .iter()
            .find(|resource| resource.sha256 == blob.sha256)
            .is_none_or(|resource| asset_allowed(resource, options));
        if allowed {
            std::fs::write(assets.join(blob.sha256.as_str()), &blob.bytes)?;
            blob_hashes.insert(blob.sha256.to_string());
        }
    }

    let visible_resources: Vec<Resource> = input
        .resources
        .iter()
        .filter(|resource| sanitized_resource_policy(resource, options))
        .cloned()
        .collect();
    let manifest = BundleManifest {
        blob_hashes: blob_hashes.into_iter().collect(),
        note_ids: input.notes.iter().map(|item| item.id.to_string()).collect(),
        card_ids: input.cards.iter().map(|item| item.id.to_string()).collect(),
        question_ids: input
            .questions
            .iter()
            .map(|item| item.id.to_string())
            .collect(),
    };
    let matter = FrontMatter {
        format: FORMAT.to_owned(),
        kind: "manifest".to_owned(),
        id: "manifest".to_owned(),
        status: None,
        verification: None,
        record: serde_json::to_string(&manifest)?,
        resources: visible_resources
            .iter()
            .map(serde_json::to_string)
            .collect::<serde_json::Result<Vec<_>>>()?,
        blob_dir: input
            .blob_dir
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
    };
    std::fs::write(out.join(MANIFEST), front_matter(&matter)?)?;

    for note in &input.notes {
        let id = note.id.to_string();
        write_entity(
            out,
            &EntityFile {
                directory: "notes",
                entity: note,
                kind: "note",
                id: &id,
                status: Some(note.status.as_str()),
                verification: None,
                resources: &[],
                body: &note_to_markdown(note, 2),
            },
        )?;
    }

    for card in &input.cards {
        let resources = blocked_resources(input, &card.blocks);
        let visible = visible_blocks(input, &card.blocks, options.viewer_is_owner);
        let record = KnowledgeCard {
            blocks: visible.clone(),
            ..card.clone()
        };
        let id = card.id.to_string();
        write_entity(
            out,
            &EntityFile {
                directory: "cards",
                entity: &record,
                kind: "card",
                id: &id,
                status: Some(card.status.as_str()),
                verification: Some(card.verification.as_str()),
                resources: &resources,
                body: &card_to_markdown(card, options, input),
            },
        )?;
    }

    for question in &input.questions {
        let mut blocks = question.stem_blocks.clone();
        blocks.extend(question.explanation_blocks.iter().cloned());
        let resources = blocked_resources(input, &blocks);
        let record = QuestionCard {
            stem_blocks: visible_blocks(input, &question.stem_blocks, options.viewer_is_owner),
            explanation_blocks: visible_blocks(
                input,
                &question.explanation_blocks,
                options.viewer_is_owner,
            ),
            ..question.clone()
        };
        let id = question.id.to_string();
        write_entity(
            out,
            &EntityFile {
                directory: "questions",
                entity: &record,
                kind: "question",
                id: &id,
                status: Some(question.status.as_str()),
                verification: Some(question.verification.as_str()),
                resources: &resources,
                body: &question_to_markdown(question, options, input),
            },
        )?;
    }

    Ok(())
}

fn split_front_matter(text: &str) -> Result<(FrontMatter, &str)> {
    let rest = text
        .strip_prefix("---\n")
        .ok_or_else(|| crate::Error::InvalidBundle("missing front matter".to_owned()))?;
    let end = rest
        .find("\n---\n")
        .ok_or_else(|| crate::Error::InvalidBundle("unterminated front matter".to_owned()))?;
    let matter: FrontMatter = serde_json::from_str(&rest[..end])?;
    Ok((matter, &rest[end + 5..]))
}

fn collect_markdown_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_markdown_files(&path, files)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("md") {
            files.push(path);
        }
    }
    Ok(())
}

/// Read a Markdown bundle produced by [`export_markdown`] back into core types.
pub fn import_markdown(path: &Path) -> Result<ExportInput> {
    if !path.is_dir() {
        return Err(crate::Error::InvalidBundle(
            "import_markdown expects the exported directory".to_owned(),
        ));
    }
    let manifest_text = std::fs::read_to_string(path.join(MANIFEST))?;
    let (matter, _) = split_front_matter(&manifest_text)?;
    if matter.kind != "manifest" || matter.format != FORMAT {
        return Err(crate::Error::InvalidBundle(format!(
            "unexpected manifest kind {} format {}",
            matter.kind, matter.format
        )));
    }
    let manifest: BundleManifest = serde_json::from_str(&matter.record)?;
    let resources = matter
        .resources
        .iter()
        .map(|value| serde_json::from_str(value))
        .collect::<serde_json::Result<Vec<Resource>>>()?;
    let mut blobs = Vec::new();
    for hash in &manifest.blob_hashes {
        let parsed: reinforce_core::ContentHash = hash.parse()?;
        let bytes = std::fs::read(path.join(ASSETS).join(hash))?;
        if reinforce_core::ContentHash::from_bytes(&bytes) != parsed {
            return Err(crate::Error::InvalidBundle(format!(
                "asset {hash} failed its sha256 check"
            )));
        }
        blobs.push(crate::ResourceBlob {
            sha256: parsed,
            bytes,
        });
    }

    let mut files = Vec::new();
    collect_markdown_files(path, &mut files)?;
    let mut notes: std::collections::BTreeMap<String, Note> = std::collections::BTreeMap::new();
    let mut cards: std::collections::BTreeMap<String, KnowledgeCard> =
        std::collections::BTreeMap::new();
    let mut questions: std::collections::BTreeMap<String, QuestionCard> =
        std::collections::BTreeMap::new();
    for file in files {
        if file.file_name().and_then(|value| value.to_str()) == Some(MANIFEST) {
            continue;
        }
        let text = std::fs::read_to_string(&file)?;
        let (matter, _) = split_front_matter(&text)?;
        if matter.format != FORMAT {
            return Err(crate::Error::InvalidBundle(format!(
                "unsupported format in {}",
                file.display()
            )));
        }
        if matter.kind == "manifest" {
            continue;
        }
        match matter.kind.as_str() {
            "note" => {
                notes.insert(matter.id.clone(), serde_json::from_str(&matter.record)?);
            }
            "card" => {
                cards.insert(matter.id.clone(), serde_json::from_str(&matter.record)?);
            }
            "question" => {
                questions.insert(matter.id.clone(), serde_json::from_str(&matter.record)?);
            }
            other => {
                return Err(crate::Error::InvalidBundle(format!(
                    "unknown entity kind {other}"
                )));
            }
        }
    }
    let input = ExportInput {
        resources,
        blobs,
        blob_dir: matter.blob_dir.map(PathBuf::from),
        ..Default::default()
    };

    let missing = |kind: &str, id: &str| {
        crate::Error::InvalidBundle(format!("missing {kind} record for {id}"))
    };
    let ordered_notes = manifest
        .note_ids
        .iter()
        .map(|id| notes.remove(id).ok_or_else(|| missing("note", id)))
        .collect::<Result<Vec<_>>>()?;
    let ordered_cards = manifest
        .card_ids
        .iter()
        .map(|id| cards.remove(id).ok_or_else(|| missing("card", id)))
        .collect::<Result<Vec<_>>>()?;
    let ordered_questions = manifest
        .question_ids
        .iter()
        .map(|id| questions.remove(id).ok_or_else(|| missing("question", id)))
        .collect::<Result<Vec<_>>>()?;

    Ok(ExportInput {
        notes: ordered_notes,
        cards: ordered_cards,
        questions: ordered_questions,
        ..input
    })
}

trait TitleForExport {
    fn title_for_export(&self) -> String;
}

impl TitleForExport for QuestionCard {
    fn title_for_export(&self) -> String {
        self.question_no
            .map(|number| format!("Question {number}"))
            .unwrap_or_else(|| format!("Question {}", self.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_round_trip() {
        let matter = FrontMatter {
            format: FORMAT.to_owned(),
            kind: "card".to_owned(),
            id: "card-id".to_owned(),
            status: Some("published".to_owned()),
            verification: Some("verified".to_owned()),
            record: "{\"a\":1}".to_owned(),
            resources: Vec::new(),
            blob_dir: None,
        };
        let text = front_matter(&matter).unwrap();
        let (parsed, body) = split_front_matter(&text).unwrap();
        assert_eq!(parsed.record, matter.record);
        assert_eq!(body, "");
    }
}
