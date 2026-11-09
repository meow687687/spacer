use std::env;
use std::path::{Path, PathBuf};
use anyhow::{bail, Result};

/// Returns default system-critical and protected directory paths.
pub fn default_protected_paths() -> Vec<PathBuf> {
    let mut protected = Vec::new();

    // Root filesystem
    protected.push(PathBuf::from("/"));

    // User home directory itself
    if let Ok(home) = env::var("HOME")
        && !home.is_empty() {
            protected.push(PathBuf::from(home));
        }

    // Unix / Linux / FHS core system directories
    for p in &[
        "/usr",
        "/etc",
        "/boot",
        "/bin",
        "/sbin",
        "/lib",
        "/lib64",
        "/lib32",
        "/libx32",
        "/var",
    ] {
        protected.push(PathBuf::from(p));
    }

    // macOS core system directories
    for p in &["/System", "/Library"] {
        protected.push(PathBuf::from(p));
    }

    protected
}

/// Parses extra user-configured protected paths from environment or custom list.
/// Checks `SPACER_EXTRA_PROTECTED_PATHS` (colon-delimited or semicolon-delimited).
pub fn extra_protected_paths() -> Vec<PathBuf> {
    let mut extra = Vec::new();
    if let Ok(val) = env::var("SPACER_EXTRA_PROTECTED_PATHS") {
        let sep = if val.contains(';') { ';' } else { ':' };
        for part in val.split(sep) {
            let trimmed = part.trim();
            if !trimmed.is_empty() {
                extra.push(PathBuf::from(trimmed));
            }
        }
    }
    extra
}

/// Resolves a path (canonicalizing symlinks where possible) and verifies it is not protected.
/// Returns an error if the path or canonicalized target matches any protected path,
/// or is the parent/root of protected paths (like `/`).
pub fn check_protected_path(path: &Path, user_extra: Option<&[PathBuf]>) -> Result<PathBuf> {
    // 1. Resolve canonical path if exists, otherwise clean/normalize
    let canonical = path.canonicalize().unwrap_or_else(|_| {
        // Even if path does not exist, resolve components without symlinks
        let mut p = PathBuf::new();
        if path.is_relative()
            && let Ok(current) = env::current_dir() {
                p.push(current);
            }
        p.push(path);
        p
    });

    // 2. Build full list of protected paths
    let mut all_protected = default_protected_paths();
    all_protected.extend(extra_protected_paths());
    if let Some(extras) = user_extra {
        all_protected.extend(extras.iter().cloned());
    }

    // Canonicalize all protected paths for reliable comparison
    let mut canonical_protected = Vec::new();
    for p in all_protected {
        if let Ok(c) = p.canonicalize() {
            canonical_protected.push(c);
        } else {
            canonical_protected.push(p);
        }
    }

    // 3. Check if target or its canonical form is in protected paths
    for prot in &canonical_protected {
        if &canonical == prot || path == prot {
            bail!(
                "Protected path violation: '{}' (resolved: '{}') is a protected system/user path and cannot be deleted or modified.",
                path.display(),
                canonical.display()
            );
        }
    }

    // Also check parent symlink escapes if any ancestor is a symlink resolving to protected
    if let Ok(meta) = std::fs::symlink_metadata(path)
        && meta.file_type().is_symlink()
            && let Ok(target) = std::fs::read_link(path) {
                let target_canonical = target.canonicalize().unwrap_or(target);
                for prot in &canonical_protected {
                    if &target_canonical == prot {
                        bail!(
                            "Protected path violation: symlink '{}' points to protected path '{}'.",
                            path.display(),
                            target_canonical.display()
                        );
                    }
                }
            }

    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use tempfile::tempdir;

    #[test]
    fn test_protects_root() {
        let res = check_protected_path(Path::new("/"), None);
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("Protected path violation"));
    }

    #[test]
    fn test_protects_home() {
        if let Ok(home) = env::var("HOME") {
            let res = check_protected_path(Path::new(&home), None);
            assert!(res.is_err());
        }
    }

    #[test]
    fn test_protects_system_dirs() {
        for sys_dir in &["/etc", "/usr", "/boot", "/bin", "/var"] {
            let p = Path::new(sys_dir);
            if p.exists() {
                let res = check_protected_path(p, None);
                assert!(res.is_err(), "Expected {} to be protected", sys_dir);
            }
        }
    }

    #[test]
    fn test_allows_safe_tempdir() {
        let dir = tempdir().unwrap();
        let safe_file = dir.path().join("safe.txt");
        File::create(&safe_file).unwrap();

        let res = check_protected_path(&safe_file, None);
        assert!(res.is_ok());
    }

    #[test]
    fn test_symlink_escape_protection() {
        let dir = tempdir().unwrap();
        let symlink_to_etc = dir.path().join("symlink_etc");
        #[cfg(unix)]
        {
            let etc_path = Path::new("/etc");
            if etc_path.exists() {
                std::os::unix::fs::symlink(etc_path, &symlink_to_etc).unwrap();
                let res = check_protected_path(&symlink_to_etc, None);
                assert!(res.is_err(), "Symlink pointing to /etc must be blocked");
            }
        }
    }

    #[test]
    fn test_extra_protected_paths() {
        let dir = tempdir().unwrap();
        let custom_protected = dir.path().join("important_project");
        fs::create_dir_all(&custom_protected).unwrap();

        let res = check_protected_path(&custom_protected, Some(&[custom_protected.clone()]));
        assert!(res.is_err(), "Custom protected path should be blocked");
    }
}
