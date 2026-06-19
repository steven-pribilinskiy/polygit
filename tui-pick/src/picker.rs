//! The filesystem folder / git-repo picker dialog: breadcrumbs, home, bookmarks, up/back, a fuzzy
//! search, git-repo badges, and a current-path footer. Modeled on a graphical folder picker but
//! keyboard- and mouse-driven for a ratatui app. State + pure render + key/click handlers; the host
//! owns persistence (bookmarks/home) and what "select" does (e.g. add a workspace root).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use std::path::{Path, PathBuf};

use crate::finder::fuzzy_match;
use crate::modal::{HintClick, HintKey, build_hint_footer, cast_shadow, centered_rect, footer_chip,
    footer_sep, modal_close_button};
use crate::style::PickerStyle;

/// One filesystem entry shown in the picker list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_git_repo: bool,
}

/// Read the immediate sub-directories of `dir`, flagging which are git repos, sorted by name.
pub fn read_dir_entries(dir: &Path) -> std::io::Result<Vec<Entry>> {
    let mut entries: Vec<Entry> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let is_git_repo = path.join(".git").exists();
        entries.push(Entry { name, path, is_dir: true, is_git_repo });
    }
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(entries)
}

/// Whether `dir` is itself a git repo (selecting it adds a single-repo root).
pub fn is_git_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// What the picker decided when fed a key/click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerOutcome {
    /// Still open.
    Pending,
    /// Esc — close without choosing.
    Cancelled,
    /// A folder/repo was chosen — the host adds it (e.g. as a workspace root).
    Selected(PathBuf),
}

/// The folder picker's state. The host seeds `home` + `bookmarks` and persists changes to them.
#[derive(Debug, Clone)]
pub struct PickerState {
    pub current_dir: PathBuf,
    pub query: String,
    pub selected: usize,
    pub scroll: usize,
    pub home: PathBuf,
    pub bookmarks: Vec<PathBuf>,
    entries: Vec<Entry>,
    /// Filtered view: (entry index, match-char indices into `name`).
    view: Vec<(usize, Vec<usize>)>,
    /// Back stack of visited dirs.
    history: Vec<PathBuf>,
}

impl PickerState {
    /// Open the picker at `start`, with `home` and existing `bookmarks`.
    pub fn new(start: PathBuf, home: PathBuf, bookmarks: Vec<PathBuf>) -> Self {
        let mut state = PickerState {
            current_dir: start.clone(),
            query: String::new(),
            selected: 0,
            scroll: 0,
            home,
            bookmarks,
            entries: Vec::new(),
            view: Vec::new(),
            history: vec![start],
        };
        state.reload();
        state
    }

    /// Re-read the current directory and re-apply the filter.
    pub fn reload(&mut self) {
        self.entries = read_dir_entries(&self.current_dir).unwrap_or_default();
        self.refilter();
    }

