use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DuplicateGroup {
    pub file_size: u64,
    pub sha256_hash: String,
    pub paths: Vec<PathBuf>,
}

impl DuplicateGroup {
    pub fn wasted_bytes(&self) -> u64 {
        if self.paths.len() > 1 {
            self.file_size * (self.paths.len() as u64 - 1)
        } else {
            0
        }
    }
}

#[derive(Debug, Clone)]
pub struct DuplicateReport {
    pub total_duplicate_files: usize,
    pub total_wasted_bytes: u64,
    pub groups: Vec<DuplicateGroup>,
}

fn compute_prefix_hash(path: &Path) -> Option<[u8; 32]> {
    let mut file = File::open(path).ok()?;
    let mut buffer = [0u8; 4096];
    let n = file.read(&mut buffer).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&buffer[..n]);
    Some(hasher.finalize().into())
}

fn compute_full_sha256(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 16384];

    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buffer[..n]),
            Err(_) => return None,
        }
    }

    Some(format!("{:x}", hasher.finalize()))
}

pub fn scan_duplicates(root: &Path) -> DuplicateReport {
    // Phase 1: Group by exact byte size
    let mut size_map: HashMap<u64, Vec<PathBuf>> = HashMap::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            if let Ok(meta) = entry.metadata() {
                let len = meta.len();
                // Ignore zero-byte files or tiny files under 512 bytes
                if len >= 512 {
                    size_map.entry(len).or_default().push(entry.path().to_path_buf());
                }
            }
        }
    }

    // Phase 2: Prefix 4KB hash for size collisions
    let mut prefix_map: HashMap<(u64, [u8; 32]), Vec<PathBuf>> = HashMap::new();

    for (size, paths) in size_map {
        if paths.len() > 1 {
            for path in paths {
                if let Some(prefix_hash) = compute_prefix_hash(&path) {
                    prefix_map.entry((size, prefix_hash)).or_default().push(path);
                }
            }
        }
    }

    // Phase 3: Full SHA-256 for exact match verification
    let mut full_hash_map: HashMap<(u64, String), Vec<PathBuf>> = HashMap::new();

    for ((size, _), paths) in prefix_map {
        if paths.len() > 1 {
            for path in paths {
                if let Some(full_hash) = compute_full_sha256(&path) {
                    full_hash_map.entry((size, full_hash)).or_default().push(path);
                }
            }
        }
    }

    let mut groups = Vec::new();
    let mut total_dupe_files = 0;
    let mut total_wasted = 0;

    for ((size, hash), paths) in full_hash_map {
        if paths.len() > 1 {
            let wasted = size * (paths.len() as u64 - 1);
            total_dupe_files += paths.len() - 1;
            total_wasted += wasted;

            groups.push(DuplicateGroup {
                file_size: size,
                sha256_hash: hash,
                paths,
            });
        }
    }

    // Sort groups by wasted bytes descending
    groups.sort_by(|a, b| b.wasted_bytes().cmp(&a.wasted_bytes()));

    DuplicateReport {
        total_duplicate_files: total_dupe_files,
        total_wasted_bytes: total_wasted,
        groups,
    }
}

pub fn replace_with_hardlinks(group: &DuplicateGroup) -> Result<usize> {
    if group.paths.len() < 2 {
        return Ok(0);
    }

    let primary = &group.paths[0];
    let mut replaced_count = 0;

    for secondary in &group.paths[1..] {
        if secondary.exists() && primary.exists() && secondary != primary {
            // Remove secondary file and hardlink to primary
            let temp_backup = secondary.with_extension("spacer_hardlink_tmp");
            fs::rename(secondary, &temp_backup).context("Failed to stage file for hardlinking")?;

            match fs::hard_link(primary, secondary) {
                Ok(_) => {
                    let _ = fs::remove_file(&temp_backup);
                    replaced_count += 1;
                }
                Err(e) => {
                    // Restore original file if cross-device or error
                    let _ = fs::rename(&temp_backup, secondary);
                    return Err(e).context("Hardlink creation failed (cross-device links not allowed by filesystem)");
                }
            }
        }
    }

    Ok(replaced_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_duplicate_detection_and_hardlink() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("original.txt");
        let file2 = dir.path().join("copy.txt");
        let file3 = dir.path().join("different.txt");

        let content = "The quick brown fox jumps over the lazy dog repeated text for testing duplicates 1234567890".repeat(20);
        fs::write(&file1, &content).unwrap();
        fs::write(&file2, &content).unwrap();
        fs::write(&file3, "Something completely different content").unwrap();

        let report = scan_duplicates(dir.path());
        assert_eq!(report.groups.len(), 1);
        assert_eq!(report.groups[0].paths.len(), 2);
        assert_eq!(report.total_duplicate_files, 1);

        // Test hardlink deduplication
        let replaced = replace_with_hardlinks(&report.groups[0]).unwrap();
        assert_eq!(replaced, 1);
    }
}
