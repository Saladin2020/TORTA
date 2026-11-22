use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, RwLock};

use crate::crypto::StoreCipher;
use crate::error::{DiceError, Result};
use crate::manifest::Manifest;

fn store_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn store_cipher_slot() -> &'static RwLock<Option<StoreCipher>> {
    static CIPHER: OnceLock<RwLock<Option<StoreCipher>>> = OnceLock::new();
    CIPHER.get_or_init(|| RwLock::new(None))
}

/// Enable org-wide at-rest encryption (preserves content-addressed dedup).
pub fn set_store_cipher(cipher: Option<StoreCipher>) {
    *store_cipher_slot().write().unwrap_or_else(|e| e.into_inner()) = cipher;
}

pub fn clear_store_cipher() {
    set_store_cipher(None);
}

fn active_cipher() -> Option<StoreCipher> {
    store_cipher_slot()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

pub fn with_store_lock<T>(f: impl FnOnce() -> T) -> T {
    let _guard = store_lock().lock().unwrap_or_else(|e| e.into_inner());
    f()
}

#[derive(Debug, Clone)]
pub struct StorePaths {
    pub root: PathBuf,
    pub chunks: PathBuf,
    pub manifests: PathBuf,
    pub refs: PathBuf,
}

pub fn store_paths(root: impl AsRef<Path>) -> StorePaths {
    let root = root.as_ref().to_path_buf();
    StorePaths {
        chunks: root.join("chunks"),
        manifests: root.join("manifests"),
        refs: root.join("refs.json"),
        root,
    }
}

pub fn init_store(root: impl AsRef<Path>) -> Result<StorePaths> {
    let paths = store_paths(root);
    fs::create_dir_all(&paths.chunks)?;
    fs::create_dir_all(&paths.manifests)?;
    if !paths.refs.exists() {
        fs::write(&paths.refs, "{}\n")?;
    }
    Ok(paths)
}

fn chunk_path(chunks_root: &Path, hash: &str) -> PathBuf {
    let (a, b) = hash.split_at(2.min(hash.len()));
    chunks_root.join(a).join(b)
}

pub fn has_chunk(root: impl AsRef<Path>, hash: &str) -> bool {
    chunk_path(&store_paths(root).chunks, hash).exists()
}

pub fn list_missing(root: impl AsRef<Path>, hashes: &[String]) -> Vec<String> {
    hashes
        .iter()
        .filter(|h| !has_chunk(root.as_ref(), h))
        .cloned()
        .collect()
}

pub fn read_chunk(root: impl AsRef<Path>, hash: &str) -> Result<Vec<u8>> {
    let blob = fs::read(chunk_path(&store_paths(root).chunks, hash))?;
    if let Some(cipher) = active_cipher() {
        return cipher.open(hash, &blob);
    }
    if StoreCipher::is_encrypted_blob(&blob) {
        return Err(DiceError::msg(
            "chunk is encrypted but no store key is configured",
        ));
    }
    Ok(blob)
}

/// Returns true if the chunk was newly written.
pub fn write_chunk(root: impl AsRef<Path>, hash: &str, compressed: &[u8]) -> Result<bool> {
    let paths = store_paths(root);
    let dest = chunk_path(&paths.chunks, hash);
    if dest.exists() {
        return Ok(false);
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let to_write = if let Some(cipher) = active_cipher() {
        cipher.seal(hash, compressed)?
    } else {
        compressed.to_vec()
    };
    let tmp = dest.with_extension("tmp");
    fs::write(&tmp, to_write)?;
    fs::rename(&tmp, &dest)?;
    Ok(true)
}

fn read_refs(root: &Path) -> Result<HashMap<String, u64>> {
    let paths = store_paths(root);
    match fs::read_to_string(&paths.refs) {
        Ok(raw) => Ok(serde_json::from_str(&raw)?),
        Err(_) => Ok(HashMap::new()),
    }
}

fn write_refs(root: &Path, refs: &HashMap<String, u64>) -> Result<()> {
    let paths = store_paths(root);
    fs::write(&paths.refs, serde_json::to_string(refs)?)?;
    Ok(())
}

pub fn bump_refs(root: impl AsRef<Path>, hashes: &[String]) -> Result<()> {
    let root = root.as_ref();
    let mut refs = read_refs(root)?;
    for h in hashes {
        *refs.entry(h.clone()).or_insert(0) += 1;
    }
    write_refs(root, &refs)
}

pub fn save_manifest(root: impl AsRef<Path>, manifest: &Manifest) -> Result<PathBuf> {
    let paths = store_paths(root);
    let dest = paths.manifests.join(format!("{}.json", manifest.id));
    fs::write(&dest, serde_json::to_string_pretty(manifest)? + "\n")?;
    Ok(dest)
}

pub fn load_manifest(root: impl AsRef<Path>, id: &str) -> Result<Manifest> {
    let dest = store_paths(root).manifests.join(format!("{id}.json"));
    let raw = fs::read_to_string(dest)?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn find_manifest(root: impl AsRef<Path>, query: &str) -> Result<Manifest> {
    let paths = store_paths(root);
    let exact = paths.manifests.join(format!("{query}.json"));
    if exact.exists() {
        let raw = fs::read_to_string(exact)?;
        return Ok(serde_json::from_str(&raw)?);
    }

    let mut matches = Vec::new();
    for entry in fs::read_dir(&paths.manifests)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".json") && (name.starts_with(query) || name.contains(query)) {
            matches.push(entry.path());
        }
    }
    if matches.is_empty() {
        return Err(DiceError::msg(format!("manifest not found: {query}")));
    }
    if matches.len() > 1 {
        return Err(DiceError::msg(format!(
            "ambiguous manifest \"{query}\": {} matches",
            matches.len()
        )));
    }
    let raw = fs::read_to_string(&matches[0])?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn list_manifests(root: impl AsRef<Path>) -> Result<Vec<Manifest>> {
    let paths = store_paths(root);
    let mut out = Vec::new();
    if !paths.manifests.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(&paths.manifests)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".json") {
            continue;
        }
        let raw = fs::read_to_string(entry.path())?;
        out.push(serde_json::from_str(&raw)?);
    }
    out.sort_by(|a, b| a.logical_path().cmp(b.logical_path()));
    Ok(out)
}

pub fn assemble_bytes(root: impl AsRef<Path>, manifest: &Manifest) -> Result<Vec<u8>> {
    use crate::codec::decompress;

    let mut hasher = blake3::Hasher::new();
    let mut whole = Vec::with_capacity(manifest.size as usize);

    for entry in &manifest.chunks {
        let compressed = read_chunk(root.as_ref(), &entry.hash)?;
        let raw = decompress(&compressed)?;
        if raw.len() != entry.raw_size {
            return Err(DiceError::msg(format!(
                "chunk size mismatch {}: expected {}, got {}",
                entry.hash,
                entry.raw_size,
                raw.len()
            )));
        }
        let got = blake3::hash(&raw).to_hex().to_string();
        if got != entry.hash {
            return Err(DiceError::msg(format!(
                "chunk hash mismatch {}: got {got}",
                entry.hash
            )));
        }
        hasher.update(&raw);
        whole.extend_from_slice(&raw);
    }

    let digest = hasher.finalize().to_hex().to_string();
    if digest != manifest.hash {
        return Err(DiceError::msg(format!(
            "file hash mismatch: expected {}, got {digest}",
            manifest.hash
        )));
    }
    if whole.len() as u64 != manifest.size {
        return Err(DiceError::msg(format!(
            "file size mismatch: expected {}, got {}",
            manifest.size,
            whole.len()
        )));
    }
    Ok(whole)
}
