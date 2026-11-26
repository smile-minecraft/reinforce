//! PDF and lossless Markdown export for Reinforce.
//!
//! This crate is a pure library: callers select the notes, cards and questions
//! to export and pass them in an [`ExportInput`]. Fonts are read from the
//! caller's directory at render time and are never embedded in the crate.

pub mod markdown;
pub mod math;
pub mod model;
pub mod pdf;
pub mod typst_source;

pub use markdown::{export_markdown, import_markdown};
pub use math::latex_to_typst;
pub use model::{Error, ExportInput, MarkdownOptions, PdfOptions, PdfPreset, ResourceBlob, Result};
pub use pdf::render_pdf;
pub use typst_source::{
    TypstDocument, escape_typst_string, markdown_to_typst, render_typst_source,
};

pub use reinforce_core::VERSION;
