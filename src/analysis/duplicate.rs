use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;
use crate::fs::model::FileItem;
use crate::fs::processes::ProcessLockInfo;

#[derive(Debug, Clone)]
pub struct DuplicateGroup {
    pub file_size: u64,
    #[allow(dead_code)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupMethod {
    Reflink,
    Hardlink,
}

#[derive(Debug, Clone)]
pub struct DedupOptions {
    pub allow_hardlink: bool,
    pub dry_run: bool,
    pub min_size: u64,
}

impl Default for DedupOptions {
    fn default() -> Self {
        Self {
            allow_hardlink: false,
            dry_run: true,
            min_size: 1_048_576, // 1 MiB default
        }
    }
}

#[derive(Debug, Clone)]
pub struct SkippedFile {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct ProcessedFile {
    pub path: PathBuf,
    pub method: DedupMethod,
    pub bytes_reclaimed: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ClusterDedupResult {
    pub cluster_idx: usize,
    pub file_size: u64,
    pub primary_path: PathBuf,
    pub processed: Vec<ProcessedFile>,
    pub skipped: Vec<SkippedFile>,
    pub total_bytes_reclaimed: u64,
}

#[derive(Debug, Clone, Default)]
pub struct DedupRunReport {
    pub clusters: Vec<ClusterDedupResult>,
    pub total_files_processed: usize,
    pub total_files_skipped: usize,
    pub total_bytes_reclaimed: u64,
    #[allow(dead_code)]
    pub is_dry_run: bool,
}

#[derive(Debug)]
pub enum CandidateOutcome {
    Processed(DedupMethod, u64),
    Skipped(String),
}

/// Trait abstracting low-level platform deduplication operations for unit testing
pub trait DedupOps {
    fn check_byte_by_byte(&self, path_a: &Path, path_b: &Path) -> std::io::Result<bool> {
        compare_byte_by_byte(path_a, path_b)
    }

    fn try_reflink(&self, src: &Path, dst: &Path) -> Result<()>;

    fn try_hardlink(&self, src: &Path, dst: &Path) -> Result<()>;

    fn get_device_id(&self, path: &Path) -> std::io::Result<u64>;

    fn check_open_processes(&self, path: &Path) -> Option<ProcessLockInfo>;

    fn is_reflink_supported(&self, path: &Path) -> bool;
}

pub struct RealDedupOps;

impl DedupOps for RealDedupOps {
    fn try_reflink(&self, src: &Path, dst: &Path) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            reflink_file_linux(src, dst)
        }
        #[cfg(target_os = "macos")]
        {
            reflink_file_macos(src, dst)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (src, dst);
            bail!("Reflink/clone is not supported on this platform")
        }
    }

    fn try_hardlink(&self, src: &Path, dst: &Path) -> Result<()> {
        std::fs::hard_link(src, dst).context("Failed to create hardlink")
    }

    fn get_device_id(&self, path: &Path) -> std::io::Result<u64> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata(path).map(|m| m.dev())
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Ok(1)
        }
    }

    fn check_open_processes(&self, path: &Path) -> Option<ProcessLockInfo> {
        crate::fs::processes::check_open_processes(path).into_iter().next()
    }

    fn is_reflink_supported(&self, path: &Path) -> bool {
        supports_reflink(path)
    }
}

/// Stage 1 helper: Prefix 4KB hash
fn compute_prefix_hash(path: &Path) -> Option<[u8; 32]> {
    let mut file = File::open(path).ok()?;
    let mut buffer = [0u8; 4096];
    let n = file.read(&mut buffer).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&buffer[..n]);
    Some(hasher.finalize().into())
}

/// Stage 2 helper: Full SHA-256 hash
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

/// Requirement 1: Byte-by-byte content comparison before any modification
pub fn compare_byte_by_byte(path_a: &Path, path_b: &Path) -> std::io::Result<bool> {
    let mut file_a = File::open(path_a)?;
    let mut file_b = File::open(path_b)?;

    let len_a = file_a.metadata()?.len();
    let len_b = file_b.metadata()?.len();
    if len_a != len_b {
        return Ok(false);
    }

    let mut buf_a = [0u8; 64 * 1024];
    let mut buf_b = [0u8; 64 * 1024];
    let mut remaining = len_a;

    while remaining > 0 {
        let to_read = std::cmp::min(remaining as usize, buf_a.len());
        file_a.read_exact(&mut buf_a[..to_read])?;
        file_b.read_exact(&mut buf_b[..to_read])?;

        if buf_a[..to_read] != buf_b[..to_read] {
            return Ok(false);
        }
        remaining -= to_read as u64;
    }

    Ok(true)
}

