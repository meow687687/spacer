use std::path::Path;
use anyhow::Result;

use crate::fs::model::FileItem;
use crate::history::db::{default_db_path, list_scans, open_or_create_db, prune_scans};

pub fn run_history_command(
    path: Option<&Path>,
    prune: bool,
    keep: usize,
) -> Result<()> {
    let db_path = default_db_path();
    let mut conn = open_or_create_db(&db_path)?;

    if prune {
        let pruned = prune_scans(&mut conn, keep, path)?;
        println!(
            "✓ Pruned {} older snapshot(s) (retained {} most recent).",
            pruned, keep
        );
        return Ok(());
    }

    let scans = list_scans(&conn, path)?;
    if scans.is_empty() {
        println!("No snapshot history found in database ({}).", db_path.display());
        println!("Run spacer in a directory to start collecting scan history.");
        return Ok(());
    }

    println!("\n📜 \x1b[1;36mSPACER SCAN HISTORY\x1b[0m ({} snapshots)\n", scans.len());
    println!(
        "{:<6} {:<26} {:>12} {:>10}  ROOT PATH",
        "ID", "TIMESTAMP", "TOTAL SIZE", "ITEMS"
    );
    println!("{}", "─".repeat(85));

    for scan in &scans {
        println!(
            "{:<6} {:<26} {:>12} {:>10}  {}",
            format!("#{}", scan.id),
            scan.timestamp,
            FileItem::format_size(scan.total_bytes),
            scan.total_items,
            scan.root_path
        );
    }
    println!();

    Ok(())
}
