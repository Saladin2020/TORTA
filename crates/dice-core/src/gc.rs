use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use crate::error::{DiceError, Result};
use crate::lock::with_store_file_lock;
use crate::store::store_paths;

#[derive(Debug, Clone, Default)]
pub struct GcReport {
    pub chunks_deleted: usize,
    pub bytes_reclaimed: u64,
    pub live_chunks: usize,
}

/// Rewrite refs from the live hash set, then delete chunk files not in that set.
pub fn gc_unreferenced(root: impl AsRef<Path>, live_hashes: &HashSet<String>) -> Result<GcReport> {
    let root = root.as_ref().to_path_buf();
    // with_store_file_lock already holds the in-process mutex — do not nest with_store_lock.
    with_store_file_lock(&root, || {
        let paths = store_paths(&root);
        let mut refs: HashMap<String, u64> = HashMap::new();
        for h in live_hashes {
            *refs.entry(h.clone()).or_insert(0) += 1;
        }
        fs::write(&paths.refs, serde_json::to_string(&refs)?)?;

        let mut report = GcReport {
            live_chunks: live_hashes.len(),
            ..Default::default()
        };

        if !paths.chunks.exists() {
            return Ok(report);
        }

        for shard in fs::read_dir(&paths.chunks)? {
            let shard = shard?;
            if !shard.file_type()?.is_dir() {
                continue;
            }
            for entry in fs::read_dir(shard.path())? {
                let entry = entry?;
                if !entry.file_type()?.is_file() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".tmp") {
                    let _ = fs::remove_file(entry.path());
                    continue;
                }
                let hash = format!("{}{}", shard.file_name().to_string_lossy(), name);
                if live_hashes.contains(&hash) {
                    continue;
                }
                let meta = entry.metadata()?;
                let len = meta.len();
                fs::remove_file(entry.path())?;
                report.chunks_deleted += 1;
                report.bytes_reclaimed += len;
            }
        }

        Ok(report)
    })
}

/// Sum compressed size of all chunk files currently on disk.
pub fn measure_store_bytes(root: impl AsRef<Path>) -> Result<u64> {
    let paths = store_paths(root);
    if !paths.chunks.exists() {
        return Ok(0);
    }
    let mut total = 0u64;
    for shard in fs::read_dir(&paths.chunks)? {
        let shard = shard?;
        if !shard.file_type()?.is_dir() {
            continue;
        }
        for entry in fs::read_dir(shard.path())? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".tmp") {
                    continue;
                }
                total += entry.metadata()?.len();
            }
        }
    }
    Ok(total)
}

pub fn extract_hashes_from_manifest_json(value: &serde_json::Value) -> Result<HashSet<String>> {
    let mut out = HashSet::new();
    let chunks = value
        .get("chunks")
        .and_then(|c| c.as_array())
        .ok_or_else(|| DiceError::msg("manifest missing chunks"))?;
    for c in chunks {
        if let Some(h) = c.get("hash").and_then(|h| h.as_str()) {
            out.insert(h.to_string());
        }
    }
    Ok(out)
}