/// Checks whether a volume/path supports copy-on-write reflink (Btrfs, XFS, APFS)
pub fn supports_reflink(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        let _ = path;
        true
    }
    #[cfg(target_os = "linux")]
    {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
            let mut best_mount_len = 0;
            let mut best_fs_type = String::new();
            for line in mounts.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    let mount_dir = Path::new(parts[1]);
                    if canonical.starts_with(mount_dir) && parts[1].len() >= best_mount_len {
                        best_mount_len = parts[1].len();
                        best_fs_type = parts[2].to_lowercase();
                    }
                }
            }
            if best_fs_type == "btrfs" || best_fs_type == "xfs" {
                return true;
            }
        }
        false
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = path;
        false
    }
}

/// Linux FICLONE ioctl implementation
#[cfg(target_os = "linux")]
pub fn reflink_file_linux(src: &Path, dst: &Path) -> Result<()> {
    use std::fs::OpenOptions;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::io::AsRawFd;

    let src_file = File::open(src).context("Failed to open source file for reflink")?;
    let src_meta = src_file.metadata()?;

    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    opts.mode(src_meta.permissions().mode());

    let dst_file = opts.open(dst).context("Failed to create temporary file for reflink")?;

    const FICLONE: libc::c_ulong = 0x40049409;

    let ret = unsafe {
        libc::ioctl(
            dst_file.as_raw_fd(),
            FICLONE as _,
            src_file.as_raw_fd(),
        )
    };

    if ret != 0 {
        let err = std::io::Error::last_os_error();
        drop(dst_file);
        let _ = std::fs::remove_file(dst);
        bail!("FICLONE reflink ioctl failed: {}", err);
    }

    Ok(())
}

/// macOS clonefile APFS implementation
#[cfg(target_os = "macos")]
pub fn reflink_file_macos(src: &Path, dst: &Path) -> Result<()> {
    use std::ffi::CString;

    let src_c = CString::new(src.to_str().context("Invalid src path")?)?;
    let dst_c = CString::new(dst.to_str().context("Invalid dst path")?)?;

    extern "C" {
        fn clonefile(
            src: *const libc::c_char,
            dst: *const libc::c_char,
            flags: libc::c_int,
        ) -> libc::c_int;
    }

    let ret = unsafe { clonefile(src_c.as_ptr(), dst_c.as_ptr(), 0) };
    if ret != 0 {
        let err = std::io::Error::last_os_error();
        let _ = std::fs::remove_file(dst);
        bail!("macOS clonefile failed: {}", err);
    }

    Ok(())
}

/// Scans directory tree using the 3-stage pipeline: (1) exact size -> (2) 4KB prefix -> (3) full SHA-256
pub fn scan_duplicates(root: &Path) -> DuplicateReport {
    // Stage 1: Group by exact byte size
    let mut size_map: HashMap<u64, Vec<PathBuf>> = HashMap::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file()
            && let Ok(meta) = entry.metadata() {
                let len = meta.len();
                if len >= 512 {
                    size_map.entry(len).or_default().push(entry.path().to_path_buf());
                }
            }
    }

    // Stage 2: Prefix 4KB hash for size collisions
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

    // Stage 3: Full SHA-256 for exact match verification
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
    groups.sort_by_key(|a| std::cmp::Reverse(a.wasted_bytes()));

    DuplicateReport {
        total_duplicate_files: total_dupe_files,
        total_wasted_bytes: total_wasted,
        groups,
    }
}

