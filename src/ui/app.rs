use std::collections::HashSet;
use std::path::PathBuf;
use std::thread;
use crossbeam_channel::{unbounded, Receiver, Sender};
use crate::analysis::cleaner::{scan_quick_wins, WasteCategory};
use crate::analysis::duplicate::{scan_duplicates, DuplicateReport};
use crate::fs::deleter::{execute_batch_delete, DeleteMode, DeletionReport};
use crate::fs::model::FileItem;
use crate::fs::partition::{get_partition_for_path, PartitionInfo};
use crate::fs::scanner::{spawn_scanner, ScanMsg};
use crate::updater::self_update::execute_self_update;
use crate::updater::version::{check_for_updates, UpdateInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    SizeDesc,
    ScoreDesc,
    AgeDesc,
    NameAsc,
}

impl SortMode {
    pub fn short_label(&self) -> &'static str {
        match self {
            SortMode::SizeDesc => "SIZE",
            SortMode::ScoreDesc => "PRIORITY",
            SortMode::AgeDesc => "AGE",
            SortMode::NameAsc => "NAME",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Table,
    Treemap,
}

#[derive(Debug, Clone)]
pub enum ModalState {
    None,
    ConfirmDelete { mode: DeleteMode },
    Help,
    DeletionResult(DeletionReport),
    UpdateModal {
        info: UpdateInfo,
        is_updating: bool,
        error: Option<String>,
        success: bool,
    },
    QuickWins {
        categories: Vec<WasteCategory>,
        selected_cat_idx: usize,
        selected_item_idx: usize,
        in_items_pane: bool,
    },
    Duplicates {
        report: DuplicateReport,
        selected_group_idx: usize,
        selected_path_idx: usize,
        is_scanning: bool,
        status_msg: Option<String>,
    },
}

pub struct App {
    pub current_dir: PathBuf,
    pub items: Vec<FileItem>,
    pub selected_index: usize,
    pub staged_set: HashSet<PathBuf>,
    pub sort_mode: SortMode,
    pub view_mode: ViewMode,
    pub partition_info: Option<PartitionInfo>,
    pub is_scanning: bool,
    pub scan_progress_text: String,
    pub total_scanned_bytes: u64,
    pub total_scanned_items: usize,
    pub filter_query: String,
    pub is_filtering: bool,
    pub active_modal: ModalState,
    pub should_quit: bool,
    pub available_update: Option<UpdateInfo>,
    
    // Channels
    pub scan_tx: Sender<ScanMsg>,
    pub scan_rx: Receiver<ScanMsg>,
    pub update_rx: Receiver<Option<UpdateInfo>>,
}

impl App {
    pub fn new(initial_path: PathBuf) -> Self {
        let (tx, rx) = unbounded();
        let (update_tx, update_rx) = unbounded();
        let canonical_path = initial_path.canonicalize().unwrap_or(initial_path);
        let part_info = get_partition_for_path(&canonical_path);

        // Spawn non-blocking background update check (with 24h cache)
        thread::spawn(move || {
            let res = check_for_updates(false).unwrap_or(None);
            let _ = update_tx.send(res);
        });

        let mut app = Self {
            current_dir: canonical_path.clone(),
            items: Vec::new(),
            selected_index: 0,
            staged_set: HashSet::new(),
            sort_mode: SortMode::SizeDesc,
            view_mode: ViewMode::Table,
            partition_info: part_info,
            is_scanning: true,
            scan_progress_text: "Initializing scan...".to_string(),
            total_scanned_bytes: 0,
            total_scanned_items: 0,
            filter_query: String::new(),
            is_filtering: false,
            active_modal: ModalState::None,
            should_quit: false,
            available_update: None,
            scan_tx: tx.clone(),
            scan_rx: rx,
            update_rx,
        };

        app.start_scan(canonical_path);
        app
    }

    pub fn start_scan(&mut self, path: PathBuf) {
        self.is_scanning = true;
        self.scan_progress_text = "Scanning directory...".to_string();
        self.items.clear();
        self.selected_index = 0;
        self.total_scanned_bytes = 0;
        self.total_scanned_items = 0;
        self.partition_info = get_partition_for_path(&path);

        spawn_scanner(path, self.scan_tx.clone());
    }

