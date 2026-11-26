//! Reader for the public newline-delimited bundle format.
//!
//! A bundle is either a `bundle.jsonl` file or a directory containing
//! `bundle.jsonl` and an optional `assets/` folder. The format itself is
//! documented in `BUNDLE_FORMAT.md` next to this crate.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use reinforce_core::{BatchOperation, ContentHash, OperationKind};
use serde_json::Value;

use crate::error::{ApiError, ApiResult};

pub const BUNDLE_FILE: &str = "bundle.jsonl";
pub const ASSETS_DIR: &str = "assets";

/// A content-addressed file discovered under `assets/`.
#[derive(Debug, Clone)]
pub struct AssetFile {
    pub hash: ContentHash,
    pub source: PathBuf,
}

/// Reads every non-empty line as one batch operation.
pub fn read_operations(path: &Path) -> ApiResult<Vec<BatchOperation>> {
    let file = bundle_file(path);
    let content = std::fs::read_to_string(&file).map_err(|error| {
        ApiError::bad_request(BUNDLE_FILE, format!("{}: {error}", file.display()))
    })?;
    let mut operations = Vec::new();
    for (index, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let operation = serde_json::from_str::<BatchOperation>(line).map_err(|error| {
            ApiError::bad_request(
                format!("{BUNDLE_FILE} line {}", index + 1),
                error.to_string(),
            )
        })?;
        operations.push(operation);
    }
    Ok(operations)
}

/// Locates `bundle.jsonl` for both accepted input shapes.
pub fn bundle_file(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.join(BUNDLE_FILE)
    } else {
        path.to_path_buf()
    }
}

/// Assets live beside `bundle.jsonl` for directory inputs, or in `assets/`
/// next to a single file input. A missing directory means no binary payloads.
pub fn assets_dir(path: &Path) -> Option<PathBuf> {
    let root = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    let directory = root.join(ASSETS_DIR);
    directory.is_dir().then_some(directory)
}

/// Hashes every regular file in `assets/` and keeps the ones referenced by a
/// resource operation. Unreferenced files are ignored so a bundle can carry
/// extra source material without publishing it.
pub fn referenced_assets(path: &Path, operations: &[BatchOperation]) -> ApiResult<Vec<AssetFile>> {
    let Some(directory) = assets_dir(path) else {
        return Ok(Vec::new());
    };
    let required: BTreeSet<String> = operations
        .iter()
        .filter(|operation| operation.kind() == OperationKind::Resource)
        .filter_map(|operation| operation.data().get("sha256").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    if required.is_empty() {
        return Ok(Vec::new());
    }
    let mut assets = Vec::new();
    for entry in std::fs::read_dir(&directory)
        .map_err(|error| ApiError::bad_request(ASSETS_DIR, error.to_string()))?
    {
        let entry = entry.map_err(|error| ApiError::bad_request(ASSETS_DIR, error.to_string()))?;
        let file_type = entry
            .file_type()
            .map_err(|error| ApiError::bad_request(ASSETS_DIR, error.to_string()))?;
        if !file_type.is_file() {
            continue;
        }
        let bytes = std::fs::read(entry.path())
            .map_err(|error| ApiError::bad_request(ASSETS_DIR, error.to_string()))?;
        let hash = ContentHash::from_bytes(&bytes);
        if required.contains(hash.as_str()) {
            assets.push(AssetFile {
                hash,
                source: entry.path(),
            });
        }
    }
    Ok(assets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_assets_directory_yields_no_assets() {
        let path = Path::new("does-not-exist/bundle.jsonl");
        assert_eq!(assets_dir(path), None);
    }
}
