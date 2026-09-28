use std::path::Path;
use anyhow::Result;
use crate::analysis::cleaner::scan_quick_wins;
use crate::fs::deleter::{execute_batch_delete, DeleteMode};
use crate::fs::model::FileItem;

pub fn run_clean_command(root: &Path, dry_run: bool, force: bool) -> Result<()> {
    println!("⚡ Scanning for Quick Wins junk in {}...", root.display());

    let categories = scan_quick_wins(root);

    let total_reclaimable: u64 = categories.iter().map(|c| c.total_size).sum();

    if total_reclaimable == 0 || categories.is_empty() {
        println!("✓ No safe-to-clean junk detected!");
        return Ok(());
    }

    println!("\nDiscovered Reclaimable Storage:\n");

    let mut all_targets = Vec::new();

    for cat in &categories {
        println!("{} {} (Total: {}):", cat.icon, cat.name, FileItem::format_size(cat.total_size));
        for item in &cat.items {
            println!("  • {:>10}  {}", FileItem::format_size(item.size), item.path.display());
            all_targets.push((item.path.clone(), item.size));
        }
        println!();
    }

    println!(
        "Total Reclaimable Space: {}",
        FileItem::format_size(total_reclaimable)
    );

    if dry_run {
        println!("\n🔍 Dry run completed. No files were removed.");
        return Ok(());
    }

    if !force {
        print!("\nMove these items to System Trash? [y/N]: ");
        use std::io::{self, Write};
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Operation cancelled.");
            return Ok(());
        }
    }

    println!("🗑️  Moving items to system trash...");
    let report = execute_batch_delete(&all_targets, DeleteMode::Trash);

    println!(
        "✓ Successfully trashed {} items! Reclaimed {}.",
        report.success_count,
        FileItem::format_size(report.bytes_freed)
    );

    if !report.errors.is_empty() {
        println!("\n⚠️ Errors encountered:");
        for (p, err) in report.errors {
            println!(" • {}: {}", p.display(), err);
        }
    }

    Ok(())
}