    fn refilter(&mut self) {
        let query = self.query.clone();
        self.view = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(idx, entry)| {
                fuzzy_match(&entry.name, &query).map(|(_, matched)| (idx, matched))
            })
            .collect();
        if self.selected >= self.view.len() {
            self.selected = self.view.len().saturating_sub(1);
        }
    }

    /// Navigate into `dir` (records the previous dir for `back`).
    pub fn navigate_to(&mut self, dir: PathBuf) {
        if dir == self.current_dir {
            return;
        }
        self.history.push(dir.clone());
        self.current_dir = dir;
        self.query.clear();
        self.selected = 0;
        self.reload();
    }

    /// Go to the parent directory.
    pub fn parent(&mut self) {
        if let Some(parent) = self.current_dir.parent() {
            self.navigate_to(parent.to_path_buf());
        }
    }

    /// Go to the home directory.
    pub fn go_home(&mut self) {
        let home = self.home.clone();
        self.navigate_to(home);
    }

    /// Pop the back stack to the previous directory.
    pub fn back(&mut self) {
        if self.history.len() > 1 {
            self.history.pop();
            if let Some(prev) = self.history.last().cloned() {
                self.current_dir = prev;
                self.query.clear();
                self.selected = 0;
                self.reload();
            }
        }
    }

    /// Toggle a bookmark for the current directory. Returns the new state (true = now bookmarked).
    pub fn toggle_bookmark(&mut self) -> bool {
        if let Some(pos) = self.bookmarks.iter().position(|path| path == &self.current_dir) {
            self.bookmarks.remove(pos);
            false
        } else {
            self.bookmarks.push(self.current_dir.clone());
            true
        }
    }

    /// The currently-highlighted entry, if any.
    pub fn selected_entry(&self) -> Option<&Entry> {
        self.view.get(self.selected).map(|(idx, _)| &self.entries[*idx])
    }

    /// Move the selection by `delta`, clamped.
    pub fn move_selection(&mut self, delta: isize) {
        if self.view.is_empty() {
            return;
        }
        let max = self.view.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }

    /// Select the view row at `view_index` (mouse click).
    pub fn select_at(&mut self, view_index: usize) {
        if view_index < self.view.len() {
            self.selected = view_index;
        }
    }

    /// Activate the selected entry: navigate into a folder, or **select** a git repo (the host adds
    /// it as a root). Returns `Selected` only for a repo.
    pub fn activate_selected(&mut self) -> PickerOutcome {
        if let Some(entry) = self.selected_entry().cloned() {
            if entry.is_git_repo {
                return PickerOutcome::Selected(entry.path);
            }
            self.navigate_to(entry.path);
        }
        PickerOutcome::Pending
    }

    /// Feed a crossterm key.
    pub fn on_key(&mut self, key: crossterm::event::KeyEvent) -> PickerOutcome {
        use crossterm::event::{KeyCode, KeyModifiers};
        match key.code {
            KeyCode::Esc => return PickerOutcome::Cancelled,
            KeyCode::Enter => return self.activate_selected(),
            KeyCode::Down => self.move_selection(1),
            KeyCode::Up => self.move_selection(-1),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::Left => self.parent(),
            KeyCode::Backspace => {
                if self.query.is_empty() {
                    self.parent();
                } else {
                    self.query.pop();
                    self.refilter();
                }
            }
            // Ctrl-S: select the current directory itself (the "Select Folder" button).
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return PickerOutcome::Selected(self.current_dir.clone());
            }
            // Ctrl-B: bookmark; Ctrl-H: home (avoid clashing with typed 'b'/'h').
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_bookmark();
            }
            KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => self.go_home(),
            KeyCode::Char(ch)
                if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.query.push(ch);
                self.refilter();
            }
            _ => {}
        }
        PickerOutcome::Pending
    }
}

/// Geometry captured during [`render_picker`] for mouse hit-testing.
#[derive(Debug, Clone, Default)]
pub struct PickerGeometry {
    pub close: Option<(u16, u16, u16)>,
    /// Breadcrumb segments: `(row, col_start, col_end, path)`.
    pub crumbs: Vec<(u16, u16, u16, PathBuf)>,
    /// List rows: `(screen_row, view_index)`.
    pub rows: Vec<(u16, usize)>,
}

