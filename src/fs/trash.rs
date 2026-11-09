use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use anyhow::{bail, Context, Result};
use chrono::Local;

/// Target destination info for moving to Trash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashLocation {
    pub files_dir: PathBuf,
    pub info_dir: PathBuf,
    pub is_cross_device: bool,
    pub trash_base: PathBuf,
}

/// Abstract Filesystem trait to allow unit testing and mocking.
pub trait TrashFs {
    fn get_device_id(&self, path: &Path) -> Result<u64>;
    fn path_exists(&self, path: &Path) -> bool;
    fn create_dir_all(&self, path: &Path) -> Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> Result<()>;
    fn write_file(&self, path: &Path, content: &str) -> Result<()>;
    fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>>;
    fn remove_entry(&self, path: &Path) -> Result<()>;
}

/// Standard OS-backed Filesystem implementation.
#[derive(Default)]
pub struct RealTrashFs;

impl TrashFs for RealTrashFs {
    fn get_device_id(&self, path: &Path) -> Result<u64> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let meta = fs::symlink_metadata(path).or_else(|_| fs::metadata(path))?;
            Ok(meta.dev())
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Ok(0)
        }
    }

    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path).map_err(Into::into)
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        fs::rename(from, to).map_err(Into::into)
    }

    fn write_file(&self, path: &Path, content: &str) -> Result<()> {
        let mut file = fs::File::create(path)?;
        file.write_all(content.as_bytes())?;
        Ok(())
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(path)? {
            entries.push(entry?.path());
        }
        Ok(entries)
    }

    fn remove_entry(&self, path: &Path) -> Result<()> {
        if path.is_dir() {
            fs::remove_dir_all(path).map_err(Into::into)
        } else {
            fs::remove_file(path).map_err(Into::into)
        }
    }
}

/// Resolves the mount point of a given path.
pub fn get_mount_point(path: &Path) -> PathBuf {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut current = canonical.as_path();

    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut best_mount = PathBuf::from("/");
    let mut best_len = 0;

    for disk in &disks {
        let mount = disk.mount_point();
        if canonical.starts_with(mount) && mount.as_os_str().len() > best_len {
            best_len = mount.as_os_str().len();
            best_mount = mount.to_path_buf();
        }
    }

    if best_len > 0 {
        return best_mount;
    }

    // Fallback on unix: check st_dev parent transitions
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = fs::metadata(current) {
            let dev = meta.dev();
            while let Some(parent) = current.parent() {
                if let Ok(parent_meta) = fs::metadata(parent) {
                    if parent_meta.dev() != dev {
                        return current.to_path_buf();
                    }
                } else {
                    break;
                }
                current = parent;
            }
            return current.to_path_buf();
        }
    }

    PathBuf::from("/")
}

