# ⚡ Spacer

> High-performance terminal storage manager with multi-factor waste prioritization and safe staged batch deletion, built in Rust.

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

* **⚡ Multithreaded Streaming Scanner**: Instantly opens and lets you browse top-level directories within milliseconds while deeply calculating recursive directory sizes asynchronously in the background.
* **🎯 Multi-Factor Waste Prioritization (0–100 Score)**:
  * **Size Magnitude (45%)**: Logarithmically scaled disk space impact.
  * **Inactivity / Age (30%)**: Scores items that have been untouched for months/years higher.
  * **Category Heuristics (25%)**: Prioritizes build artifacts (`target/`, `node_modules/`, `.next/`, `build/`, `venv/`), package caches (`.cache/`, `.npm/`, `.cargo-cache/`), temporary files (`*.log`, `*.tmp`, core dumps), and stale archives (`*.tar.gz`, `*.iso`).
* **🛡️ Safe Staged Batch Deletion**:
  * Stage individual files or folders with <kbd>Space</kbd> or batch select with <kbd>a</kbd>.
  * Live reclaimable space tracker and marked item preview.
  * Defaults to **Move to System Trash** (<kbd>d</kbd>) with unrecoverable **Permanent Deletion** (<kbd>Shift+D</kbd>) toggle.
  * Full safety confirmation modal before any deletion is executed.
* **🖥️ Dual-Pane Terminal Dashboard**:
  * **Left (Explorer)**: File list with visual size bars, category badges, human-readable sizes, and colored waste priority badges.
  * **Right (Inspector)**: Detailed metadata, access/modified timestamps, priority score breakdown meters, and staged batch queue summary.
* **🔍 Search & Dynamic Sort**:
  * Instant search filter (<kbd>/</kbd>).
  * Cycle sort order (<kbd>s</kbd>) by Size (Largest first), Priority Score (Highest waste first), Age (Oldest first), or Name (A-Z).

---

## ⌨️ Keyboard Shortcuts

| Shortcut | Action |
|---|---|
| <kbd>↑</kbd> / <kbd>k</kbd> | Move cursor up |
| <kbd>↓</kbd> / <kbd>j</kbd> | Move cursor down |
| <kbd>Enter</kbd> / <kbd>→</kbd> / <kbd>l</kbd> | Enter selected directory |
| <kbd>Backspace</kbd> / <kbd>←</kbd> / <kbd>h</kbd> | Navigate to parent directory |
| <kbd>Home</kbd> / <kbd>g</kbd> | Jump to first item |
| <kbd>End</kbd> / <kbd>G</kbd> | Jump to last item |
| <kbd>PageUp</kbd> / <kbd>PageDown</kbd> | Scroll by page |
| <kbd>Space</kbd> | Toggle stage/unstage for batch deletion |
| <kbd>a</kbd> | Stage/unstage all visible items |
| <kbd>c</kbd> | Clear all staged items |
| <kbd>d</kbd> | Open confirmation modal (Move to System Trash) |
| <kbd>Shift+D</kbd> | Open confirmation modal (Permanent unrecoverable delete) |
| <kbd>s</kbd> | Cycle sort order (Size ↓, Priority Score ↓, Age ↓, Name A-Z) |
| <kbd>/</kbd> | Filter items by name |
| <kbd>r</kbd> | Rescan current directory |
| <kbd>?</kbd> | Open Help cheat sheet |
| <kbd>q</kbd> / <kbd>Ctrl+C</kbd> | Quit |

---

## 🛠️ Usage

```bash
# Scan and manage current directory
spacer

# Scan your home directory
spacer ~

# Scan any specific directory
spacer /path/to/directory
```

---

## 🧪 Testing

Run all unit tests:
```bash
cargo test
```
