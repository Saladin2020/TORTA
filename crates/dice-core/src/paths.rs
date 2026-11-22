use std::path::{Component, Path, PathBuf};

use crate::error::{DiceError, Result};

pub fn normalize_relative_path(input: &str) -> Result<String> {
    let trimmed = input.trim().trim_matches(|c| c == '/' || c == '\\');
    if trimmed.is_empty() {
        return Err(DiceError::msg("relative path is empty"));
    }
    if trimmed.contains('\0') {
        return Err(DiceError::msg("relative path contains NUL"));
    }

    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Err(DiceError::msg(format!(
            "absolute path not allowed: {trimmed}"
        )));
    }

    let mut parts: Vec<String> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::Normal(s) => {
                let s = s.to_string_lossy();
                if s.is_empty() || s == "." {
                    continue;
                }
                if s == ".." {
                    return Err(DiceError::msg("relative path must not contain '..'"));
                }
                if s.contains(':') {
                    return Err(DiceError::msg(format!("invalid path segment \"{s}\"")));
                }
                parts.push(s.into_owned());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(DiceError::msg("relative path must not contain '..'"));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(DiceError::msg(format!(
                    "absolute path not allowed: {trimmed}"
                )));
            }
        }
    }

    if parts.is_empty() {
        return Err(DiceError::msg("relative path is empty"));
    }

    Ok(parts.join("/"))
}

pub fn relative_under(base: impl AsRef<Path>, file: impl AsRef<Path>) -> Result<String> {
    let base = base.as_ref();
    let file = file.as_ref();
    let rel = file.strip_prefix(base).map_err(|_| {
        DiceError::msg(format!(
            "path {} is outside base {}",
            file.display(),
            base.display()
        ))
    })?;
    normalize_relative_path(&rel.to_string_lossy())
}

pub fn safe_join(dest: impl AsRef<Path>, relative: &str) -> Result<PathBuf> {
    let dest = dest.as_ref();
    let norm = normalize_relative_path(relative)?;
    let mut out = dest.to_path_buf();
    for part in norm.split('/') {
        out.push(part);
    }

    let dest_canon = std::fs::canonicalize(dest).map_err(|e| {
        DiceError::msg(format!("canonicalize {}: {e}", dest.display()))
    })?;

    let out_check = if out.exists() {
        std::fs::canonicalize(&out).map_err(|e| {
            DiceError::msg(format!("canonicalize {}: {e}", out.display()))
        })?
    } else if let Some(parent) = out.parent() {
        let parent_existing = first_existing_ancestor(parent)?;
        let parent_canon = std::fs::canonicalize(&parent_existing).map_err(|e| {
            DiceError::msg(format!("canonicalize {}: {e}", parent_existing.display()))
        })?;
        if !is_under(&parent_canon, &dest_canon) {
            return Err(DiceError::msg(format!(
                "refusing path escape: {}",
                out.display()
            )));
        }
        let suffix = out
            .strip_prefix(&parent_existing)
            .unwrap_or(Path::new(""));
        parent_canon.join(suffix)
    } else {
        return Err(DiceError::msg("invalid destination path"));
    };

    if !is_under(&out_check, &dest_canon) && out_check != dest_canon {
        return Err(DiceError::msg(format!(
            "refusing path escape: {}",
            out.display()
        )));
    }

    Ok(out)
}

fn first_existing_ancestor(path: &Path) -> Result<PathBuf> {
    let mut cur = path.to_path_buf();
    loop {
        if cur.exists() {
            return Ok(cur);
        }
        if !cur.pop() {
            return Err(DiceError::msg(format!(
                "no existing ancestor for {}",
                path.display()
            )));
        }
    }
}

fn is_under(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
}
