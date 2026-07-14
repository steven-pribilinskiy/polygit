//! Group/tree infrastructure: dynamic group assignment, tree building, fold state.

use super::*;

impl AppState {
    pub fn init_groups(&mut self, config: GroupsConfig, cache: &GroupsCache) -> Vec<String> {
        self.collapse_threshold = config.collapse_threshold();
        self.group_cache_ttl_minutes = config.cache_ttl_minutes();
        let mut errors = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        self.groups = config
            .groups
            .into_iter()
            .filter_map(|def| {
                if !def.name.trim().is_empty() && !seen.insert(def.name.clone()) {
                    errors.push(format!("group '{}': duplicate name", def.name));
                    return None;
                }
                match def.source() {
                    Ok(source) => {
                        let cached = cache
                            .entries
                            .get(&def.name)
                            .filter(|entry| entry.fingerprint == source.fingerprint());
                        let members = match &source {
                            GroupSource::Pattern(_) => None,
                            GroupSource::Repos(list) => {
                                Some(list.iter().map(|name| name.to_lowercase()).collect())
                            }
                            _ => cached.map(|entry| {
                                entry.members.iter().map(|name| name.to_lowercase()).collect()
                            }),
                        };
                        Some(GroupRuntime {
                            name: def.name,
                            source,
                            members,
                            resolving: false,
                            error: None,
                            resolved_at: cached.map(|entry| entry.resolved_at),
                        })
                    }
                    Err(err) => {
                        errors.push(err);
                        None
                    }
                }
            })
            .collect();
        self.recompute_group_assignments();
        errors
    }

    pub fn recompute_group_assignments(&mut self) {
        self.repo_group_map = self
            .repos
            .iter()
            .map(|repo| {
                let (name, rel) = {
                    let state = repo.lock().unwrap();
                    (state.name.to_lowercase(), state.rel_path.to_lowercase())
                };
                self.groups.iter().position(|group| group.contains(&name, &rel))
            })
            .collect();
    }

    pub fn rebuild_tree(&mut self) {
        let labels = self.root_labels();
        let multi = self.root_dirs.len() > 1;
        let pairs: Vec<(usize, String)> = self
            .repos
            .iter()
            .enumerate()
            .map(|(idx, repo)| {
                let repo = repo.lock().unwrap();
                let path = if multi {
                    let label =
                        labels.get(&repo.root).cloned().unwrap_or_else(|| repo.root.display().to_string());
                    format!("{label}/{}", repo.rel_path)
                } else {
                    repo.rel_path.clone()
                };
                (idx, path)
            })
            .collect();
        self.tree_nodes = build_tree(&pairs);
    }

    fn root_labels(&self) -> std::collections::HashMap<PathBuf, String> {
        let mut basename_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for root in &self.root_dirs {
            *basename_counts.entry(root_basename(root)).or_insert(0) += 1;
        }
        self.root_dirs
            .iter()
            .map(|root| {
                let base = root_basename(root);
                let label = if basename_counts.get(&base).copied().unwrap_or(0) > 1 {
                    home_relative(root)
                } else {
                    base
                };
                (root.clone(), label)
            })
            .collect()
    }

    pub fn tree_active(&self) -> bool {
        self.tree_enabled && !self.tree_nodes.is_empty()
    }

    pub fn toggle_tree_view(&mut self) {
        if self.tree_nodes.is_empty() {
            self.show_toast("no nested folders — every repo is at the scan root");
            return;
        }
        let prev = self.selected_repo_index();
        self.tree_enabled = !self.tree_enabled;
        self.reselect_repo(prev);
        let toast = if self.tree_enabled { "tree view on" } else { "tree view off" };
        self.show_toast(toast);
    }

    pub fn tree_subtree_repos(&self, node_idx: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut stack = vec![node_idx];
        while let Some(idx) = stack.pop() {
            let Some(node) = self.tree_nodes.get(idx) else {
                continue;
            };
            out.extend(node.repos.iter().copied());
            stack.extend(node.children.iter().copied());
        }
        out
    }

    pub fn repo_parent_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        for repo in &self.repos {
            if let Some(parent) = repo.lock().unwrap().path.parent() {
                let parent = parent.to_path_buf();
                if !dirs.contains(&parent) {
                    dirs.push(parent);
                }
            }
        }
        dirs
    }

    pub fn grouping_active(&self) -> bool {
        self.grouping_enabled && !self.groups.is_empty()
    }

    pub fn group_name(&self, group_idx: usize) -> &str {
        self.groups.get(group_idx).map_or("ungrouped", |group| group.name.as_str())
    }

    pub fn any_dynamic_groups(&self) -> bool {
        self.groups.iter().any(|group| group.source.is_dynamic())
    }

    pub fn toggle_grouping_view(&mut self) {
        if self.groups.is_empty() {
            self.show_toast("no groups configured — see ~/.config/polygit/groups.json");
            return;
        }
        let prev = self.selected_repo_index();
        self.grouping_enabled = !self.grouping_enabled;
        self.reselect_repo(prev);
        let toast = if self.grouping_enabled { "grouping on" } else { "grouping off" };
        self.show_toast(toast);
    }
}
