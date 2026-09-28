use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};
use crate::analysis::duplicate::DuplicateReport;
use crate::fs::model::FileItem;

pub fn render_duplicate_modal(
    frame: &mut Frame,
    report: &DuplicateReport,
    selected_group_idx: usize,
    selected_path_idx: usize,
    is_scanning: bool,
    status_msg: Option<&str>,
) {
    let area = centered_rect(80, 75, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(" 👥 Duplicate File Finder & Hardlink Deduplicator ")
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Min(8),     // Body split
            Constraint::Length(3),  // Actions
        ])
        .split(inner);

    // 1. Header
    let header_lines = if is_scanning {
        vec![
            Line::from(Span::styled("⟳ Scanning for duplicate files (size & SHA-256 matching)...", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        ]
    } else if let Some(msg) = status_msg {
        vec![
            Line::from(Span::styled(msg, Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD))),
        ]
    } else {
        vec![
            Line::from(vec![
                Span::styled("Total Duplicates Found: ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{} files ", report.total_duplicate_files), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("│ Wasted Space: ", Style::default().fg(Color::DarkGray)),
                Span::styled(FileItem::format_size(report.total_wasted_bytes), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" (across {} duplicate clusters)", report.groups.len()), Style::default().fg(Color::DarkGray)),
            ]),
        ]
    };
    frame.render_widget(Paragraph::new(header_lines), chunks[0]);

    if report.groups.is_empty() && !is_scanning {
        frame.render_widget(Paragraph::new("✓ No duplicate files detected in this directory tree.").alignment(Alignment::Center), chunks[1]);
        let action_lines = vec![Line::from(Span::styled("Press <Esc> or <Enter> to close", Style::default().fg(Color::DarkGray)))];
        frame.render_widget(Paragraph::new(action_lines).alignment(Alignment::Center), chunks[2]);
        return;
    }

    // 2. Body split: Groups (40%) + Paths in Group (60%)
    let body_split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(chunks[1]);

    // Group list
    let group_items: Vec<ListItem> = report
        .groups
        .iter()
        .enumerate()
        .map(|(idx, g)| {
            let is_sel = idx == selected_group_idx;
            let line = Line::from(vec![
                Span::styled(format!("• {} files ", g.paths.len()), if is_sel { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) }),
                Span::styled(format!("({} each, ", FileItem::format_size(g.file_size)), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("wasting {})", FileItem::format_size(g.wasted_bytes())), Style::default().fg(Color::Red)),
            ]);

            let style = if is_sel {
                Style::default().bg(Color::Rgb(30, 45, 65))
            } else {
                Style::default()
            };
            ListItem::new(line).style(style)
        })
        .collect();

    let group_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(" Duplicate Clusters ({}) ", report.groups.len()));
    frame.render_widget(List::new(group_items).block(group_block), body_split[0]);

    // Paths in currently selected group
    let current_group = report.groups.get(selected_group_idx);
    let path_items: Vec<ListItem> = if let Some(g) = current_group {
        g.paths
            .iter()
            .enumerate()
            .map(|(idx, p)| {
                let is_sel = idx == selected_path_idx;
                let tag = if idx == 0 { " [Primary Copy]" } else { " [Duplicate]" };
                let tag_color = if idx == 0 { Color::LightGreen } else { Color::Yellow };

                let line = Line::from(vec![
                    Span::styled(format!("{}) ", idx + 1), Style::default().fg(Color::DarkGray)),
                    Span::styled(p.display().to_string(), if is_sel { Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) }),
                    Span::styled(tag, Style::default().fg(tag_color)),
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

    let path_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Identical Files ");
    frame.render_widget(List::new(path_items).block(path_block), body_split[1]);

    // 3. Actions
    let actions = vec![
        Line::from(vec![
            Span::styled(" [h] ", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
            Span::styled("Hardlink this cluster │ ", Style::default().fg(Color::White)),
            Span::styled("[H] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled("Hardlink ALL duplicates (Reclaim 100% space) │ ", Style::default().fg(Color::White)),
            Span::styled("[Esc] ", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
            Span::styled("Close", Style::default().fg(Color::DarkGray)),
        ]),
    ];
    frame.render_widget(Paragraph::new(actions).alignment(Alignment::Center), chunks[2]);
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
