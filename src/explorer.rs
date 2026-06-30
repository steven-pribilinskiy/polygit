//! A reusable two-pane file explorer: a file LIST pane (name always shown; size / permissions /
//! created / modified / kind are toggleable columns, off by default) over a syntax-highlighted
//! PREVIEW pane, split by a draggable divider. Opened for a repo (its directory is the root).
//! Pure data + filesystem logic here; rendering lives in `render::modals::render_explorer`, input
//! in `main.rs`.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ratatui::layout::Rect;
use ratatui::text::Line;

/// Which optional columns the list pane shows. Name is always on; the rest default OFF. Persisted.
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ExplorerColumns {
    pub size: bool,
    pub permissions: bool,
    pub modified: bool,
    pub created: bool,
    pub kind: bool,
}

/// Which optional column a toggle targets (the explorer's `t` columns menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerColumn {
    Size,
    Permissions,
    Modified,
    Created,
    Kind,
}

impl ExplorerColumn {
    /// All optional columns in display order, with their menu label + mnemonic letter.
    pub const ALL: [(ExplorerColumn, &'static str, char); 5] = [
        (ExplorerColumn::Size, "size", 's'),
        (ExplorerColumn::Permissions, "permissions", 'p'),
        (ExplorerColumn::Modified, "modified", 'm'),
        (ExplorerColumn::Created, "created", 'c'),
        (ExplorerColumn::Kind, "kind", 'k'),
    ];

    pub fn enabled(self, columns: &ExplorerColumns) -> bool {
        match self {
            ExplorerColumn::Size => columns.size,
            ExplorerColumn::Permissions => columns.permissions,
            ExplorerColumn::Modified => columns.modified,
            ExplorerColumn::Created => columns.created,
            ExplorerColumn::Kind => columns.kind,
        }
    }
}

/// Which column the listing is sorted by. Directories always sort before files regardless.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    #[default]
    Name,
    Size,
    Modified,
    Created,
    Kind,
    Permissions,
}

impl SortKey {
    /// All sort keys in menu order, with their label + mnemonic letter.
    pub const ALL: [(SortKey, &'static str, char); 6] = [
        (SortKey::Name, "name", 'n'),
        (SortKey::Size, "size", 's'),
        (SortKey::Modified, "modified", 'm'),
        (SortKey::Created, "created", 'c'),
        (SortKey::Kind, "kind", 'k'),
        (SortKey::Permissions, "permissions", 'p'),
    ];

}

/// How time columns (modified / created) render: a relative "2d ago", or an absolute stamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateFormat {
    #[default]
    Relative,
    Stamp,
}

/// Persisted explorer preferences (columns, sort, date format) — seeded into each opened explorer.
/// Manual `Default` so `sort_ascending` is true (name-ascending), which serde's `bool` default isn't.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ExplorerPrefs {
    pub columns: ExplorerColumns,
    pub sort: SortKey,
    pub sort_ascending: bool,
    pub date_format: DateFormat,
}

impl Default for ExplorerPrefs {
    fn default() -> Self {
        ExplorerPrefs {
            columns: ExplorerColumns::default(),
            sort: SortKey::Name,
            sort_ascending: true,
            date_format: DateFormat::Relative,
        }
    }
}

/// One filesystem entry in the current directory listing.
#[derive(Debug, Clone)]
pub struct FsEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    /// `true` for the synthetic ".." parent row.
    pub is_parent: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    /// Unix `rwxr-xr-x`-style permissions (empty on non-unix).
    pub permissions: String,
}

/// The selected file's loaded preview (syntax-highlighted lines, or a placeholder).
#[derive(Debug, Clone)]
pub struct Preview {
    pub lines: Vec<Line<'static>>,
    pub scroll: usize,
}

/// The explorer's full state. Lives in `AppState::explorer` as an `Option` (None = closed).
pub struct Explorer {
    /// The directory the explorer is anchored to (never navigates above it).
    pub root: PathBuf,
    /// The directory currently listed.
    pub cwd: PathBuf,
    pub entries: Vec<FsEntry>,
    pub selected: usize,
    pub list_scroll: usize,
    /// List-pane fraction of the modal width (the draggable divider position).
    pub split: f64,
    pub columns: ExplorerColumns,
    pub sort: SortKey,
    pub sort_ascending: bool,
    pub date_format: DateFormat,
    /// The selected file's preview (lazy; refreshed on selection change).
    pub preview: Option<Preview>,
    /// Horizontal scroll offset (columns) for the preview pane.
    pub preview_hscroll: usize,
    pub focus: ExplorerFocus,
    /// The fuzzy file finder overlay (`/` or `Ctrl+P`), filtering the listing by name.
    pub finder: Option<Finder>,
    /// Recursive directory sizes, computed on a background thread (path → bytes). `None` = pending.
    pub dir_sizes: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<PathBuf, Option<u64>>>>,

