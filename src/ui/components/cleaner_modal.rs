use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};
use crate::analysis::cleaner::WasteCategory;
use crate::fs::model::FileItem;

pub fn render_cleaner_modal(
    frame: &mut Frame,
    categories: &[WasteCategory],
    selected_cat_idx: usize,
    selected_item_idx: usize,
) {
    let area = centered_rect(75, 75, frame.area());
    frame.render_widget(Clear, area);

    let total_reclaimable: u64 = categories
        .iter()
        .flat_map(|c| c.items.iter())
        .filter(|i| i.selected)
        .map(|i| i.size)
        .sum();

    let total_count: usize = categories
        .iter()
        .flat_map(|c| c.items.iter())
        .filter(|i| i.selected)
        .count();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(" 🧹 Smart Quick Wins Cleanup Wizard ")
        .border_style(Style::default().fg(Color::LightGreen));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header summary
            Constraint::Min(8),     // Body split
            Constraint::Length(3),  // Action footer
        ])
        .split(inner);

    // 1. Header summary
    let summary_lines = vec![
        Line::from(vec![
            Span::styled("Selected Waste to Reclaim: ", Style::default().fg(Color::DarkGray)),
            Span::styled(FileItem::format_size(total_reclaimable), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" ({} targets selected)", total_count), Style::default().fg(Color::White)),
        ]),
    ];
    frame.render_widget(Paragraph::new(summary_lines), chunks[0]);

    // 2. Body split: Categories (35%) + Items in Category (65%)
    let body_split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(chunks[1]);

    // Categories list
    let cat_items: Vec<ListItem> = categories
        .iter()
        .enumerate()
        .map(|(idx, cat)| {
            let is_sel = idx == selected_cat_idx;
            let active_count = cat.items.iter().filter(|i| i.selected).count();
            let check_mark = if active_count == cat.items.len() && !cat.items.is_empty() {
                "[✓]"
            } else if active_count > 0 {
                "[-]"
            } else {
                "[ ]"
            };

            let line = Line::from(vec![
                Span::styled(format!("{} ", check_mark), Style::default().fg(if active_count > 0 { Color::LightGreen } else { Color::DarkGray })),
                Span::styled(format!("{} {} ", cat.icon, cat.name), if is_sel { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) }),
                Span::styled(format!("({})", FileItem::format_size(cat.total_size)), Style::default().fg(Color::DarkGray)),
            ]);

            let style = if is_sel {
                Style::default().bg(Color::Rgb(30, 45, 65))
            } else {
                Style::default()
            };
            ListItem::new(line).style(style)
        })
        .collect();

    let cat_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Categories ");
    frame.render_widget(List::new(cat_items).block(cat_block), body_split[0]);

    // Items list for currently selected category
    let current_cat = categories.get(selected_cat_idx);
    let item_list: Vec<ListItem> = if let Some(cat) = current_cat {
        cat.items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let is_sel = idx == selected_item_idx;
                let mark = if item.selected { "[✓]" } else { "[ ]" };
                let mark_color = if item.selected { Color::LightGreen } else { Color::DarkGray };

                let line = Line::from(vec![
                    Span::styled(format!("{} ", mark), Style::default().fg(mark_color)),
                    Span::styled(&item.name, if is_sel { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) }),
                    Span::styled(format!(" ({})", FileItem::format_size(item.size)), Style::default().fg(Color::Green)),
                ]);

                let style = if is_sel {
                    Style::default().bg(Color::Rgb(30, 45, 65))
                } else {
                    Style::default()
                };
                ListItem::new(line).style(style)
            })
            .collect()
    } else {
        Vec::new()
    };

    let item_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(" Items in Category ({}) ", current_cat.map(|c| c.items.len()).unwrap_or(0)));
    frame.render_widget(List::new(item_list).block(item_block), body_split[1]);

    // 3. Footer instructions
    let action_lines = vec![
        Line::from(vec![
            Span::styled(" [Tab] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled("Switch pane │ ", Style::default().fg(Color::DarkGray)),
            Span::styled("[Space] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled("Toggle select │ ", Style::default().fg(Color::DarkGray)),
            Span::styled("[Enter] ", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
            Span::styled("TRASH SELECTED JUNK │ ", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
            Span::styled("[Esc] ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("Close", Style::default().fg(Color::DarkGray)),
        ]),
    ];
    frame.render_widget(Paragraph::new(action_lines).alignment(Alignment::Center), chunks[2]);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
