//! The fzf-style fuzzy finder: the matcher used everywhere (inline filter + the overlay) plus the
//! overlay widget itself. The overlay state/render are added alongside the host integration.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};


use crate::modal::{HintClick, HintKey, build_hint_footer, cast_shadow, centered_rect,
    footer_chip, footer_sep, modal_close_button};
use crate::ranking::{History, SortMode};
use crate::style::FinderStyle;

/// Fuzzy-match `query` against `haystack` (case-insensitive, smart normalization). Returns the
/// match `score` (higher = better) and the byte-agnostic **char indices** of the matched chars in
/// `haystack` (for highlighting). An empty query matches everything with score 0 and no indices.
pub fn fuzzy_match(haystack: &str, query: &str) -> Option<(u32, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut hbuf = Vec::new();
    let haystack = Utf32Str::new(haystack, &mut hbuf);
    let mut indices: Vec<u32> = Vec::new();
    pattern.indices(haystack, &mut matcher, &mut indices).map(|score| {
        indices.sort_unstable();
        indices.dedup();
        (score, indices.into_iter().map(|index| index as usize).collect())
    })
}

/// Whether `query` fuzzy-matches `haystack` at all (cheap membership test for filtering).
pub fn fuzzy_matches(haystack: &str, query: &str) -> bool {
    fuzzy_match(haystack, query).is_some()
}

/// One selectable row in the finder (e.g. a repo or worktree).
#[derive(Debug, Clone)]
pub struct FinderRow {
    /// Stable identity returned on accept + used to look up usage in [`History`] (an absolute path).
    pub key: String,
    /// Leading type column (e.g. `repo` / `wt`).
    pub kind: String,
    /// The path text shown (matched chars are highlighted against this).
    pub display: String,
}

/// What the user did to the selected row. `Open` is Enter / a click; the rest are action verbs a
/// host can bind to its own hotkeys (the host inspects `selected_row` and acts — see polygit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinderAction {
    Open,
}

/// The result of feeding a key to the finder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinderOutcome {
    /// Still open; keep rendering.
    Pending,
    /// Esc / cleared — close without selecting.
    Cancelled,
    /// A row was accepted (Enter / Space). The host acts on `key` (jump, record usage, …).
    Accepted { key: String, action: FinderAction },
}

/// A scrollable fzf-style finder over [`FinderRow`]s: query editing, fuzzy filtering, sort cycling,
/// and selection. The host owns rendering cadence + action hotkeys.
#[derive(Debug, Clone)]
pub struct FinderState {
    pub query: String,
    pub sort: SortMode,
    pub selected: usize,
    pub scroll: usize,
    rows: Vec<FinderRow>,
    /// Filtered+sorted view: (row index, match-char indices into `display`).
    view: Vec<(usize, Vec<usize>)>,
}

impl FinderState {
    /// Build a finder over `rows`, applying the initial `sort` (relevance with an empty query keeps
    /// input order). Pass the shared [`History`] so recent/most-used can rank immediately.
    pub fn new(rows: Vec<FinderRow>, sort: SortMode, history: &History) -> Self {
        let mut state =
            FinderState { query: String::new(), sort, selected: 0, scroll: 0, rows, view: Vec::new() };
        state.recompute(history);
        state
    }

    /// Rows matching the query (the visible count).
    pub fn matched(&self) -> usize {
        self.view.len()
    }

    /// Total rows (the unfiltered count).
    pub fn total(&self) -> usize {
        self.rows.len()
    }

    /// The currently-selected row, if any.
    pub fn selected_row(&self) -> Option<&FinderRow> {
        self.view.get(self.selected).map(|(idx, _)| &self.rows[*idx])
    }

    /// Re-filter + re-sort after a query, sort, or row change.
    fn recompute(&mut self, history: &History) {
        let query = self.query.clone();
        let mut view: Vec<(usize, Vec<usize>, u32)> = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(idx, row)| {
                fuzzy_match(&row.display, &query).map(|(score, matched)| (idx, matched, score))
            })
            .collect();
        match self.sort {
            SortMode::Relevance => view.sort_by(|left, right| {
                right.2.cmp(&left.2).then_with(|| self.rows[left.0].display.cmp(&self.rows[right.0].display))
            }),
            SortMode::Name => view.sort_by(|left, right| {
                self.rows[left.0].display.to_lowercase().cmp(&self.rows[right.0].display.to_lowercase())
            }),
            SortMode::Recent => view.sort_by(|left, right| {
                history.last_used(&self.rows[right.0].key).cmp(&history.last_used(&self.rows[left.0].key))
            }),
            SortMode::MostUsed => view.sort_by(|left, right| {
                history.count(&self.rows[right.0].key).cmp(&history.count(&self.rows[left.0].key))
            }),
        }
        self.view = view.into_iter().map(|(idx, matched, _)| (idx, matched)).collect();
        if self.selected >= self.view.len() {
            self.selected = self.view.len().saturating_sub(1);
        }
    }

    /// Cycle the sort mode (mirrors goto-repo's `^S`).
    pub fn cycle_sort(&mut self, history: &History) {
        self.sort = self.sort.cycle();
        self.recompute(history);
    }

    /// Move the selection by `delta`, clamped.
    pub fn move_selection(&mut self, delta: isize) {
        if self.view.is_empty() {
            return;
        }
        let max = self.view.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }

    /// Select the row at a screen row given the captured geometry (mouse click).
    pub fn select_at(&mut self, view_index: usize) {
        if view_index < self.view.len() {
            self.selected = view_index;
        }
    }

    /// Feed a crossterm key. Edits the query, navigates, cycles sort, accepts, or cancels.
    pub fn on_key(&mut self, key: crossterm::event::KeyEvent, history: &History) -> FinderOutcome {
        use crossterm::event::{KeyCode, KeyModifiers};
        match key.code {
            KeyCode::Esc => return FinderOutcome::Cancelled,
            KeyCode::Enter => {
                if let Some(row) = self.selected_row() {
                    return FinderOutcome::Accepted { key: row.key.clone(), action: FinderAction::Open };
                }
            }
            KeyCode::Down => self.move_selection(1),
            KeyCode::Up => self.move_selection(-1),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::Backspace => {
                self.query.pop();
                self.recompute(history);
            }
            // ^S cycles the sort mode (goto-repo parity).
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cycle_sort(history);
            }
            // Plain chars edit the query; Ctrl/Alt combos are left for the host (action hotkeys).
            KeyCode::Char(ch)
                if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.query.push(ch);
                self.recompute(history);
            }
            _ => {}
        }
        FinderOutcome::Pending
    }
}

