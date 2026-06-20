//! A format-agnostic structural-data tree: a generic node model plus a flatten-to-visible-rows
//! pass for the collapsible tree viewer (used by the build-info modal's settings preview). The
//! model is deliberately not JSON-specific — any source (JSON, JSONL, YAML, TOML, …) only needs a
//! `DataNode` adapter; the viewer renders the model, not the syntax. JSON is the first adapter.

use std::collections::HashSet;

/// A scalar's type, for coloring (and so the renderer can quote strings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarKind {
    String,
    Number,
    Bool,
    Null,
}

/// A node in a generic structural tree: an ordered object, an array, or a leaf scalar.
#[derive(Debug, Clone)]
pub enum DataNode {
    Object(Vec<(String, DataNode)>),
    Array(Vec<DataNode>),
    Scalar { text: String, kind: ScalarKind },
}

impl DataNode {
    /// Adapt a parsed `serde_json::Value` into the agnostic model (preserving object order when
    /// serde_json's `preserve_order` feature is on, which it is here).
    pub fn from_json(value: serde_json::Value) -> Self {
        use serde_json::Value;
        match value {
            Value::Null => DataNode::Scalar { text: "null".to_string(), kind: ScalarKind::Null },
            Value::Bool(flag) => {
                DataNode::Scalar { text: flag.to_string(), kind: ScalarKind::Bool }
            }
            Value::Number(number) => {
                DataNode::Scalar { text: number.to_string(), kind: ScalarKind::Number }
            }
            Value::String(text) => DataNode::Scalar { text, kind: ScalarKind::String },
            Value::Array(items) => {
                DataNode::Array(items.into_iter().map(DataNode::from_json).collect())
            }
            Value::Object(map) => DataNode::Object(
                map.into_iter().map(|(key, value)| (key, DataNode::from_json(value))).collect(),
            ),
        }
    }

    /// Parse a JSON document into a tree, or `None` if it isn't valid JSON.
    pub fn parse_json(text: &str) -> Option<Self> {
        serde_json::from_str::<serde_json::Value>(text).ok().map(DataNode::from_json)
    }

    fn is_container(&self) -> bool {
        matches!(self, DataNode::Object(_) | DataNode::Array(_))
    }
}

/// Path-component separator (a control char that won't appear in keys), used to key the expanded set.
pub const SEP: char = '\u{1f}';

/// What a flattened row carries for rendering / hit-testing.
#[derive(Debug, Clone)]
pub enum RowKind {
    Scalar { text: String, kind: ScalarKind },
    /// A container: `is_object` picks `{N}` vs `[N]` braces, `count` is its child count.
    Container { is_object: bool, count: usize, collapsed: bool },
}

/// One visible row of the flattened tree.
#[derive(Debug, Clone)]
pub struct TreeRow {
    pub depth: usize,
    pub path: String,
    /// The object key, or `[i]` for an array element, or "" for a root scalar.
    pub label: String,
    /// True when `label` is an array index (rendered dimmer).
    pub label_is_index: bool,
    pub kind: RowKind,
}

/// Flatten a tree into the rows visible given `expanded` (the set of expanded container paths —
/// everything else is collapsed, i.e. containers are collapsed by default). The root container's
/// own children are shown at depth 0 (the root itself isn't a row).
pub fn flatten(root: &DataNode, expanded: &HashSet<String>) -> Vec<TreeRow> {
    let mut rows = Vec::new();
    match root {
        DataNode::Object(entries) => {
            for (key, child) in entries {
                push_node(key.clone(), false, child, "", 0, expanded, &mut rows);
            }
        }
        DataNode::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                push_node(format!("[{index}]"), true, child, "", 0, expanded, &mut rows);
            }
        }
        DataNode::Scalar { text, kind } => rows.push(TreeRow {
            depth: 0,
            path: String::new(),
            label: String::new(),
            label_is_index: false,
            kind: RowKind::Scalar { text: text.clone(), kind: *kind },
        }),
    }
    rows
}

