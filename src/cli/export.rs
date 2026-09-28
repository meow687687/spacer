use std::fs::File;
use std::io::Write;
use std::path::Path;
use anyhow::Result;
use serde::Serialize;
use walkdir::WalkDir;
use crate::fs::model::{detect_category, FileItem};
use crate::fs::scorer::calculate_score;

#[derive(Serialize)]
struct ExportItem {
    path: String,
    name: String,
    is_dir: bool,
    size_bytes: u64,
    size_formatted: String,
    category: String,
    waste_score: u64,
}

pub fn run_export_command(
    root: &Path,
    json_path: Option<&Path>,
    csv_path: Option<&Path>,
) -> Result<()> {
    println!("⚡ Generating storage export for {}...", root.display());

    let mut items = Vec::new();

    for entry in WalkDir::new(root)
        .max_depth(4)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.path() == root {
            continue;
        }

        if let Ok(meta) = entry.metadata() {
            let is_dir = meta.is_dir();
            let size = if is_dir {
                WalkDir::new(entry.path())
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
            let category = detect_category(entry.path(), is_dir);
            let score = calculate_score(entry.path(), is_dir, size, modified, None, category);

            items.push(ExportItem {
                path: entry.path().display().to_string(),
                name: entry.file_name().to_string_lossy().to_string(),
                is_dir,
                size_bytes: size,
                size_formatted: FileItem::format_size(size),
                category: category.label().to_string(),
                waste_score: score.total_score.round() as u64,
            });
        }
    }

    if let Some(j_path) = json_path {
        let json_str = serde_json::to_string_pretty(&items)?;
        let mut f = File::create(j_path)?;
        f.write_all(json_str.as_bytes())?;
        println!("✓ Exported {} records to JSON: {}", items.len(), j_path.display());
    }

    if let Some(c_path) = csv_path {
        let mut f = File::create(c_path)?;
        writeln!(f, "path,name,is_dir,size_bytes,size_formatted,category,waste_score")?;
        for item in &items {
            writeln!(
                f,
                "\"{}\",\"{}\",{},{},\"{}\",\"{}\",{}",
                item.path.replace('\"', "\"\""),
                item.name.replace('\"', "\"\""),
                item.is_dir,
                item.size_bytes,
                item.size_formatted,
                item.category,
                item.waste_score
            )?;
        }
        println!("✓ Exported {} records to CSV: {}", items.len(), c_path.display());
    }

    Ok(())
}
