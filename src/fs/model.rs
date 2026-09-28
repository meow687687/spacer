use std::path::{Path, PathBuf};
use chrono::{DateTime, Local};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    DevArtifact,
    PackageCache,
    LogsAndTemp,
    Archive,
    Media,
    Executable,
    Document,
    Other,
}

impl Category {
    pub fn label(&self) -> &'static str {
        match self {
            Category::DevArtifact => "Dev Artifact",
            Category::PackageCache => "Package Cache",
            Category::LogsAndTemp => "Logs / Temp",
            Category::Archive => "Archive",
            Category::Media => "Media File",
            Category::Executable => "Binary / Executable",
            Category::Document => "Document",
            Category::Other => "General",
        }
    }

    pub fn badge(&self) -> &'static str {
        match self {
            Category::DevArtifact => "DEV-BUILD",
            Category::PackageCache => "PKG-CACHE",
            Category::LogsAndTemp => "LOGS/TMP",
            Category::Archive => "ARCHIVE",
            Category::Media => "MEDIA",
            Category::Executable => "BIN/EXE",
            Category::Document => "DOCUMENT",
            Category::Other => "FILE",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScoreBreakdown {
    pub size_score: f64,
    pub age_score: f64,
    pub category_score: f64,
    pub total_score: f64,
    pub recommendation: String,
}

#[derive(Debug, Clone)]
pub struct FileItem {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub item_count: usize,
    pub modified: Option<DateTime<Local>>,
    pub accessed: Option<DateTime<Local>>,
    pub category: Category,
    pub score: ScoreBreakdown,
}

impl FileItem {
    pub fn format_size(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = 1024 * KB;
        const GB: u64 = 1024 * MB;
        const TB: u64 = 1024 * GB;

        if bytes >= TB {
            format!("{:.2} TB", bytes as f64 / TB as f64)
        } else if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }

    pub fn formatted_size(&self) -> String {
        Self::format_size(self.size)
    }

    pub fn relative_age_string(&self) -> String {
        if let Some(mod_time) = self.modified {
            let now = Local::now();
            let duration = now.signed_duration_since(mod_time);
            let days = duration.num_days();

            if days <= 0 {
                let hours = duration.num_hours();
                if hours <= 0 {
                    let mins = duration.num_minutes();
                    if mins <= 0 {
                        "Just now".to_string()
                    } else {
                        format!("{}m ago", mins)
                    }
                } else {
                    format!("{}h ago", hours)
                }
            } else if days < 30 {
                format!("{}d ago", days)
            } else if days < 365 {
                let months = days / 30;
                format!("{}mo ago", months)
            } else {
                let years = (days as f64 / 365.25).floor() as i64;
                format!("{}y ago", years)
            }
        } else {
            "Unknown".to_string()
        }
    }
}

pub fn detect_category(path: &Path, is_dir: bool) -> Category {
    let name = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    if is_dir {
        match name.as_str() {
            "target" | "node_modules" | ".next" | "build" | "dist" | ".gradle" | "venv"
            | ".venv" | "__pycache__" | ".tox" | ".parcel-cache" | ".turbo" | ".pytest_cache"
            | ".nuxt" | ".svelte-kit" | "out" | "cmake-build-debug" | "cmake-build-release" => {
                return Category::DevArtifact;
            }
            ".cache" | "cacheddata" | "crashpad" | "logs" | "npm-cache" | ".npm" | ".yarn"
            | ".cargo-cache" | ".rustup" | "tmp" | "temp" => {
                return Category::PackageCache;
            }
            _ => {
                if name.ends_with(".cache") || name.starts_with(".cache") {
                    return Category::PackageCache;
                }
            }
        }
    }

    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "log" | "tmp" | "temp" | "dmp" | "bak" | "swp" | "old" | "cache" => Category::LogsAndTemp,
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "tgz" | "zst" | "iso" | "img"
        | "dmg" | "qcow2" | "vmdk" => Category::Archive,
        "mp4" | "mkv" | "mov" | "avi" | "webm" | "flv" | "wmv" | "mp3" | "wav" | "flac"
        | "aac" | "ogg" | "png" | "jpg" | "jpeg" | "raw" | "svg" | "webp" => Category::Media,
        "deb" | "rpm" | "appimage" | "exe" | "msi" | "pkg" | "bin" | "so" | "dylib" | "dll"
        | "apk" => Category::Executable,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "epub" => Category::Document,
        _ => {
            if name.starts_with("core.") || name.ends_with(".log") {
                Category::LogsAndTemp
            } else {
                Category::Other
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_category_detection() {
        assert_eq!(detect_category(Path::new("target"), true), Category::DevArtifact);
        assert_eq!(detect_category(Path::new("node_modules"), true), Category::DevArtifact);
        assert_eq!(detect_category(Path::new(".cache"), true), Category::PackageCache);
        assert_eq!(detect_category(Path::new("video.mp4"), false), Category::Media);
        assert_eq!(detect_category(Path::new("archive.tar.gz"), false), Category::Archive);
        assert_eq!(detect_category(Path::new("app.log"), false), Category::LogsAndTemp);
        assert_eq!(detect_category(Path::new("installer.deb"), false), Category::Executable);
        assert_eq!(detect_category(Path::new("doc.pdf"), false), Category::Document);
    }

    #[test]
    fn test_format_size() {
        assert_eq!(FileItem::format_size(500), "500 B");
        assert_eq!(FileItem::format_size(1024 * 50), "50.00 KB");
        assert_eq!(FileItem::format_size(1024 * 1024 * 150), "150.00 MB");
        assert_eq!(FileItem::format_size(1024 * 1024 * 1024 * 3), "3.00 GB");
    }
}
