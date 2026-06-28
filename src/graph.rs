//! Pure commit-DAG lane layout for the repo page's Commits graph column. One row per commit (keeps
//! the row model 1:1 with the selectable commit rows); branches/merges show as parallel colored
//! lanes with `●` nodes (`◆` for a merge commit, 2+ parents). No separate connector rows — a forked
//! parent simply continues as its own colored lane on the rows below, and a lane ends where its
//! commit is reached. Cap the lane count so a pathological history can't blow up the column width.

use ratatui::style::Color;

/// One column-cell of a commit's graph row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphCell {
    pub glyph: char,
    /// Lane index — picks a stable 24-bit color via [`lane_color`].
    pub lane: usize,
}

/// The graph layout for a single commit row: the per-lane cells + which cell holds the node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRow {
    pub cells: Vec<GraphCell>,
    pub node: usize,
}

/// Max lanes drawn; extra concurrent branches collapse onto the last lane rather than widen forever.
const MAX_LANES: usize = 12;

/// Build a one-row-per-commit lane layout. `commits` are in display order (newest first); each is
/// `(sha, parents)` using whatever sha form `git log` emits (matched verbatim). The first parent
/// continues a commit's lane straight down; extra parents (merges) take fresh lanes to the right.
pub fn build_graph(commits: &[(String, Vec<String>)]) -> Vec<GraphRow> {
    let mut active: Vec<Option<String>> = Vec::new(); // lane -> the sha that lane is waiting for
    let mut rows: Vec<GraphRow> = Vec::with_capacity(commits.len());

    for (sha, parents) in commits {
        // The commit's lane: an existing lane already expecting this sha, else the first free lane
        // (a branch tip whose children aren't in the window), else a new lane.
        let node = active
            .iter()
            .position(|lane| lane.as_deref() == Some(sha.as_str()))
            .or_else(|| active.iter().position(|lane| lane.is_none()))
            .unwrap_or_else(|| {
                active.push(None);
                active.len() - 1
            });
        active[node] = Some(sha.clone());
        // Other lanes that also expected this sha are children merging back in — collapse them.
        for lane in active.iter_mut() {
            if lane.as_deref() == Some(sha.as_str()) {
                *lane = None;
            }
        }
        active[node] = Some(sha.clone());

        // Snapshot this row's cells (vertical lanes + the node) before re-pointing to parents.
        let merge = parents.len() >= 2;
        let cells: Vec<GraphCell> = active
            .iter()
            .enumerate()
            .map(|(lane, tracked)| {
                let glyph = if lane == node {
                    if merge { '◆' } else { '●' }
                } else if tracked.is_some() {
                    '│'
                } else {
                    ' '
                };
                GraphCell { glyph, lane }
            })
            .collect();

        // Re-point lanes to this commit's parents: first parent continues in the node lane; extra
        // parents reuse a lane already waiting for them, else take a fresh lane (capped).
        active[node] = parents.first().cloned();
        for parent in parents.iter().skip(1) {
            if active.iter().any(|lane| lane.as_deref() == Some(parent.as_str())) {
                continue;
            }
            match active.iter().position(|lane| lane.is_none()) {
                Some(free) => active[free] = Some(parent.clone()),
                None if active.len() < MAX_LANES => active.push(Some(parent.clone())),
                None => {
                    if let Some(last) = active.last_mut() {
                        *last = Some(parent.clone());
                    }
                }
            }
        }
        // Trim trailing free lanes so the column stays as narrow as the live branches need.
        while matches!(active.last(), Some(None)) {
            active.pop();
        }

        rows.push(GraphRow { cells, node });
    }
    rows
}

/// A stable, vivid 24-bit color per lane — cycles a GitKraken-ish palette. Truecolor so adjacent
/// branches stay visually distinct (the theme's ANSI remap doesn't touch `Color::Rgb`).
pub fn lane_color(lane: usize) -> Color {
    const COLORS: &[(u8, u8, u8)] = &[
        (0x4F, 0x9C, 0xFF), // blue
        (0x46, 0xC3, 0x84), // green
        (0xF2, 0x9B, 0x3C), // orange
        (0xB1, 0x7A, 0xF0), // purple
        (0x35, 0xC9, 0xC9), // teal
        (0xE5, 0x6B, 0x6F), // red
        (0xE7, 0xC4, 0x51), // yellow
        (0xEE, 0x7A, 0xC9), // pink
    ];
    let (red, green, blue) = COLORS[lane % COLORS.len()];
    Color::Rgb(red, green, blue)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(commits: &[(&str, &[&str])]) -> Vec<GraphRow> {
        let owned: Vec<(String, Vec<String>)> = commits
            .iter()
            .map(|(sha, parents)| (sha.to_string(), parents.iter().map(|p| p.to_string()).collect()))
            .collect();
        build_graph(&owned)
    }

    #[test]
    fn linear_history_is_one_lane() {
        // c -> b -> a (each parent is the next commit in the window).
        let graph = rows(&[("c", &["b"]), ("b", &["a"]), ("a", &[])]);
        assert_eq!(graph.len(), 3);
        for row in &graph {
            assert_eq!(row.node, 0, "linear history stays in lane 0");
            assert_eq!(row.cells[0].glyph, '●');
            assert_eq!(row.cells.len(), 1, "no extra lanes");
        }
    }

    #[test]
    fn merge_commit_uses_diamond_and_opens_a_second_lane() {
        // m is a merge of a + b; then b, then a.
        let graph = rows(&[("m", &["a", "b"]), ("b", &["a"]), ("a", &[])]);
        assert_eq!(graph[0].cells[graph[0].node].glyph, '◆', "merge node is a diamond");
        // Row for `b` should sit in its own lane (1), with lane 0 (waiting for `a`) drawn as │.
        assert!(graph[1].cells.len() >= 2, "second lane opened for the merge's 2nd parent");
        assert_eq!(graph[1].node, 1, "b is on the forked lane");
        assert_eq!(graph[1].cells[0].glyph, '│', "lane 0 still tracks a");
        // Final commit `a` collapses back to a single lane.
        assert_eq!(graph[2].node, 0);
    }

    #[test]
    fn non_merge_node_is_a_dot() {
        let graph = rows(&[("a", &[])]);
        assert_eq!(graph[0].cells[0].glyph, '●');
    }
}
