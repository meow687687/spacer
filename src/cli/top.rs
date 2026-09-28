use std::path::Path;
use anyhow::Result;
use walkdir::WalkDir;
use crate::fs::model::{detect_category, FileItem};
use crate::fs::scorer::calculate_score;

pub fn run_top_command(root: &Path, count: usize) -> Result<()> {
    println!("⚡ Scanning top space consumers in {}...", root.display());

    let mut items = Vec::new();

    for entry in WalkDir::new(root)
        .max_depth(3)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path == root {
            continue;
        }

        if let Ok(meta) = entry.metadata() {
            let is_dir = meta.is_dir();
            let size = if is_dir {
                WalkDir::new(path)
                    .follow_links(false)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .filter_map(|e| e.metadata().ok())
                    .filter(|m| m.is_file())
                    .map(|m| m.len())
                    .sum()
            } else {
                meta.len()
            };

            let modified = meta.modified().ok().map(|t| chrono::DateTime::from(t));
            let category = detect_category(path, is_dir);
            let score = calculate_score(path, is_dir, size, modified, None, category);

            items.push(FileItem {
                path: path.to_path_buf(),
                name: entry.file_name().to_string_lossy().to_string(),
                is_dir,
                size,
                item_count: 1,
                modified,
                accessed: None,
                category,
                score,
            });
        }
    }

    items.sort_by(|a, b| b.size.cmp(&a.size));

    println!("\n{:<6} {:<12} {:<12} {:<6} {}", "RANK", "SIZE", "TAG", "WASTE", "PATH");
    println!("{}", "─".repeat(70));

    for (idx, item) in items.iter().take(count).enumerate() {
        let type_icon = if item.is_dir { "📁" } else { "📄" };
        println!(
            "{:<6} {:>10}   [{:<8}] {:>4}/100  {} {}",
            format!("#{}", idx + 1),
            item.formatted_size(),
            item.category.badge(),
            item.score.total_score.round() as u64,
            type_icon,
            item.path.display()
        );
    }

    println!("");
    Ok(())
}
