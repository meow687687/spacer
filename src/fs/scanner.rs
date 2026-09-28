use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use chrono::{DateTime, Local};
use crossbeam_channel::Sender;
use walkdir::WalkDir;

use crate::fs::model::{detect_category, FileItem, ScoreBreakdown};
use crate::fs::scorer::calculate_score;

#[derive(Debug, Clone)]
pub enum ScanMsg {
    /// Initial direct children of the target directory
    InitialChildren {
        root: PathBuf,
        items: Vec<FileItem>,
    },
    /// An item's recursive size and metadata have been resolved
    ItemUpdated {
        path: PathBuf,
        size: u64,
        item_count: usize,
        score: ScoreBreakdown,
    },
    /// Status update
    Progress {
        scanned_count: usize,
        total_bytes: u64,
        current_name: String,
    },
    /// Finished scanning all children in the current root
    ScanFinished {
        root: PathBuf,
        total_items: usize,
        #[allow(dead_code)]
        total_bytes: u64,
    },
}

pub fn spawn_scanner(root: PathBuf, tx: Sender<ScanMsg>) {
    thread::spawn(move || {
        scan_directory(&root, &tx);
    });
}

pub fn scan_directory(root: &Path, tx: &Sender<ScanMsg>) {
    let mut initial_items = Vec::new();

    // 1. Read immediate entries
    let entries = match fs::read_dir(root) {
        Ok(read_dir) => read_dir,
        Err(_) => {
            let _ = tx.send(ScanMsg::ScanFinished {
                root: root.to_path_buf(),
                total_items: 0,
                total_bytes: 0,
            });
            return;
        }
    };

    let mut direct_entries = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let metadata = entry.metadata().ok();
        let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = if !is_dir {
            metadata.as_ref().map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };

        let modified = metadata.as_ref().and_then(|m| m.modified().ok()).map(|t| {
            let dt: DateTime<Local> = DateTime::from(t);
            dt
        });

        let accessed = metadata.as_ref().and_then(|m| m.accessed().ok()).map(|t| {
            let dt: DateTime<Local> = DateTime::from(t);
            dt
        });

        let category = detect_category(&path, is_dir);
        let score = calculate_score(&path, is_dir, size, modified, accessed, category);

        let item = FileItem {
            path: path.clone(),
            name,
            is_dir,
            size,
            item_count: if is_dir { 0 } else { 1 },
            modified,
            accessed,
            category,
            score,
        };

        initial_items.push(item);
        direct_entries.push((path, is_dir, modified, accessed, category));
    }

    // Send initial items immediately so UI is populated instantly
    let _ = tx.send(ScanMsg::InitialChildren {
        root: root.to_path_buf(),
        items: initial_items,
    });

    // 2. Perform deep scanning for directories
    let mut total_bytes: u64 = 0;
    let mut total_items: usize = 0;

    for (path, is_dir, modified, accessed, category) in direct_entries {
        if is_dir {
            let (dir_size, dir_count) = compute_directory_size_deep(&path, tx, &mut total_items, &mut total_bytes);
            let score = calculate_score(&path, true, dir_size, modified, accessed, category);

            let _ = tx.send(ScanMsg::ItemUpdated {
                path,
                size: dir_size,
                item_count: dir_count,
                score,
            });
        } else if let Ok(meta) = fs::symlink_metadata(&path) {
            let file_len = meta.len();
            total_bytes += file_len;
            total_items += 1;
        }
    }

    let _ = tx.send(ScanMsg::ScanFinished {
        root: root.to_path_buf(),
        total_items,
        total_bytes,
    });
}

fn compute_directory_size_deep(
    dir_path: &Path,
    tx: &Sender<ScanMsg>,
    global_items: &mut usize,
    global_bytes: &mut u64,
) -> (u64, usize) {
    let mut dir_size = 0u64;
    let mut dir_count = 0usize;
    let mut progress_counter = 0;

    let dir_name = dir_path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();

    for entry in WalkDir::new(dir_path)
        .follow_links(false)
        .same_file_system(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if let Ok(metadata) = entry.metadata() {
            if metadata.is_file() {
                let len = metadata.len();
                dir_size += len;
                dir_count += 1;
                *global_bytes += len;
                *global_items += 1;
                progress_counter += 1;

                if progress_counter % 200 == 0 {
                    let _ = tx.send(ScanMsg::Progress {
                        scanned_count: *global_items,
                        total_bytes: *global_bytes,
                        current_name: dir_name.clone(),
                    });
                }
            } else if metadata.is_dir() && entry.path() != dir_path {
                dir_count += 1;
                *global_items += 1;
            }
        }
    }

    (dir_size, dir_count)
}
