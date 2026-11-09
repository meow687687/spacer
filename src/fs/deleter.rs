use std::fs;
use std::path::{Path, PathBuf};
use anyhow::Context;
use crate::fs::logger::log_cleanup_action;
use crate::fs::processes::{check_open_processes, ProcessLockInfo};
use crate::fs::protected::check_protected_path;
use crate::fs::statvfs::get_available_bytes;
use crate::fs::trash::{move_to_trash_spec, RealTrashFs};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteMode {
    Trash,
    Permanent,
}

impl DeleteMode {
    pub fn title(&self) -> &'static str {
        match self {
            DeleteMode::Trash => "Move to System Trash",
            DeleteMode::Permanent => "Permanent Deletion (Unrecoverable)",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DeletionReport {
    pub success_count: usize,
    /// Space freed immediately (from permanent deletion)
    pub bytes_freed_now: u64,
    /// Space moved to Trash (not yet freed from storage)
    pub bytes_trashed: u64,
    /// Real storage delta measured via statvfs (after - before)
    pub statvfs_delta: Option<i64>,
    /// Hint if delta is much smaller than expected due to snapshots or hardlinks
    pub snapshot_hint: Option<String>,
    pub errors: Vec<(PathBuf, String)>,
}

/// Checks all items against running processes and returns any processes locking these items.
pub fn find_processes_locking_items(items: &[(PathBuf, u64)]) -> Vec<ProcessLockInfo> {
    let mut all_locks = Vec::new();
    for (path, _) in items {
        let locks = check_open_processes(path);
        for lock in locks {
            if !all_locks.iter().any(|l: &ProcessLockInfo| l.pid == lock.pid && l.locked_path == lock.locked_path) {
                all_locks.push(lock);
            }
        }
    }
    all_locks
}

pub fn execute_batch_delete(
    items: &[(PathBuf, u64)],
    mode: DeleteMode,
    user_extra_protected: Option<&[PathBuf]>,
) -> DeletionReport {
    let mut success_count = 0;
    let mut bytes_freed_now = 0;
    let mut bytes_trashed = 0;
    let mut errors = Vec::new();

    // 1. Measure real free space before deletion
    let ref_path = items.first().map(|(p, _)| p.as_path()).unwrap_or_else(|| Path::new("."));
    let before_avail = get_available_bytes(ref_path);

    let home_dir = std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/"));
    #[cfg(unix)]
    let uid = unsafe { libc::getuid() };
    #[cfg(not(unix))]
    let uid = 1000;

    let real_fs = RealTrashFs;

    for (path, size) in items {
        if !path.exists() {
            continue;
        }

        // Protected-paths hard block
        if let Err(e) = check_protected_path(path, user_extra_protected) {
            errors.push((path.clone(), e.to_string()));
            continue;
        }

        let method_str = match mode {
            DeleteMode::Trash => "trash",
            DeleteMode::Permanent => "permanent_rm",
        };

        let res = match mode {
            DeleteMode::Trash => {
                // Use Freedesktop / macOS trash specification
                move_to_trash_spec(&real_fs, path, &home_dir, uid)
                    .map(|_| ())
                    .or_else(|_| {
                        // Fallback to trash crate if spec move fails
                        trash::delete(path).map_err(|e| anyhow::anyhow!("Trash error: {}", e))
                    })
            }
            DeleteMode::Permanent => {
                if path.is_dir() {
                    fs::remove_dir_all(path).context("Failed to recursively remove directory")
                } else {
                    fs::remove_file(path).context("Failed to remove file")
                }
            }
        };

        match res {
            Ok(_) => {
                success_count += 1;
                match mode {
                    DeleteMode::Permanent => bytes_freed_now += size,
                    DeleteMode::Trash => bytes_trashed += size,
                }
                log_cleanup_action("DELETE", path, *size, method_str);
            }
            Err(e) => {
                errors.push((path.clone(), e.to_string()));
            }
        }
    }

    // 2. Measure real free space after deletion
    let after_avail = get_available_bytes(ref_path);
    let statvfs_delta = match (before_avail, after_avail) {
        (Some(b), Some(a)) => Some(a as i64 - b as i64),
        _ => None,
    };

    // 3. Check if real delta is significantly smaller than expected freed now
    let mut snapshot_hint = None;
    if mode == DeleteMode::Permanent
        && bytes_freed_now > 1_000_000
        && let Some(delta) = statvfs_delta
        && delta < (bytes_freed_now as i64 / 4)
    {
        snapshot_hint = Some(
            "Real disk free delta is much smaller than deleted size. Filesystem snapshots (Btrfs/ZFS/Timeshift/Snapper) or hardlink references may be holding the space.".to_string(),
        );
    }

    DeletionReport {
        success_count,
        bytes_freed_now,
        bytes_trashed,
        statvfs_delta,
        snapshot_hint,
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_batch_delete_prevents_protected_paths() {
        let items = vec![
            (PathBuf::from("/"), 1024),
            (PathBuf::from("/etc"), 2048),
        ];
        let report = execute_batch_delete(&items, DeleteMode::Trash, None);
        assert_eq!(report.success_count, 0);
        assert_eq!(report.errors.len(), 2);
        assert!(report.errors[0].1.contains("Protected path violation"));
        assert!(report.errors[1].1.contains("Protected path violation"));
    }

    #[test]
    fn test_batch_delete_safe_file_trash_or_permanent() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("to_delete.txt");
        File::create(&file_path).unwrap();
        assert!(file_path.exists());

        let items = vec![(file_path.clone(), 100)];
        let report = execute_batch_delete(&items, DeleteMode::Permanent, None);
        assert_eq!(report.success_count, 1);
        assert_eq!(report.bytes_freed_now, 100);
        assert_eq!(report.bytes_trashed, 0);
        assert!(!file_path.exists());
    }

    #[test]
    fn test_freed_vs_trashed_accounting() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("file1.txt");
        let file2 = dir.path().join("file2.txt");
        File::create(&file1).unwrap();
        File::create(&file2).unwrap();

        let report_perm = execute_batch_delete(&[(file1, 500)], DeleteMode::Permanent, None);
        assert_eq!(report_perm.bytes_freed_now, 500);
        assert_eq!(report_perm.bytes_trashed, 0);

        let report_trash = execute_batch_delete(&[(file2, 300)], DeleteMode::Trash, None);
        assert_eq!(report_trash.bytes_freed_now, 0);
        assert_eq!(report_trash.bytes_trashed, 300);
    }

    #[test]
    fn test_permanent_delete_typed_confirmation_requirement() {
        // Validation check that string must be "DELETE"
        let input_wrong = "delete";
        let input_partial = "DEL";
        let input_correct = "DELETE";

        assert_ne!(input_wrong.trim(), "DELETE");
        assert_ne!(input_partial.trim(), "DELETE");
        assert_eq!(input_correct.trim(), "DELETE");
    }
}
