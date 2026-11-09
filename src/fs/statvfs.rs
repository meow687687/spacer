use std::ffi::CString;
use std::mem::MaybeUninit;
use std::path::Path;

/// Queries the available bytes on the filesystem volume containing `path` using statvfs.
#[allow(clippy::unnecessary_cast)]
pub fn get_available_bytes(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        let path_str = path.to_str()?;
        let c_path = CString::new(path_str).ok()?;
        unsafe {
            let mut stat: MaybeUninit<libc::statvfs> = MaybeUninit::uninit();
            if libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) == 0 {
                let stat = stat.assume_init();
                // f_bavail is available blocks for unprivileged users, f_frsize is fragment/block size
                let frsize = if stat.f_frsize > 0 {
                    stat.f_frsize as u64
                } else {
                    stat.f_bsize as u64
                };
                Some(stat.f_bavail as u64 * frsize)
            } else {
                None
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// Checks whether snapshots or snapshot-related tools are active/detected on the system
/// e.g. Btrfs snapshots, ZFS, Timeshift, Snapper.
pub fn detect_btrfs_and_snapshots(target_path: &Path) -> Option<String> {
    // 1. Check if filesystem is Btrfs
    let mut is_btrfs = false;
    let canonical = target_path.canonicalize().unwrap_or_else(|_| target_path.to_path_buf());

    let disks = sysinfo::Disks::new_with_refreshed_list();
    for disk in &disks {
        let mount = disk.mount_point();
        if canonical.starts_with(mount) {
            let fs = disk.file_system().to_string_lossy().to_lowercase();
            if fs.contains("btrfs") {
                is_btrfs = true;
                break;
            }
        }
    }

    // Also check /proc/mounts on Linux as fallback
    #[cfg(target_os = "linux")]
    if !is_btrfs
        && let Ok(mounts) = std::fs::read_to_string("/proc/mounts")
    {
        for line in mounts.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let mount_dir = Path::new(parts[1]);
                let fs_type = parts[2];
                if canonical.starts_with(mount_dir) && fs_type == "btrfs" {
                    is_btrfs = true;
                    break;
                }
            }
        }
    }

    // 2. Detect snapshot tools
    let has_timeshift = Path::new("/etc/timeshift").exists()
        || Path::new("/var/log/timeshift").exists()
        || Path::new("/timeshift").exists();
    let has_snapper = Path::new("/etc/snapper").exists()
        || Path::new("/.snapshots").exists()
        || canonical.join(".snapshots").exists();

    if is_btrfs {
        if has_snapper && has_timeshift {
            Some("Btrfs + Snapper & Timeshift snapshots detected (deleted files might stay in snapshots)".to_string())
        } else if has_snapper {
            Some("Btrfs + Snapper snapshots detected (deleted files might stay in snapshots)".to_string())
        } else if has_timeshift {
            Some("Btrfs + Timeshift snapshots detected (deleted files might stay in snapshots)".to_string())
        } else {
            Some("Btrfs filesystem active (snapshots may preserve deleted file extents)".to_string())
        }
    } else if has_timeshift {
        Some("Timeshift snapshots active on system (freed space might remain retained)".to_string())
    } else if has_snapper {
        Some("Snapper snapshots active on system (freed space might remain retained)".to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_statvfs_available_bytes() {
        let path = Path::new("/");
        if path.exists() {
            let bytes = get_available_bytes(path);
            assert!(bytes.is_some());
            assert!(bytes.unwrap() > 0);
        }
    }
}