    // ── geometry captured each render for hit-testing ──
    pub area: Rect,
    pub list_area: Rect,
    pub preview_area: Rect,
    pub divider_col: u16,
    pub rows_click: Vec<(u16, u16, u16, usize)>,
    pub close_click: Option<(u16, u16, u16)>,
    /// Sortable column header click regions: `(row, start, end, sort_key)`.
    pub header_click: Vec<(u16, u16, u16, SortKey)>,
}

/// The fuzzy file finder: a query + the matching entry indices (into the current `entries`).
#[derive(Debug, Clone, Default)]
pub struct Finder {
    pub query: String,
    /// Indices into `entries` that match `query` (fuzzy, case-insensitive), best first.
    pub matches: Vec<usize>,
    pub selected: usize,
}

/// Which pane the scroll/nav keys drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExplorerFocus {
    #[default]
    List,
    Preview,
}

impl Explorer {
    pub const MIN_SPLIT: f64 = 0.2;
    pub const MAX_SPLIT: f64 = 0.8;
    pub const DEFAULT_SPLIT: f64 = 0.42;

    /// Open an explorer rooted at `root`, listing it with the given prefs.
    pub fn open(root: PathBuf, prefs: ExplorerPrefs) -> Explorer {
        let cwd = root.clone();
        let mut explorer = Explorer {
            root,
            cwd,
            entries: Vec::new(),
            selected: 0,
            list_scroll: 0,
            split: Self::DEFAULT_SPLIT,
            columns: prefs.columns,
            sort: prefs.sort,
            sort_ascending: prefs.sort_ascending,
            date_format: prefs.date_format,
            preview: None,
            preview_hscroll: 0,
            focus: ExplorerFocus::List,
            finder: None,
            dir_sizes: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            area: Rect::default(),
            list_area: Rect::default(),
            preview_area: Rect::default(),
            divider_col: 0,
            rows_click: Vec::new(),
            close_click: None,
            header_click: Vec::new(),
        };
        explorer.reload();
        explorer
    }

    pub fn selected_entry(&self) -> Option<&FsEntry> {
        self.entries.get(self.selected)
    }

    /// Re-list the current directory, sort it, and kick off background dir-size computation.
    fn reload(&mut self) {
        self.entries = list_dir(&self.cwd, &self.root);
        self.apply_sort();
        self.spawn_dir_sizes();
    }

