//! Bundle discovery for offline imports.
//!
//! A bundle is either a `bundle.jsonl` file or a directory holding
//! `bundle.jsonl` plus an optional `assets/` folder. The reader mirrors the
//! server importer so one package produces the same rows and blobs on a device
//! as it does on the server.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use reinforce_core::{BatchOperation, ContentHash, OperationKind};
use serde_json::Value;
use uuid::Uuid;

use crate::error::FfiError;

pub(crate) const BUNDLE_FILE: &str = "bundle.jsonl";
pub(crate) const ASSETS_DIR: &str = "assets";

/// A content-addressed file discovered under `assets/`.
pub(crate) struct AssetFile {
    pub hash: ContentHash,
    pub source: PathBuf,
}

/// Reads every non-empty line of a bundle document as one batch operation.
pub(crate) fn read_operations_from_text(content: &str) -> Result<Vec<BatchOperation>, FfiError> {
    let mut operations = Vec::new();
    for (index, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let operation = serde_json::from_str::<BatchOperation>(line.trim()).map_err(|error| {
            FfiError::invalid(format!("bundle line {}", index + 1), error.to_string())
        })?;
        operations.push(operation);
    }
    Ok(operations)
}

/// Reads every non-empty line as one batch operation.
pub(crate) fn read_operations(path: &Path) -> Result<Vec<BatchOperation>, FfiError> {
    let file = bundle_file(path);
    let content = std::fs::read_to_string(&file)?;
    read_operations_from_text(&content)
}

/// Locates `bundle.jsonl` for both accepted input shapes.
pub(crate) fn bundle_file(path: &Path) -> PathBuf {
    if path.is_dir() {
        path.join(BUNDLE_FILE)
    } else {
        path.to_path_buf()
    }
}

/// Assets live beside `bundle.jsonl` for directory inputs, or in `assets/`
/// next to a single file input. A missing directory means no binary payloads.
pub(crate) fn assets_dir(path: &Path) -> Option<PathBuf> {
    let root = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    let directory = root.join(ASSETS_DIR);
    directory.is_dir().then_some(directory)
}

/// Hashes every regular file in an `assets/` directory and keeps the ones a
/// resource operation references. Unreferenced files are ignored so a bundle
/// can carry extra source material without publishing it.
pub(crate) fn assets_in(
    directory: &Path,
    operations: &[BatchOperation],
) -> Result<Vec<AssetFile>, FfiError> {
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
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let bytes = std::fs::read(entry.path())?;
        let hash = ContentHash::from_bytes(&bytes);
        if required.contains(hash.as_str()) {
            assets.push(AssetFile {
                hash,
                source: entry.path(),
            });
        }
    }
    assets.sort_by(|left, right| left.hash.as_str().cmp(right.hash.as_str()));
    Ok(assets)
}

/// Hashes every regular file in `assets/` and keeps the ones a resource
/// operation references. Unreferenced files are ignored so a bundle can carry
/// extra source material without publishing it.
pub(crate) fn referenced_assets(
    path: &Path,
    operations: &[BatchOperation],
) -> Result<Vec<AssetFile>, FfiError> {
    let Some(directory) = assets_dir(path) else {
        return Ok(Vec::new());
    };
    assets_in(&directory, operations)
}

/// Copies content-addressed assets into `blobs_dir`, returning how many files
/// were newly written.
///
/// Blobs are content-addressed, so an existing file is never rewritten and an
/// interrupted copy never becomes visible: each asset is copied to a hidden
/// temporary file and renamed into place.
pub(crate) fn copy_assets(blobs_dir: &Path, assets: &[AssetFile]) -> Result<u32, FfiError> {
    if assets.is_empty() {
        return Ok(0);
    }
    std::fs::create_dir_all(blobs_dir)?;

    let mut written = 0;
    for asset in assets {
        let target = blobs_dir.join(asset.hash.as_str());
        if target.is_file() {
            continue;
        }
        let temporary = blobs_dir.join(format!(".{}.{}.tmp", asset.hash.as_str(), Uuid::now_v7()));
        if let Err(error) = std::fs::copy(&asset.source, &temporary) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error.into());
        }
        if let Err(error) = std::fs::rename(&temporary, &target) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error.into());
        }
        written += 1;
    }
    Ok(written)
}

/// Copies every referenced asset into `blobs_dir` under its sha256, returning
/// how many files were newly written.
pub(crate) fn copy_referenced_assets(
    blobs_dir: &Path,
    bundle_path: &Path,
    operations: &[BatchOperation],
) -> Result<u32, FfiError> {
    let assets = referenced_assets(bundle_path, operations)?;
    copy_assets(blobs_dir, &assets)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("reinforce-ffi-bundle-{}", Uuid::now_v7()));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn operation(line: &str) -> BatchOperation {
        serde_json::from_str(line).unwrap()
    }

    fn resource_operation(hash: &str) -> BatchOperation {
        operation(&format!(
            r#"{{"op":"resource","data":{{"id":"01a21714-c98c-7bfe-8cf8-a60b84573f56","kind":"image","title":"t","sha256":"{hash}","byte_size":3,"page_count":null,"width":1,"height":1,"license":null,"attribution_url":null,"attribution_notice":null,"sensitivity":"normal","export_policy":"allow","created_at":"2026-01-01T00:00:00.000000Z"}}}}"#
        ))
    }

    #[test]
    fn missing_assets_directory_yields_no_assets() {
        assert_eq!(assets_dir(Path::new("does-not-exist/bundle.jsonl")), None);
        assert_eq!(
            referenced_assets(Path::new("does-not-exist/bundle.jsonl"), &[])
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn only_referenced_assets_are_collected_and_copied_once() {
        let temp = TempDir::new();
        let assets = temp.path.join(ASSETS_DIR);
        std::fs::create_dir_all(&assets).unwrap();
        std::fs::write(assets.join("referenced.svg"), b"abc").unwrap();
        std::fs::write(assets.join("unreferenced.svg"), b"xyz").unwrap();
        let hash = ContentHash::from_bytes(b"abc");
        let operations = vec![resource_operation(hash.as_str())];

        let collected = referenced_assets(&temp.path, &operations).unwrap();
        assert_eq!(collected.len(), 1, "unreferenced files stay out");
        assert_eq!(collected[0].hash, hash);

        let blobs = temp.path.join("blobs");
        assert_eq!(
            copy_referenced_assets(&blobs, &temp.path, &operations).unwrap(),
            1
        );
        let blob = blobs.join(hash.as_str());
        assert_eq!(std::fs::read(&blob).unwrap(), b"abc");

        assert_eq!(
            copy_referenced_assets(&blobs, &temp.path, &operations).unwrap(),
            0,
            "a repeated import reuses the content-addressed blob"
        );
        let entries: Vec<_> = std::fs::read_dir(&blobs)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries.len(), 1, "no duplicate or temporary files remain");
    }

    #[test]
    fn directory_bundles_and_single_files_resolve_the_same_assets() {
        let temp = TempDir::new();
        let assets = temp.path.join(ASSETS_DIR);
        std::fs::create_dir_all(&assets).unwrap();
        std::fs::write(assets.join("a.svg"), b"abc").unwrap();
        std::fs::write(temp.path.join(BUNDLE_FILE), b"").unwrap();

        assert_eq!(bundle_file(&temp.path), temp.path.join(BUNDLE_FILE));
        let single = temp.path.join(BUNDLE_FILE);
        assert_eq!(bundle_file(&single), single);
        assert_eq!(assets_dir(&temp.path), assets_dir(&single));
    }
}
