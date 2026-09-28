use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct WasteItem {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub category_name: &'static str,
    pub description: String,
    pub selected: bool,
}

#[derive(Debug, Clone)]
pub struct WasteCategory {
    pub name: &'static str,
    pub icon: &'static str,
    pub total_size: u64,
    pub items: Vec<WasteItem>,
}

fn dir_size(path: &Path) -> u64 {
    WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

pub fn scan_quick_wins(root: &Path) -> Vec<WasteCategory> {
    let mut dev_items = Vec::new();
    let mut pkg_items = Vec::new();
    let mut ai_items = Vec::new();
    let mut log_items = Vec::new();

    // Check well-known user cache locations if scanning from home or root
    if let Ok(home_str) = std::env::var("HOME") {
        let home = Path::new(&home_str);
        if root == home || root.starts_with(home) {
            let pkg_candidates = [
                (home.join(".npm"), "Package Cache", "Node.js npm cache"),
                (home.join(".cache/yarn"), "Package Cache", "Yarn package cache"),
                (home.join(".cargo/registry/cache"), "Package Cache", "Rust Cargo crate download cache"),
                (home.join(".cargo/git/db"), "Package Cache", "Rust Cargo Git dependency cache"),
                (home.join(".rustup/toolchains"), "Package Cache", "Rustup toolchains (check unused toolchains)"),
                (home.join(".cache/pip"), "Package Cache", "Python pip wheel cache"),
                (home.join(".cache/thumbnails"), "System Cache", "Freedesktop image/video thumbnails"),
                (home.join(".cache/google-chrome/Default/Cache"), "Browser Cache", "Google Chrome web cache"),
                (home.join(".cache/mozilla/firefox"), "Browser Cache", "Firefox browser cache"),
            ];

            for (path, cat, desc) in pkg_candidates {
                if path.exists() {
                    let sz = dir_size(&path);
                    if sz > 1_000_000 {
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                        pkg_items.push(WasteItem {
                            path,
                            name,
                            size: sz,
                            category_name: cat,
                            description: desc.to_string(),
                            selected: true,
                        });
                    }
                }
            }

            let ai_candidates = [
                (home.join(".cache/huggingface"), "AI / ML Cache", "Hugging Face model downloads & checkpoints"),
                (home.join(".cache/torch"), "AI / ML Cache", "PyTorch hub & model cache"),
                (home.join(".lmstudio/models"), "AI / ML Cache", "LM Studio downloaded model weights"),
                (home.join(".ollama/models"), "AI / ML Cache", "Ollama local LLM blob store"),
            ];

            for (path, cat, desc) in ai_candidates {
                if path.exists() {
                    let sz = dir_size(&path);
                    if sz > 1_000_000 {
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                        ai_items.push(WasteItem {
                            path,
                            name,
                            size: sz,
                            category_name: cat,
                            description: desc.to_string(),
                            selected: true,
                        });
                    }
                }
            }
        }
    }

    // Traverse directory tree (up to depth 4) to find dev artifacts & logs
    let walker = WalkDir::new(root)
        .max_depth(4)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_str().unwrap_or("");
            !(name == "target" || name == "node_modules" || name == ".git" || name == ".cache")
        });

    for entry in walker.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name().to_str().unwrap_or("");
        let is_dir = entry.file_type().is_dir();

        if is_dir {
            match name {
                "target" | "node_modules" | ".next" | "build" | "dist" | ".gradle" | ".tox"
                | ".pytest_cache" | "__pycache__" => {
                    let sz = dir_size(path);
                    if sz > 5_000_000 {
                        dev_items.push(WasteItem {
                            path: path.to_path_buf(),
                            name: format!("{}/{}", path.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()).unwrap_or(""), name),
                            size: sz,
                            category_name: "Dev Build Artifact",
                            description: format!("Rebuildable output directory ({})", name),
                            selected: true,
                        });
                    }
                }
                _ => {}
            }
        } else {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            if ext == "log" || ext == "dmp" || ext == "tmp" || name.starts_with("core.") {
                if let Ok(meta) = fs::metadata(path) {
                    let len = meta.len();
                    if len > 500_000 {
                        log_items.push(WasteItem {
                            path: path.to_path_buf(),
                            name: name.to_string(),
                            size: len,
                            category_name: "Logs & Temp",
                            description: "Log or temporary diagnostic file".to_string(),
                            selected: true,
                        });
                    }
                }
            }
        }
    }

    dev_items.sort_by(|a, b| b.size.cmp(&a.size));
    pkg_items.sort_by(|a, b| b.size.cmp(&a.size));
    ai_items.sort_by(|a, b| b.size.cmp(&a.size));
    log_items.sort_by(|a, b| b.size.cmp(&a.size));

    let mut categories = Vec::new();

    if !dev_items.is_empty() {
        let total = dev_items.iter().map(|i| i.size).sum();
        categories.push(WasteCategory {
            name: "Developer Build Artifacts",
            icon: "🔨",
            total_size: total,
            items: dev_items,
        });
    }

    if !pkg_items.is_empty() {
        let total = pkg_items.iter().map(|i| i.size).sum();
        categories.push(WasteCategory {
            name: "Package & Browser Caches",
            icon: "📦",
            total_size: total,
            items: pkg_items,
        });
    }

    if !ai_items.is_empty() {
        let total = ai_items.iter().map(|i| i.size).sum();
        categories.push(WasteCategory {
            name: "AI & Model Checkpoint Caches",
            icon: "🤖",
            total_size: total,
            items: ai_items,
        });
    }

    if !log_items.is_empty() {
        let total = log_items.iter().map(|i| i.size).sum();
        categories.push(WasteCategory {
            name: "Logs & Temporary Dumps",
            icon: "📝",
            total_size: total,
            items: log_items,
        });
    }

    categories
}