#[allow(clippy::too_many_arguments)]
fn push_node(
    label: String,
    label_is_index: bool,
    node: &DataNode,
    parent_path: &str,
    depth: usize,
    expanded: &HashSet<String>,
    rows: &mut Vec<TreeRow>,
) {
    let path =
        if parent_path.is_empty() { label.clone() } else { format!("{parent_path}{SEP}{label}") };
    match node {
        DataNode::Scalar { text, kind } => rows.push(TreeRow {
            depth,
            path,
            label,
            label_is_index,
            kind: RowKind::Scalar { text: text.clone(), kind: *kind },
        }),
        DataNode::Object(entries) => {
            let collapsed = !expanded.contains(&path);
            rows.push(TreeRow {
                depth,
                path: path.clone(),
                label,
                label_is_index,
                kind: RowKind::Container { is_object: true, count: entries.len(), collapsed },
            });
            if !collapsed {
                for (key, child) in entries {
                    push_node(key.clone(), false, child, &path, depth + 1, expanded, rows);
                }
            }
        }
        DataNode::Array(items) => {
            let collapsed = !expanded.contains(&path);
            rows.push(TreeRow {
                depth,
                path: path.clone(),
                label,
                label_is_index,
                kind: RowKind::Container { is_object: false, count: items.len(), collapsed },
            });
            if !collapsed {
                for (index, child) in items.iter().enumerate() {
                    push_node(format!("[{index}]"), true, child, &path, depth + 1, expanded, rows);
                }
            }
        }
    }
}

/// Every container path in the tree (for "expand all"). Order matches a depth-first walk.
pub fn all_container_paths(root: &DataNode) -> Vec<String> {
    let mut paths = Vec::new();
    fn walk(node: &DataNode, parent: &str, out: &mut Vec<String>) {
        match node {
            DataNode::Object(entries) => {
                for (key, child) in entries {
                    let path =
                        if parent.is_empty() { key.clone() } else { format!("{parent}{SEP}{key}") };
                    if child.is_container() {
                        out.push(path.clone());
                    }
                    walk(child, &path, out);
                }
            }
            DataNode::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    let label = format!("[{index}]");
                    let path =
                        if parent.is_empty() { label.clone() } else { format!("{parent}{SEP}{label}") };
                    if child.is_container() {
                        out.push(path.clone());
                    }
                    walk(child, &path, out);
                }
            }
            DataNode::Scalar { .. } => {}
        }
    }
    walk(root, "", &mut paths);
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> DataNode {
        DataNode::parse_json(
            r#"{"a": 1, "obj": {"x": true, "y": "hi"}, "arr": [1, 2, {"z": null}]}"#,
        )
        .unwrap()
    }

    #[test]
    fn collapsed_by_default_shows_only_top_level() {
        let tree = sample();
        let rows = flatten(&tree, &HashSet::new());
        // a (scalar), obj (collapsed), arr (collapsed) — no children.
        assert_eq!(rows.len(), 3);
        assert!(matches!(rows[1].kind, RowKind::Container { is_object: true, count: 2, collapsed: true }));
        assert!(matches!(rows[2].kind, RowKind::Container { is_object: false, count: 3, collapsed: true }));
    }

    #[test]
    fn expanding_reveals_children() {
        let tree = sample();
        let expanded: HashSet<String> = ["obj".to_string()].into_iter().collect();
        let rows = flatten(&tree, &expanded);
        // a, obj(expanded), obj.x, obj.y, arr(collapsed) = 5 rows.
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[2].label, "x");
        assert_eq!(rows[2].depth, 1);
    }

    #[test]
    fn all_container_paths_finds_nested() {
        let paths = all_container_paths(&sample());
        assert!(paths.contains(&"obj".to_string()));
        assert!(paths.contains(&"arr".to_string()));
        assert!(paths.iter().any(|p| p.starts_with("arr") && p.ends_with("[2]")));
    }
}