/// Safely processes a single duplicate candidate file against the primary file
pub fn deduplicate_candidate<O: DedupOps>(
    primary: &Path,
    secondary: &Path,
    file_size: u64,
    cluster_idx: usize,
    counter: usize,
    options: &DedupOptions,
    ops: &O,
) -> Result<CandidateOutcome> {
    if !secondary.exists() {
        return Ok(CandidateOutcome::Skipped("Duplicate file no longer exists".into()));
    }
    if !primary.exists() {
        return Ok(CandidateOutcome::Skipped("Primary file no longer exists".into()));
    }
    if primary == secondary {
        return Ok(CandidateOutcome::Skipped("Cannot deduplicate file onto itself".into()));
    }

    // Check protected paths
    if let Err(e) = crate::fs::protected::check_protected_path(secondary, None) {
        return Ok(CandidateOutcome::Skipped(format!("Protected path: {}", e)));
    }

    // Requirement 4: Skip files smaller than configurable minimum size (default 1 MiB)
    if file_size < options.min_size {
        return Ok(CandidateOutcome::Skipped(format!(
            "File size ({}) is smaller than minimum threshold ({})",
            FileItem::format_size(file_size),
            FileItem::format_size(options.min_size)
        )));
    }

    // Requirement 4: Never cross filesystem boundaries
    let p_dev = ops.get_device_id(primary).unwrap_or(0);
    let s_dev = ops.get_device_id(secondary).unwrap_or(0);
    if p_dev != s_dev || p_dev == 0 {
        return Ok(CandidateOutcome::Skipped(
            "Cross-filesystem boundary (different storage devices)".into(),
        ));
    }

    // Inode check: Already hardlinked?
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(p_meta), Ok(s_meta)) = (std::fs::metadata(primary), std::fs::metadata(secondary))
            && p_meta.dev() == s_meta.dev()
            && p_meta.ino() == s_meta.ino()
        {
            return Ok(CandidateOutcome::Skipped("Files are already hardlinked (share same inode)".into()));
        }
    }

    // Requirement 4: Skip files with different ownership or permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::fs::PermissionsExt;
        if let (Ok(p_meta), Ok(s_meta)) = (std::fs::metadata(primary), std::fs::metadata(secondary)) {
            if p_meta.uid() != s_meta.uid() || p_meta.gid() != s_meta.gid() {
                return Ok(CandidateOutcome::Skipped(format!(
                    "Different file ownership (primary UID:GID {}:{} vs secondary {}:{})",
                    p_meta.uid(), p_meta.gid(), s_meta.uid(), s_meta.gid()
                )));
            }
            let p_mode = p_meta.permissions().mode() & 0o7777;
            let s_mode = s_meta.permissions().mode() & 0o7777;
            if p_mode != s_mode {
                return Ok(CandidateOutcome::Skipped(format!(
                    "Different file permissions (primary {:04o} vs secondary {:04o})",
                    p_mode, s_mode
                )));
            }
        }
    }

    // Requirement 4: Skip files open by a running process
    if let Some(lock) = ops.check_open_processes(primary) {
        return Ok(CandidateOutcome::Skipped(format!(
            "Primary file is currently open by process '{}' (PID {})",
            lock.process_name, lock.pid
        )));
    }
    if let Some(lock) = ops.check_open_processes(secondary) {
        return Ok(CandidateOutcome::Skipped(format!(
            "File is currently open by process '{}' (PID {})",
            lock.process_name, lock.pid
        )));
    }

    // Requirement 1: Byte-by-byte comparison before ANY modification
    match ops.check_byte_by_byte(primary, secondary) {
        Ok(true) => {}
        Ok(false) => {
            return Ok(CandidateOutcome::Skipped(
                "Byte-by-byte content verification failed (files differ)".into(),
            ));
        }
        Err(e) => {
            return Ok(CandidateOutcome::Skipped(format!(
                "Failed to read files for byte-by-byte comparison: {}",
                e
            )));
        }
    }

    // In Dry-run mode: evaluate strategy and return without touching filesystem
    if options.dry_run {
        if ops.is_reflink_supported(secondary) {
            return Ok(CandidateOutcome::Processed(DedupMethod::Reflink, file_size));
        } else if options.allow_hardlink {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let (Ok(p_meta), Ok(s_meta)) = (std::fs::metadata(primary), std::fs::metadata(secondary))
                    && ((p_meta.permissions().mode() & 0o022) != 0 || (s_meta.permissions().mode() & 0o022) != 0)
                {
                    return Ok(CandidateOutcome::Skipped(
                        "File is writable by group or others (unsafe for hardlink; risk of shared mutation)".into(),
                    ));
                }
            }
            return Ok(CandidateOutcome::Processed(DedupMethod::Hardlink, file_size));
        } else {
            return Ok(CandidateOutcome::Skipped(
                "Reflink not supported on this filesystem (pass --hardlink to allow hardlinking)".into(),
            ));
        }
    }

    // Requirement 3: Atomic operation using temporary file in same directory, then rename()
    let parent_dir = secondary.parent().unwrap_or_else(|| Path::new("."));
    let temp_name = format!(
        ".spacer_dedup_{}_{}_{}.tmp",
        std::process::id(),
        cluster_idx,
        counter
    );
    let temp_path = parent_dir.join(temp_name);
    if temp_path.exists() {
        let _ = std::fs::remove_file(&temp_path);
    }

    // Requirement 2: Dedup strategy order: (a) reflink/clone, (b) hardlink only if explicitly enabled
    let (method, reflink_err) = match ops.try_reflink(primary, &temp_path) {
        Ok(_) => (Some(DedupMethod::Reflink), None),
        Err(e) => {
            let _ = std::fs::remove_file(&temp_path);
            (None, Some(e))
        }
    };

    let method = if let Some(m) = method {
        m
    } else if options.allow_hardlink {
        // Clean temp file before trying hardlink
        let _ = std::fs::remove_file(&temp_path);

        // Check group/other writable for hardlink safety
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let (Ok(p_meta), Ok(s_meta)) = (std::fs::metadata(primary), std::fs::metadata(secondary))
                && ((p_meta.permissions().mode() & 0o022) != 0 || (s_meta.permissions().mode() & 0o022) != 0)
            {
                return Ok(CandidateOutcome::Skipped(
                    "File is writable by group or others (unsafe for hardlink; risk of shared mutation)".into(),
                ));
            }
        }

        if ops.try_hardlink(primary, &temp_path).is_ok() {
            DedupMethod::Hardlink
        } else {
            let _ = std::fs::remove_file(&temp_path);
            return Ok(CandidateOutcome::Skipped("Hardlink creation failed".into()));
        }
    } else {
        let _ = std::fs::remove_file(&temp_path);
        let err_msg = reflink_err.map(|e| e.to_string()).unwrap_or_default();
        return Ok(CandidateOutcome::Skipped(format!(
            "Reflink/clone not supported on this filesystem ({}) — pass --hardlink to allow hardlinking",
            err_msg
        )));
    };

    // Atomic rename over duplicate
    if let Err(e) = std::fs::rename(&temp_path, secondary) {
        let _ = std::fs::remove_file(&temp_path);
        return Ok(CandidateOutcome::Skipped(format!(
            "Atomic rename over duplicate failed (original file kept intact): {}",
            e
        )));
    }

    // Log cleanup action
    crate::fs::logger::log_cleanup_action(
        match method {
            DedupMethod::Reflink => "DEDUP_REFLINK",
            DedupMethod::Hardlink => "DEDUP_HARDLINK",
        },
        secondary,
        file_size,
        match method {
            DedupMethod::Reflink => "reflink_ficlone",
            DedupMethod::Hardlink => "hardlink_ln_f",
        },
    );

    Ok(CandidateOutcome::Processed(method, file_size))
}

