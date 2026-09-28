use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Row, Table, Wrap,
    },
    Frame,
};
use crate::fs::deleter::DeleteMode;
use crate::fs::model::{Category, FileItem};
use crate::ui::app::{App, ModalState};
use crate::updater::version::UpdateInfo;

pub fn render_ui(frame: &mut Frame, app: &App) {
    let size = frame.area();

    // Main layout: Header (3), Body (Fill), Footer (1)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(1),
        ])
        .split(size);

    render_header(frame, app, chunks[0]);
    render_body(frame, app, chunks[1]);
    render_footer(frame, app, chunks[2]);

    // Modals
    match &app.active_modal {
        ModalState::ConfirmDelete { mode } => render_delete_confirm_modal(frame, app, *mode),
        ModalState::DeletionResult(report) => render_delete_result_modal(frame, report),
        ModalState::Help => render_help_modal(frame),
        ModalState::UpdateModal { info, is_updating, error, success } => {
            render_update_modal(frame, info, *is_updating, error.as_deref(), *success);
        }
        ModalState::None => {}
    }
}

fn format_number_commas(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();

    for (i, &ch) in chars.iter().enumerate() {
        result.push(ch);
        let rem = len - 1 - i;
        if rem > 0 && rem % 3 == 0 {
            result.push(',');
        }
    }
    result
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let scan_status_span = if app.is_scanning {
        Span::styled(
            format!(" ⟳ {} ", app.scan_progress_text),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(" ✓ Ready ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
    };

    let total_size_str = FileItem::format_size(app.total_scanned_bytes);
    let items_count = app.items.len();

    let path_str = app.current_dir.display().to_string();

    let mut line2_spans = vec![
        Span::styled("Sort: ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("[{}] ", app.sort_mode.short_label()), Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        Span::styled("│ Total: ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{} ({} items) ", total_size_str, format_number_commas(items_count)), Style::default().fg(Color::LightCyan)),
        Span::styled("│ Status: ", Style::default().fg(Color::DarkGray)),
        scan_status_span,
    ];

    if let Some(ref update) = app.available_update {
        line2_spans.push(Span::styled(
            format!("│ ✨ Update: {} [u] ", update.latest_version),
            Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
        ));
    }

    if !app.filter_query.is_empty() {
        line2_spans.push(Span::styled(format!("│ Filter: '{}' ", app.filter_query), Style::default().fg(Color::Yellow)));
    }

    let header_text = vec![
        Line::from(vec![
            Span::styled("⚡ SPACER ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled("│ Path: ", Style::default().fg(Color::DarkGray)),
            Span::styled(path_str, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(line2_spans),
    ];

    let header_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan));

    let header_widget = Paragraph::new(header_text).block(header_block);
    frame.render_widget(header_widget, area);
}

fn render_body(frame: &mut Frame, app: &App, area: Rect) {
    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(area);

    render_explorer(frame, app, body_chunks[0]);
    render_inspector(frame, app, body_chunks[1]);
}

fn render_explorer(frame: &mut Frame, app: &App, area: Rect) {
    let visible = app.visible_items();
    let max_size = visible.iter().map(|i| i.size).max().unwrap_or(1).max(1);

    let rows: Vec<Row> = visible
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let is_selected = idx == app.selected_index;
            let is_staged = app.staged_set.contains(&item.path);

            let stage_span = if is_staged {
                Span::styled("[✓]", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD))
            } else {
                Span::styled("[ ]", Style::default().fg(Color::DarkGray))
            };

            let icon = if item.is_dir { "📁 " } else { "📄 " };
            let name_style = if is_selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if item.is_dir {
                Style::default().fg(Color::LightBlue).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let name_span = Span::styled(format!("{}{}", icon, item.name), name_style);

            // Category badge
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
            let badge_span = Span::styled(
                format!("{:^9}", item.category.badge()),
                Style::default().fg(badge_color),
            );

            // Size percentage bar
            let ratio = (item.size as f64 / max_size as f64).clamp(0.0, 1.0);
            let bar_len = 8;
            let filled_len = ((ratio * bar_len as f64).round() as usize).min(bar_len);
            let empty_len = bar_len.saturating_sub(filled_len);
            let bar_str = format!("{}{}", "█".repeat(filled_len), "░".repeat(empty_len));
            let bar_span = Span::styled(bar_str, Style::default().fg(Color::Cyan));

            let size_span = Span::styled(
                format!("{:>9}", item.formatted_size()),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            );

            // Waste score badge
            let score_val = item.score.total_score.round() as u64;
            let score_color = if score_val >= 80 {
                Color::Red
            } else if score_val >= 50 {
                Color::Yellow
            } else {
                Color::Green
            };
            let score_span = Span::styled(
                format!("{:>3}", score_val),
                Style::default().fg(score_color).add_modifier(Modifier::BOLD),
            );

            let row_style = if is_selected {
                Style::default().bg(Color::Rgb(30, 45, 65))
            } else {
                Style::default()
            };

            Row::new(vec![
                Line::from(stage_span),
                Line::from(name_span),
                Line::from(badge_span),
                Line::from(bar_span),
                Line::from(size_span),
                Line::from(score_span),
            ]).style(row_style)
        })
        .collect();

    let widths = [
        Constraint::Length(4),         // Stage check
        Constraint::Min(16),           // Name
        Constraint::Length(11),        // Category Tag
        Constraint::Length(10),        // Graph Bar
        Constraint::Length(10),        // Size
        Constraint::Length(6),         // Waste Score
    ];

    let title = format!(" Explorer ({} items) ", format_number_commas(visible.len()));
    let table = Table::new(rows, widths)
        .header(
            Row::new(vec![
                Span::styled("Sel", Style::default().fg(Color::DarkGray)),
                Span::styled("Name", Style::default().fg(Color::DarkGray)),
                Span::styled("Tag", Style::default().fg(Color::DarkGray)),
                Span::styled("Graph", Style::default().fg(Color::DarkGray)),
                Span::styled("Size", Style::default().fg(Color::DarkGray)),
                Span::styled("Waste", Style::default().fg(Color::DarkGray)),
            ])
            .style(Style::default().add_modifier(Modifier::UNDERLINED)),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(title)
                .border_style(Style::default().fg(Color::LightBlue)),
        );

    frame.render_widget(table, area);
}

fn render_inspector(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10),  // Item Details
            Constraint::Min(10),     // Waste Scorecard
            Constraint::Length(6),   // Staged Batch Summary
        ])
        .split(area);

    let selected = app.selected_item();

    // 1. Details Box
    let details_lines = if let Some(item) = selected {
        vec![
            Line::from(vec![
                Span::styled("Path: ", Style::default().fg(Color::DarkGray)),
                Span::styled(item.path.display().to_string(), Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("Type: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    if item.is_dir { format!("Directory ({} contents)", format_number_commas(item.item_count)) } else { "File".to_string() },
                    Style::default().fg(Color::LightCyan),
                ),
            ]),
            Line::from(vec![
                Span::styled("Category: ", Style::default().fg(Color::DarkGray)),
                Span::styled(item.category.label(), Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" [{}]", item.category.badge()), Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(vec![
                Span::styled("Size: ", Style::default().fg(Color::DarkGray)),
                Span::styled(item.formatted_size(), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" ({} bytes)", format_number_commas(item.size as usize)), Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(vec![
                Span::styled("Last Modified: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    item.modified.map(|m| m.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_else(|| "Unknown".to_string()),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(format!(" ({})", item.relative_age_string()), Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(vec![
                Span::styled("Last Accessed: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    item.accessed.map(|m| m.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_else(|| "Unknown".to_string()),
                    Style::default().fg(Color::LightBlue),
                ),
            ]),
        ]
    } else {
        vec![Line::from("No item selected")]
    };

    let details_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Inspector ")
        .border_style(Style::default().fg(Color::Cyan));

    let details_p = Paragraph::new(details_lines)
        .block(details_block)
        .wrap(Wrap { trim: true });
    frame.render_widget(details_p, chunks[0]);

    // 2. Prioritization Scorecard Box
    if let Some(item) = selected {
        let score = &item.score;
        let score_val = score.total_score.round() as u64;

        let verdict_color = if score_val >= 80 {
            Color::Red
        } else if score_val >= 50 {
            Color::Yellow
        } else {
            Color::Green
        };

        let verdict_text = if score_val >= 80 {
            "🔥 HIGH WASTE PRIORITY"
        } else if score_val >= 50 {
            "⚡ MODERATE CANDIDATE"
        } else {
            "✓ LOW WASTE PRIORITY"
        };

        let scorecard_lines = vec![
            Line::from(vec![
                Span::styled("Waste Rating: ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{}/100 ", score_val), Style::default().fg(verdict_color).add_modifier(Modifier::BOLD)),
                Span::styled(verdict_text, Style::default().fg(verdict_color).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("• Size Impact (45%):     ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:.0}/100 ", score.size_score), Style::default().fg(Color::White)),
                Span::styled(make_meter(score.size_score), Style::default().fg(Color::LightBlue)),
            ]),
            Line::from(vec![
                Span::styled("• Inactivity/Age (30%):  ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:.0}/100 ", score.age_score), Style::default().fg(Color::White)),
                Span::styled(make_meter(score.age_score), Style::default().fg(Color::Yellow)),
            ]),
            Line::from(vec![
                Span::styled("• Category Type (25%):   ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:.0}/100 ", score.category_score), Style::default().fg(Color::White)),
                Span::styled(make_meter(score.category_score), Style::default().fg(Color::Magenta)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Recommendation: ", Style::default().fg(Color::DarkGray)),
            ]),
            Line::from(vec![
                Span::styled(format!("💡 {}", score.recommendation), Style::default().fg(Color::White).add_modifier(Modifier::ITALIC)),
            ]),
        ];

        let scorecard_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Deletion Priority Scorecard ")
            .border_style(Style::default().fg(verdict_color));

        let scorecard_p = Paragraph::new(scorecard_lines)
            .block(scorecard_block)
            .wrap(Wrap { trim: true });
        frame.render_widget(scorecard_p, chunks[1]);
    } else {
        let empty_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Deletion Priority Scorecard ");
        frame.render_widget(Paragraph::new("Select an item").block(empty_block), chunks[1]);
    }

    // 3. Staged Deletion Queue Box
    let (staged_count, staged_bytes, _) = app.staged_items_info();
    let staged_color = if staged_count > 0 { Color::LightGreen } else { Color::DarkGray };

    let staged_lines = vec![
        Line::from(vec![
            Span::styled("Marked Items: ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{} item(s)", format_number_commas(staged_count)), Style::default().fg(staged_color).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Reclaimable Space: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                FileItem::format_size(staged_bytes),
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Press ", Style::default().fg(Color::DarkGray)),
            Span::styled("[d]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" to Trash, ", Style::default().fg(Color::DarkGray)),
            Span::styled("[Shift+D]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::styled(" to Permanently Delete", Style::default().fg(Color::DarkGray)),
        ]),
    ];

    let staged_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Staged Deletion Queue ")
        .border_style(Style::default().fg(staged_color));

    let staged_p = Paragraph::new(staged_lines).block(staged_block);
    frame.render_widget(staged_p, chunks[2]);
}

fn make_meter(score: f64) -> String {
    let total_ticks = 10;
    let filled = ((score / 100.0) * total_ticks as f64).round() as usize;
    let filled = filled.min(total_ticks);
    let empty = total_ticks.saturating_sub(filled);
    format!("[{}{}]", "■".repeat(filled), " ".repeat(empty))
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let keymap = if app.is_filtering {
        vec![
            Span::styled("Type to filter: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(&app.filter_query, Style::default().fg(Color::White).add_modifier(Modifier::UNDERLINED)),
            Span::styled(" █ ", Style::default().fg(Color::Yellow)),
            Span::styled("│ <Enter> Confirm │ <Esc> Clear filter", Style::default().fg(Color::DarkGray)),
        ]
    } else {
        let mut spans = vec![
            Span::styled(" [Space] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled("Stage ", Style::default().fg(Color::White)),
            Span::styled(" [d] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("Trash ", Style::default().fg(Color::White)),
            Span::styled(" [D] ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::styled("Permanent ", Style::default().fg(Color::White)),
            Span::styled(" [s] ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::styled("Sort ", Style::default().fg(Color::White)),
            Span::styled(" [/] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled("Filter ", Style::default().fg(Color::White)),
            Span::styled(" [r] ", Style::default().fg(Color::LightBlue).add_modifier(Modifier::BOLD)),
            Span::styled("Rescan ", Style::default().fg(Color::White)),
        ];

        if app.available_update.is_some() {
            spans.push(Span::styled(" [u] ", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)));
            spans.push(Span::styled("Update ", Style::default().fg(Color::LightGreen)));
        }

        spans.push(Span::styled(" [?] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled("Help ", Style::default().fg(Color::White)));
        spans.push(Span::styled(" [q] ", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled("Quit", Style::default().fg(Color::White)));

        spans
    };

    let p = Paragraph::new(Line::from(keymap)).alignment(Alignment::Left);
    frame.render_widget(p, area);
}

fn render_delete_confirm_modal(frame: &mut Frame, app: &App, mode: DeleteMode) {
    let area = centered_rect(65, 55, frame.area());
    frame.render_widget(Clear, area);

    let (count, bytes, items) = app.staged_items_info();
    let mode_color = match mode {
        DeleteMode::Trash => Color::Yellow,
        DeleteMode::Permanent => Color::Red,
    };

    let title = format!(" ⚠️ Confirm Deletion: {} ", mode.title());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(title)
        .border_style(Style::default().fg(mode_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(inner);

    let summary = vec![
        Line::from(vec![
            Span::styled("Target Mode: ", Style::default().fg(Color::DarkGray)),
            Span::styled(mode.title(), Style::default().fg(mode_color).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Total Items: ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{} ", format_number_commas(count)), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled("│ Space to Free: ", Style::default().fg(Color::DarkGray)),
            Span::styled(FileItem::format_size(bytes), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]),
    ];
    frame.render_widget(Paragraph::new(summary), chunks[0]);

    // Item preview list
    let list_items: Vec<ListItem> = items
        .iter()
        .take(10)
        .map(|(p, s)| {
            let line = Line::from(vec![
                Span::styled(" • ", Style::default().fg(mode_color)),
                Span::styled(p.display().to_string(), Style::default().fg(Color::White)),
                Span::styled(format!(" ({})", FileItem::format_size(*s)), Style::default().fg(Color::DarkGray)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(" Items to Remove ({}) ", format_number_commas(count)));

    frame.render_widget(List::new(list_items).block(list_block), chunks[1]);

    let action_prompt = vec![
        Line::from(vec![
            Span::styled("Press ", Style::default().fg(Color::DarkGray)),
            Span::styled("<Enter>", Style::default().fg(mode_color).add_modifier(Modifier::BOLD)),
            Span::styled(" to EXECUTE DELETION, or ", Style::default().fg(Color::DarkGray)),
            Span::styled("<Esc>", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            Span::styled(" to CANCEL", Style::default().fg(Color::DarkGray)),
        ]),
    ];
    frame.render_widget(Paragraph::new(action_prompt).alignment(Alignment::Center), chunks[2]);
}

fn render_delete_result_modal(frame: &mut Frame, report: &crate::fs::deleter::DeletionReport) {
    let area = centered_rect(60, 45, frame.area());
    frame.render_widget(Clear, area);

    let is_err = !report.errors.is_empty();
    let border_color = if is_err { Color::Yellow } else { Color::Green };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(" Batch Deletion Summary ")
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Successfully Deleted: ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{} items", format_number_commas(report.success_count)), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Space Reclaimed: ", Style::default().fg(Color::DarkGray)),
            Span::styled(FileItem::format_size(report.bytes_freed), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
    ];

    if is_err {
        lines.push(Line::from(Span::styled("Errors Encountered:", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))));
        for (path, err) in &report.errors {
            lines.push(Line::from(format!(" • {}: {}", path.display(), err)));
        }
    } else {
        lines.push(Line::from(Span::styled("✓ All items removed successfully.", Style::default().fg(Color::LightGreen))));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Press <Enter> or <Esc> to dismiss", Style::default().fg(Color::DarkGray))));

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn render_update_modal(
    frame: &mut Frame,
    info: &UpdateInfo,
    is_updating: bool,
    error: Option<&str>,
    success: bool,
) {
    let area = centered_rect(65, 55, frame.area());
    frame.render_widget(Clear, area);

    let border_color = if success {
        Color::Green
    } else if error.is_some() {
        Color::Red
    } else {
        Color::LightGreen
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(format!(" ✨ Spacer Update Available: {} ", info.latest_version))
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(inner);

    let header_lines = vec![
        Line::from(vec![
            Span::styled("Current Version: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&info.current_version, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("  ➜  Latest Version: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&info.latest_version, Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
        ]),
    ];
    frame.render_widget(Paragraph::new(header_lines), chunks[0]);

    // Body
    let body_text = if success {
        vec![
            Line::from(Span::styled("✓ Spacer was successfully updated in-place!", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from("Please quit and restart spacer to launch the new version."),
        ]
    } else if let Some(err) = error {
        vec![
            Line::from(Span::styled("⚠️ Update Failed:", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))),
            Line::from(err),
            Line::from(""),
            Line::from("You can manually update by running:"),
            Line::from("  curl -fsSL https://raw.githubusercontent.com/meow687687/spacer/main/install.sh | bash"),
        ]
    } else if is_updating {
        vec![
            Line::from(Span::styled("⟳ Downloading and replacing binary in-place...", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
            Line::from("Please wait..."),
        ]
    } else {
        let mut lines = vec![
            Line::from(Span::styled("Release Notes:", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
        ];
        for line in info.release_notes.lines().take(10) {
            lines.push(Line::from(format!("  {}", line)));
        }
        lines
    };

    let body_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Information ");
    frame.render_widget(Paragraph::new(body_text).block(body_block).wrap(Wrap { trim: true }), chunks[1]);

    // Action footer
    let footer_text = if success || error.is_some() {
        vec![
            Line::from(Span::styled("Press <Enter> or <Esc> to dismiss", Style::default().fg(Color::DarkGray))),
        ]
    } else if is_updating {
        vec![
            Line::from(Span::styled("Updating in progress...", Style::default().fg(Color::DarkGray))),
        ]
    } else {
        vec![
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(Color::DarkGray)),
                Span::styled("<Enter>", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
                Span::styled(" to UPDATE NOW in-place, or ", Style::default().fg(Color::DarkGray)),
                Span::styled("<Esc>", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled(" to Dismiss", Style::default().fg(Color::DarkGray)),
            ]),
        ]
    };
    frame.render_widget(Paragraph::new(footer_text).alignment(Alignment::Center), chunks[2]);
}

fn render_help_modal(frame: &mut Frame) {
    let area = centered_rect(70, 70, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .title(" 📖 Spacer Help & Keybindings ")
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = vec![
        Line::from(Span::styled("NAVIGATION", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from("  ↑ / k           : Move selection up"),
        Line::from("  ↓ / j           : Move selection down"),
        Line::from("  Enter / → / l   : Enter directory"),
        Line::from("  Backspace / ← / h: Go to parent directory"),
        Line::from("  g / Home        : Jump to first item"),
        Line::from("  G / End         : Jump to last item"),
        Line::from("  PageUp / PageDn : Scroll by page"),
        Line::from(""),
        Line::from(Span::styled("DELETION & STAGING", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from("  Space           : Toggle stage/unstage item for batch delete"),
        Line::from("  a               : Select/deselect all visible items"),
        Line::from("  c               : Clear all staged items"),
        Line::from("  d               : Open deletion confirmation (Move to System Trash)"),
        Line::from("  D (Shift+d)     : Open permanent deletion confirmation (Unrecoverable rm)"),
        Line::from(""),
        Line::from(Span::styled("SORTING, FILTERING & UPDATES", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from("  s               : Cycle sort order (Size ↓, Priority Score ↓, Age ↓, Name A-Z)"),
        Line::from("  /               : Filter items by name"),
        Line::from("  r               : Refresh / rescan current directory"),
        Line::from("  u               : View and install available update"),
        Line::from(""),
        Line::from(Span::styled("PRIORITIZATION SCORE (0 - 100)", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        Line::from("  Spacer computes a multi-factor score to identify waste:"),
        Line::from("  • Size (45%)        : Larger items get higher scores"),
        Line::from("  • Inactivity (30%)  : Files untouched for months/years score higher"),
        Line::from("  • Category (25%)    : High weight for build artifacts (target, node_modules),"),
        Line::from("                        caches (.cache, .npm, .bun), AI models (ollama, .lmstudio),"),
        Line::from("                        logs/temp (*.log, *.tmp), archives (*.tar.gz)"),
        Line::from(""),
        Line::from(Span::styled("Press <Esc> or <Enter> to close help", Style::default().fg(Color::DarkGray))),
    ];

    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), inner);
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
