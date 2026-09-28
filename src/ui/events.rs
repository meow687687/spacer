use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;
use crate::fs::deleter::DeleteMode;
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
    match &app.active_modal {
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