    /// Spawn a low-priority background thread to compute recursive sizes for the directories in the
    /// current listing (results land in `dir_sizes`; the render reads them, showing "…" until ready).
    fn spawn_dir_sizes(&self) {
        let dirs: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|entry| entry.is_dir && !entry.is_parent)
            .map(|entry| entry.path.clone())
            .filter(|path| !self.dir_sizes.lock().unwrap().contains_key(path))
            .collect();
        if dirs.is_empty() {
            return;
        }
        // Mark pending so we don't re-spawn for them.
        {
            let mut sizes = self.dir_sizes.lock().unwrap();
            for path in &dirs {
                sizes.insert(path.clone(), None);
            }
        }
        let shared = std::sync::Arc::clone(&self.dir_sizes);
        std::thread::spawn(move || {
            for path in dirs {
                let total = dir_size_bounded(&path);
                shared.lock().unwrap().insert(path, Some(total));
            }
        });
    }

    /// Set the sort key: same key flips direction; a new key selects it ascending. Re-sorts in place.
    pub fn set_sort(&mut self, key: SortKey) {
        if self.sort == key {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort = key;
            self.sort_ascending = true;
        }
        let keep = self.selected_entry().map(|entry| entry.path.clone());
        self.apply_sort();
        // Keep the selection on the same entry after a re-sort.
        if let Some(path) = keep {
            if let Some(index) = self.entries.iter().position(|entry| entry.path == path) {
                self.selected = index;
            }
        }
        self.preview = None;
    }

    /// Sort `entries`: the synthetic ".." first, then directories before files, then by the sort key
    /// (ascending/descending). A stable name tiebreak keeps equal keys in a predictable order.
    fn apply_sort(&mut self) {
        let key = self.sort;
        let ascending = self.sort_ascending;
        // Snapshot the size used for sorting (dirs use their cached recursive size) so the comparator
        // doesn't borrow `self` while `self.entries` is being reordered.
        let sizes: std::collections::HashMap<PathBuf, u64> = if key == SortKey::Size {
            let cache = self.dir_sizes.lock().unwrap();
            self.entries
                .iter()
                .map(|entry| {
                    let size = if entry.is_dir {
                        cache.get(&entry.path).and_then(|value| *value).unwrap_or(0)
                    } else {
                        entry.size
                    };
                    (entry.path.clone(), size)
                })
                .collect()
        } else {
            std::collections::HashMap::new()
        };
        self.entries.sort_by(|left, right| {
            // ".." pinned to the very top, dirs before files — independent of key/direction.
            let group = |entry: &FsEntry| (!entry.is_parent, !entry.is_dir);
            if group(left) != group(right) {
                return group(left).cmp(&group(right));
            }
            let ordering = match key {
                SortKey::Name => std::cmp::Ordering::Equal,
                SortKey::Size => sizes[&left.path].cmp(&sizes[&right.path]),
                SortKey::Modified => left.modified.cmp(&right.modified),
                SortKey::Created => left.created.cmp(&right.created),
                SortKey::Kind => left.kind_cell().cmp(&right.kind_cell()),
                SortKey::Permissions => left.permissions.cmp(&right.permissions),
            };
            let name_tiebreak = left.name.to_lowercase().cmp(&right.name.to_lowercase());
            let ordering = ordering.then(name_tiebreak);
            if ascending { ordering } else { ordering.reverse() }
        });
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let last = self.entries.len() as isize - 1;
        self.selected = (self.selected as isize).saturating_add(delta).clamp(0, last) as usize;
        self.preview = None; // re-load lazily for the new selection
    }

    pub fn select_first(&mut self) {
        self.selected = 0;
        self.preview = None;
    }

    pub fn select_last(&mut self) {
        self.selected = self.entries.len().saturating_sub(1);
        self.preview = None;
    }

    /// Enter the selected row: descend into a directory (or `..`), or focus the preview for a file.
    pub fn enter(&mut self) {
        let Some(entry) = self.entries.get(self.selected).cloned() else {
            return;
        };
        if entry.is_dir {
            self.navigate_to(entry.path);
        } else {
            self.focus = ExplorerFocus::Preview;
        }
    }

    /// Go up to the parent directory (bounded by `root`).
    pub fn go_up(&mut self) {
        if self.cwd == self.root {
            return;
        }
        if let Some(parent) = self.cwd.parent().map(Path::to_path_buf) {
            // Remember which child we came from so the selection lands back on it.
            let came_from = self.cwd.clone();
            self.navigate_to(parent);
            if let Some(index) = self.entries.iter().position(|entry| entry.path == came_from) {
                self.selected = index;
            }
        }
    }

    fn navigate_to(&mut self, dir: PathBuf) {
        self.cwd = dir;
        self.selected = 0;
        self.list_scroll = 0;
        self.preview = None;
        self.preview_hscroll = 0;
        self.focus = ExplorerFocus::List;
        self.finder = None;
        self.reload();
    }

    // ── Fuzzy file finder (`/` or `Ctrl+P`) ──────────────────────────────────────────────────────

    /// Open the finder (empty query matches everything).
    pub fn open_finder(&mut self) {
        let mut finder = Finder::default();
        self.recompute_finder_matches(&mut finder);
        self.finder = Some(finder);
    }

    pub fn close_finder(&mut self) {
        self.finder = None;
    }

    pub fn finder_push(&mut self, ch: char) {
        if let Some(mut finder) = self.finder.take() {
            finder.query.push(ch);
            self.recompute_finder_matches(&mut finder);
            self.finder = Some(finder);
        }
    }

    pub fn finder_backspace(&mut self) {
        if let Some(mut finder) = self.finder.take() {
            finder.query.pop();
            self.recompute_finder_matches(&mut finder);
            self.finder = Some(finder);
        }
    }

    pub fn finder_move(&mut self, delta: isize) {
        if let Some(finder) = self.finder.as_mut() {
            if finder.matches.is_empty() {
                return;
            }
            let last = finder.matches.len() as isize - 1;
            finder.selected = (finder.selected as isize + delta).clamp(0, last) as usize;
        }
    }

    /// Commit the finder: jump the list selection to the highlighted match, then close the finder.
    /// If the match is a directory, also open it.
    pub fn finder_commit(&mut self) {
        let Some(finder) = self.finder.take() else {
            return;
        };
        if let Some(&entry_index) = finder.matches.get(finder.selected) {
            self.selected = entry_index;
            self.preview = None;
        }
        // `enter` re-reads `self.selected`; open a directory match, else just land on the file.
        if self.selected_entry().is_some_and(|entry| entry.is_dir) {
            self.enter();
        }
    }

    fn recompute_finder_matches(&self, finder: &mut Finder) {
        let query = finder.query.to_lowercase();
        finder.matches = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| !entry.is_parent)
            .filter_map(|(index, entry)| {
                fuzzy_score(&query, &entry.name.to_lowercase()).map(|score| (index, score))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|(index, _)| index)
            .collect();
        // Stable: entries are already in sort order, so matches preserve it.
        finder.selected = finder.selected.min(finder.matches.len().saturating_sub(1));
    }

    /// Horizontal preview scroll (the `←`/`→` keys while the preview is focused).
    pub fn scroll_preview_h(&mut self, delta: isize) {
        self.preview_hscroll = (self.preview_hscroll as isize + delta).max(0) as usize;
    }

    /// Set the divider split from a screen column within the modal (clamped).
    pub fn set_split_from_col(&mut self, col: u16) {
        if self.area.width == 0 {
            return;
        }
        let rel = (col.saturating_sub(self.area.x)) as f64 / self.area.width as f64;
        self.split = rel.clamp(Self::MIN_SPLIT, Self::MAX_SPLIT);
    }

    /// Adjust the divider by `delta` (the `[`/`]` keys).
    pub fn adjust_split(&mut self, delta: f64) {
        self.split = (self.split + delta).clamp(Self::MIN_SPLIT, Self::MAX_SPLIT);
    }

    pub fn scroll_preview(&mut self, delta: isize) {
        if let Some(preview) = self.preview.as_mut() {
            preview.scroll = (preview.scroll as isize)
                .saturating_add(delta)
                .clamp(0, preview.lines.len() as isize - 1)
                .max(0) as usize;
        }
    }

    /// Load the selected file's preview if not already loaded (lazy; `dark` picks the theme).
    /// Directories and the `..` row get no preview. Binary / oversized files get a placeholder.
    pub fn ensure_preview(&mut self, dark: bool) {
        if self.preview.is_some() {
            return;
        }
        let Some(entry) = self.entries.get(self.selected) else {
            return;
        };
        if entry.is_dir {
            return;
        }
        let path = entry.path.clone();
        let name = entry.name.clone();
        let lines = load_preview_lines(&path, &name, dark);
        self.preview = Some(Preview { lines, scroll: 0 });
        self.preview_hscroll = 0;
    }

    /// The `size` cell for an entry: a file's byte size, or a directory's recursive size from the
    /// background cache (`…` while pending, blank for `..`).
    pub fn size_cell(&self, entry: &FsEntry) -> String {
        if entry.is_parent {
            return String::new();
        }
        if entry.is_dir {
            return match self.dir_sizes.lock().unwrap().get(&entry.path) {
                Some(Some(bytes)) => human_size(*bytes),
                _ => "…".to_string(),
            };
        }
        human_size(entry.size)
    }

    /// The raw byte size used for sorting (a dir's cached recursive size, or 0 if pending).
    pub fn sort_size(&self, entry: &FsEntry) -> u64 {
        if entry.is_dir {
            self.dir_sizes.lock().unwrap().get(&entry.path).and_then(|value| *value).unwrap_or(0)
        } else {
            entry.size
        }
    }
}

