use std::collections::HashMap;
use std::env;
use std::fs::{create_dir_all, rename};
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use chrono::Local;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

pub const DEFAULT_MIN_DIR_SIZE_BYTES: u64 = 100 * 1024; // 100 KB

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanRecord {
    pub id: i64,
    pub timestamp: String,
    pub root_path: String,
    pub total_bytes: u64,
    pub total_items: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirRecord {
    pub path: String,
    pub size_bytes: u64,
    pub item_count: usize,
}

/// Resolves standard history DB path for spacer:
/// macOS: ~/Library/Application Support/spacer/history.db
/// Linux: $XDG_DATA_HOME/spacer/history.db or ~/.local/share/spacer/history.db
pub fn default_db_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = env::var("HOME") {
            let p = PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("spacer");
            return p.join("history.db");
        }
    }

    if let Ok(data_home) = env::var("XDG_DATA_HOME") {
        return PathBuf::from(data_home).join("spacer").join("history.db");
    }

    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("spacer")
            .join("history.db");
    }

    PathBuf::from("spacer_history.db")
}

/// Opens connection to SQLite DB at specified path with schema migrations.
/// If the database is corrupt, renames it with `.corrupt.<timestamp>` and starts fresh.
pub fn open_or_create_db(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent() {
        let _ = create_dir_all(parent);
    }

    match try_open_and_migrate(db_path) {
        Ok(conn) => Ok(conn),
        Err(err) => {
            eprintln!(
                "Warning: spacer history DB at {} appeared corrupt or failed to open: {}. Recovering...",
                db_path.display(),
                err
            );
            handle_corrupt_db(db_path)?;
            try_open_and_migrate(db_path)
                .context("Failed to initialize a fresh history database after corruption recovery")
        }
    }
}

fn handle_corrupt_db(db_path: &Path) -> Result<()> {
    if db_path.exists() {
        let ts = Local::now().format("%Y%m%d%H%M%S").to_string();
        let corrupt_path = db_path.with_file_name(format!(
            "{}.corrupt.{}",
            db_path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "history.db".to_string()),
            ts
        ));
        let _ = rename(db_path, &corrupt_path);
    }
    Ok(())
}

fn try_open_and_migrate(db_path: &Path) -> Result<Connection> {
    let mut conn = Connection::open(db_path)?;

    // Enable foreign keys
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;

    // Integrity check
    {
        let check_res: String = conn.query_row("PRAGMA quick_check;", [], |row| row.get(0))?;
        if check_res != "ok" {
            anyhow::bail!("Database integrity check failed: {}", check_res);
        }
    }

    // Run migrations
    migrate_schema(&mut conn)?;

    Ok(conn)
}

fn migrate_schema(conn: &mut Connection) -> Result<()> {
    let user_version: i32 = conn.query_row("PRAGMA user_version;", [], |row| row.get(0))?;

    if user_version == 0 {
        let tx = conn.transaction()?;
        tx.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS scans (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                root_path TEXT NOT NULL,
                total_bytes INTEGER NOT NULL,
                total_items INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS scan_directories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scan_id INTEGER NOT NULL REFERENCES scans(id) ON DELETE CASCADE,
                path TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                item_count INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_scan_dirs_scan_id ON scan_directories(scan_id);
            CREATE INDEX IF NOT EXISTS idx_scan_dirs_scan_id_path ON scan_directories(scan_id, path);
            CREATE INDEX IF NOT EXISTS idx_scans_root_path ON scans(root_path);

            PRAGMA user_version = 1;
            "#,
        )?;
        tx.commit()?;
    }

    Ok(())
}

