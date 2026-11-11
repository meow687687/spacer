use std::collections::{HashMap, HashSet};
use std::path::Path;
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Local};
use serde::{Deserialize, Serialize};

use crate::fs::model::FileItem;
use crate::history::db::{DirRecord, ScanRecord};

#[derive(Debug, Clone, PartialEq)]
pub enum SinceTarget {
    ScanId(i64),
    Duration(Duration),
}

/// Parses the --since CLI argument into a ScanId or a Duration
pub fn parse_since_argument(arg: &str) -> Result<SinceTarget> {
    let s = arg.trim();
    if let Ok(id) = s.parse::<i64>() {
        return Ok(SinceTarget::ScanId(id));
    }

    let s_lower = s.to_lowercase();
    let num_part: String = s_lower.chars().take_while(|c| c.is_ascii_digit()).collect();
    let unit_part: String = s_lower.chars().skip_while(|c| c.is_ascii_digit()).collect();

    if num_part.is_empty() {
        anyhow::bail!(
            "Invalid --since value '{}'. Expected numeric scan ID (e.g. '5') or duration (e.g. '7d', '24h', '30m').",
            arg
        );
    }

    let count: i64 = num_part.parse().context("Invalid number in --since")?;
    let seconds = match unit_part.as_str() {
        "s" | "sec" | "secs" | "second" | "seconds" => count,
        "m" | "min" | "mins" | "minute" | "minutes" => count * 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => count * 3600,
        "d" | "day" | "days" => count * 86400,
        "w" | "wk" | "wks" | "week" | "weeks" => count * 86400 * 7,
        "y" | "yr" | "yrs" | "year" | "years" => count * 86400 * 365,
        _ => anyhow::bail!(
            "Unknown duration unit '{}' in '{}'. Supported units: s, m, h, d, w, y.",
            unit_part,
            arg
        ),
    };

    Ok(SinceTarget::Duration(Duration::seconds(seconds)))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DirDiffItem {
    pub path: String,
    pub old_size_bytes: u64,
    pub new_size_bytes: u64,
    pub delta_bytes: i64,
    pub percent_change: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiffReport {
    pub base_scan_id: i64,
    pub base_timestamp: String,
    pub target_scan_id: i64,
    pub target_timestamp: String,
    pub root_path: String,
    pub total_base_bytes: u64,
    pub total_target_bytes: u64,
    pub total_delta_bytes: i64,
    pub growers: Vec<DirDiffItem>,
    pub shrinkers: Vec<DirDiffItem>,
}

/// Computes the diff between two snapshots.
pub fn compute_diff(
    base_scan: &ScanRecord,
    base_dirs: &HashMap<String, DirRecord>,
    target_scan: &ScanRecord,
    target_dirs: &HashMap<String, DirRecord>,
    path_filter: Option<&Path>,
) -> DiffReport {
    let mut all_paths = HashSet::new();
    for p in base_dirs.keys() {
        all_paths.insert(p);
    }
    for p in target_dirs.keys() {
        all_paths.insert(p);
    }

    let filter_canonical = path_filter.and_then(|p| {
        p.canonicalize()
            .ok()
            .map(|c| c.to_string_lossy().to_string())
            .or_else(|| Some(p.to_string_lossy().to_string()))
    });

    let mut growers = Vec::new();
    let mut shrinkers = Vec::new();

    for path in all_paths {
        if let Some(ref filter) = filter_canonical
            && path != filter && !path.starts_with(&format!("{}/", filter)) {
                continue;
            }

        let old_size = base_dirs.get(path).map(|d| d.size_bytes).unwrap_or(0);
        let new_size = target_dirs.get(path).map(|d| d.size_bytes).unwrap_or(0);

        let delta_bytes = new_size as i64 - old_size as i64;
        if delta_bytes == 0 {
            continue;
        }

        let percent_change = if old_size == 0 {
            100.0
        } else if new_size == 0 {
            -100.0
        } else {
            (delta_bytes as f64 / old_size as f64) * 100.0
        };

        let item = DirDiffItem {
            path: path.clone(),
            old_size_bytes: old_size,
            new_size_bytes: new_size,
            delta_bytes,
            percent_change,
        };

        if delta_bytes > 0 {
            growers.push(item);
        } else {
            shrinkers.push(item);
        }
    }

    growers.sort_by_key(|a| std::cmp::Reverse(a.delta_bytes));
    shrinkers.sort_by_key(|a| a.delta_bytes);

    let total_delta_bytes = target_scan.total_bytes as i64 - base_scan.total_bytes as i64;

    DiffReport {
        base_scan_id: base_scan.id,
        base_timestamp: base_scan.timestamp.clone(),
        target_scan_id: target_scan.id,
        target_timestamp: target_scan.timestamp.clone(),
        root_path: target_scan.root_path.clone(),
        total_base_bytes: base_scan.total_bytes,
        total_target_bytes: target_scan.total_bytes,
        total_delta_bytes,
        growers,
        shrinkers,
    }
}

/// Resolves the base scan record given a list of scans (sorted newest first) and an optional `--since` target.
pub fn select_scans_for_diff<'a>(
    scans: &'a [ScanRecord],
    since: Option<&SinceTarget>,
) -> Result<(&'a ScanRecord, &'a ScanRecord)> {
    if scans.is_empty() {
        anyhow::bail!("No snapshots found in history. Run a scan with spacer first.");
    }

    let target_scan = &scans[0];

    let base_scan = match since {
        Some(SinceTarget::ScanId(id)) => {
            scans.iter().find(|s| s.id == *id).ok_or_else(|| {
                anyhow::anyhow!("Snapshot #{} specified in --since not found in history.", id)
            })?
        }
        Some(SinceTarget::Duration(dur)) => {
            let target_dt = DateTime::parse_from_rfc3339(&target_scan.timestamp)
                .map(|dt| dt.with_timezone(&Local))
                .unwrap_or_else(|_| Local::now());
            let threshold_dt = target_dt - *dur;

            // Find scan closest to threshold_dt (and older than or equal to threshold_dt if possible)
            let mut candidate = None;
            for scan in scans.iter().skip(1) {
                if let Ok(scan_dt) = DateTime::parse_from_rfc3339(&scan.timestamp) {
                    let local_dt = scan_dt.with_timezone(&Local);
                    if local_dt <= threshold_dt {
                        candidate = Some(scan);
                        break;
                    }
                }
            }

            candidate.unwrap_or_else(|| {
                // If no scan is older than the duration, pick the oldest available scan
                scans.last().unwrap()
            })
        }
        None => {
            if scans.len() < 2 {
                anyhow::bail!(
                    "At least two snapshots are required to compute diff (found 1 snapshot #{}). Run another scan after making filesystem changes.",
                    target_scan.id
                );
            }
            &scans[1]
        }
    };

    if target_scan.id == base_scan.id {
        anyhow::bail!("Cannot diff snapshot #{} against itself.", target_scan.id);
    }

    Ok((base_scan, target_scan))
}

pub fn format_diff_table(report: &DiffReport, max_items: usize) -> String {
    let mut out = String::new();

    let delta_sign = if report.total_delta_bytes >= 0 { "+" } else { "" };
    let delta_formatted = format!(
        "{}{}",
        delta_sign,
        FileItem::format_size(report.total_delta_bytes.unsigned_abs())
    );

    out.push_str(&format!(
        "\n📊 \x1b[1;36mSTORAGE DIFF\x1b[0m: {} (Scan #{}) \x1b[1;33mvs\x1b[0m {} (Scan #{})\n",
        report.base_timestamp, report.base_scan_id, report.target_timestamp, report.target_scan_id
    ));
    out.push_str(&format!("   Root Path:      \x1b[1m{}\x1b[0m\n", report.root_path));
    out.push_str(&format!(
        "   Base Total:     {}  ──►  Target Total: {}  (\x1b[1;33mNet Delta: {}\x1b[0m)\n\n",
        FileItem::format_size(report.total_base_bytes),
        FileItem::format_size(report.total_target_bytes),
        delta_formatted
    ));

    // Growers
    out.push_str(&format!(
        "🔼 \x1b[1;31mTOP GROWERS\x1b[0m (Total: {})\n",
        report.growers.len()
    ));
    out.push_str(&format!(
        "{:<5} {:>12} {:>10} {:>12} {:>12}  {}\n",
        "RANK", "DELTA", "% CHANGE", "PREV SIZE", "NEW SIZE", "DIRECTORY"
    ));
    out.push_str(&format!("{}\n", "─".repeat(80)));

    if report.growers.is_empty() {
        out.push_str("  (No directories increased in size)\n");
    } else {
        for (idx, item) in report.growers.iter().take(max_items).enumerate() {
            let pct_str = format!("+{:+.1}%", item.percent_change);
            out.push_str(&format!(
                "{:<5} \x1b[1;31m{:>12}\x1b[0m {:>10} {:>12} {:>12}  {}\n",
                format!("#{}", idx + 1),
                format!("+{}", FileItem::format_size(item.delta_bytes as u64)),
                pct_str,
                FileItem::format_size(item.old_size_bytes),
                FileItem::format_size(item.new_size_bytes),
                item.path
            ));
        }
    }

    out.push('\n');

    // Shrinkers
    out.push_str(&format!(
        "🔽 \x1b[1;32mTOP SHRINKERS\x1b[0m (Total: {})\n",
        report.shrinkers.len()
    ));
    out.push_str(&format!(
        "{:<5} {:>12} {:>10} {:>12} {:>12}  {}\n",
        "RANK", "DELTA", "% CHANGE", "PREV SIZE", "NEW SIZE", "DIRECTORY"
    ));
    out.push_str(&format!("{}\n", "─".repeat(80)));

    if report.shrinkers.is_empty() {
        out.push_str("  (No directories decreased in size)\n");
    } else {
        for (idx, item) in report.shrinkers.iter().take(max_items).enumerate() {
            let pct_str = format!("{:+.1}%", item.percent_change);
            out.push_str(&format!(
                "{:<5} \x1b[1;32m{:>12}\x1b[0m {:>10} {:>12} {:>12}  {}\n",
                format!("#{}", idx + 1),
                format!("-{}", FileItem::format_size(item.delta_bytes.unsigned_abs())),
                pct_str,
                FileItem::format_size(item.old_size_bytes),
                FileItem::format_size(item.new_size_bytes),
                item.path
            ));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_since_scan_id() {
        let res = parse_since_argument("42").unwrap();
        assert_eq!(res, SinceTarget::ScanId(42));
    }

    #[test]
    fn test_parse_since_durations() {
        assert_eq!(
            parse_since_argument("7d").unwrap(),
            SinceTarget::Duration(Duration::days(7))
        );
        assert_eq!(
            parse_since_argument("24h").unwrap(),
            SinceTarget::Duration(Duration::hours(24))
        );
        assert_eq!(
            parse_since_argument("30m").unwrap(),
            SinceTarget::Duration(Duration::minutes(30))
        );
        assert_eq!(
            parse_since_argument("2w").unwrap(),
            SinceTarget::Duration(Duration::weeks(2))
        );
    }

    #[test]
    fn test_diff_calculation_synthetic() {
        let base_scan = ScanRecord {
            id: 1,
            timestamp: "2026-11-01T10:00:00+00:00".to_string(),
            root_path: "/test/project".to_string(),
            total_bytes: 1_000_000,
            total_items: 100,
        };

        let target_scan = ScanRecord {
            id: 2,
            timestamp: "2026-11-08T10:00:00+00:00".to_string(),
            root_path: "/test/project".to_string(),
            total_bytes: 1_500_000,
            total_items: 120,
        };

        let mut base_dirs = HashMap::new();
        base_dirs.insert(
            "/test/project/target".to_string(),
            DirRecord {
                path: "/test/project/target".to_string(),
                size_bytes: 500_000,
                item_count: 50,
            },
        );
        base_dirs.insert(
            "/test/project/.cache".to_string(),
            DirRecord {
                path: "/test/project/.cache".to_string(),
                size_bytes: 300_000,
                item_count: 20,
            },
        );

        let mut target_dirs = HashMap::new();
        // target grew from 500k to 900k (+400k)
        target_dirs.insert(
            "/test/project/target".to_string(),
            DirRecord {
                path: "/test/project/target".to_string(),
                size_bytes: 900_000,
                item_count: 80,
            },
        );
        // .cache shrunk from 300k to 100k (-200k)
        target_dirs.insert(
            "/test/project/.cache".to_string(),
            DirRecord {
                path: "/test/project/.cache".to_string(),
                size_bytes: 100_000,
                item_count: 10,
            },
        );
        // new folder appeared
        target_dirs.insert(
            "/test/project/docs".to_string(),
            DirRecord {
                path: "/test/project/docs".to_string(),
                size_bytes: 150_000,
                item_count: 15,
            },
        );

        let report = compute_diff(&base_scan, &base_dirs, &target_scan, &target_dirs, None);

        assert_eq!(report.total_delta_bytes, 500_000);
        assert_eq!(report.growers.len(), 2);
        assert_eq!(report.shrinkers.len(), 1);

        // First grower should be /test/project/target (+400_000)
        assert_eq!(report.growers[0].path, "/test/project/target");
        assert_eq!(report.growers[0].delta_bytes, 400_000);
        assert_eq!(report.growers[0].percent_change, 80.0);

        // Second grower is /test/project/docs (+150_000, new)
        assert_eq!(report.growers[1].path, "/test/project/docs");
        assert_eq!(report.growers[1].delta_bytes, 150_000);
        assert_eq!(report.growers[1].percent_change, 100.0);

        // Shrinker is /test/project/.cache (-200_000)
        assert_eq!(report.shrinkers[0].path, "/test/project/.cache");
        assert_eq!(report.shrinkers[0].delta_bytes, -200_000);
        assert!((report.shrinkers[0].percent_change - (-66.6666)).abs() < 0.1);
    }

    #[test]
    fn test_select_scans_for_diff_by_id_and_duration() {
        let s1 = ScanRecord {
            id: 1,
            timestamp: "2026-11-01T10:00:00+00:00".to_string(),
            root_path: "/test".to_string(),
            total_bytes: 100,
            total_items: 1,
        };
        let s2 = ScanRecord {
            id: 2,
            timestamp: "2026-11-05T10:00:00+00:00".to_string(),
            root_path: "/test".to_string(),
            total_bytes: 200,
            total_items: 2,
        };
        let s3 = ScanRecord {
            id: 3,
            timestamp: "2026-11-10T10:00:00+00:00".to_string(),
            root_path: "/test".to_string(),
            total_bytes: 300,
            total_items: 3,
        };
        let scans = vec![s3.clone(), s2.clone(), s1.clone()]; // newest first

        // Default: diff between latest (s3) and previous (s2)
        let (base, target) = select_scans_for_diff(&scans, None).unwrap();
        assert_eq!(target.id, 3);
        assert_eq!(base.id, 2);

        // Explicit scan ID
        let (base_id, target_id) = select_scans_for_diff(&scans, Some(&SinceTarget::ScanId(1))).unwrap();
        assert_eq!(target_id.id, 3);
        assert_eq!(base_id.id, 1);

        // Duration: 7 days ago from Nov 10 is Nov 3 -> s1 (Nov 1) is <= Nov 3
        let (base_dur, target_dur) = select_scans_for_diff(&scans, Some(&SinceTarget::Duration(Duration::days(7)))).unwrap();
        assert_eq!(target_dur.id, 3);
        assert_eq!(base_dur.id, 1);
    }

    #[test]
    fn test_diff_with_path_filter() {
        let base_scan = ScanRecord {
            id: 1,
            timestamp: "2026-11-01T10:00:00+00:00".to_string(),
            root_path: "/test/project".to_string(),
            total_bytes: 1_000,
            total_items: 10,
        };
        let target_scan = ScanRecord {
            id: 2,
            timestamp: "2026-11-02T10:00:00+00:00".to_string(),
            root_path: "/test/project".to_string(),
            total_bytes: 1_500,
            total_items: 12,
        };

        let mut base_dirs = HashMap::new();
        base_dirs.insert("/test/project/src".to_string(), DirRecord { path: "/test/project/src".to_string(), size_bytes: 200, item_count: 2 });
        base_dirs.insert("/test/project/target".to_string(), DirRecord { path: "/test/project/target".to_string(), size_bytes: 800, item_count: 8 });

        let mut target_dirs = HashMap::new();
        target_dirs.insert("/test/project/src".to_string(), DirRecord { path: "/test/project/src".to_string(), size_bytes: 400, item_count: 4 });
        target_dirs.insert("/test/project/target".to_string(), DirRecord { path: "/test/project/target".to_string(), size_bytes: 1100, item_count: 10 });

        // Filter only /test/project/src
        let report = compute_diff(&base_scan, &base_dirs, &target_scan, &target_dirs, Some(Path::new("/test/project/src")));
        assert_eq!(report.growers.len(), 1);
        assert_eq!(report.growers[0].path, "/test/project/src");
    }
}
