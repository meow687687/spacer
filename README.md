# ⚡ Spacer v0.2.0

> High-performance terminal storage manager with multi-factor waste prioritization, smart one-click cleanup, fast duplicate detection with hardlink deduplication, interactive squarified treemap visualizer, real-time partition monitor, and automatic self-updates, built in Rust.

---

## ⚡ Quick Install

### Via `curl` (Recommended)
```bash
curl -fsSL https://raw.githubusercontent.com/meow687687/spacer/main/install.sh | bash
```

### Via `install.sh`
```bash
git clone https://github.com/meow687687/spacer.git
cd spacer
./install.sh
```

### Via `cargo`
```bash
cargo install --git https://github.com/meow687687/spacer.git
```

---

## 🚀 Key Features

* **⚡ Multithreaded Streaming Scanner**: Instantly opens and lets you browse top-level directories in milliseconds while deeply calculating recursive directory sizes asynchronously in the background.
* **💾 Drive & Partition Monitor**: Live hardware partition status in the header showing filesystem type, capacity used/free, and visual usage meter.
* **🧹 Smart "Quick Wins" Cleanup Wizard (<kbd>w</kbd> / `spacer clean`)**:
  * One-click discovery of safe-to-clean developer artifacts (`target/`, `node_modules/`, `.next/`, `build/`, `venv/`), package caches (`~/.npm`, `~/.cargo/registry/cache`, `~/.cache`), AI model caches (`.cache/huggingface`, `.lmstudio/models`), and log dumps.
* **👥 3-Phase Fast Duplicate File Finder (<kbd>F</kbd> / `spacer dupes`)**:
  * Groups identical files via exact byte size $\rightarrow$ 4KB prefix hash $\rightarrow$ full SHA-256.
  * **🔗 Hardlink Deduplication**: Replace duplicate copies with hardlinks (`ln -f`) to reclaim **100% of wasted space** while keeping all files intact!
* **🗺️ Interactive Squarified Treemap View (<kbd>t</kbd>)**:
  * Seamlessly toggle between the Explorer Table List and a proportional geometric block map (like Diskonaut/Baobab).
  * Zoom in (<kbd>Enter</kbd>) and Zoom out (<kbd>Backspace</kbd>).
* **🎯 Multi-Factor Waste Prioritization (0–100 Score)**:
  * Size (45%) + Inactivity/Age (30%) + Category Heuristics (25%).
* **🛡️ Safe Staged Batch Deletion**:
  * Stage items with <kbd>Space</kbd> or <kbd>a</kbd>, preview reclaimable space, and move to System Trash (<kbd>d</kbd>) or Permanent Delete (<kbd>Shift+D</kbd>).
* **🔄 In-Place Self-Updates (`spacer update` / <kbd>u</kbd>)**:
  * Checks GitHub Releases with a 24-hour cache and performs zero-downtime binary replacement in-place.

---

## ⌨️ Keyboard Shortcuts

| Shortcut | Action |
|---|---|
| <kbd>↑</kbd> / <kbd>k</kbd> | Move cursor up |
| <kbd>↓</kbd> / <kbd>j</kbd> | Move cursor down |
| <kbd>Enter</kbd> / <kbd>→</kbd> / <kbd>l</kbd> | Enter directory / Zoom into Treemap block |
| <kbd>Backspace</kbd> / <kbd>←</kbd> / <kbd>h</kbd> | Navigate to parent / Zoom out Treemap |
| <kbd>t</kbd> | Toggle View Mode (Table List ↔ Squarified Treemap) |
| <kbd>w</kbd> | Open Smart "Quick Wins" Cleanup Wizard |
| <kbd>F</kbd> (Shift+f) | Open Duplicate File Finder & Hardlink Deduplicator |
| <kbd>Space</kbd> | Toggle stage/unstage for batch deletion |
| <kbd>a</kbd> | Stage/unstage all visible items |
| <kbd>c</kbd> | Clear all staged items |
| <kbd>d</kbd> | Open confirmation modal (Move to System Trash) |
| <kbd>Shift+D</kbd> | Open permanent deletion confirmation (Unrecoverable rm) |
| <kbd>s</kbd> | Cycle sort order (Size ↓, Priority Score ↓, Age ↓, Name A-Z) |
| <kbd>/</kbd> | Filter items by name |
| <kbd>r</kbd> | Rescan current directory |
| <kbd>u</kbd> | View and install available update |
| <kbd>?</kbd> | Open Help cheat sheet |
| <kbd>q</kbd> / <kbd>Ctrl+C</kbd> | Quit |

---

## 🛠️ CLI Subcommands Suite

```bash
# Launch interactive TUI
spacer [PATH]

# Top largest items & waste candidates
spacer top 20 --path ~

# Smart Quick Wins cleanup (dry-run or interactive)
spacer clean --dry-run
spacer clean --path ~

# Find duplicates & optionally replace with hardlinks
spacer dupes /path/to/folder
spacer dupes /path/to/folder --hardlink

# Export complete scan tree and scorecards to JSON / CSV
spacer export --path ~ --json /tmp/storage_report.json
spacer export --path ~ --csv /tmp/storage_report.csv

# Self-update to latest release
spacer update
```

---

## 🧪 Testing

Run all unit tests:
```bash
cargo test
```