/// Stores a new scan snapshot in the database.
pub fn save_snapshot(
    conn: &mut Connection,
    root_path: &Path,
    total_bytes: u64,
    total_items: usize,
    dirs: &[DirRecord],
) -> Result<i64> {
    let timestamp = Local::now().to_rfc3339();
    let root_str = root_path
        .canonicalize()
        .unwrap_or_else(|_| root_path.to_path_buf())
        .to_string_lossy()
        .to_string();

    let tx = conn.transaction()?;

    tx.execute(
        "INSERT INTO scans (timestamp, root_path, total_bytes, total_items) VALUES (?1, ?2, ?3, ?4);",
        params![timestamp, root_str, total_bytes, total_items],
    )?;

    let scan_id = tx.last_insert_rowid();

    {
        let mut stmt = tx.prepare(
            "INSERT INTO scan_directories (scan_id, path, size_bytes, item_count) VALUES (?1, ?2, ?3, ?4);",
        )?;

        for dir in dirs {
            stmt.execute(params![scan_id, dir.path, dir.size_bytes, dir.item_count])?;
        }
    }

    tx.commit()?;
    Ok(scan_id)
}

/// Lists all scan records in history, optionally filtered by root path.
pub fn list_scans(conn: &Connection, root_filter: Option<&Path>) -> Result<Vec<ScanRecord>> {
    let mut query = String::from(
        "SELECT id, timestamp, root_path, total_bytes, total_items FROM scans",
    );
    let mut filter_str = None;

    if let Some(rf) = root_filter {
        let canonical = rf
            .canonicalize()
            .unwrap_or_else(|_| rf.to_path_buf())
            .to_string_lossy()
            .to_string();
        query.push_str(" WHERE root_path = ?1");
        filter_str = Some(canonical);
    }
    query.push_str(" ORDER BY id DESC;");

    let mut stmt = conn.prepare(&query)?;
    let mut result = Vec::new();
    if let Some(ref f) = filter_str {
        let rows = stmt.query_map(params![f], |row| {
            Ok(ScanRecord {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                root_path: row.get(2)?,
                total_bytes: row.get(3)?,
                total_items: row.get(4)?,
            })
        })?;
        for r in rows {
            result.push(r?);
        }
    } else {
        let rows = stmt.query_map([], |row| {
            Ok(ScanRecord {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                root_path: row.get(2)?,
                total_bytes: row.get(3)?,
                total_items: row.get(4)?,
            })
        })?;
        for r in rows {
            result.push(r?);
        }
    }

    Ok(result)
}

/// Retrieves a specific scan record by id.
#[allow(dead_code)]
pub fn get_scan(conn: &Connection, scan_id: i64) -> Result<Option<ScanRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, root_path, total_bytes, total_items FROM scans WHERE id = ?1;",
    )?;

    let mut rows = stmt.query(params![scan_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(ScanRecord {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            root_path: row.get(2)?,
            total_bytes: row.get(3)?,
            total_items: row.get(4)?,
        }))
    } else {
        Ok(None)
    }
}

/// Retrieves all directory records for a given scan id mapped by path string.
pub fn get_scan_directories(conn: &Connection, scan_id: i64) -> Result<HashMap<String, DirRecord>> {
    let mut stmt = conn.prepare(
        "SELECT path, size_bytes, item_count FROM scan_directories WHERE scan_id = ?1;",
    )?;

    let rows = stmt.query_map(params![scan_id], |row| {
        Ok(DirRecord {
            path: row.get(0)?,
            size_bytes: row.get(1)?,
            item_count: row.get(2)?,
        })
    })?;

    let mut map = HashMap::new();
    for r in rows {
        let record = r?;
        map.insert(record.path.clone(), record);
    }
    Ok(map)
}

/// Prunes older snapshots, keeping only the `keep_count` most recent scans.
/// Returns the number of pruned scans.
pub fn prune_scans(conn: &mut Connection, keep_count: usize, root_filter: Option<&Path>) -> Result<usize> {
    let scans = list_scans(conn, root_filter)?;
    if scans.len() <= keep_count {
        return Ok(0);
    }

    let to_remove = &scans[keep_count..];
    let tx = conn.transaction()?;
    let mut pruned_count = 0;

    {
        let mut del_dirs = tx.prepare("DELETE FROM scan_directories WHERE scan_id = ?1;")?;
        let mut del_scan = tx.prepare("DELETE FROM scans WHERE id = ?1;")?;

        for scan in to_remove {
            del_dirs.execute(params![scan.id])?;
            del_scan.execute(params![scan.id])?;
            pruned_count += 1;
        }
    }

    tx.commit()?;
    Ok(pruned_count)
}
