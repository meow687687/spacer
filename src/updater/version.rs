use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::Result;
use semver::Version;
use serde::{Deserialize, Serialize};

const GITHUB_REPO: &str = "meow687687/spacer";
const CACHE_TTL_SECONDS: u64 = 86400; // 24 hours

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub release_notes: String,
    pub tag_name: String,
    pub download_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheData {
    last_check_timestamp: u64,
    latest_version: String,
    release_notes: String,
    download_url: Option<String>,
}

fn cache_file_path() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HOME") {
        let path = PathBuf::from(home).join(".cache").join("spacer").join("update_cache.json");
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        Some(path)
    } else {
        None
    }
}

pub fn get_current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn current_target_triple() -> &'static str {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return "x86_64-unknown-linux-gnu";
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return "aarch64-unknown-linux-gnu";
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return "x86_64-apple-darwin";
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return "aarch64-apple-darwin";
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return "x86_64-pc-windows-msvc";

    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "windows", target_arch = "x86_64")
    )))]
    return "unknown";
}

pub fn check_for_updates(force: bool) -> Result<Option<UpdateInfo>> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let cache_path = cache_file_path();
    let current_ver_str = get_current_version();
    let current_ver = Version::parse(current_ver_str).unwrap_or(Version::new(0, 1, 0));

    // 1. Check local cache if not forced
    if !force {
        if let Some(ref path) = cache_path {
            if let Ok(content) = fs::read_to_string(path) {
                if let Ok(cache) = serde_json::from_str::<CacheData>(&content) {
                    if now.saturating_sub(cache.last_check_timestamp) < CACHE_TTL_SECONDS {
                        let clean_ver = cache.latest_version.trim_start_matches('v');
                        if let Ok(remote_ver) = Version::parse(clean_ver) {
                            if remote_ver > current_ver {
                                return Ok(Some(UpdateInfo {
                                    current_version: format!("v{}", current_ver_str),
                                    latest_version: cache.latest_version.clone(),
                                    release_notes: cache.release_notes,
                                    tag_name: cache.latest_version,
                                    download_url: cache.download_url,
                                }));
                            } else {
                                return Ok(None);
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Fetch from GitHub API
    let url = format!("https://api.github.com/repos/{}/releases/latest", GITHUB_REPO);
    let resp: serde_json::Value = match ureq::get(&url)
        .set("User-Agent", "spacer-updater")
        .set("Accept", "application/vnd.github.v3+json")
        .timeout(std::time::Duration::from_secs(5))
        .call()
    {
        Ok(r) => r.into_json()?,
        Err(_) => return Ok(None),
    };

    let tag_name = resp["tag_name"].as_str().unwrap_or("").to_string();
    let release_notes = resp["body"].as_str().unwrap_or("No release notes provided.").to_string();
    let clean_tag = tag_name.trim_start_matches('v');

    let remote_ver = match Version::parse(clean_tag) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    // Find asset download URL for current target
    let target = current_target_triple();
    let expected_asset_name = format!("spacer-{}.tar.gz", target);
    let mut download_url = None;

    if let Some(assets) = resp["assets"].as_array() {
        for asset in assets {
            if let Some(name) = asset["name"].as_str() {
                if name == expected_asset_name || name == format!("spacer-{}", target) {
                    if let Some(browser_download_url) = asset["browser_download_url"].as_str() {
                        download_url = Some(browser_download_url.to_string());
                        break;
                    }
                }
            }
        }
    }

    // Update cache
    if let Some(ref path) = cache_path {
        let cache_data = CacheData {
            last_check_timestamp: now,
            latest_version: tag_name.clone(),
            release_notes: release_notes.clone(),
            download_url: download_url.clone(),
        };
        if let Ok(serialized) = serde_json::to_string_pretty(&cache_data) {
            let _ = fs::write(path, serialized);
        }
    }

    if remote_ver > current_ver {
        Ok(Some(UpdateInfo {
            current_version: format!("v{}", current_ver_str),
            latest_version: tag_name.clone(),
            release_notes,
            tag_name,
            download_url,
        }))
    } else {
        Ok(None)
    }
}
