use std::path::Path;
use anyhow::{bail, Context, Result};
use crate::analysis::duplicate::{
    deduplicate_all, scan_duplicates, DedupMethod, DedupOptions, RealDedupOps,
};
use crate::fs::model::FileItem;

/// Parses a human-readable size string (e.g. "1M", "500K", "2G", "1048576") into bytes
pub fn parse_size_string(s: &str) -> Result<u64> {
    let s = s.trim();
    if s.is_empty() {
        bail!("Size string cannot be empty");
    }

    if let Ok(bytes) = s.parse::<u64>() {
        return Ok(bytes);
    }

    let upper = s.to_uppercase();
    let (num_part, multiplier) = if upper.ends_with("G") || upper.ends_with("GB") || upper.ends_with("GIB") {
        let trimmed = upper.trim_end_matches("GIB").trim_end_matches("GB").trim_end_matches('G');
        (trimmed, 1024 * 1024 * 1024)
    } else if upper.ends_with("M") || upper.ends_with("MB") || upper.ends_with("MIB") {
        let trimmed = upper.trim_end_matches("MIB").trim_end_matches("MB").trim_end_matches('M');
        (trimmed, 1024 * 1024)
    } else if upper.ends_with("K") || upper.ends_with("KB") || upper.ends_with("KIB") {
        let trimmed = upper.trim_end_matches("KIB").trim_end_matches("KB").trim_end_matches('K');
        (trimmed, 1024)
    } else if upper.ends_with("B") {
        let trimmed = upper.trim_end_matches('B');
        (trimmed, 1)
    } else {
        bail!(
            "Invalid size string '{}'. Expected format e.g. '1M', '500K', '2G', or bytes integer.",
            s
        );
    };

    let val: f64 = num_part
        .trim()
        .parse()
        .with_context(|| format!("Invalid number in size string '{}'", s))?;

    Ok((val * multiplier as f64) as u64)
}

pub fn run_dupes_command(
    root: &Path,
    is_dry_run: bool,
    allow_hardlink: bool,
    min_size: u64,
) -> Result<()> {
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

    if is_dry_run {
        println!("🔍 DRY RUN MODE (default) — no filesystem modifications will be made.");
        println!("   To apply deduplication, run: spacer dupes {} --apply\n", root.display());
    } else {
        println!("🚀 APPLY MODE — performing atomic deduplication...\n");
    }

    let options = DedupOptions {
        allow_hardlink,
        dry_run: is_dry_run,
        min_size,
    };

    let dedup_report = deduplicate_all(&report, &options, &RealDedupOps);

    for cluster in &dedup_report.clusters {
        let original_group = &report.groups[cluster.cluster_idx - 1];
        println!(
            "Cluster #{} ({} files, {} each, wasting {}):",
            cluster.cluster_idx,
            original_group.paths.len(),
            FileItem::format_size(cluster.file_size),
            FileItem::format_size(original_group.wasted_bytes())
        );
        println!("  • [Primary] {}", cluster.primary_path.display());

        for item in &cluster.processed {
            let method_tag = match (is_dry_run, item.method) {
                (true, DedupMethod::Reflink) => "[Dry-run: would reflink]",
                (true, DedupMethod::Hardlink) => "[Dry-run: would hardlink]",
                (false, DedupMethod::Reflink) => "[Reflink: cloned]",
                (false, DedupMethod::Hardlink) => "[Hardlink: linked]",
            };
            println!(
                "  • {} {} (+{} reclaimable)",
                method_tag,
                item.path.display(),
                FileItem::format_size(item.bytes_reclaimed)
            );
        }

        for item in &cluster.skipped {
            println!(
                "  • [Skipped] {} - {}",
                item.path.display(),
                item.reason
            );
        }
        println!();
    }

    println!("========================================================================");
    if is_dry_run {
        println!("Deduplication Summary (Dry Run - No Changes Made):");
        println!("  • Total clusters found   : {}", dedup_report.clusters.len());
        println!("  • Files eligible to dedup: {}", dedup_report.total_files_processed);
        println!("  • Files skipped          : {}", dedup_report.total_files_skipped);
        println!("  • Storage reclaimable    : {}", FileItem::format_size(dedup_report.total_bytes_reclaimed));
        println!("========================================================================");
        println!("\n💡 Run 'spacer dupes {} --apply' to apply these changes.", root.display());
        if !allow_hardlink {
            println!("💡 To allow hardlinking where filesystems lack reflink/clone support, pass '--hardlink'.");
        }
    } else {
        println!("Deduplication Summary (Applied):");
        println!("  • Total clusters         : {}", dedup_report.clusters.len());
        println!("  • Files deduplicated     : {}", dedup_report.total_files_processed);
        println!("  • Files skipped          : {}", dedup_report.total_files_skipped);
        println!("  • Storage reclaimed      : {}", FileItem::format_size(dedup_report.total_bytes_reclaimed));
        println!("========================================================================");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size_string() {
        assert_eq!(parse_size_string("1048576").unwrap(), 1_048_576);
        assert_eq!(parse_size_string("1M").unwrap(), 1_048_576);
        assert_eq!(parse_size_string("1MB").unwrap(), 1_048_576);
        assert_eq!(parse_size_string("1MiB").unwrap(), 1_048_576);
        assert_eq!(parse_size_string("500K").unwrap(), 512_000);
        assert_eq!(parse_size_string("2G").unwrap(), 2 * 1024 * 1024 * 1024);
        assert!(parse_size_string("invalid").is_err());
        assert!(parse_size_string("").is_err());
    }
}
