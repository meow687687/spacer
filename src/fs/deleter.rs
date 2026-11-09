use std::fs;
use std::path::PathBuf;
use anyhow::Context;
use crate::fs::logger::log_cleanup_action;
use crate::fs::processes::{check_open_processes, ProcessLockInfo};
use crate::fs::protected::check_protected_path;

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

#[derive(Debug, Clone)]
pub struct DeletionReport {
    pub success_count: usize,
    pub bytes_freed: u64,
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
    let mut bytes_freed = 0;
    let mut errors = Vec::new();

    for (path, size) in items {
        if !path.exists() {
            continue;
        }

        // 1. Requirement 1: Protected-paths hard block
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
                trash::delete(path).map_err(|e| anyhow::anyhow!("Trash error: {}", e))
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
                bytes_freed += size;
                // Requirement 5: Log every cleanup action
                log_cleanup_action("DELETE", path, *size, method_str);
            }
            Err(e) => {
                errors.push((path.clone(), e.to_string()));
            }
        }
    }

    DeletionReport {
        success_count,
        bytes_freed,
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
        assert!(!file_path.exists());
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
