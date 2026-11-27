//! Resource, anchor and personal metadata access for offline clients.
//!
//! Media bytes are never moved through FFI. A bundle import copies the
//! referenced `assets/` files into the database's blob directory under their
//! sha256, and clients play or render them straight from
//! [`LocalDatabase::resource_blob_path`].

use reinforce_core::content::{
    Anchor as CoreAnchor, MediaSpan as CoreMediaSpan, NormalizedRect as CoreNormalizedRect,
    Resource as CoreResource,
};
use reinforce_core::db::{content as content_store, study as study_store};
use reinforce_core::primitives::{
    AnchorKind as CoreAnchorKind, ExportPolicy as CoreExportPolicy,
    ResourceKind as CoreResourceKind, Sensitivity as CoreSensitivity,
};
use reinforce_core::{AnchorId, ResourceId, Timestamp, UserCardMeta as CoreUserCardMeta};

use crate::database::LocalDatabase;
use crate::error::{FfiError, guard, locked, parse_id};
use crate::types::{parse_card_id, parse_user_id, stamp};

/// The media kind of a stored resource.
#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum ResourceKind {
    Pdf,
    Image,
    Audio,
    Video,
    Transcript,
    Web,
}

impl From<CoreResourceKind> for ResourceKind {
    fn from(value: CoreResourceKind) -> Self {
        match value {
            CoreResourceKind::Pdf => Self::Pdf,
            CoreResourceKind::Image => Self::Image,
            CoreResourceKind::Audio => Self::Audio,
            CoreResourceKind::Video => Self::Video,
            CoreResourceKind::Transcript => Self::Transcript,
            CoreResourceKind::Web => Self::Web,
        }
    }
}

/// How far a resource may travel outside the owner's device.
#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum Sensitivity {
    Normal,
    Restricted,
}

impl From<CoreSensitivity> for Sensitivity {
    fn from(value: CoreSensitivity) -> Self {
        match value {
            CoreSensitivity::Normal => Self::Normal,
            CoreSensitivity::Restricted => Self::Restricted,
        }
    }
}

/// Export handling for restricted resources.
#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum ExportPolicy {
    Allow,
    Redact,
    OwnerOnly,
}

impl From<CoreExportPolicy> for ExportPolicy {
    fn from(value: CoreExportPolicy) -> Self {
        match value {
            CoreExportPolicy::Allow => Self::Allow,
            CoreExportPolicy::Redact => Self::Redact,
            CoreExportPolicy::OwnerOnly => Self::OwnerOnly,
        }
    }
}

/// A content-addressed media file referenced by cards and exam sets.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Resource {
    pub id: String,
    pub kind: ResourceKind,
    pub title: String,
    /// Lowercase sha256 of the blob; also the blob file name.
    pub sha256: String,
    pub byte_size: i64,
    pub page_count: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub license: Option<String>,
    pub attribution_url: Option<String>,
    pub attribution_notice: Option<String>,
    pub sensitivity: Sensitivity,
    pub export_policy: ExportPolicy,
    pub created_at: String,
}

impl From<CoreResource> for Resource {
    fn from(value: CoreResource) -> Self {
        Self {
            id: value.id.as_str(),
            kind: value.kind.into(),
            title: value.title,
            sha256: value.sha256.as_str().to_owned(),
            byte_size: value.byte_size,
            page_count: value.page_count,
            width: value.width,
            height: value.height,
            license: value.license,
            attribution_url: value.attribution_url,
            attribution_notice: value.attribution_notice,
            sensitivity: value.sensitivity.into(),
            export_policy: value.export_policy.into(),
            created_at: stamp(&value.created_at),
        }
    }
}

/// A normalised rectangle inside a resource, with `x`, `y`, `w`, `h` in
/// `0.0..=1.0`.
#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct NormalizedRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl From<CoreNormalizedRect> for NormalizedRect {
    fn from(value: CoreNormalizedRect) -> Self {
        Self {
            x: value.x,
            y: value.y,
            w: value.w,
            h: value.h,
        }
    }
}

/// A time range inside an audio or video resource.
#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct MediaSpan {
    pub start_ms: i64,
    pub end_ms: i64,
}

impl From<CoreMediaSpan> for MediaSpan {
    fn from(value: CoreMediaSpan) -> Self {
        Self {
            start_ms: value.start_ms,
            end_ms: value.end_ms,
        }
    }
}

/// Where a card points inside a resource.
#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum AnchorKind {
    PdfPage,
    Rect,
    MediaSpan,
}

impl From<CoreAnchorKind> for AnchorKind {
    fn from(value: CoreAnchorKind) -> Self {
        match value {
            CoreAnchorKind::PdfPage => Self::PdfPage,
            CoreAnchorKind::Rect => Self::Rect,
            CoreAnchorKind::MediaSpan => Self::MediaSpan,
        }
    }
}

