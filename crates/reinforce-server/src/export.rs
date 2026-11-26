//! Rendering and packaging helpers shared by the CLI and the HTTP endpoint.

use std::io::Write;
use std::path::{Path, PathBuf};

use axum::http::StatusCode;
use reinforce_export::{
    Error as ExportError, ExportInput, MarkdownOptions, PdfOptions, PdfPreset, export_markdown,
    render_pdf,
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone)]
pub enum ExportJob {
    Pdf {
        preset: PdfPreset,
        fonts_dir: PathBuf,
        web_base_url: Option<String>,
        viewer_is_owner: bool,
    },
    Markdown {
        viewer_is_owner: bool,
    },
}

impl ExportJob {
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Pdf { .. } => "pdf",
            Self::Markdown { .. } => "zip",
        }
    }

    pub fn content_type(&self) -> &'static str {
        match self {
            Self::Pdf { .. } => "application/pdf",
            Self::Markdown { .. } => "application/zip",
        }
    }
}

pub fn empty_export() -> ApiError {
    ApiError::new(
        StatusCode::UNPROCESSABLE_ENTITY,
        "empty_export",
        "nothing matched this export scope",
    )
}

pub fn fonts_missing(path: &Path) -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "fonts_missing",
        format!("no export fonts found in {}", path.display()),
    )
}

fn is_font(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("ttf" | "otf" | "ttc")
    )
}

fn has_fonts(dir: &Path) -> std::io::Result<bool> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            if has_fonts(&path)? {
                return Ok(true);
            }
        } else if is_font(&path) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn map_export_error(error: ExportError) -> ApiError {
    match error {
        ExportError::NoFonts(_) => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "fonts_missing",
            "no export fonts available",
        ),
        ExportError::Io(source) if source.kind() == std::io::ErrorKind::NotFound => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "fonts_missing",
            "export fonts directory is missing",
        ),
        other => ApiError::internal(format!("export failed: {other}")),
    }
}

pub fn has_content(input: &ExportInput) -> bool {
    !(input.notes.is_empty() && input.cards.is_empty() && input.questions.is_empty())
}

pub fn render_blocking(input: &ExportInput, job: ExportJob) -> ApiResult<Vec<u8>> {
    match job {
        ExportJob::Pdf {
            preset,
            fonts_dir,
            web_base_url,
            viewer_is_owner,
        } => {
            if !has_fonts(&fonts_dir).map_err(|_| fonts_missing(&fonts_dir))? {
                return Err(fonts_missing(&fonts_dir));
            }
            render_pdf(
                input,
                &PdfOptions {
                    preset,
                    fonts_dir,
                    web_base_url,
                    viewer_is_owner,
                },
            )
            .map_err(map_export_error)
        }
        ExportJob::Markdown { viewer_is_owner } => {
            let directory = ExportDirectory::new();
            export_markdown(
                input,
                &MarkdownOptions { viewer_is_owner },
                directory.path(),
            )
            .map_err(map_export_error)?;
            zip_directory(directory.path()).map_err(map_export_error)
        }
    }
}

pub async fn render(input: ExportInput, job: ExportJob) -> ApiResult<Vec<u8>> {
    tokio::task::spawn_blocking(move || render_blocking(&input, job))
        .await
        .map_err(|error| ApiError::internal(format!("export task failed: {error}")))?
}

pub fn write_markdown_directory(
    input: &ExportInput,
    viewer_is_owner: bool,
    out: &Path,
) -> ApiResult<()> {
    export_markdown(input, &MarkdownOptions { viewer_is_owner }, out).map_err(map_export_error)
}

struct ExportDirectory(PathBuf);

impl ExportDirectory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("reinforce-export-{}", uuid::Uuid::now_v7())))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ExportDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn zip_directory(root: &Path) -> Result<Vec<u8>, ExportError> {
    let mut paths = Vec::new();
    collect_files(root, root, &mut paths)?;
    paths.sort();
    let mut writer = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, path) in paths {
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        writer.start_file(name, options).map_err(|error| {
            ExportError::InvalidBundle(format!("cannot add zip entry: {error}"))
        })?;
        writer.write_all(&std::fs::read(&path)?)?;
    }
    let cursor = writer.finish().map_err(|error| {
        ExportError::InvalidBundle(format!("cannot finish zip archive: {error}"))
    })?;
    Ok(cursor.into_inner())
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .iter()
                .map(|part| part.to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            out.push((relative, path));
        }
    }
    Ok(())
}

pub fn safe_filename(scope: &str, extension: &str) -> String {
    let mut stem = String::with_capacity(scope.len().min(64) + 12);
    for character in scope.chars() {
        if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
            stem.push(character);
        } else if stem.ends_with('-') {
            continue;
        } else {
            stem.push('-');
        }
    }
    let stem = stem.trim_matches('-');
    let stem = if stem.is_empty() { "reinforce" } else { stem };
    let stem = stem.chars().take(64).collect::<String>();
    format!("{stem}.{extension}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_are_safe() {
        assert_eq!(safe_filename("physics/notes", "pdf"), "physics-notes.pdf");
        assert_eq!(safe_filename("../bad name", "zip"), "bad-name.zip");
        assert_eq!(safe_filename("***", "pdf"), "reinforce.pdf");
    }
}