/// Render the folder picker into `area`. Returns click geometry; appends footer hints.
pub fn render_picker(
    frame: &mut Frame,
    area: Rect,
    state: &PickerState,
    style: &PickerStyle,
    hints: &mut Vec<HintClick>,
) -> PickerGeometry {
    let width = area.width.saturating_sub(8).clamp(40, 110);
    let height = area.height.saturating_sub(4).max(10);
    let modal = centered_rect(width, height, area);
    let (close_line, close) = modal_close_button(modal);

    let mut footer: Vec<(String, Style, Option<HintKey>)> = Vec::new();
    footer.extend(footer_chip("enter", " open/select", HintKey::Enter));
    footer.push(footer_sep());
    footer.extend(footer_chip("^s", " select folder", HintKey::Char('s')));
    footer.push(footer_sep());
    footer.extend(footer_chip("esc", " cancel", HintKey::Esc));
    let footer_line = build_hint_footer(footer, modal.x + 1, modal.y + modal.height - 1, hints);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(style.border))
        .title(" Add folder ")
        .title_top(close_line)
        .title_bottom(footer_line);
    let inner = block.inner(modal);
    cast_shadow(frame, modal);
    frame.render_widget(Clear, modal);
    frame.render_widget(block, modal);

    let mut geo = PickerGeometry { close: Some(close), crumbs: Vec::new(), rows: Vec::new() };
    if inner.height < 4 {
        return geo;
    }

    // Breadcrumb row: a leading `/` then clickable `seg` segments joined by ` / `.
    let mut crumb_spans: Vec<Span> = Vec::new();
    let mut col = inner.x;
    crumb_spans.push(Span::styled("/", style.breadcrumb));
    col += 1;
    let mut acc = PathBuf::from("/");
    let segments: Vec<String> = state
        .current_dir
        .components()
        .filter_map(|comp| match comp {
            std::path::Component::Normal(name) => Some(name.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();
    for (index, seg) in segments.iter().enumerate() {
        if index > 0 {
            crumb_spans.push(Span::styled(" / ", style.breadcrumb));
            col += 3;
        }
        acc.push(seg);
        let start = col;
        col += seg.chars().count() as u16;
        geo.crumbs.push((inner.y, start, col, acc.clone()));
        crumb_spans.push(Span::styled(seg.clone(), style.breadcrumb));
    }
    let search = Line::from(vec![
        Span::styled("Search: ", style.breadcrumb),
        Span::styled(state.query.clone(), style.repo),
    ]);

    // List region (below breadcrumb + search + a blank).
    let rows_top = inner.y + 3;
    let rows_height = inner.height.saturating_sub(4) as usize; // breadcrumb+search+blank+pathfooter
    let scroll = if state.selected < rows_height { 0 } else { state.selected + 1 - rows_height };

    let mut lines: Vec<Line> = vec![Line::from(crumb_spans), search, Line::from("")];
    for offset in 0..rows_height {
        let view_index = scroll + offset;
        let Some((entry_index, matched)) = state.view.get(view_index) else {
            break;
        };
        let entry = &state.entries[*entry_index];
        let selected = view_index == state.selected;
        let base = if selected {
            style.selected
        } else if entry.is_git_repo {
            style.repo
        } else {
            style.folder
        };
        let icon = if entry.is_git_repo { "\u{1f4e6}" } else { "\u{1f4c1}" }; // 📦 repo / 📁 folder
        let mut spans = vec![Span::styled(format!("{icon} "), base)];
        spans.extend(highlight(&entry.name, matched, base, style.matched, selected));
        if entry.is_git_repo {
            spans.push(Span::styled("  ", base));
            spans.push(Span::styled("git repo", style.badge));
        }
        geo.rows.push((rows_top + offset as u16, view_index));
        lines.push(Line::from(spans));
    }

    // Current-path footer line at the very bottom of the inner area.
    let path_line = Line::from(vec![
        Span::styled("Current: ", style.breadcrumb),
        Span::styled(state.current_dir.display().to_string(), style.path),
    ]);
    while lines.len() < inner.height.saturating_sub(1) as usize {
        lines.push(Line::from(""));
    }
    lines.push(path_line);

    frame.render_widget(Paragraph::new(lines), inner);
    geo
}

/// Span builder highlighting matched chars (selected row keeps its bar style).
fn highlight(
    text: &str,
    matched: &[usize],
    base: Style,
    match_style: Style,
    selected: bool,
) -> Vec<Span<'static>> {
    let set: std::collections::HashSet<usize> = matched.iter().copied().collect();
    let chars: Vec<char> = text.chars().collect();
    let hl = if selected { base.add_modifier(Modifier::UNDERLINED) } else { match_style };
    let mut spans = Vec::new();
    let mut run = String::new();
    let mut run_matched = set.contains(&0);
    for (index, ch) in chars.iter().enumerate() {
        let is_matched = set.contains(&index);
        if is_matched != run_matched && !run.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut run), if run_matched { hl } else { base }));
        }
        run_matched = is_matched;
        run.push(*ch);
    }
    if !run.is_empty() {
        spans.push(Span::styled(run, if run_matched { hl } else { base }));
    }
    spans
}