/// Maximum bytes read for a preview (keeps a giant log from blocking the UI).
const PREVIEW_MAX_BYTES: usize = 512 * 1024;

/// Read + highlight a file into preview lines, or a single placeholder line for binary / unreadable
/// / oversized files.
fn load_preview_lines(path: &Path, name: &str, dark: bool) -> Vec<Line<'static>> {
    let placeholder = |text: &str| vec![Line::from(text.to_string())];
    let Ok(bytes) = std::fs::read(path) else {
        return placeholder("(can't read file)");
    };
    if crate::highlight::looks_binary(&bytes) {
        return placeholder("(binary file)");
    }
    let truncated = bytes.len() > PREVIEW_MAX_BYTES;
    let slice = &bytes[..bytes.len().min(PREVIEW_MAX_BYTES)];
    let content = String::from_utf8_lossy(slice);
    let mut lines = crate::highlight::highlight_file(name, &content, dark);
    if truncated {
        lines.push(Line::from("… (truncated)".to_string()));
    }
    if lines.is_empty() {
        lines.push(Line::from("(empty file)".to_string()));
    }
    lines
}

/// List a directory: a synthetic `..` (unless at root) first, then directories, then files —
/// each group sorted case-insensitively by name. Hidden entries (dotfiles) are included.
fn list_dir(dir: &Path, root: &Path) -> Vec<FsEntry> {
    let mut dirs: Vec<FsEntry> = Vec::new();
    let mut files: Vec<FsEntry> = Vec::new();
    if let Ok(read) = std::fs::read_dir(dir) {
        for entry in read.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let meta = entry.metadata().ok();
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let fs_entry = FsEntry {
                name,
                path,
                is_dir,
                is_parent: false,
                size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                modified: meta.as_ref().and_then(|m| m.modified().ok()),
                created: meta.as_ref().and_then(|m| m.created().ok()),
                permissions: meta.as_ref().map(permission_string).unwrap_or_default(),
            };
            if is_dir {
                dirs.push(fs_entry);
            } else {
                files.push(fs_entry);
            }
        }
    }
    let by_name = |a: &FsEntry, b: &FsEntry| a.name.to_lowercase().cmp(&b.name.to_lowercase());
    dirs.sort_by(by_name);
    files.sort_by(by_name);

    let mut out = Vec::with_capacity(dirs.len() + files.len() + 1);
    if dir != root {
        if let Some(parent) = dir.parent() {
            out.push(FsEntry {
                name: "..".to_string(),
                path: parent.to_path_buf(),
                is_dir: true,
                is_parent: true,
                size: 0,
                modified: None,
                created: None,
                permissions: String::new(),
            });
        }
    }
    out.extend(dirs);
    out.extend(files);
    out
}

