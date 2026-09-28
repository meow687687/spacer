use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};
use crate::fs::model::{Category, FileItem};

#[derive(Debug, Clone)]
pub struct TreemapRect {
    pub item_index: usize,
    pub rect: Rect,
}

pub fn render_treemap(
    frame: &mut Frame,
    items: &[&FileItem],
    selected_index: usize,
    area: Rect,
) -> Vec<TreemapRect> {
    if items.is_empty() || area.width < 4 || area.height < 4 {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Treemap Visualizer ");
        frame.render_widget(Paragraph::new("No items to visualize").block(block), area);
        return Vec::new();
    }

    let total_size: u64 = items.iter().map(|i| i.size).sum();
    if total_size == 0 {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Treemap Visualizer ");
        frame.render_widget(Paragraph::new("Directory is empty (0 bytes)").block(block), area);
        return Vec::new();
    }

    // Outer container block
    let container_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(" Treemap View ({} items) [Press 't' for Table] ", items.len()))
        .border_style(Style::default().fg(Color::LightBlue));

    let inner = container_block.inner(area);
    frame.render_widget(container_block, area);

    // Compute layout partitions
    let mut mapped_rects = Vec::new();
    layout_squarified(items, 0, items.len(), inner, total_size, &mut mapped_rects);

    // Render individual treemap rectangles
    for tr in &mapped_rects {
        if tr.item_index >= items.len() {
            continue;
        }
        let item = items[tr.item_index];
        let is_selected = tr.item_index == selected_index;

        let badge_color = match item.category {
            Category::DevArtifact => Color::Red,
            Category::ModelCache => Color::LightMagenta,
            Category::LogsAndTemp => Color::LightRed,
            Category::PackageCache => Color::Magenta,
            Category::Archive => Color::Yellow,
            Category::Media => Color::Blue,
            Category::Executable => Color::Green,
            Category::Document => Color::Cyan,
            Category::Other => Color::DarkGray,
        };

        let border_color = if is_selected {
            Color::Yellow
        } else {
            badge_color
        };

        let block_style = if is_selected {
            Style::default().bg(Color::Rgb(30, 45, 65)).fg(border_color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(border_color)
        };

        let icon = if item.is_dir { "📁 " } else { "📄 " };
        let box_block = Block::default()
            .borders(Borders::ALL)
            .border_type(if is_selected { BorderType::Double } else { BorderType::Plain })
            .border_style(Style::default().fg(border_color));

        let box_inner = box_block.inner(tr.rect);
        frame.render_widget(box_block, tr.rect);

        // Content inside block if large enough
        if tr.rect.width >= 8 && tr.rect.height >= 2 {
            let mut lines = Vec::new();
            let name_str = format!("{}{}", icon, item.name);
            lines.push(Line::from(Span::styled(name_str, block_style)));

            if tr.rect.height >= 3 {
                lines.push(Line::from(Span::styled(
                    item.formatted_size(),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                )));
            }

            let p = Paragraph::new(lines);
            frame.render_widget(p, box_inner);
        }
    }

    mapped_rects
}

fn layout_squarified(
    items: &[&FileItem],
    start: usize,
    end: usize,
    area: Rect,
    current_total: u64,
    out: &mut Vec<TreemapRect>,
) {
    if start >= end || area.width == 0 || area.height == 0 || current_total == 0 {
        return;
    }

    if end - start == 1 {
        out.push(TreemapRect {
            item_index: start,
            rect: area,
        });
        return;
    }

    // Split along the longer dimension
    let first_item_size = items[start].size;
    let ratio = (first_item_size as f64 / current_total as f64).clamp(0.05, 0.95);

    if area.width >= area.height * 2 {
        // Split horizontally (left / right)
        let split_w = ((area.width as f64 * ratio).round() as u16).max(1).min(area.width.saturating_sub(1));
        let left_rect = Rect::new(area.x, area.y, split_w, area.height);
        let right_rect = Rect::new(area.x + split_w, area.y, area.width.saturating_sub(split_w), area.height);

        out.push(TreemapRect {
            item_index: start,
            rect: left_rect,
        });

        let remaining_total = current_total.saturating_sub(first_item_size);
        layout_squarified(items, start + 1, end, right_rect, remaining_total, out);
    } else {
        // Split vertically (top / bottom)
        let split_h = ((area.height as f64 * ratio).round() as u16).max(1).min(area.height.saturating_sub(1));
        let top_rect = Rect::new(area.x, area.y, area.width, split_h);
        let bottom_rect = Rect::new(area.x, area.y + split_h, area.width, area.height.saturating_sub(split_h));

        out.push(TreemapRect {
            item_index: start,
            rect: top_rect,
        });

        let remaining_total = current_total.saturating_sub(first_item_size);
        layout_squarified(items, start + 1, end, bottom_rect, remaining_total, out);
    }
}
