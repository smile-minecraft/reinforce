use std::path::{Path, PathBuf};

use reinforce_core::content::ContentBlock;
use reinforce_core::primitives::ExportPolicy;
use reinforce_core::{ContentHash, Resource, ResourceId};
use serde::{Deserialize, Serialize};

/// A content-addressed blob supplied by the caller.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceBlob {
    pub sha256: ContentHash,
    pub bytes: Vec<u8>,
}

/// Everything an export needs, already selected by the caller.
///
/// The library never queries the database and never decides which cards are
/// in scope. Blobs may be provided inline or read lazily from `blob_dir`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExportInput {
    pub notes: Vec<reinforce_core::Note>,
    pub cards: Vec<reinforce_core::KnowledgeCard>,
    pub questions: Vec<reinforce_core::QuestionCard>,
    pub resources: Vec<Resource>,
    pub blobs: Vec<ResourceBlob>,
    pub blob_dir: Option<PathBuf>,
}

impl ExportInput {
    /// Resolve a blob from inline data first, then from the caller's blob dir.
    pub fn blob(&self, sha256: &ContentHash) -> Result<Option<Vec<u8>>> {
        if let Some(blob) = self.blobs.iter().find(|blob| &blob.sha256 == sha256) {
            return Ok(Some(blob.bytes.clone()));
        }
        let Some(dir) = &self.blob_dir else {
            return Ok(None);
        };
        let path = dir.join(sha256.as_str());
        if !path.is_file() {
            return Ok(None);
        }
        let bytes = std::fs::read(path)?;
        if &ContentHash::from_bytes(&bytes) != sha256 {
            return Err(Error::InvalidBundle(format!(
                "blob at {} does not match {}",
                dir.display(),
                sha256
            )));
        }
        Ok(Some(bytes))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("core error: {0}")]
    Core(#[from] reinforce_core::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("typst compilation failed: {0}")]
    Typst(String),
    #[error("pdf export failed: {0}")]
    Pdf(String),
    #[error("no usable export fonts: {0}")]
    FontsUnavailable(String),
    #[error("no font files found in {0}")]
    NoFonts(PathBuf),
    #[error("missing blob for sha256 {0}")]
    MissingBlob(String),
    #[error("invalid export bundle: {0}")]
    InvalidBundle(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PdfPreset {
    /// Compact handout with repeating table headers and vector diagrams.
    #[default]
    Standard,
    /// Wide leading and a 32% right-hand annotation margin.
    GoodNotes,
    /// Pinned-diagram recall page with an answer key on the next page.
    Worksheet,
}

impl PdfPreset {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::GoodNotes => "goodnotes",
            Self::Worksheet => "worksheet",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PdfOptions {
    pub preset: PdfPreset,
    /// Directory of open-licensed `.ttf` / `.otf` / `.ttc` files supplied by the caller.
    pub fonts_dir: PathBuf,
    /// Optional second deep link target, for example `https://notes.example.test`.
    pub web_base_url: Option<String>,
    pub viewer_is_owner: bool,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            preset: PdfPreset::Standard,
            fonts_dir: PathBuf::from("fonts"),
            web_base_url: None,
            viewer_is_owner: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MarkdownOptions {
    pub viewer_is_owner: bool,
}

pub(crate) enum ResourceTreatment<'a> {
    Include(&'a Resource),
    Redacted(&'a Resource),
    Skip,
}

pub(crate) fn resource_treatment<'a>(
    input: &'a ExportInput,
    resource_id: &ResourceId,
    viewer_is_owner: bool,
) -> ResourceTreatment<'a> {
    let Some(resource) = input.resources.iter().find(|item| &item.id == resource_id) else {
        return ResourceTreatment::Skip;
    };
    if viewer_is_owner {
        return ResourceTreatment::Include(resource);
    }
    match resource.export_policy {
        ExportPolicy::Allow => ResourceTreatment::Include(resource),
        ExportPolicy::Redact => ResourceTreatment::Redacted(resource),
        ExportPolicy::OwnerOnly => ResourceTreatment::Skip,
    }
}

fn redaction_notice(resource: &Resource) -> ContentBlock {
    ContentBlock::Markdown {
        text: format!(
            "[Restricted resource: {}. Content redacted for this viewer.]",
            resource.title
        ),
    }
}

/// Apply export policy to a block, dropping blocks that exist only for hidden media.
pub(crate) fn visible_block(
    input: &ExportInput,
    block: &ContentBlock,
    viewer_is_owner: bool,
) -> Option<ContentBlock> {
    let resource_ids: Vec<&ResourceId> = match block {
        ContentBlock::ImageOcclusion { resource_id, .. } => vec![resource_id],
        ContentBlock::InteractivePins {
            resource_id,
            overlay_resource_id,
            ..
        } => {
            let mut ids = vec![resource_id];
            if let Some(id) = overlay_resource_id {
                ids.push(id);
            }
            ids
        }
        _ => return Some(block.clone()),
    };

    let mut redacted = None;
    for id in resource_ids {
        match resource_treatment(input, id, viewer_is_owner) {
            ResourceTreatment::Include(_) => {}
            ResourceTreatment::Redacted(resource) => redacted = Some(resource),
            ResourceTreatment::Skip => return None,
        }
    }
    Some(
        redacted
            .map(redaction_notice)
            .unwrap_or_else(|| block.clone()),
    )
}

pub(crate) fn visible_blocks<'a>(
    input: &'a ExportInput,
    blocks: &'a [ContentBlock],
    viewer_is_owner: bool,
) -> Vec<ContentBlock> {
    blocks
        .iter()
        .filter_map(|block| visible_block(input, block, viewer_is_owner))
        .collect()
}

pub(crate) fn asset_extension(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "jpg"
    } else if bytes.starts_with(b"GIF8") {
        "gif"
    } else if bytes.starts_with(b"BM") {
        "bmp"
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "webp"
    } else if bytes.starts_with(b"<svg") || bytes.starts_with(b"<?xml") {
        "svg"
    } else {
        "bin"
    }
}

pub(crate) fn is_font_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("ttf" | "otf" | "ttc")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use reinforce_core::content::ContentBlock;
    use reinforce_core::primitives::{ExportPolicy, ResourceKind, Sensitivity};
    use reinforce_core::{ResourceId, Timestamp};

    fn resource(policy: ExportPolicy) -> Resource {
        Resource {
            id: ResourceId::generate(),
            kind: ResourceKind::Image,
            title: "specimen".to_owned(),
            sha256: ContentHash::from_bytes(b"specimen"),
            byte_size: 8,
            page_count: None,
            width: Some(100),
            height: Some(80),
            license: None,
            attribution_url: None,
            attribution_notice: None,
            sensitivity: Sensitivity::Restricted,
            export_policy: policy,
            created_at: Timestamp::unix_epoch(),
        }
    }

    #[test]
    fn owner_only_blocks_are_dropped_for_non_owners() {
        let resource = resource(ExportPolicy::OwnerOnly);
        let input = ExportInput {
            resources: vec![resource.clone()],
            ..Default::default()
        };
        let block = ContentBlock::ImageOcclusion {
            resource_id: resource.id.clone(),
            masks: Vec::new(),
        };
        assert!(visible_block(&input, &block, false).is_none());
        assert!(visible_block(&input, &block, true).is_some());
    }

    #[test]
    fn redacted_blocks_keep_only_a_notice_for_non_owners() {
        let resource = resource(ExportPolicy::Redact);
        let input = ExportInput {
            resources: vec![resource.clone()],
            ..Default::default()
        };
        let block = ContentBlock::ImageOcclusion {
            resource_id: resource.id.clone(),
            masks: Vec::new(),
        };
        let Some(ContentBlock::Markdown { text }) = visible_block(&input, &block, false) else {
            panic!("redacted resource should become a markdown notice");
        };
        assert!(text.contains("Content redacted"));
        assert!(visible_block(&input, &block, true) == Some(block));
    }
}