/// Geometry captured during [`render_finder`] for mouse hit-testing.
#[derive(Debug, Clone, Default)]
pub struct FinderGeometry {
    /// The `[x]` close button `(row, col_start, col_end)`.
    pub close: Option<(u16, u16, u16)>,
    /// Per visible row: `(screen_row, view_index)`.
    pub rows: Vec<(u16, usize)>,
}

/// Render the finder overlay into `area`. Returns geometry for click hit-testing; appends footer
/// hint click regions to `hints`. Draws with the host's [`FinderStyle`].
pub fn render_finder(
    frame: &mut Frame,
    area: Rect,
    state: &FinderState,
    history: &History,
    style: &FinderStyle,
    hints: &mut Vec<HintClick>,
) -> FinderGeometry {
    let width = area.width.saturating_sub(8).clamp(40, 120);
    let height = area.height.saturating_sub(4).max(8);
    let modal = centered_rect(width, height, area);
    let (close_line, close) = modal_close_button(modal);

    // Footer hints on the bottom border (mirrors goto-repo's action row).
    let mut footer: Vec<(String, Style, Option<HintKey>)> = Vec::new();
    footer.extend(footer_chip("enter", " open", HintKey::Enter));
    footer.push(footer_sep());
    footer.extend(footer_chip("^s", " sort", HintKey::Char('s')));
    footer.push(footer_sep());
    footer.extend(footer_chip("esc", " close", HintKey::Esc));
    let footer_line = build_hint_footer(footer, modal.x + 1, modal.y + modal.height - 1, hints);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(style.border))
        .title(" Find repo ")
        .title_top(close_line)
        .title_bottom(footer_line);
    let inner = block.inner(modal);
    cast_shadow(frame, modal);
    frame.render_widget(Clear, modal);
    frame.render_widget(block, modal);

    let mut geo = FinderGeometry { close: Some(close), rows: Vec::new() };
    if inner.height < 3 {
        return geo;
    }

    // Prompt + live query.
    let prompt = Line::from(vec![
        Span::styled("Select repo: ", style.prompt),
        Span::styled(state.query.clone(), style.query),
    ]);
    // Counter + sort + total header.
    let header = Line::from(Span::styled(
        format!(
            "{}/{}  \u{b7}  ^S:sort({})",
            state.matched(),
            state.total(),
            state.sort.label()
        ),
        style.header,
    ));

    // Rows region (below the 2 header lines).
    let rows_top = inner.y + 2;
    let rows_height = inner.height.saturating_sub(2) as usize;
    // Keep the selection in view.
    let scroll = if state.selected < rows_height {
        0
    } else {
        state.selected + 1 - rows_height
    };

    let mut lines: Vec<Line> = vec![prompt, header];
    for offset in 0..rows_height {
        let view_index = scroll + offset;
        let Some((row_idx, matched)) = state.view.get(view_index) else {
            break;
        };
        let row = &state.rows[*row_idx];
        let selected = view_index == state.selected;
        let base = if selected { style.selected } else { style.row };
        let count = history.count(&row.key);
        let count_text = if count > 0 { count.to_string() } else { String::new() };
        let mut spans = vec![
            Span::styled(format!("{:<5}", row.kind), if selected { base } else { style.kind }),
            Span::styled(format!("{count_text:>4}  "), if selected { base } else { style.count }),
        ];
        spans.extend(highlight_spans(&row.display, matched, base, style.matched, selected));
        geo.rows.push((rows_top + offset as u16, view_index));
        lines.push(Line::from(spans));
    }

    frame.render_widget(Paragraph::new(lines), inner);
    geo
}

/// Build display spans with matched chars highlighted; the selected row keeps its bar style.
fn highlight_spans(
    display: &str,
    matched: &[usize],
    base: Style,
    match_style: Style,
    selected: bool,
) -> Vec<Span<'static>> {
    let set: std::collections::HashSet<usize> = matched.iter().copied().collect();
    let chars: Vec<char> = display.chars().collect();
    let mut spans = Vec::new();
    let mut run = String::new();
    let mut run_matched = set.contains(&0);
    let hl = if selected { base.add_modifier(Modifier::UNDERLINED) } else { match_style };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsequence_matches_and_reports_indices() {
        // "mfc" matches m..f..c as a subsequence of "microfrontends-calendar".
        let (_score, idx) = fuzzy_match("microfrontends-calendar", "mfc").expect("matches");
        assert!(!idx.is_empty());
        // Non-subsequence does not match.
        assert!(fuzzy_match("alpha", "zzz").is_none());
        // Empty query matches with no highlight.
        assert_eq!(fuzzy_match("anything", ""), Some((0, Vec::new())));
    }

    #[test]
    fn case_insensitive() {
        assert!(fuzzy_matches("PolyGit", "polygit"));
        assert!(fuzzy_matches("polygit", "PG"));
    }
}