/// Resolves Freedesktop / macOS trash directories.
pub fn resolve_trash_location<F: TrashFs>(
    fs: &F,
    target: &Path,
    home_dir: &Path,
    uid: u32,
) -> Result<TrashLocation> {
    #[cfg(target_os = "macos")]
    {
        let mac_home_trash = home_dir.join(".Trash");
        let is_cross_device = if let (Ok(target_dev), Ok(home_dev)) =
            (fs.get_device_id(target), fs.get_device_id(home_dir))
        {
            target_dev != home_dev
        } else {
            false
        };

        return Ok(TrashLocation {
            files_dir: mac_home_trash.clone(),
            info_dir: mac_home_trash.clone(),
            is_cross_device,
            trash_base: mac_home_trash,
        });
    }

    #[cfg(not(target_os = "macos"))]
    {
        let target_canonical = target.canonicalize().unwrap_or_else(|_| target.to_path_buf());
        let home_canonical = home_dir.canonicalize().unwrap_or_else(|_| home_dir.to_path_buf());

        let target_dev = fs.get_device_id(&target_canonical).unwrap_or(0);
        let home_dev = fs.get_device_id(&home_canonical).unwrap_or(0);

        // 1. If target is on the same device as the user's home directory:
        // Use $XDG_DATA_HOME/Trash or $HOME/.local/share/Trash
        if target_dev == home_dev || target_dev == 0 || home_dev == 0 {
            let xdg_data_home = std::env::var("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| home_dir.join(".local").join("share"));
            let trash_base = xdg_data_home.join("Trash");
            return Ok(TrashLocation {
                files_dir: trash_base.join("files"),
                info_dir: trash_base.join("info"),
                is_cross_device: false,
                trash_base,
            });
        }

        // 2. Target is on a different filesystem volume / mount point:
        // Per Freedesktop specification:
        // First try $topdir/.Trash/$uid
        // If not usable (e.g. .Trash not existing or not a sticky dir), try $topdir/.Trash-$uid
        let topdir = get_mount_point(&target_canonical);
        let dot_trash = topdir.join(".Trash");
        let dot_trash_uid = dot_trash.join(uid.to_string());
        let fallback_trash = topdir.join(format!(".Trash-{}", uid));

        let candidate_base = if fs.path_exists(&dot_trash) {
            dot_trash_uid
        } else {
            fallback_trash
        };

        // If candidate directory can be created/used on this volume, use it.
        // Otherwise, falling back to home trash would be cross-device.
        let (trash_base, is_cross_device) = if fs.create_dir_all(&candidate_base.join("files")).is_ok() {
            (candidate_base, false)
        } else {
            let xdg_data_home = std::env::var("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| home_dir.join(".local").join("share"));
            (xdg_data_home.join("Trash"), true)
        };

        Ok(TrashLocation {
            files_dir: trash_base.join("files"),
            info_dir: trash_base.join("info"),
            is_cross_device,
            trash_base,
        })
    }
}

/// Moves an item into the resolved trash according to the Freedesktop / macOS specification.
pub fn move_to_trash_spec<F: TrashFs>(
    fs: &F,
    path: &Path,
    home_dir: &Path,
    uid: u32,
) -> Result<PathBuf> {
    if !fs.path_exists(path) {
        bail!("Source path does not exist: {}", path.display());
    }

    let trash_loc = resolve_trash_location(fs, path, home_dir, uid)?;

    if trash_loc.is_cross_device {
        bail!("Cannot atomically move to trash: '{}' is on a different volume. Moving to trash would require a cross-device copy+delete.", path.display());
    }

    // Ensure trash directories exist
    fs.create_dir_all(&trash_loc.files_dir)?;
    #[cfg(not(target_os = "macos"))]
    fs.create_dir_all(&trash_loc.info_dir)?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");

    // Resolve unique filename in trash
    let mut dest_name = file_name.to_string();
    let mut counter = 1;
    while fs.path_exists(&trash_loc.files_dir.join(&dest_name)) {
        dest_name = format!("{}.{}", file_name, counter);
        counter += 1;
    }

    let dest_file_path = trash_loc.files_dir.join(&dest_name);

    #[cfg(not(target_os = "macos"))]
    {
        // Write .trashinfo file
        let info_path = trash_loc.info_dir.join(format!("{}.trashinfo", dest_name));
        let abs_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let deletion_date = Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        let encoded_path = urlencoding::encode(&abs_path.to_string_lossy()).to_string();

        let trashinfo_content = format!(
            "[Trash Info]\nPath={}\nDeletionDate={}\n",
            encoded_path, deletion_date
        );
        fs.write_file(&info_path, &trashinfo_content)?;
    }

    // Rename (atomic move on same filesystem)
    if let Err(e) = fs.rename(path, &dest_file_path) {
        #[cfg(not(target_os = "macos"))]
        {
            let info_path = trash_loc.info_dir.join(format!("{}.trashinfo", dest_name));
            let _ = fs.remove_entry(&info_path);
        }
        return Err(e).context("Failed to move item to trash directory (possible cross-device copy needed)");
    }

    Ok(dest_file_path)
}

/// Empties the user's trash directory (home trash + mounted trash dirs).
pub fn empty_trash_all<F: TrashFs>(fs: &F, home_dir: &Path, uid: u32) -> Result<(usize, u64)> {
    let mut total_deleted = 0;
    let mut total_freed = 0;

    // 1. Home trash
    #[cfg(target_os = "macos")]
    let trash_dirs = vec![home_dir.join(".Trash")];

    #[cfg(not(target_os = "macos"))]
    let mut trash_dirs = vec![
        home_dir.join(".local").join("share").join("Trash"),
    ];

    #[cfg(not(target_os = "macos"))]
    {
        // Add per-volume trash directories
        let disks = sysinfo::Disks::new_with_refreshed_list();
        for disk in &disks {
            let m = disk.mount_point();
            trash_dirs.push(m.join(format!(".Trash-{}", uid)));
            trash_dirs.push(m.join(".Trash").join(uid.to_string()));
        }
    }

    for trash in trash_dirs {
        #[cfg(target_os = "macos")]
        let files_dirs = vec![trash.clone()];
        #[cfg(not(target_os = "macos"))]
        let files_dirs = vec![trash.join("files"), trash.join("info")];

        for dir in files_dirs {
            if fs.path_exists(&dir)
                && let Ok(entries) = fs.read_dir(&dir)
            {
                for entry in entries {
                    let size = std::fs::symlink_metadata(&entry).map(|m| m.len()).unwrap_or(0);
                    if fs.remove_entry(&entry).is_ok() {
                        total_deleted += 1;
                        total_freed += size;
                    }
                }
            }
        }
    }

    Ok((total_deleted, total_freed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct MockTrashFs {
        files: RefCell<HashMap<PathBuf, String>>,
        devices: RefCell<HashMap<PathBuf, u64>>,
        fail_create_dirs: RefCell<bool>,
    }

    impl TrashFs for MockTrashFs {
        fn get_device_id(&self, path: &Path) -> Result<u64> {
            Ok(*self.devices.borrow().get(path).unwrap_or(&1))
        }

        fn path_exists(&self, path: &Path) -> bool {
            self.files.borrow().contains_key(path)
                || self.files.borrow().keys().any(|k| k.starts_with(path))
        }

        fn create_dir_all(&self, _path: &Path) -> Result<()> {
            if *self.fail_create_dirs.borrow() {
                bail!("Read-only or forbidden filesystem");
            }
            Ok(())
        }

        fn rename(&self, from: &Path, to: &Path) -> Result<()> {
            let mut files = self.files.borrow_mut();
            if let Some(content) = files.remove(from) {
                files.insert(to.to_path_buf(), content);
                Ok(())
            } else {
                bail!("File not found");
            }
        }

        fn write_file(&self, path: &Path, content: &str) -> Result<()> {
            self.files.borrow_mut().insert(path.to_path_buf(), content.to_string());
            Ok(())
        }

        fn read_dir(&self, path: &Path) -> Result<Vec<PathBuf>> {
            let files = self.files.borrow();
            let mut res = Vec::new();
            for k in files.keys() {
                if k.parent() == Some(path) {
                    res.push(k.clone());
                }
            }
            Ok(res)
        }

        fn remove_entry(&self, path: &Path) -> Result<()> {
            self.files.borrow_mut().remove(path);
            Ok(())
        }
    }

    #[test]
    fn test_trash_path_selection_home() {
        let mock_fs = MockTrashFs::default();
        let home = Path::new("/home/user");
        let target = Path::new("/home/user/test.txt");

        mock_fs.files.borrow_mut().insert(target.to_path_buf(), "content".into());
        mock_fs.devices.borrow_mut().insert(home.to_path_buf(), 100);
        mock_fs.devices.borrow_mut().insert(target.to_path_buf(), 100);

        let loc = resolve_trash_location(&mock_fs, target, home, 1000).unwrap();
        assert!(!loc.is_cross_device);
        assert!(loc.files_dir.starts_with(home));
    }

    #[test]
    fn test_cross_device_detection() {
        let mock_fs = MockTrashFs::default();
        let home = Path::new("/home/user");
        let foreign_target = Path::new("/mnt/external/data.iso");

        mock_fs.files.borrow_mut().insert(foreign_target.to_path_buf(), "iso content".into());
        mock_fs.devices.borrow_mut().insert(home.to_path_buf(), 100);
        mock_fs.devices.borrow_mut().insert(foreign_target.to_path_buf(), 200);
        // Simulate unable to create per-volume trash
        *mock_fs.fail_create_dirs.borrow_mut() = true;

        let loc = resolve_trash_location(&mock_fs, foreign_target, home, 1000).unwrap();
        assert!(loc.is_cross_device);

        let result = move_to_trash_spec(&mock_fs, foreign_target, home, 1000);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cross-device"));
    }

    #[test]
    fn test_move_to_trash_spec() {
        let mock_fs = MockTrashFs::default();
        let home = Path::new("/home/user");
        let target = Path::new("/home/user/document.pdf");

        mock_fs.files.borrow_mut().insert(target.to_path_buf(), "pdf bytes".into());
        mock_fs.devices.borrow_mut().insert(home.to_path_buf(), 1);
        mock_fs.devices.borrow_mut().insert(target.to_path_buf(), 1);

        let dest = move_to_trash_spec(&mock_fs, target, home, 1000).unwrap();
        assert!(!mock_fs.files.borrow().contains_key(target));
        assert!(mock_fs.files.borrow().contains_key(&dest));

        #[cfg(not(target_os = "macos"))]
        {
            // Verify .trashinfo was written
            let info_file = dest.parent().unwrap().parent().unwrap().join("info").join("document.pdf.trashinfo");
            assert!(mock_fs.files.borrow().contains_key(&info_file));
            let content = mock_fs.files.borrow().get(&info_file).unwrap().clone();
            assert!(content.contains("[Trash Info]"));
            assert!(content.contains("Path="));
            assert!(content.contains("DeletionDate="));
        }
    }

    #[test]
    fn test_empty_trash_all() {
        let mock_fs = MockTrashFs::default();
        let home = Path::new("/home/user");
        let trash_files = home.join(".local/share/Trash/files");
        let trash_info = home.join(".local/share/Trash/info");

        mock_fs.files.borrow_mut().insert(trash_files.join("a.txt"), "a".into());
        mock_fs.files.borrow_mut().insert(trash_files.join("b.txt"), "b".into());
        mock_fs.files.borrow_mut().insert(trash_info.join("a.txt.trashinfo"), "info a".into());

        let res = empty_trash_all(&mock_fs, home, 1000).unwrap();
        assert_eq!(res.0, 3);
        assert!(mock_fs.files.borrow().is_empty());
    }
}
