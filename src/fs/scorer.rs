use std::path::{Path, PathBuf};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use crate::fs::config::{ScorerWeights, SpacerConfig};
use crate::fs::model::Category;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SafetyLevel {
    Safe,
    Review,
    Keep,
}

impl SafetyLevel {
    pub fn label(&self) -> &'static str {
        match self {
            SafetyLevel::Safe => "Safe to Clean",
            SafetyLevel::Review => "Review Needed",
            SafetyLevel::Keep => "Keep / Protected",
        }
    }

    pub fn badge(&self) -> &'static str {
        match self {
            SafetyLevel::Safe => "SAFE",
            SafetyLevel::Review => "REVIEW",
            SafetyLevel::Keep => "KEEP",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectType {
    Rust,
    Node,
    Python,
    Go,
    Gradle,
    Maven,
    Git,
}

#[derive(Debug, Clone)]
pub struct ProjectInfo {
    pub root: PathBuf,
    #[allow(dead_code)]
    pub project_type: ProjectType,
    pub display_name: &'static str,
    #[allow(dead_code)]
    pub marker_file: &'static str,
    pub rebuild_cmd: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WasteScore {
    /// Composite waste score (0.0 to 100.0)
    pub score: f64,
    /// Safety classification: Safe, Review, Keep
    pub safety: SafetyLevel,
    /// Plain language explanations for score and safety
    pub reasons: Vec<String>,
    /// Component breakdown scores
    pub size_score: f64,
    pub age_score: f64,
    pub category_score: f64,
    /// Project context if detected
    pub project_info: Option<String>,
}

impl WasteScore {
    #[allow(dead_code)]
    pub fn total_score(&self) -> f64 {
        self.score
    }

    #[allow(dead_code)]
    pub fn recommendation(&self) -> String {
        self.reasons.first().cloned().unwrap_or_else(|| "No recommendation".to_string())
    }
}

/// Detects project root and type by inspecting parent and ancestor directories for known markers
pub fn detect_project_root(path: &Path) -> Option<ProjectInfo> {
    let mut current = path.parent();

    // If path is a rebuildable folder (e.g. target, node_modules), check its immediate parent first
    let home_dir = std::env::var("HOME").ok().map(PathBuf::from);

    let mut depth = 0;
    while let Some(dir) = current {
        if depth >= 5 {
            break;
        }
        if dir == Path::new("/") {
            break;
        }
        if let Some(ref home) = home_dir
            && dir == home
        {
            // Don't treat entire home directory as a single project
            break;
        }

        // Check markers in priority order
        if dir.join("Cargo.toml").exists() {
            return Some(ProjectInfo {
                root: dir.to_path_buf(),
                project_type: ProjectType::Rust,
                display_name: "Rust",
                marker_file: "Cargo.toml",
                rebuild_cmd: "cargo build",
            });
        }
        if dir.join("package.json").exists() {
            let cmd = if dir.join("pnpm-lock.yaml").exists() {
                "pnpm install"
            } else if dir.join("yarn.lock").exists() {
                "yarn"
            } else {
                "npm install"
            };
            return Some(ProjectInfo {
                root: dir.to_path_buf(),
                project_type: ProjectType::Node,
                display_name: "Node.js",
                marker_file: "package.json",
                rebuild_cmd: cmd,
            });
        }
        if dir.join("pyproject.toml").exists() {
            return Some(ProjectInfo {
                root: dir.to_path_buf(),
                project_type: ProjectType::Python,
                display_name: "Python",
                marker_file: "pyproject.toml",
                rebuild_cmd: "pip install -e .",
            });
        }
        if dir.join("requirements.txt").exists() {
            return Some(ProjectInfo {
                root: dir.to_path_buf(),
                project_type: ProjectType::Python,
                display_name: "Python",
                marker_file: "requirements.txt",
                rebuild_cmd: "pip install -r requirements.txt",
            });
        }
        if dir.join("go.mod").exists() {
            return Some(ProjectInfo {
                root: dir.to_path_buf(),
                project_type: ProjectType::Go,
                display_name: "Go",
                marker_file: "go.mod",
                rebuild_cmd: "go build",
            });
        }
        if dir.join("build.gradle").exists() || dir.join("build.gradle.kts").exists() {
            let cmd = if dir.join("gradlew").exists() {
                "./gradlew build"
            } else {
                "gradle build"
            };
            return Some(ProjectInfo {
                root: dir.to_path_buf(),
                project_type: ProjectType::Gradle,
                display_name: "Gradle",
                marker_file: "build.gradle",
                rebuild_cmd: cmd,
            });
        }
        if dir.join("pom.xml").exists() {
            return Some(ProjectInfo {
                root: dir.to_path_buf(),
                project_type: ProjectType::Maven,
                display_name: "Maven",
                marker_file: "pom.xml",
                rebuild_cmd: "mvn package",
            });
        }
        if dir.join(".git").exists() {
            return Some(ProjectInfo {
                root: dir.to_path_buf(),
                project_type: ProjectType::Git,
                display_name: "Git Repository",
                marker_file: ".git",
                rebuild_cmd: "rebuild",
            });
        }

        current = dir.parent();
        depth += 1;
    }

    None
}

/// Reads the latest commit or ref activity timestamp from a .git directory without shelling out
fn get_last_git_activity(git_dir: &Path) -> Option<DateTime<Local>> {
    let actual_git_dir = if git_dir.is_file() {
        if let Ok(content) = std::fs::read_to_string(git_dir) {
            if let Some(path_str) = content.trim().strip_prefix("gitdir:") {
                let p = Path::new(path_str.trim());
                if p.is_relative() {
                    git_dir.parent()?.join(p)
                } else {
                    p.to_path_buf()
                }
            } else {
                git_dir.to_path_buf()
            }
        } else {
            git_dir.to_path_buf()
        }
    } else {
        git_dir.to_path_buf()
    };

    let mut latest_time: Option<DateTime<Local>> = None;

    // 1. Try reading .git/logs/HEAD for exact commit timestamp
    let reflog_head = actual_git_dir.join("logs").join("HEAD");
    if let Ok(content) = std::fs::read_to_string(&reflog_head)
        && let Some(last_line) = content.lines().last()
        && let Some(header) = last_line.split('\t').next()
    {
        let parts: Vec<&str> = header.split_whitespace().collect();
        if parts.len() >= 5
            && let Ok(secs) = parts[parts.len() - 2].parse::<i64>()
            && let Some(utc_dt) = DateTime::from_timestamp(secs, 0)
        {
            let local_dt: DateTime<Local> = DateTime::from(utc_dt);
            latest_time = Some(local_dt);
        }
    }

    // 2. Check mtimes of .git/index and .git/HEAD
    for name in &["index", "HEAD", "FETCH_HEAD"] {
        let f = actual_git_dir.join(name);
        if let Ok(meta) = std::fs::metadata(&f)
            && let Ok(mtime) = meta.modified() {
                let dt: DateTime<Local> = DateTime::from(mtime);
                latest_time = Some(match latest_time {
                    Some(prev) => std::cmp::max(prev, dt),
                    None => dt,
                });
            }
    }

    latest_time
}

/// Requirement 2: For rebuildable folders, compute staleness from the PROJECT's last activity
/// (newest mtime among source files/manifests or last git commit time), not folder's own mtime/atime.
pub fn get_project_last_activity(project_root: &Path) -> Option<DateTime<Local>> {
    let mut newest_time: Option<DateTime<Local>> = None;

    // Check git activity first if .git exists
    let git_path = project_root.join(".git");
    if git_path.exists()
        && let Some(git_time) = get_last_git_activity(&git_path)
    {
        newest_time = Some(git_time);
    }

    let Ok(entries) = std::fs::read_dir(project_root) else {
        return newest_time;
    };

    let skip_dirs = [
        "target", "node_modules", ".venv", "venv", "build", "dist",
        "__pycache__", ".gradle", ".git", ".cache", ".next", ".turbo",
        ".pytest_cache", ".tox"
    ];

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if skip_dirs.contains(&name.as_str()) {
            continue;
        }

        if let Ok(meta) = entry.metadata() {
            if meta.is_file() {
                if let Ok(m) = meta.modified() {
                    let dt: DateTime<Local> = DateTime::from(m);
                    newest_time = Some(match newest_time {
                        Some(prev) => std::cmp::max(prev, dt),
                        None => dt,
                    });
                }
            } else if meta.is_dir()
                && let Ok(sub_entries) = std::fs::read_dir(&path)
            {
                // Check immediate files in source directory (e.g. src/, lib/, cmd/)
                for sub in sub_entries.flatten() {
                    if let Ok(sub_meta) = sub.metadata()
                        && sub_meta.is_file()
                        && let Ok(m) = sub_meta.modified()
                    {
                        let dt: DateTime<Local> = DateTime::from(m);
                        newest_time = Some(match newest_time {
                            Some(prev) => std::cmp::max(prev, dt),
                            None => dt,
                        });
                    }
                }
            }
        }
    }

    newest_time
}

pub fn is_rebuildable_folder(name: &str) -> bool {
    matches!(
        name,
        "target"
            | "node_modules"
            | ".venv"
            | "venv"
            | "build"
            | "dist"
            | "__pycache__"
            | ".gradle"
            | ".next"
            | ".turbo"
            | ".tox"
            | ".pytest_cache"
    )
}

/// Calculates project-aware, explainable waste score using default configured weights
pub fn calculate_score(
    path: &Path,
    is_dir: bool,
    size: u64,
    modified: Option<DateTime<Local>>,
    _accessed: Option<DateTime<Local>>,
    category: Category,
) -> WasteScore {
    let config = SpacerConfig::load_default();
    calculate_score_with_config(path, is_dir, size, modified, category, &config.weights)
}

/// Calculates score with explicit weights
pub fn calculate_score_with_config(
    path: &Path,
    is_dir: bool,
    size: u64,
    modified: Option<DateTime<Local>>,
    category: Category,
    weights: &ScorerWeights,
) -> WasteScore {
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let is_rebuildable = is_dir && is_rebuildable_folder(file_name);

    let mut reasons = Vec::new();
    let mut safety = match category {
        Category::DevArtifact => SafetyLevel::Safe,
        Category::LogsAndTemp => SafetyLevel::Safe,
        Category::PackageCache => SafetyLevel::Safe,
        Category::ModelCache => SafetyLevel::Review,
        Category::Archive => SafetyLevel::Review,
        Category::Executable => SafetyLevel::Review,
        Category::Media => SafetyLevel::Review,
        Category::Document => SafetyLevel::Keep,
        Category::Other => if is_dir { SafetyLevel::Review } else { SafetyLevel::Keep },
    };

    // 1. Project detection and staleness calculation
    let project_info = if is_rebuildable {
        detect_project_root(path)
    } else {
        None
    };

    let effective_mod_time = if let Some(ref proj) = project_info {
        // Requirement 2: Compute staleness from PROJECT's last activity, not from folder's own mtime
        get_project_last_activity(&proj.root).or(modified)
    } else {
        modified
    };

    // 2. Size Score (0.0 to 100.0)
    let size_score = if size < 100_000 {
        (size as f64 / 100_000.0) * 10.0
    } else {
        let min_log = (100_000f64).log10();
        let max_log = (10_000_000_000f64).log10(); // 10 GB
        let current_log = (size as f64).log10();

        let ratio = (current_log - min_log) / (max_log - min_log);
        (ratio * 90.0 + 10.0).clamp(0.0, 100.0)
    };

    // 3. Age / Staleness Score (0.0 to 100.0)
    let (age_score, days_stale) = if let Some(mod_time) = effective_mod_time {
        let now = Local::now();
        let days = now.signed_duration_since(mod_time).num_days().max(0);
        let s = if days >= 365 {
            100.0
        } else if days >= 180 {
            85.0 + ((days - 180) as f64 / 185.0) * 15.0
        } else if days >= 90 {
            70.0 + ((days - 90) as f64 / 90.0) * 15.0
        } else if days >= 30 {
            50.0 + ((days - 30) as f64 / 60.0) * 20.0
        } else if days >= 7 {
            20.0 + ((days - 7) as f64 / 23.0) * 30.0
        } else {
            (days as f64 / 7.0) * 20.0
        };
        (s, Some(days))
    } else {
        (30.0, None)
    };

    // 4. Category Score
    let category_score = match category {
        Category::DevArtifact => 95.0,
        Category::LogsAndTemp => 90.0,
        Category::PackageCache => 85.0,
        Category::ModelCache => 80.0,
        Category::Archive => 70.0,
        Category::Executable => 65.0,
        Category::Media => 55.0,
        Category::Document => 25.0,
        Category::Other => 20.0,
    };

    // 5. Requirement 3: Regenerability check and Downgrades
    if is_rebuildable {
        if let Some(ref proj) = project_info {
            let stale_desc = if let Some(days) = days_stale {
                if days >= 365 {
                    format!("project untouched for {} year(s)", days / 365)
                } else if days >= 30 {
                    format!("project untouched for {} months", days / 30)
                } else if days >= 7 {
                    format!("project untouched for {} weeks", days / 7)
                } else if days >= 1 {
                    format!("project untouched for {} days", days)
                } else {
                    "project modified recently".to_string()
                }
            } else {
                "project untouched".to_string()
            };

            match file_name {
                "node_modules" => {
                    // node_modules is "safe" only if a lockfile exists
                    let has_lockfile = proj.root.join("package-lock.json").exists()
                        || proj.root.join("yarn.lock").exists()
                        || proj.root.join("pnpm-lock.yaml").exists()
                        || proj.root.join("bun.lockb").exists()
                        || proj.root.join("bun.lock").exists();

                    if has_lockfile {
                        safety = SafetyLevel::Safe;
                        reasons.push(format!(
                            "Node.js dependencies, {}, can be rebuilt with `{}`",
                            stale_desc, proj.rebuild_cmd
                        ));
                        reasons.push("Lockfile present: exact dependency versions are pinned.".to_string());
                    } else {
                        safety = SafetyLevel::Review;
                        reasons.push(format!(
                            "Node.js dependencies, {}, re-installable with `{}`",
                            stale_desc, proj.rebuild_cmd
                        ));
                        reasons.push("⚠️ Missing lockfile (package-lock.json/yarn.lock/pnpm-lock.yaml). Re-installing may resolve different dependency versions. Downgraded to Review.".to_string());
                    }
                }
                "target" => {
                    if proj.project_type == ProjectType::Maven {
                        if proj.root.join("pom.xml").exists() {
                            safety = SafetyLevel::Safe;
                            reasons.push(format!(
                                "Maven build output, {}, can be rebuilt with `{}`",
                                stale_desc, proj.rebuild_cmd
                            ));
                        } else {
                            safety = SafetyLevel::Review;
                            reasons.push("⚠️ Missing pom.xml in Maven project root. Downgraded to Review.".to_string());
                        }
                    } else {
                        // target/ is "safe" only if Cargo.lock or Cargo.toml exists
                        let has_cargo = proj.root.join("Cargo.lock").exists() || proj.root.join("Cargo.toml").exists();
                        if has_cargo {
                            safety = SafetyLevel::Safe;
                            reasons.push(format!(
                                "Rust build output, {}, can be rebuilt with `{}`",
                                stale_desc, proj.rebuild_cmd
                            ));
                            if proj.root.join("Cargo.lock").exists() {
                                reasons.push("Cargo.lock present: dependencies are fully locked and reproducible.".to_string());
                            }
                        } else {
                            safety = SafetyLevel::Review;
                            reasons.push(format!("Build directory ({}), {}", file_name, stale_desc));
                            reasons.push("⚠️ Missing Cargo.lock and Cargo.toml. Cannot guarantee automatic rebuildability. Downgraded to Review.".to_string());
                        }
                    }
                }
                ".venv" | "venv" => {
                    // .venv only if a requirements/pyproject file exists
                    let has_manifest = proj.root.join("requirements.txt").exists()
                        || proj.root.join("pyproject.toml").exists()
                        || proj.root.join("Pipfile.lock").exists()
                        || proj.root.join("poetry.lock").exists();

                    if has_manifest {
                        safety = SafetyLevel::Safe;
                        reasons.push(format!(
                            "Python virtual environment, {}, can be rebuilt with `{}`",
                            stale_desc, proj.rebuild_cmd
                        ));
                        reasons.push("Dependency manifest present: packages can be re-installed.".to_string());
                    } else {
                        safety = SafetyLevel::Review;
                        reasons.push(format!("Python virtual environment ({}), {}", file_name, stale_desc));
                        reasons.push("⚠️ Missing requirements.txt or pyproject.toml. Installed packages cannot be automatically reproduced. Downgraded to Review.".to_string());
                    }
                }
                "__pycache__" => {
                    safety = SafetyLevel::Safe;
                    reasons.push("Python bytecode cache, automatically regenerated on execution.".to_string());
                }
                ".gradle" | "build" => {
                    safety = SafetyLevel::Safe;
                    reasons.push(format!(
                        "{} build output, {}, can be rebuilt with `{}`",
                        proj.display_name, stale_desc, proj.rebuild_cmd
                    ));
                }
                _ => {
                    safety = SafetyLevel::Safe;
                    reasons.push(format!(
                        "Rebuildable build artifact ({}), {}, can be rebuilt with `{}`",
                        file_name, stale_desc, proj.rebuild_cmd
                    ));
                }
            }
        } else {
            // Rebuildable folder candidate with NO project root marker detected
            safety = SafetyLevel::Review;
            if file_name == ".venv" || file_name == "venv" {
                reasons.push("⚠️ Missing requirements.txt or pyproject.toml. Installed packages cannot be automatically reproduced. Downgraded to Review.".to_string());
            } else if file_name == "target" {
                reasons.push("⚠️ Missing Cargo.lock and Cargo.toml. Cannot guarantee automatic rebuildability. Downgraded to Review.".to_string());
            } else if file_name == "node_modules" {
                reasons.push("⚠️ Missing lockfile (package-lock.json/yarn.lock/pnpm-lock.yaml). Re-installing may resolve different dependency versions. Downgraded to Review.".to_string());
            } else {
                reasons.push(format!(
                    "⚠️ Rebuildable folder candidate ({}/) with no detected project root or build manifest. Downgraded to Review.",
                    file_name
                ));
            }
        }
    } else {
        // Non-rebuildable items: provide plain language rationale
        match category {
            Category::LogsAndTemp => {
                reasons.push("Temporary file or diagnostic log. Safe to reclaim space.".to_string());
            }
            Category::PackageCache => {
                reasons.push("Package manager download cache. Can be re-downloaded if needed.".to_string());
            }
            Category::ModelCache => {
                reasons.push("AI / Machine Learning model cache. Verify if weights are still needed before deleting.".to_string());
            }
            Category::Archive => {
                if age_score > 60.0 {
                    reasons.push("Old compressed archive. Likely no longer actively needed.".to_string());
                } else {
                    reasons.push("Compressed archive file. Verify contents before deleting.".to_string());
                }
            }
            Category::Media => {
                reasons.push("Media file (audio, video, or image). Review before deleting.".to_string());
            }
            Category::Executable => {
                reasons.push("Binary package or standalone executable.".to_string());
            }
            Category::Document => {
                reasons.push("User document or workspace file. Retained to prevent data loss.".to_string());
            }
            Category::Other => {
                if is_dir {
                    reasons.push("Standard directory. Review contents before deleting.".to_string());
                } else {
                    reasons.push("Standard user file. Retained to prevent data loss.".to_string());
                }
            }
            Category::DevArtifact => {}
        }
    }

    // Size impact reason
    if size >= 1_000_000_000 {
        reasons.push(format!("Large storage footprint: {} occupies substantial disk space.", crate::fs::model::FileItem::format_size(size)));
    } else if size >= 100_000_000 {
        reasons.push(format!("Moderate storage footprint ({}).", crate::fs::model::FileItem::format_size(size)));
    }

    // 6. Weighted composite score
    let (w_size, w_age, w_cat) = weights.normalized();
    let mut total_score = (w_size * size_score) + (w_age * age_score) + (w_cat * category_score);

    // If item is classified as Keep, cap waste score to prevent accidental deletion recommendation
    if safety == SafetyLevel::Keep && total_score > 40.0 {
        total_score = 40.0;
    }

    let project_str = project_info.map(|p| format!("{} project at {}", p.display_name, p.root.display()));

    WasteScore {
        score: total_score,
        safety,
        reasons,
        size_score,
        age_score,
        category_score,
        project_info: project_str,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use tempfile::tempdir;

    #[test]
    fn test_rust_project_target_safe_with_cargo_lock() {
        let dir = tempdir().unwrap();
        let proj_root = dir.path().join("my_rust_app");
        std::fs::create_dir_all(&proj_root).unwrap();

        std::fs::write(proj_root.join("Cargo.toml"), "[package]\nname = \"test\"\n").unwrap();
        std::fs::write(proj_root.join("Cargo.lock"), "# lockfile\n").unwrap();

        let src_dir = proj_root.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("main.rs"), "fn main() {}\n").unwrap();

        let target_dir = proj_root.join("target");
        std::fs::create_dir_all(&target_dir).unwrap();

        let score = calculate_score(&target_dir, true, 2_000_000_000, None, None, Category::DevArtifact);
        assert_eq!(score.safety, SafetyLevel::Safe);
        assert!(score.reasons.iter().any(|r| r.contains("Rust build output") && r.contains("cargo build")));
        assert!(score.reasons.iter().any(|r| r.contains("Cargo.lock present")));
    }

    #[test]
    fn test_rust_project_target_downgrade_without_cargo_manifest() {
        let dir = tempdir().unwrap();
        let orphan_target = dir.path().join("target");
        std::fs::create_dir_all(&orphan_target).unwrap();

        let score = calculate_score(&orphan_target, true, 500_000_000, None, None, Category::DevArtifact);
        assert_eq!(score.safety, SafetyLevel::Review);
        assert!(score.reasons.iter().any(|r| r.contains("Downgraded to Review") || r.contains("Missing Cargo.lock")));
    }

    #[test]
    fn test_node_project_node_modules_safe_with_lockfile() {
        let dir = tempdir().unwrap();
        let proj_root = dir.path().join("web_app");
        std::fs::create_dir_all(&proj_root).unwrap();

        std::fs::write(proj_root.join("package.json"), "{\"name\": \"app\"}\n").unwrap();
        std::fs::write(proj_root.join("package-lock.json"), "{\"lockfileVersion\": 3}\n").unwrap();

        let nm_dir = proj_root.join("node_modules");
        std::fs::create_dir_all(&nm_dir).unwrap();

        let score = calculate_score(&nm_dir, true, 400_000_000, None, None, Category::DevArtifact);
        assert_eq!(score.safety, SafetyLevel::Safe);
        assert!(score.reasons.iter().any(|r| r.contains("Node.js dependencies") && r.contains("npm install")));
        assert!(score.reasons.iter().any(|r| r.contains("Lockfile present")));
    }

    #[test]
    fn test_node_project_node_modules_downgrade_without_lockfile() {
        let dir = tempdir().unwrap();
        let proj_root = dir.path().join("web_app_no_lock");
        std::fs::create_dir_all(&proj_root).unwrap();

        std::fs::write(proj_root.join("package.json"), "{\"name\": \"app\"}\n").unwrap();

        let nm_dir = proj_root.join("node_modules");
        std::fs::create_dir_all(&nm_dir).unwrap();

        let score = calculate_score(&nm_dir, true, 400_000_000, None, None, Category::DevArtifact);
        assert_eq!(score.safety, SafetyLevel::Review);
        assert!(score.reasons.iter().any(|r| r.contains("Missing lockfile")));
    }

    #[test]
    fn test_python_project_venv_safe_with_requirements() {
        let dir = tempdir().unwrap();
        let proj_root = dir.path().join("py_service");
        std::fs::create_dir_all(&proj_root).unwrap();

        std::fs::write(proj_root.join("requirements.txt"), "flask==2.0.1\n").unwrap();

        let venv_dir = proj_root.join(".venv");
        std::fs::create_dir_all(&venv_dir).unwrap();

        let score = calculate_score(&venv_dir, true, 300_000_000, None, None, Category::DevArtifact);
        assert_eq!(score.safety, SafetyLevel::Safe);
        assert!(score.reasons.iter().any(|r| r.contains("Python virtual environment") && r.contains("pip install")));
    }

    #[test]
    fn test_python_project_venv_downgrade_without_manifest() {
        let dir = tempdir().unwrap();
        let proj_root = dir.path().join("orphan_py");
        std::fs::create_dir_all(&proj_root).unwrap();

        let venv_dir = proj_root.join(".venv");
        std::fs::create_dir_all(&venv_dir).unwrap();

        let score = calculate_score(&venv_dir, true, 300_000_000, None, None, Category::DevArtifact);
        assert_eq!(score.safety, SafetyLevel::Review);
        assert!(score.reasons.iter().any(|r| r.contains("Missing requirements.txt or pyproject.toml")));
    }

    #[test]
    fn test_go_project_detection() {
        let dir = tempdir().unwrap();
        let proj_root = dir.path().join("go_app");
        std::fs::create_dir_all(&proj_root).unwrap();

        std::fs::write(proj_root.join("go.mod"), "module example/app\n").unwrap();
        let build_dir = proj_root.join("build");
        std::fs::create_dir_all(&build_dir).unwrap();

        let score = calculate_score(&build_dir, true, 50_000_000, None, None, Category::DevArtifact);
        assert_eq!(score.safety, SafetyLevel::Safe);
        assert!(score.reasons.iter().any(|r| r.contains("Go build output") && r.contains("go build")));
    }

    #[test]
    fn test_gradle_project_detection() {
        let dir = tempdir().unwrap();
        let proj_root = dir.path().join("java_app");
        std::fs::create_dir_all(&proj_root).unwrap();

        std::fs::write(proj_root.join("build.gradle"), "plugins { id 'java' }\n").unwrap();
        let gradle_dir = proj_root.join(".gradle");
        std::fs::create_dir_all(&gradle_dir).unwrap();

        let score = calculate_score(&gradle_dir, true, 100_000_000, None, None, Category::DevArtifact);
        assert_eq!(score.safety, SafetyLevel::Safe);
        assert!(score.reasons.iter().any(|r| r.contains("Gradle build output")));
    }

    #[test]
    fn test_maven_project_detection() {
        let dir = tempdir().unwrap();
        let proj_root = dir.path().join("maven_app");
        std::fs::create_dir_all(&proj_root).unwrap();

        std::fs::write(proj_root.join("pom.xml"), "<project></project>\n").unwrap();
        let target_dir = proj_root.join("target");
        std::fs::create_dir_all(&target_dir).unwrap();

        let score = calculate_score(&target_dir, true, 150_000_000, None, None, Category::DevArtifact);
        assert_eq!(score.safety, SafetyLevel::Safe);
        assert!(score.reasons.iter().any(|r| r.contains("Maven build output") && r.contains("mvn package")));
    }

    #[test]
    fn test_git_staleness_calculation() {
        let dir = tempdir().unwrap();
        let proj_root = dir.path().join("git_repo");
        std::fs::create_dir_all(&proj_root).unwrap();

        // Create mock .git reflog with a timestamp 240 days ago
        let git_dir = proj_root.join(".git").join("logs");
        std::fs::create_dir_all(&git_dir).unwrap();

        let old_time = Local::now() - Duration::days(240);
        let old_secs = old_time.timestamp();
        let reflog_line = format!(
            "0000000000000000000000000000000000000000 1111111111111111111111111111111111111111 User <u@e.com> {} +0000\tcommit: old commit\n",
            old_secs
        );
        std::fs::write(git_dir.join("HEAD"), reflog_line).unwrap();

        let target_dir = proj_root.join("target");
        std::fs::create_dir_all(&target_dir).unwrap();

        // Pass recent folder mtime (today)
        let now = Local::now();
        let score = calculate_score(&target_dir, true, 2_000_000_000, Some(now), None, Category::DevArtifact);

        // Staleness should reflect the git project age (240 days), not the folder mtime!
        assert!(score.age_score >= 85.0);
        assert!(score.reasons.iter().any(|r| r.contains("project untouched for 8 months") || r.contains("untouched for 7 months")));
    }

    #[test]
    fn test_configurable_weights() {
        let path = Path::new("target");
        let now = Local::now();

        // High size weight, low age/category
        let weights = ScorerWeights {
            size: 0.90,
            age: 0.05,
            category: 0.05,
        };

        let score = calculate_score_with_config(path, true, 10_000_000_000, Some(now), Category::DevArtifact, &weights);
        // Size score is 100, with 90% weight score should be >= 85
        assert!(score.score >= 85.0);
    }
}