/// Deduplicates a single cluster
pub fn deduplicate_cluster<O: DedupOps>(
    group: &DuplicateGroup,
    cluster_idx: usize,
    options: &DedupOptions,
    ops: &O,
) -> ClusterDedupResult {
    if group.paths.len() < 2 {
        return ClusterDedupResult {
            cluster_idx,
            file_size: group.file_size,
            primary_path: group.paths.first().cloned().unwrap_or_default(),
            processed: Vec::new(),
            skipped: Vec::new(),
            total_bytes_reclaimed: 0,
        };
    }

    let primary = &group.paths[0];
    let mut processed = Vec::new();
    let mut skipped = Vec::new();
    let mut total_bytes_reclaimed = 0;

    for (counter, secondary) in group.paths[1..].iter().enumerate() {
        match deduplicate_candidate(primary, secondary, group.file_size, cluster_idx, counter, options, ops) {
            Ok(CandidateOutcome::Processed(method, size)) => {
                total_bytes_reclaimed += size;
                processed.push(ProcessedFile {
                    path: secondary.clone(),
                    method,
                    bytes_reclaimed: size,
                });
            }
            Ok(CandidateOutcome::Skipped(reason)) => {
                skipped.push(SkippedFile {
                    path: secondary.clone(),
                    reason,
                });
            }
            Err(e) => {
                skipped.push(SkippedFile {
                    path: secondary.clone(),
                    reason: format!("Unexpected error: {}", e),
                });
            }
        }
    }

    ClusterDedupResult {
        cluster_idx,
        file_size: group.file_size,
        primary_path: primary.clone(),
        processed,
        skipped,
        total_bytes_reclaimed,
    }
}

