use std::path::Path;
use anyhow::Result;

use crate::history::db::{default_db_path, get_scan_directories, list_scans, open_or_create_db};
use crate::history::diff::{
    compute_diff, format_diff_table, parse_since_argument, select_scans_for_diff,
};
use crate::history::scan_path_and_record_snapshot;

pub fn run_diff_command(
    path: Option<&Path>,
    since: Option<&str>,
    json: bool,
) -> Result<()> {
    let db_path = default_db_path();
    let conn = open_or_create_db(&db_path)?;

    let parsed_since = match since {
        Some(s) => Some(parse_since_argument(s)?),
        None => None,
    };

    let mut scans = list_scans(&conn, path)?;

    // If only 1 snapshot exists and user specified a path, take a fresh snapshot of current state to diff against it
    if scans.len() == 1 && parsed_since.is_none() {
        if let Some(target_path) = path {
            if !json {
                println!(
                    "⚡ Found 1 previous snapshot for {}. Recording current filesystem state to compute diff...",
                    target_path.display()
                );
            }
            let _ = scan_path_and_record_snapshot(target_path)?;
            scans = list_scans(&conn, path)?;
        }
    } else if scans.is_empty() {
        if let Some(target_path) = path {
            if !json {
                println!(
                    "⚡ No previous snapshots found for {}. Recording initial snapshot...",
                    target_path.display()
                );
            }
            let scan_id = scan_path_and_record_snapshot(target_path)?;
            if !json {
                println!(
                    "✓ Initial snapshot #{} recorded. Run spacer diff again after storage changes to see growth deltas.",
                    scan_id
                );
            }
            return Ok(());
        } else {
            anyhow::bail!("No scan history found in database. Run spacer or spacer diff --path <dir> to create an initial snapshot.");
        }
    }

    let (base_scan, target_scan) = select_scans_for_diff(&scans, parsed_since.as_ref())?;

    let base_dirs = get_scan_directories(&conn, base_scan.id)?;
    let target_dirs = get_scan_directories(&conn, target_scan.id)?;

    let report = compute_diff(base_scan, &base_dirs, target_scan, &target_dirs, path);

    if json {
        let json_str = serde_json::to_string_pretty(&report)?;
        println!("{}", json_str);
    } else {
        println!("{}", format_diff_table(&report, 25));
    }

    Ok(())
}
