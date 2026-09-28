use std::path::Path;
use anyhow::Result;
use crate::analysis::duplicate::{replace_with_hardlinks, scan_duplicates};
use crate::fs::model::FileItem;

pub fn run_dupes_command(root: &Path, hardlink_all: bool) -> Result<()> {
    println!("⚡ Scanning for duplicate files in {}...", root.display());

    let report = scan_duplicates(root);

    if report.groups.is_empty() {
        println!("✓ No duplicate files detected in {}.", root.display());
        return Ok(());
    }

    println!(
        "\nFound {} duplicate files wasting {}\n",
        report.total_duplicate_files,
        FileItem::format_size(report.total_wasted_bytes)
    );

    for (idx, group) in report.groups.iter().enumerate() {
        println!(
            "Cluster #{} ({} files, {} each, wasting {}):",
            idx + 1,
            group.paths.len(),
            FileItem::format_size(group.file_size),
            FileItem::format_size(group.wasted_bytes())
        );
        for (p_idx, p) in group.paths.iter().enumerate() {
            let label = if p_idx == 0 { "[Primary]" } else { "[Duplicate]" };
            println!("  • {} {}", label, p.display());
        }
        println!();
    }

    if hardlink_all {
        println!("🔗 Replacing all duplicates with hardlinks...");
        let mut total_replaced = 0;
        for group in &report.groups {
            if let Ok(count) = replace_with_hardlinks(group) {
                total_replaced += count;
            }
        }
        println!(
            "✓ Successfully hardlinked {} duplicate files! Reclaimed {}.",
            total_replaced,
            FileItem::format_size(report.total_wasted_bytes)
        );
    } else {
        println!("💡 Run 'spacer dupes --hardlink' to automatically deduplicate all identical files into hardlinks and reclaim space!");
    }

    Ok(())
}