/// Format a file's mode as `rwxr-xr-x` (unix). Empty on other platforms.
#[cfg(unix)]
fn permission_string(meta: &std::fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode();
    let bit = |shift: u32, ch: char| if mode & (1 << shift) != 0 { ch } else { '-' };
    [
        bit(8, 'r'), bit(7, 'w'), bit(6, 'x'),
        bit(5, 'r'), bit(4, 'w'), bit(3, 'x'),
        bit(2, 'r'), bit(1, 'w'), bit(0, 'x'),
    ]
    .iter()
    .collect()
}

#[cfg(not(unix))]
fn permission_string(_meta: &std::fs::Metadata) -> String {
    String::new()
}

impl FsEntry {
    /// The `kind` column cell: `dir`, the lowercased extension, or `file`.
    pub fn kind_cell(&self) -> String {
        if self.is_parent {
            String::new()
        } else if self.is_dir {
            "dir".to_string()
        } else {
            Path::new(&self.name)
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_lowercase())
                .unwrap_or_else(|| "file".to_string())
        }
    }

}

/// A `modified`/`created` time as either a relative label (`2d ago`) or an absolute stamp
/// (`2026-06-30 14:05`), or blank when unavailable.
pub fn time_cell(time: Option<SystemTime>, format: DateFormat) -> String {
    let Some(time) = time else {
        return String::new();
    };
    let Ok(epoch) = time.duration_since(SystemTime::UNIX_EPOCH) else {
        return String::new();
    };
    let secs = epoch.as_secs() as i64;
    match format {
        DateFormat::Relative => {
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|dur| dur.as_secs() as i64)
                .unwrap_or(secs);
            crate::timeago::relative(now, secs)
        }
        DateFormat::Stamp => civil_stamp(secs),
    }
}

/// A `YYYY-MM-DD HH:MM` UTC stamp from a unix timestamp (no chrono dep — a civil-from-days
/// conversion via Howard Hinnant's algorithm).
fn civil_stamp(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (hour, minute) = (rem / 3600, (rem % 3600) / 60);
    // days since 1970-01-01 → civil (y, m, d). See howardhinnant.github.io/date_algorithms.html
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

/// Size-magnitude tier, for the contrast-by-magnitude coloring (B < KB < MB < GB).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeTier {
    Bytes,
    Kilo,
    Mega,
    Giga,
}

impl SizeTier {
    pub fn of(bytes: u64) -> SizeTier {
        const KB: u64 = 1024;
        const MB: u64 = 1024 * KB;
        const GB: u64 = 1024 * MB;
        if bytes >= GB {
            SizeTier::Giga
        } else if bytes >= MB {
            SizeTier::Mega
        } else if bytes >= KB {
            SizeTier::Kilo
        } else {
            SizeTier::Bytes
        }
    }
}