    pub fn process_scan_messages(&mut self) {
        // Poll scanner updates
        while let Ok(msg) = self.scan_rx.try_recv() {
            match msg {
                ScanMsg::InitialChildren { root, items } => {
                    if root == self.current_dir {
                        self.items = items;
                        self.sort_items();
                    }
                }
                ScanMsg::ItemUpdated { path, size, item_count, score } => {
                    if let Some(item) = self.items.iter_mut().find(|i| i.path == path) {
                        item.size = size;
                        item.item_count = item_count;
                        item.score = score;
                    }
                    self.sort_items();
                }
                ScanMsg::Progress { scanned_count, total_bytes, current_name } => {
                    self.total_scanned_items = scanned_count;
                    self.total_scanned_bytes = total_bytes;
                    self.scan_progress_text = format!("Indexing {} ({} items)", current_name, scanned_count);
                }
                ScanMsg::ScanFinished { root, total_items, total_bytes: _ } => {
                    if root == self.current_dir {
                        self.is_scanning = false;
                        self.total_scanned_items = total_items;
                        self.total_scanned_bytes = self.items.iter().map(|i| i.size).sum();
                        self.scan_progress_text = "Scan complete".to_string();
                        self.sort_items();
                    }
                }
            }
        }

        // Poll update check result
        if let Ok(update_res) = self.update_rx.try_recv() {
            if let Some(info) = update_res {
                self.available_update = Some(info);
            }
        }
    }

    pub fn visible_items(&self) -> Vec<&FileItem> {
        if self.filter_query.is_empty() {
            self.items.iter().collect()
        } else {
            let q = self.filter_query.to_lowercase();
            self.items.iter().filter(|i| i.name.to_lowercase().contains(&q)).collect()
        }
    }

    pub fn selected_item(&self) -> Option<&FileItem> {
        let visible = self.visible_items();
        if visible.is_empty() {
            None
        } else {
            let idx = self.selected_index.min(visible.len().saturating_sub(1));
            Some(visible[idx])
        }
    }

    pub fn select_next(&mut self) {
        let count = self.visible_items().len();
        if count > 0 && self.selected_index + 1 < count {
            self.selected_index += 1;
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn select_first(&mut self) {
        self.selected_index = 0;
    }

    pub fn select_last(&mut self) {
        let count = self.visible_items().len();
        if count > 0 {
            self.selected_index = count - 1;
        }
    }

    pub fn page_down(&mut self, page_size: usize) {
        let count = self.visible_items().len();
        if count > 0 {
            self.selected_index = (self.selected_index + page_size).min(count - 1);
        }
    }

    pub fn page_up(&mut self, page_size: usize) {
        self.selected_index = self.selected_index.saturating_sub(page_size);
    }

    pub fn enter_selected(&mut self) {
        if let Some(item) = self.selected_item() {
            if item.is_dir {
                let target = item.path.clone();
                self.current_dir = target.clone();
                self.start_scan(target);
            }
        }
    }

    pub fn navigate_parent(&mut self) {
        if let Some(parent) = self.current_dir.parent() {
            let parent_path = parent.to_path_buf();
            self.current_dir = parent_path.clone();
            self.start_scan(parent_path);
        }
    }

    pub fn toggle_stage_selected(&mut self) {
        if let Some(item) = self.selected_item() {
            let path = item.path.clone();
            if self.staged_set.contains(&path) {
                self.staged_set.remove(&path);
            } else {
                self.staged_set.insert(path);
            }
        }
        self.select_next();
    }

    pub fn stage_all_visible(&mut self) {
        let visible_paths: Vec<PathBuf> = self.visible_items().into_iter().map(|i| i.path.clone()).collect();
        let all_staged = visible_paths.iter().all(|p| self.staged_set.contains(p));

        if all_staged {
            for p in &visible_paths {
                self.staged_set.remove(p);
            }
        } else {
            for p in visible_paths {
                self.staged_set.insert(p);
            }
        }
    }

    pub fn clear_staged(&mut self) {
        self.staged_set.clear();
    }

    pub fn staged_items_info(&self) -> (usize, u64, Vec<(PathBuf, u64)>) {
        let mut count = 0;
        let mut bytes = 0;
        let mut list = Vec::new();

        for item in &self.items {
            if self.staged_set.contains(&item.path) {
                count += 1;
                bytes += item.size;
                list.push((item.path.clone(), item.size));
            }
        }

        // If item was staged from subfolder or not in current list
        for staged_path in &self.staged_set {
            if !list.iter().any(|(p, _)| p == staged_path) {
                let size = if let Ok(meta) = staged_path.metadata() {
                    meta.len()
                } else {
                    0
                };
                count += 1;
                bytes += size;
                list.push((staged_path.clone(), size));
            }
        }

        (count, bytes, list)
    }

    pub fn cycle_sort(&mut self) {
        self.sort_mode = match self.sort_mode {
            SortMode::SizeDesc => SortMode::ScoreDesc,
            SortMode::ScoreDesc => SortMode::AgeDesc,
            SortMode::AgeDesc => SortMode::NameAsc,
            SortMode::NameAsc => SortMode::SizeDesc,
        };
        self.sort_items();
    }

    pub fn toggle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Table => ViewMode::Treemap,
            ViewMode::Treemap => ViewMode::Table,
        };
    }

