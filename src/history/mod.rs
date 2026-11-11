pub mod db;
pub mod diff;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use anyhow::Result;

pub use db::{default_db_path, open_or_create_db, DirRecord, DEFAULT_MIN_DIR_SIZE_BYTES};

/// Records a scan snapshot to the standard platform history SQLite DB.
pub fn record_scan_snapshot(
    root: &Path,
    total_bytes: u64,
    total_items: usize,
    dir_sizes: &HashMap<PathBuf, (u64, usize)>,
) -> Result<i64> {
    record_scan_snapshot_to(
        &default_db_path(),
        root,
        total_bytes,
        total_items,
        dir_sizes,
        DEFAULT_MIN_DIR_SIZE_BYTES,
    )
}

/// Records a scan snapshot to a specified SQLite DB path.
pub fn record_scan_snapshot_to(
    db_path: &Path,
    root: &Path,
    total_bytes: u64,
    total_items: usize,
    dir_sizes: &HashMap<PathBuf, (u64, usize)>,
    min_size_bytes: u64,
) -> Result<i64> {
    let mut conn = open_or_create_db(db_path)?;
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let root_str = canonical_root.to_string_lossy().to_string();

    let mut dir_records = Vec::new();
    for (dir_path, (size, count)) in dir_sizes {
        let dir_str = dir_path.to_string_lossy().to_string();
        // Always include root directory, and subdirectories meeting the minimum size threshold
        if *size >= min_size_bytes || dir_str == root_str {
            dir_records.push(DirRecord {
                path: dir_str,
                size_bytes: *size,
                item_count: *count,
            });
        }
    }

    db::save_snapshot(&mut conn, &canonical_root, total_bytes, total_items, &dir_records)
}

/// Recursively scans a path and records a compact snapshot to the history database.
pub fn scan_path_and_record_snapshot(root: &Path) -> Result<i64> {
    scan_path_and_record_snapshot_to(&default_db_path(), root, DEFAULT_MIN_DIR_SIZE_BYTES)
}

/// Recursively scans a path and records a snapshot to a specific database path.
pub fn scan_path_and_record_snapshot_to(
    db_path: &Path,
    root: &Path,
    min_size_bytes: u64,
) -> Result<i64> {
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut dir_sizes: HashMap<PathBuf, (u64, usize)> = HashMap::new();
    let mut total_bytes = 0u64;
    let mut total_items = 0usize;

    dir_sizes.insert(canonical.clone(), (0, 0));

    for entry in walkdir::WalkDir::new(&canonical)
        .follow_links(false)
        .same_file_system(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if let Ok(meta) = entry.metadata() {
            if meta.is_file() {
                let len = meta.len();
                total_bytes += len;
                total_items += 1;

                let mut curr_parent = entry.path().parent();
                while let Some(parent) = curr_parent {
                    let d = dir_sizes.entry(parent.to_path_buf()).or_insert((0, 0));
                    d.0 += len;
                    d.1 += 1;
                    if parent == canonical {
                        break;
                    }
                    curr_parent = parent.parent();
                }
            } else if meta.is_dir() {
                dir_sizes.entry(entry.path().to_path_buf()).or_insert((0, 0));
                if entry.path() != canonical {
                    total_items += 1;
                }
            }
        }
    }

    record_scan_snapshot_to(
        db_path,
        &canonical,
        total_bytes,
        total_items,
        &dir_sizes,
        min_size_bytes,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_record_snapshot_and_query() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_history.db");

        let mut dir_sizes = HashMap::new();
        let root = PathBuf::from("/test/root");
        dir_sizes.insert(root.clone(), (500_000, 10));
        dir_sizes.insert(root.join("sub1"), (200_000, 5));
        dir_sizes.insert(root.join("small"), (5_000, 1)); // below 100KB threshold

        let scan_id = record_scan_snapshot_to(
            &db_path,
            &root,
            500_000,
            10,
            &dir_sizes,
            100_000,
        )
        .unwrap();

        assert_eq!(scan_id, 1);

        let conn = open_or_create_db(&db_path).unwrap();
        let scans = db::list_scans(&conn, None).unwrap();
        assert_eq!(scans.len(), 1);
        assert_eq!(scans[0].id, 1);
        assert_eq!(scans[0].total_bytes, 500_000);

        let dirs = db::get_scan_directories(&conn, scan_id).unwrap();
        // Should contain root and sub1, but small is filtered out
        assert!(dirs.contains_key(&root.to_string_lossy().to_string()));
        assert!(dirs.contains_key(&root.join("sub1").to_string_lossy().to_string()));
        assert!(!dirs.contains_key(&root.join("small").to_string_lossy().to_string()));
    }

    #[test]
    fn test_corrupt_db_fallback() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("history.db");

        // Write junk to db_path to simulate corruption
        std::fs::write(&db_path, b"NOT A VALID SQLITE DATABASE HEADER GIBBERISH").unwrap();

        // open_or_create_db should detect corruption, rename to .corrupt.*, and create fresh working DB
        let conn = open_or_create_db(&db_path).unwrap();
        let scans = db::list_scans(&conn, None).unwrap();
        assert_eq!(scans.len(), 0);

        // Verify that a .corrupt file was created
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();

        assert!(entries.iter().any(|name| name.contains(".corrupt.")));
    }

    #[test]
    fn test_prune_scans_keeping_n() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_prune.db");
        let root = PathBuf::from("/test/prune_root");
        let mut dir_sizes = HashMap::new();
        dir_sizes.insert(root.clone(), (100_000, 1));

        // Create 5 snapshots
        for _ in 1..=5 {
            record_scan_snapshot_to(&db_path, &root, 100_000, 1, &dir_sizes, 10_000).unwrap();
        }

        let mut conn = open_or_create_db(&db_path).unwrap();
        let scans = db::list_scans(&conn, None).unwrap();
        assert_eq!(scans.len(), 5);

        // Keep 2 most recent snapshots, prune 3
        let pruned = db::prune_scans(&mut conn, 2, None).unwrap();
        assert_eq!(pruned, 3);

        let remaining = db::list_scans(&conn, None).unwrap();
        assert_eq!(remaining.len(), 2);
        // The remaining should be the newest ones (IDs 5 and 4)
        assert_eq!(remaining[0].id, 5);
        assert_eq!(remaining[1].id, 4);
    }
}