/// A pinpoint or region a card is grounded in.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Anchor {
    pub id: String,
    pub resource_id: String,
    pub anchor_kind: AnchorKind,
    pub pdf_page: Option<i64>,
    pub printed_page: Option<i64>,
    pub legend_pdf_page: Option<i64>,
    pub rect: Option<NormalizedRect>,
    pub media_span: Option<MediaSpan>,
}

impl From<CoreAnchor> for Anchor {
    fn from(value: CoreAnchor) -> Self {
        Self {
            id: value.id.as_str(),
            resource_id: value.resource_id.as_str(),
            anchor_kind: value.anchor_kind.into(),
            pdf_page: value.pdf_page,
            printed_page: value.printed_page,
            legend_pdf_page: value.legend_pdf_page,
            rect: value.rect.map(NormalizedRect::from),
            media_span: value.media_span.map(MediaSpan::from),
        }
    }
}

/// Per-user personalisation of one card.
#[derive(Debug, Clone, uniffi::Record)]
pub struct UserCardMeta {
    pub user_id: String,
    pub card_id: String,
    pub starred: bool,
    pub personal_tags: Vec<String>,
    pub personal_note: Option<String>,
    pub updated_at: String,
}

impl From<CoreUserCardMeta> for UserCardMeta {
    fn from(value: CoreUserCardMeta) -> Self {
        Self {
            user_id: value.user_id.as_str(),
            card_id: value.card_id.as_str(),
            starred: value.starred,
            personal_tags: value.personal_tags,
            personal_note: value.personal_note,
            updated_at: stamp(&value.updated_at),
        }
    }
}

#[allow(unsafe_code)]
#[uniffi::export]
impl LocalDatabase {
    /// Reads one resource row.
    pub fn resource(&self, resource_id: String) -> Result<Resource, FfiError> {
        guard("read resource", || {
            let resource_id: ResourceId = parse_id("resource id", &resource_id)?;
            let database = locked("database", &self.inner)?;
            content_store::get_resource(database.connection(), &resource_id)?
                .map(Resource::from)
                .ok_or_else(|| FfiError::missing("resource does not exist"))
        })
    }

    /// Reads one anchor row.
    pub fn anchor(&self, anchor_id: String) -> Result<Anchor, FfiError> {
        guard("read anchor", || {
            let anchor_id: AnchorId = parse_id("anchor id", &anchor_id)?;
            let database = locked("database", &self.inner)?;
            content_store::get_anchor(database.connection(), &anchor_id)?
                .map(Anchor::from)
                .ok_or_else(|| FfiError::missing("anchor does not exist"))
        })
    }

    /// Returns the absolute path of a resource's local blob.
    ///
    /// Fails with `NotFound` when the resource is unknown or its bytes were
    /// never imported. The file name is the resource sha256, so two resources
    /// with identical bytes share one blob.
    pub fn resource_blob_path(&self, resource_id: String) -> Result<String, FfiError> {
        guard("resolve resource blob", || {
            let resource_id: ResourceId = parse_id("resource id", &resource_id)?;
            let resource = {
                let database = locked("database", &self.inner)?;
                content_store::get_resource(database.connection(), &resource_id)?
                    .ok_or_else(|| FfiError::missing("resource does not exist"))?
            };
            let path = self.blobs_dir.join(resource.sha256.as_str());
            if !path.is_file() {
                return Err(FfiError::missing(
                    "resource blob is not available locally; import a bundle that carries it",
                ));
            }
            Ok(std::path::absolute(&path)?.to_string_lossy().into_owned())
        })
    }

    /// Reads a user's personal metadata for one card.
    pub fn user_card_meta(
        &self,
        user_id: String,
        card_id: String,
    ) -> Result<Option<UserCardMeta>, FfiError> {
        guard("read user card meta", || {
            let user_id = parse_user_id(&user_id)?;
            let card_id = parse_card_id(&card_id)?;
            let database = locked("database", &self.inner)?;
            Ok(
                study_store::get_user_card_meta(database.connection(), &user_id, &card_id)?
                    .map(UserCardMeta::from),
            )
        })
    }

    /// Writes a user's star, personal tags and personal note for one card and
    /// returns the stored row. Values are stored as given; an empty tag is not
    /// filtered out.
    pub fn save_user_card_meta(
        &self,
        user_id: String,
        card_id: String,
        starred: bool,
        personal_tags: Vec<String>,
        personal_note: Option<String>,
    ) -> Result<UserCardMeta, FfiError> {
        guard("save user card meta", || {
            let user_id = parse_user_id(&user_id)?;
            let card_id = parse_card_id(&card_id)?;
            let meta = CoreUserCardMeta {
                user_id,
                card_id,
                starred,
                personal_tags,
                personal_note,
                updated_at: Timestamp::now(),
            };
            let database = locked("database", &self.inner)?;
            study_store::save_user_card_meta(database.connection(), &meta)?;
            Ok(UserCardMeta::from(meta))
        })
    }
}
