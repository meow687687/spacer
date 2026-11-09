use std::path::Path;
#[cfg(target_os = "macos")]
use std::process::Command;
#[cfg(target_os = "macos")]
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ProcessLockInfo {
    pub pid: u32,
    pub process_name: String,
    pub locked_path: String,
}

/// Checks whether any running process has open file descriptors on or inside target_path.
pub fn check_open_processes(target_path: &Path) -> Vec<ProcessLockInfo> {
    #[cfg(target_os = "linux")]
    {
        check_open_processes_linux(target_path)
    }

    #[cfg(target_os = "macos")]
    {
        check_open_processes_macos(target_path)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = target_path;
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
fn check_open_processes_linux(target_path: &Path) -> Vec<ProcessLockInfo> {
    let mut locks = Vec::new();
    let canonical = target_path.canonicalize().unwrap_or_else(|_| target_path.to_path_buf());

    let proc_dir = Path::new("/proc");
    let Ok(entries) = std::fs::read_dir(proc_dir) else {
        return locks;
    };

    let my_pid = std::process::id();

    for entry in entries.flatten() {
        let Ok(file_name) = entry.file_name().into_string() else {
            continue;
        };
        let Ok(pid) = file_name.parse::<u32>() else {
            continue;
        };

        if pid == my_pid {
            continue;
        }

        let pid_path = entry.path();
        let fd_dir = pid_path.join("fd");
        let Ok(fd_entries) = std::fs::read_dir(fd_dir) else {
            continue;
        };

        let mut matched = false;
        let mut matched_path = String::new();

        for fd_entry in fd_entries.flatten() {
            if let Ok(link_target) = std::fs::read_link(fd_entry.path())
                && (link_target.starts_with(&canonical) || link_target == canonical) {
                    matched = true;
                    matched_path = link_target.display().to_string();
                    break;
                }
        }

        if matched {
            let comm_path = pid_path.join("comm");
            let process_name = std::fs::read_to_string(comm_path)
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| format!("PID {}", pid));

            locks.push(ProcessLockInfo {
                pid,
                process_name,
                locked_path: matched_path,
            });
        }
    }

    locks
}

#[cfg(target_os = "macos")]
fn check_open_processes_macos(target_path: &Path) -> Vec<ProcessLockInfo> {
    let mut locks = Vec::new();
    let path_str = target_path.display().to_string();

    // Run lsof with a timeout mechanism: wait up to 3 seconds
    let mut child = match Command::new("lsof")
        .args(["+D", &path_str, "-F", "pcn"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn() {
            Ok(c) => c,
            Err(_) => return locks,
        };

    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(3);
    let mut finished = false;

    while start.elapsed() < timeout {
        if let Ok(Some(_status)) = child.try_wait() {
            finished = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    if !finished {
        let _ = child.kill();
        let _ = child.wait();
        return locks;
    }

    if let Ok(out) = child.wait_with_output() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut current_pid = 0u32;
        let mut current_name = String::new();

        for line in stdout.lines() {
            if let Some(rest) = line.strip_prefix('p') {
                current_pid = rest.parse().unwrap_or(0);
            } else if let Some(rest) = line.strip_prefix('c') {
                current_name = rest.to_string();
            } else if let Some(rest) = line.strip_prefix('n') {
                if current_pid != 0 && current_pid != std::process::id() {
                    locks.push(ProcessLockInfo {
                        pid: current_pid,
                        process_name: current_name.clone(),
                        locked_path: rest.to_string(),
                    });
                }
            }
        }
    }

    locks
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_check_open_processes_empty_dir() {
        let dir = tempdir().unwrap();
        let locks = check_open_processes(dir.path());
        assert!(locks.is_empty());
    }

    #[test]
    fn test_check_open_processes_with_open_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("active.txt");
        let file = File::create(&file_path).unwrap();

        // Own process is filtered out in check_open_processes, so locks should not contain self
        let locks = check_open_processes(dir.path());
        for lock in locks {
            assert_ne!(lock.pid, std::process::id());
        }
        drop(file);
    }
}