pub fn deduplicate_group(
    group: &DuplicateGroup,
    cluster_idx: usize,
    options: &DedupOptions,
) -> ClusterDedupResult {
    deduplicate_cluster(group, cluster_idx, options, &RealDedupOps)
}

/// Runs deduplication across all clusters in a DuplicateReport
pub fn deduplicate_all<O: DedupOps>(
    report: &DuplicateReport,
    options: &DedupOptions,
    ops: &O,
) -> DedupRunReport {
    let mut clusters = Vec::new();
    let mut total_files_processed = 0;
    let mut total_files_skipped = 0;
    let mut total_bytes_reclaimed = 0;

    for (idx, group) in report.groups.iter().enumerate() {
        let cluster_res = deduplicate_cluster(group, idx + 1, options, ops);
        total_files_processed += cluster_res.processed.len();
        total_files_skipped += cluster_res.skipped.len();
        total_bytes_reclaimed += cluster_res.total_bytes_reclaimed;
        clusters.push(cluster_res);
    }

    DedupRunReport {
        clusters,
        total_files_processed,
        total_files_skipped,
        total_bytes_reclaimed,
        is_dry_run: options.dry_run,
    }
}

/// Backward-compatible helper for hardlinking duplicates
#[allow(dead_code)]
pub fn replace_with_hardlinks(group: &DuplicateGroup) -> Result<usize> {
    let options = DedupOptions {
        allow_hardlink: true,
        dry_run: false,
        min_size: 0,
    };
    let ops = RealDedupOps;
    let res = deduplicate_cluster(group, 1, &options, &ops);
    Ok(res.processed.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;
    use std::collections::HashSet;

    #[derive(Default)]
    struct MockDedupOps {
        pub reflink_supported: bool,
        pub fail_reflink: bool,
        pub fail_hardlink: bool,
        pub device_ids: HashMap<PathBuf, u64>,
        pub open_processes: HashSet<PathBuf>,
    }

    impl DedupOps for MockDedupOps {
        fn try_reflink(&self, src: &Path, dst: &Path) -> Result<()> {
            if self.fail_reflink || !self.reflink_supported {
                bail!("Reflink not supported or simulated failure");
            }
            std::fs::copy(src, dst)?;
            Ok(())
        }

        fn try_hardlink(&self, src: &Path, dst: &Path) -> Result<()> {
            if self.fail_hardlink {
                bail!("Simulated hardlink error");
            }
            std::fs::hard_link(src, dst).context("Hardlink error")
        }

        fn get_device_id(&self, path: &Path) -> std::io::Result<u64> {
            if let Some(&dev) = self.device_ids.get(path) {
                return Ok(dev);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                std::fs::metadata(path).map(|m| m.dev())
            }
            #[cfg(not(unix))]
            {
                let _ = path;
                Ok(1)
            }
        }

        fn check_open_processes(&self, path: &Path) -> Option<ProcessLockInfo> {
            if self.open_processes.contains(path) {
                Some(ProcessLockInfo {
                    pid: 9999,
                    process_name: "test_runner".to_string(),
                    locked_path: path.display().to_string(),
                })
            } else {
                None
            }
        }

        fn is_reflink_supported(&self, _path: &Path) -> bool {
            self.reflink_supported
        }
    }

    #[test]
    fn test_duplicate_detection_3stage() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("original.dat");
        let file2 = dir.path().join("copy.dat");
        let file3 = dir.path().join("different.dat");

        let content = "The quick brown fox jumps over the lazy dog repeated text for testing duplicates 1234567890".repeat(50);
        fs::write(&file1, &content).unwrap();
        fs::write(&file2, &content).unwrap();
        fs::write(&file3, "Something completely different content").unwrap();

        let report = scan_duplicates(dir.path());
        assert_eq!(report.groups.len(), 1);
        assert_eq!(report.groups[0].paths.len(), 2);
        assert_eq!(report.total_duplicate_files, 1);
    }

    #[test]
    fn test_identical_files_dedup_success() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("file1.bin");
        let file2 = dir.path().join("file2.bin");

        // 1.5 MiB file to satisfy default min_size
        let content = vec![0xAB; 1_500_000];
        fs::write(&file1, &content).unwrap();
        fs::write(&file2, &content).unwrap();

        let group = DuplicateGroup {
            file_size: 1_500_000,
            sha256_hash: "mockhash".into(),
            paths: vec![file1.clone(), file2.clone()],
        };

        let mut ops = MockDedupOps::default();
        ops.reflink_supported = true;

        let options = DedupOptions {
            allow_hardlink: false,
            dry_run: false,
            min_size: 1_048_576,
        };

        let result = deduplicate_cluster(&group, 1, &options, &ops);
        assert_eq!(result.processed.len(), 1);
        assert_eq!(result.skipped.len(), 0);
        assert_eq!(result.total_bytes_reclaimed, 1_500_000);
        assert_eq!(result.processed[0].method, DedupMethod::Reflink);

        // Verify both files still exist and have identical content
        assert_eq!(fs::read(&file1).unwrap(), content);
        assert_eq!(fs::read(&file2).unwrap(), content);
    }

    #[test]
    fn test_same_size_different_content_rejected() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("f1.bin");
        let file2 = dir.path().join("f2.bin");

        let content1 = vec![0x11; 1_500_000];
        let mut content2 = vec![0x11; 1_500_000];
        // Mutate byte in the second file
        content2[5000] = 0x99;

        fs::write(&file1, &content1).unwrap();
        fs::write(&file2, &content2).unwrap();

        let group = DuplicateGroup {
            file_size: 1_500_000,
            sha256_hash: "same_size".into(),
            paths: vec![file1.clone(), file2.clone()],
        };

        let ops = MockDedupOps::default();
        let options = DedupOptions {
            allow_hardlink: true,
            dry_run: false,
            min_size: 1_048_576,
        };

        let result = deduplicate_cluster(&group, 1, &options, &ops);
        assert_eq!(result.processed.len(), 0);
        assert_eq!(result.skipped.len(), 1);
        assert!(result.skipped[0].reason.contains("Byte-by-byte content verification failed"));

        // Verify files are untouched
        assert_eq!(fs::read(&file1).unwrap(), content1);
        assert_eq!(fs::read(&file2).unwrap(), content2);
    }

    #[test]
    fn test_cross_device_skip() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("primary.bin");
        let file2 = dir.path().join("secondary.bin");

        let content = vec![0xEE; 1_500_000];
        fs::write(&file1, &content).unwrap();
        fs::write(&file2, &content).unwrap();

        let group = DuplicateGroup {
            file_size: 1_500_000,
            sha256_hash: "hash".into(),
            paths: vec![file1.clone(), file2.clone()],
        };

        let mut ops = MockDedupOps::default();
        ops.device_ids.insert(file1.clone(), 100);
        ops.device_ids.insert(file2.clone(), 200); // simulated different device

        let options = DedupOptions {
            allow_hardlink: true,
            dry_run: false,
            min_size: 1_048_576,
        };

        let result = deduplicate_cluster(&group, 1, &options, &ops);
        assert_eq!(result.processed.len(), 0);
        assert_eq!(result.skipped.len(), 1);
        assert!(result.skipped[0].reason.contains("Cross-filesystem boundary"));

        // Files untouched
        assert_eq!(fs::read(&file2).unwrap(), content);
    }

    #[test]
    fn test_failure_rollback_and_cleanup() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("prime.bin");
        let file2 = dir.path().join("dupe.bin");

        let content = vec![0x77; 1_500_000];
        fs::write(&file1, &content).unwrap();
        fs::write(&file2, &content).unwrap();

        let group = DuplicateGroup {
            file_size: 1_500_000,
            sha256_hash: "hash".into(),
            paths: vec![file1.clone(), file2.clone()],
        };

        let mut ops = MockDedupOps::default();
        ops.reflink_supported = true;
        ops.fail_reflink = true; // Inject reflink failure
        ops.fail_hardlink = true; // Inject hardlink failure

        let options = DedupOptions {
            allow_hardlink: true,
            dry_run: false,
            min_size: 1_048_576,
        };

        let result = deduplicate_cluster(&group, 1, &options, &ops);
        assert_eq!(result.processed.len(), 0);
        assert_eq!(result.skipped.len(), 1);

        // Verify original file is 100% untouched and intact
        assert_eq!(fs::read(&file2).unwrap(), content);

        // Verify no temp file left in directory
        let files_in_dir: Vec<_> = fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(files_in_dir.len(), 2);
    }

    #[test]
    fn test_min_size_skip() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("small1.bin");
        let file2 = dir.path().join("small2.bin");

        let content = vec![0x42; 500_000]; // 500 KB (< 1 MiB)
        fs::write(&file1, &content).unwrap();
        fs::write(&file2, &content).unwrap();

        let group = DuplicateGroup {
            file_size: 500_000,
            sha256_hash: "hash".into(),
            paths: vec![file1.clone(), file2.clone()],
        };

        let ops = MockDedupOps::default();
        let options = DedupOptions {
            allow_hardlink: true,
            dry_run: false,
            min_size: 1_048_576, // 1 MiB default
        };

        let result = deduplicate_cluster(&group, 1, &options, &ops);
        assert_eq!(result.processed.len(), 0);
        assert_eq!(result.skipped.len(), 1);
        assert!(result.skipped[0].reason.contains("smaller than minimum threshold"));
    }

    #[test]
    #[cfg(unix)]
    fn test_writable_by_others_skip_hardlink() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("f1.bin");
        let file2 = dir.path().join("f2.bin");

        let content = vec![0x55; 1_500_000];
        fs::write(&file1, &content).unwrap();
        fs::write(&file2, &content).unwrap();

        // Make both files group and world writable: 0o666 (so their permissions match)
        let mut perms1 = fs::metadata(&file1).unwrap().permissions();
        perms1.set_mode(0o666);
        fs::set_permissions(&file1, perms1).unwrap();

        let mut perms2 = fs::metadata(&file2).unwrap().permissions();
        perms2.set_mode(0o666);
        fs::set_permissions(&file2, perms2).unwrap();

        let group = DuplicateGroup {
            file_size: 1_500_000,
            sha256_hash: "hash".into(),
            paths: vec![file1.clone(), file2.clone()],
        };

        let ops = MockDedupOps::default(); // reflink not supported, attempts hardlink
        let options = DedupOptions {
            allow_hardlink: true,
            dry_run: false,
            min_size: 1_048_576,
        };

        let result = deduplicate_cluster(&group, 1, &options, &ops);
        assert_eq!(result.processed.len(), 0);
        assert_eq!(result.skipped.len(), 1);
        assert!(result.skipped[0].reason.contains("writable by group or others"));
    }

    #[test]
    #[cfg(unix)]
    fn test_different_permissions_skip() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("p1.bin");
        let file2 = dir.path().join("p2.bin");

        let content = vec![0x66; 1_500_000];
        fs::write(&file1, &content).unwrap();
        fs::write(&file2, &content).unwrap();

        // Change file2 to 0o600 while file1 is 0o644
        let mut perms = fs::metadata(&file2).unwrap().permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&file2, perms).unwrap();

        let group = DuplicateGroup {
            file_size: 1_500_000,
            sha256_hash: "hash".into(),
            paths: vec![file1.clone(), file2.clone()],
        };

        let ops = MockDedupOps::default();
        let options = DedupOptions {
            allow_hardlink: true,
            dry_run: false,
            min_size: 1_048_576,
        };

        let result = deduplicate_cluster(&group, 1, &options, &ops);
        assert_eq!(result.processed.len(), 0);
        assert_eq!(result.skipped.len(), 1);
        assert!(result.skipped[0].reason.contains("Different file permissions"));
    }

    #[test]
    fn test_open_process_skip() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("open1.bin");
        let file2 = dir.path().join("open2.bin");

        let content = vec![0x33; 1_500_000];
        fs::write(&file1, &content).unwrap();
        fs::write(&file2, &content).unwrap();

        let group = DuplicateGroup {
            file_size: 1_500_000,
            sha256_hash: "hash".into(),
            paths: vec![file1.clone(), file2.clone()],
        };

        let mut ops = MockDedupOps::default();
        ops.open_processes.insert(file2.clone());

        let options = DedupOptions {
            allow_hardlink: true,
            dry_run: false,
            min_size: 1_048_576,
        };

        let result = deduplicate_cluster(&group, 1, &options, &ops);
        assert_eq!(result.processed.len(), 0);
        assert_eq!(result.skipped.len(), 1);
        assert!(result.skipped[0].reason.contains("open by process"));
    }
}