    pub fn sort_items(&mut self) {
        match self.sort_mode {
            SortMode::SizeDesc => {
                self.items.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
            }
            SortMode::ScoreDesc => {
                self.items.sort_by(|a, b| {
                    b.score.total_score.partial_cmp(&a.score.total_score).unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| b.size.cmp(&a.size))
                });
            }
            SortMode::AgeDesc => {
                self.items.sort_by(|a, b| {
                    let a_time = a.modified.map(|t| t.timestamp()).unwrap_or(0);
                    let b_time = b.modified.map(|t| t.timestamp()).unwrap_or(0);
                    a_time.cmp(&b_time).then_with(|| b.size.cmp(&a.size))
                });
            }
            SortMode::NameAsc => {
                self.items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            }
        }
    }

    pub fn trigger_delete(&mut self, mode: DeleteMode) {
        let (count, _, _) = self.staged_items_info();
        if count == 0 {
            if let Some(item) = self.selected_item() {
                self.staged_set.insert(item.path.clone());
            } else {
                return;
            }
        }
        self.active_modal = ModalState::ConfirmDelete { mode };
    }

    pub fn execute_deletion(&mut self, mode: DeleteMode) {
        let (_, _, items_to_delete) = self.staged_items_info();
        let report = execute_batch_delete(&items_to_delete, mode);

        self.staged_set.clear();
        self.active_modal = ModalState::DeletionResult(report);
        self.start_scan(self.current_dir.clone());
    }

    pub fn trigger_quick_wins(&mut self) {
        let categories = scan_quick_wins(&self.current_dir);
        self.active_modal = ModalState::QuickWins {
            categories,
            selected_cat_idx: 0,
            selected_item_idx: 0,
            in_items_pane: false,
        };
    }

    pub fn trigger_duplicate_scan(&mut self) {
        let report = scan_duplicates(&self.current_dir);
        self.active_modal = ModalState::Duplicates {
            report,
            selected_group_idx: 0,
            selected_path_idx: 0,
            is_scanning: false,
            status_msg: None,
        };
    }

    pub fn trigger_update_modal(&mut self) {
        if let Some(ref info) = self.available_update {
            self.active_modal = ModalState::UpdateModal {
                info: info.clone(),
                is_updating: false,
                error: None,
                success: false,
            };
        }
    }

    pub fn perform_in_app_update(&mut self) {
        if let ModalState::UpdateModal { ref info, ref mut is_updating, ref mut error, ref mut success } = self.active_modal {
            *is_updating = true;
            match execute_self_update(info) {
                Ok(_) => {
                    *is_updating = false;
                    *success = true;
                }
                Err(e) => {
                    *is_updating = false;
                    *error = Some(e.to_string());
                }
            }
        }
    }

    pub fn rescan(&mut self) {
        self.start_scan(self.current_dir.clone());
    }
}
