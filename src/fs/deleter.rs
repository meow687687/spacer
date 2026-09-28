use std::fs;
use std::path::PathBuf;
use anyhow::Context;

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

pub fn execute_batch_delete(
    items: &[(PathBuf, u64)],
    mode: DeleteMode,
) -> DeletionReport {
    let mut success_count = 0;
    let mut bytes_freed = 0;
    let mut errors = Vec::new();

    for (path, size) in items {
        if !path.exists() {
            continue;
        }

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
