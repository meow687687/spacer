use std::env;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use chrono::Local;

/// Determines log file directory path.
/// Linux/Unix: ~/.local/state/spacer/actions.log (or $XDG_STATE_HOME/spacer/actions.log)
/// macOS: ~/Library/Logs/spacer/actions.log or ~/.local/state/spacer/actions.log
pub fn action_log_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = env::var("HOME") {
            let p = PathBuf::from(home).join("Library").join("Logs").join("spacer");
            let _ = create_dir_all(&p);
            return p.join("actions.log");
        }
    }

    if let Ok(state_home) = env::var("XDG_STATE_HOME") {
        let p = PathBuf::from(state_home).join("spacer");
        let _ = create_dir_all(&p);
        return p.join("actions.log");
    }

    if let Ok(home) = env::var("HOME") {
        let p = PathBuf::from(home).join(".local").join("state").join("spacer");
        let _ = create_dir_all(&p);
        return p.join("actions.log");
    }

    PathBuf::from("spacer_actions.log")
}

/// Appends an action entry to the action log.
/// Format: <ISO-8601 Timestamp> | <Action> | <Path> | <Size bytes> (<Formatted>) | <Method>
pub fn log_cleanup_action(action: &str, path: &Path, size: u64, method: &str) {
    let log_path = action_log_path();
    log_cleanup_action_to(&log_path, action, path, size, method);
}

pub fn log_cleanup_action_to(log_path: &Path, action: &str, path: &Path, size: u64, method: &str) {
    if let Some(parent) = log_path.parent() {
        let _ = create_dir_all(parent);
    }

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_path) {
        let timestamp = Local::now().to_rfc3339();
        let _ = writeln!(
            file,
            "{} | ACTION: {} | PATH: {} | SIZE: {} | METHOD: {}",
            timestamp,
            action,
            path.display(),
            size,
            method
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_action_log_writes_entry() {
        let dir = tempdir().unwrap();
        let log_file = dir.path().join("test_actions.log");
        let test_path = dir.path().join("deleted_item.tmp");

        log_cleanup_action_to(&log_file, "DELETE", &test_path, 1024, "trash");

        let content = std::fs::read_to_string(&log_file).unwrap();
        assert!(content.contains("ACTION: DELETE"));
        assert!(content.contains("deleted_item.tmp"));
        assert!(content.contains("SIZE: 1024"));
        assert!(content.contains("METHOD: trash"));
    }
}