/// A subsequence fuzzy match: `Some(score)` if every char of `query` (already lowercased) appears in
/// `candidate` in order. Higher score = tighter / earlier match. `None` = no match. Empty query
/// matches everything with a neutral score.
pub fn fuzzy_score(query: &str, candidate: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let candidate: Vec<char> = candidate.chars().collect();
    let mut score: i64 = 0;
    let mut last_match: Option<usize> = None;
    let mut cand_index = 0;
    for query_char in query.chars() {
        let found = candidate[cand_index..].iter().position(|&ch| ch == query_char)?;
        let absolute = cand_index + found;
        // Reward adjacency (consecutive matches) and earlier positions.
        if last_match == Some(absolute.wrapping_sub(1)) {
            score += 10;
        }
        score -= absolute as i64;
        last_match = Some(absolute);
        cand_index = absolute + 1;
    }
    Some(score)
}

/// Recursive directory size in bytes, bounded so a huge tree (node_modules) can't hang the worker:
/// stops after `MAX_ENTRIES` files and does not follow symlinks.
fn dir_size_bounded(path: &Path) -> u64 {
    const MAX_ENTRIES: usize = 200_000;
    let mut total: u64 = 0;
    let mut seen = 0usize;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            seen += 1;
            if seen > MAX_ENTRIES {
                return total;
            }
            // `symlink_metadata` so we count the link, not its target, and never traverse it.
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    total
}

/// Human-readable byte size (e.g. `1.2 KB`). Shared with the columns renderer.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_size_scales_units() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn columns_default_all_off() {
        let cols = ExplorerColumns::default();
        assert!(!cols.size && !cols.permissions && !cols.modified && !cols.created && !cols.kind);
    }

    fn entry(name: &str, is_dir: bool) -> FsEntry {
        FsEntry {
            name: name.to_string(),
            path: PathBuf::from(name),
            is_dir,
            is_parent: false,
            size: 0,
            modified: None,
            created: None,
            permissions: String::new(),
        }
    }

    #[test]
    fn kind_cell_reads_extension_or_dir() {
        assert_eq!(entry("src", true).kind_cell(), "dir");
        assert_eq!(entry("config.json", false).kind_cell(), "json");
        assert_eq!(entry("Makefile", false).kind_cell(), "file");
        assert_eq!(entry("App.TSX", false).kind_cell(), "tsx");
    }

    #[test]
    fn fuzzy_score_matches_subsequence_only() {
        assert!(fuzzy_score("app", "app.ts").is_some());
        assert!(fuzzy_score("ats", "app.ts").is_some()); // subsequence a-t-s
        assert!(fuzzy_score("xyz", "app.ts").is_none());
        assert!(fuzzy_score("", "anything").is_some()); // empty matches all
        // A tighter (adjacent) match scores higher than a scattered one.
        assert!(fuzzy_score("app", "apple").unwrap() > fuzzy_score("app", "a_p_p").unwrap());
    }

    #[test]
    fn civil_stamp_formats_known_epoch() {
        // 2021-01-01 00:00:00 UTC = 1_609_459_200.
        assert_eq!(civil_stamp(1_609_459_200), "2021-01-01 00:00");
    }

    #[test]
    fn size_tier_thresholds() {
        assert_eq!(SizeTier::of(0), SizeTier::Bytes);
        assert_eq!(SizeTier::of(2048), SizeTier::Kilo);
        assert_eq!(SizeTier::of(5 * 1024 * 1024), SizeTier::Mega);
        assert_eq!(SizeTier::of(3 * 1024 * 1024 * 1024), SizeTier::Giga);
    }

    #[test]
    fn list_dir_orders_parent_then_dirs_then_files() {
        let root = std::env::temp_dir().join("polygit-explorer-test-list");
        let sub = root.join("sub");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(root.join("zeta.txt"), b"z").unwrap();
        std::fs::write(root.join("alpha.txt"), b"a").unwrap();
        // Listing the SUBdir (not root) so a synthetic ".." is included.
        let rows = list_dir(&sub, &root);
        // sub is empty except for "..".
        assert_eq!(rows.len(), 1);
        assert!(rows[0].is_parent && rows[0].name == "..");

        let rows = list_dir(&root, &root);
        // root != root is false, so no "..": just sub (dir) then alpha.txt, zeta.txt (files, sorted).
        let names: Vec<&str> = rows.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, vec!["sub", "alpha.txt", "zeta.txt"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
