use std::path::Path;
use sysinfo::Disks;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PartitionInfo {
    pub name: String,
    pub mount_point: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
    pub used_percent: f64,
    pub fs_type: String,
}

impl PartitionInfo {
    pub fn format_bar(&self, width: usize) -> String {
        let ratio = (self.used_percent / 100.0).clamp(0.0, 1.0);
        let filled = ((ratio * width as f64).round() as usize).min(width);
        let empty = width.saturating_sub(filled);
        format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
    }
}

pub fn get_partition_for_path(target_path: &Path) -> Option<PartitionInfo> {
    let disks = Disks::new_with_refreshed_list();
    let canonical = target_path.canonicalize().unwrap_or_else(|_| target_path.to_path_buf());

    let mut best_match: Option<(&sysinfo::Disk, usize)> = None;

    for disk in &disks {
        let mount = disk.mount_point();
        if canonical.starts_with(mount) {
            let match_len = mount.as_os_str().len();
            match best_match {
                Some((_, current_len)) if match_len > current_len => {
                    best_match = Some((disk, match_len));
                }
                None => {
                    best_match = Some((disk, match_len));
                }
                _ => {}
            }
        }
    }

    if let Some((disk, _)) = best_match {
        let total = disk.total_space();
        let available = disk.available_space();
        let used = total.saturating_sub(available);
        let used_percent = if total > 0 {
            (used as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        Some(PartitionInfo {
            name: disk.name().to_string_lossy().to_string(),
            mount_point: disk.mount_point().to_string_lossy().to_string(),
            total_bytes: total,
            available_bytes: available,
            used_bytes: used,
            used_percent,
            fs_type: disk.file_system().to_string_lossy().to_string(),
        })
    } else {
        None
    }
}
