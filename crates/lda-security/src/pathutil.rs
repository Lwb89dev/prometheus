use std::path::{Component, Path, PathBuf};

use lda_common::{Error, Result};

/// Resolve `requested` so the result stays inside `root`.
///
/// Absolute paths, `..`, and symlinks that leave `root` are rejected.
/// `root` must already exist. The final path may not exist yet.
pub fn resolve_within(root: &Path, requested: &Path) -> Result<PathBuf> {
    if path_has_nul(requested) || path_has_nul(root) {
        return Err(Error::InvalidInput("path contains a NUL byte".into()));
    }
    let root = root
        .canonicalize()
        .map_err(|e| Error::InvalidInput(format!("root is not available: {e}")))?;
    let combined = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        root.join(requested)
    };
    let lexical = normalize_lexical(&combined)?;
    if !is_inside(&root, &lexical) {
        return Err(Error::PermissionDenied(
            "path escapes the allowed root".into(),
        ));
    }
    let relative = lexical
        .strip_prefix(&root)
        .map_err(|_| Error::PermissionDenied("path escapes the allowed root".into()))?;
    let mut current = root.clone();
    for component in relative.components() {
        current.push(component);
        if !current.exists() {
            continue;
        }
        let canon = std::fs::canonicalize(&current)?;
        if !is_inside(&root, &canon) {
            return Err(Error::PermissionDenied(
                "symlink escapes the allowed root".into(),
            ));
        }
    }
    Ok(lexical)
}

fn path_has_nul(path: &Path) -> bool {
    path.to_str().is_none_or(|value| value.contains('\0'))
}

fn normalize_lexical(path: &Path) -> Result<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return Err(Error::PermissionDenied(
                        "path escapes the allowed root".into(),
                    ));
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    Ok(out)
}

fn is_inside(root: &Path, path: &Path) -> bool {
    path.starts_with(root)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    #[test]
    fn rejects_parent_and_absolute_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("ok")).unwrap();
        assert!(resolve_within(root, Path::new("ok/file.txt")).is_ok());
        assert!(resolve_within(root, Path::new("../etc/passwd")).is_err());
        assert!(resolve_within(root, Path::new("/etc/passwd")).is_err());
        assert!(resolve_within(root, Path::new("ok/../../etc/passwd")).is_err());
    }

    #[test]
    fn rejects_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("sub")).unwrap();
        symlink("/etc", root.join("sub/escape")).unwrap();
        let err = resolve_within(root, Path::new("sub/escape/passwd")).unwrap_err();
        assert!(err.to_string().contains("symlink") || err.to_string().contains("escapes"));
    }

    #[test]
    fn rejects_nul() {
        let dir = tempfile::tempdir().unwrap();
        let err = resolve_within(dir.path(), Path::new("bad\0name")).unwrap_err();
        assert!(err.to_string().contains("NUL"));
    }
}
