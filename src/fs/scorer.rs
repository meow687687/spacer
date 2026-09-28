use std::path::Path;
use chrono::{DateTime, Local};
use crate::fs::model::{Category, ScoreBreakdown};

pub fn calculate_score(
    path: &Path,
    is_dir: bool,
    size: u64,
    modified: Option<DateTime<Local>>,
    _accessed: Option<DateTime<Local>>,
    category: Category,
) -> ScoreBreakdown {
    // 1. Size Score (0.0 to 100.0)
    // Scale smoothly from 100 KB (0) to 10 GB (100)
    let size_score = if size < 100_000 {
        (size as f64 / 100_000.0) * 10.0
    } else {
        let min_log = (100_000f64).log10();
        let max_log = (10_000_000_000f64).log10(); // 10 GB
        let current_log = (size as f64).log10();

        let ratio = (current_log - min_log) / (max_log - min_log);
        (ratio * 90.0 + 10.0).clamp(0.0, 100.0)
    };

    // 2. Age / Inactivity Score (0.0 to 100.0)
    let age_score = if let Some(mod_time) = modified {
        let now = Local::now();
        let days = now.signed_duration_since(mod_time).num_days().max(0);
        if days >= 365 {
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
        }
    } else {
        30.0 // Default if timestamp unavailable
    };

    // 3. Category Score (0.0 to 100.0)
    let category_score = match category {
        Category::DevArtifact => 95.0,
        Category::LogsAndTemp => 90.0,
        Category::PackageCache => 85.0,
        Category::Archive => 70.0,
        Category::Executable => 65.0,
        Category::Media => 55.0,
        Category::Document => 25.0,
        Category::Other => 20.0,
    };

    // Weighted composite: Size (45%), Age (30%), Category (25%)
    let total_score = (0.45 * size_score) + (0.30 * age_score) + (0.25 * category_score);

    // Build recommendation rationale
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let recommendation = match category {
        Category::DevArtifact => {
            format!("Rebuildable build artifact/cache ({}). High priority for deletion.", file_name)
        }
        Category::LogsAndTemp => {
            "Temporary file or diagnostic log. Safe to reclaim space.".to_string()
        }
        Category::PackageCache => {
            "Package manager/application cache. Can be re-fetched if needed.".to_string()
        }
        Category::Archive => {
            if age_score > 60.0 {
                "Old compressed archive. Likely no longer actively needed.".to_string()
            } else {
                "Compressed archive file.".to_string()
            }
        }
        Category::Media => {
            if size > 500_000_000 {
                "Large media file occupying significant disk space.".to_string()
            } else {
                "Media file.".to_string()
            }
        }
        Category::Executable => {
            "Binary package or standalone executable installer.".to_string()
        }
        _ => {
            if total_score > 70.0 {
                "Untouched large item. Good candidate for review and removal.".to_string()
            } else if is_dir {
                "Directory item.".to_string()
            } else {
                "Standard file.".to_string()
            }
        }
    };

    ScoreBreakdown {
        size_score,
        age_score,
        category_score,
        total_score,
        recommendation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_scorer_dev_artifact() {
        let path = Path::new("target");
        let now = Local::now() - Duration::days(120);
        let score = calculate_score(path, true, 2_000_000_000, Some(now), None, Category::DevArtifact);

        assert!(score.size_score > 70.0);
        assert!(score.age_score > 70.0);
        assert_eq!(score.category_score, 95.0);
        assert!(score.total_score >= 75.0);
        assert!(score.recommendation.contains("Rebuildable build artifact"));
    }
}
