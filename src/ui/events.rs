use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;
use crate::analysis::duplicate::replace_with_hardlinks;
use crate::fs::deleter::{execute_batch_delete, DeleteMode};
use crate::ui::app::{App, ModalState};

pub fn handle_events(app: &mut App) -> std::io::Result<()> {
    if event::poll(Duration::from_millis(50))? {
        if let Event::Key(key) = event::read()? {
            handle_key_event(app, key);
        }
    }
    Ok(())
}

fn handle_key_event(app: &mut App, key: KeyEvent) {
    // 1. If a modal is open, modal handles inputs
    match &mut app.active_modal {
        ModalState::ConfirmDelete { mode } => {
            let current_mode = *mode;
            match key.code {
                KeyCode::Enter => {
                    app.execute_deletion(current_mode);
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    app.active_modal = ModalState::None;
                }
                KeyCode::Tab | KeyCode::Char('t') => {
                    let new_mode = match current_mode {
                        DeleteMode::Trash => DeleteMode::Permanent,
                        DeleteMode::Permanent => DeleteMode::Trash,
                    };
                    app.active_modal = ModalState::ConfirmDelete { mode: new_mode };
                }
                _ => {}
            }
            return;
        }
        ModalState::QuickWins { categories, selected_cat_idx, selected_item_idx, in_items_pane } => {
            match key.code {
                KeyCode::Tab | KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l') => {
                    *in_items_pane = !*in_items_pane;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if *in_items_pane {
                        if *selected_item_idx > 0 {
                            *selected_item_idx -= 1;
                        }
                    } else {
                        if *selected_cat_idx > 0 {
                            *selected_cat_idx -= 1;
                            *selected_item_idx = 0;
                        }
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *in_items_pane {
                        if let Some(cat) = categories.get(*selected_cat_idx) {
                            if *selected_item_idx + 1 < cat.items.len() {
                                *selected_item_idx += 1;
                            }
                        }
                    } else {
                        if *selected_cat_idx + 1 < categories.len() {
                            *selected_cat_idx += 1;
                            *selected_item_idx = 0;
                        }
                    }
                }
                KeyCode::Char(' ') => {
                    if *in_items_pane {
                        if let Some(cat) = categories.get_mut(*selected_cat_idx) {
                            if let Some(item) = cat.items.get_mut(*selected_item_idx) {
                                item.selected = !item.selected;
                            }
                        }
                    } else {
                        if let Some(cat) = categories.get_mut(*selected_cat_idx) {
                            let all_sel = cat.items.iter().all(|i| i.selected);
                            for item in &mut cat.items {
                                item.selected = !all_sel;
                            }
                        }
                    }
                }
                KeyCode::Enter => {
                    let mut targets = Vec::new();
                    for cat in categories {
                        for item in &cat.items {
                            if item.selected {
                                targets.push((item.path.clone(), item.size));
                            }
                        }
                    }
                    if !targets.is_empty() {
                        let report = execute_batch_delete(&targets, DeleteMode::Trash);
                        app.active_modal = ModalState::DeletionResult(report);
                        app.start_scan(app.current_dir.clone());
                    } else {
                        app.active_modal = ModalState::None;
                    }
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    app.active_modal = ModalState::None;
                }
                _ => {}
            }
            return;
        }
        ModalState::Duplicates { report, selected_group_idx, selected_path_idx, is_scanning: _, status_msg } => {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if *selected_group_idx > 0 {
                        *selected_group_idx -= 1;
                        *selected_path_idx = 0;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *selected_group_idx + 1 < report.groups.len() {
                        *selected_group_idx += 1;
                        *selected_path_idx = 0;
                    }
                }
                KeyCode::Char('h') => {
                    if let Some(group) = report.groups.get(*selected_group_idx) {
                        match replace_with_hardlinks(group) {
                            Ok(count) => {
                                *status_msg = Some(format!("✓ Successfully hardlinked {} duplicate files in cluster #{}!", count, *selected_group_idx + 1));
                            }
                            Err(e) => {
                                *status_msg = Some(format!("⚠️ Hardlink error: {}", e));
                            }
                        }
                    }
                }
                KeyCode::Char('H') => {
                    let mut total = 0;
                    for group in &report.groups {
                        if let Ok(count) = replace_with_hardlinks(group) {
                            total += count;
                        }
                    }
                    *status_msg = Some(format!("✓ Successfully hardlinked ALL {} duplicate files!", total));
                }
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                    app.active_modal = ModalState::None;
                }
                _ => {}
            }
            return;
        }
        ModalState::UpdateModal { success, error, .. } => {
            if *success || error.is_some() {
                match key.code {
                    KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char(' ') => {
                        app.active_modal = ModalState::None;
                    }
                    _ => {}
                }
            } else {
                match key.code {
                    KeyCode::Enter => {
                        app.perform_in_app_update();
                    }
                    KeyCode::Esc | KeyCode::Char('q') => {
                        app.active_modal = ModalState::None;
                    }
                    _ => {}
                }
            }
            return;
        }
        ModalState::Help | ModalState::DeletionResult(_) => {
            match key.code {
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char(' ') => {
                    app.active_modal = ModalState::None;
                }
                _ => {}
            }
            return;
        }
        ModalState::None => {}
    }

    // 2. If filtering
    if app.is_filtering {
        match key.code {
            KeyCode::Char(c) => {
                app.filter_query.push(c);
                app.selected_index = 0;
            }
            KeyCode::Backspace => {
                app.filter_query.pop();
                app.selected_index = 0;
            }
            KeyCode::Enter => {
                app.is_filtering = false;
            }
            KeyCode::Esc => {
                app.filter_query.clear();
                app.is_filtering = false;
                app.selected_index = 0;
            }
            _ => {}
        }
        return;
    }

    // 3. Normal navigation & action mode
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }
        KeyCode::Char('q') => {
            app.should_quit = true;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.select_next();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.select_prev();
        }
        KeyCode::Char('g') | KeyCode::Home => {
            app.select_first();
        }
        KeyCode::Char('G') | KeyCode::End => {
            app.select_last();
        }
        KeyCode::PageDown => {
            app.page_down(15);
        }
        KeyCode::PageUp => {
            app.page_up(15);
        }
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
            app.enter_selected();
        }
        KeyCode::Backspace | KeyCode::Char('h') | KeyCode::Left => {
            app.navigate_parent();
        }
        KeyCode::Char(' ') => {
            app.toggle_stage_selected();
        }
        KeyCode::Char('a') => {
            app.stage_all_visible();
        }
        KeyCode::Char('c') => {
            app.clear_staged();
        }
        KeyCode::Char('d') => {
            app.trigger_delete(DeleteMode::Trash);
        }
        KeyCode::Char('D') => {
            app.trigger_delete(DeleteMode::Permanent);
        }
        KeyCode::Char('s') => {
            app.cycle_sort();
        }
        KeyCode::Char('t') => {
            app.toggle_view_mode();
        }
        KeyCode::Char('w') => {
            app.trigger_quick_wins();
        }
        KeyCode::Char('F') => {
            app.trigger_duplicate_scan();
        }
        KeyCode::Char('/') => {
            app.is_filtering = true;
        }
        KeyCode::Char('r') => {
            app.rescan();
        }
        KeyCode::Char('u') | KeyCode::Char('U') => {
            if app.available_update.is_some() {
                app.trigger_update_modal();
            }
        }
        KeyCode::Char('?') => {
            app.active_modal = ModalState::Help;
        }
        _ => {}
    }
}
