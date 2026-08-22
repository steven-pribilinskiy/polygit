//! Where a selected repo belongs on disk, and the diff between that and where it is now.
//!
//! One plan model serves both actions: a repo absent locally is a clone, a repo in the wrong place
//! is a move, and a repo already correct is left alone. Everything here is pure — no network, no
//! filesystem — so the whole "what would happen" question is answerable and testable offline.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::app::{TreeNode, build_tree};
use crate::select::{ClusterOpts, RepoFacts, clusters, prefix_families, prefix_key};

/// A field a layout placeholder can resolve to. A field that resolves to nothing collapses its
/// whole path segment away, which is what keeps a `{project}/{repo}` layout from scattering the
/// 71.7% of repos that belong to no cluster into one folder each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field {
    Repo,
    Owner,
    /// The project stem, when the repo belongs to a real cluster.
    Project,
    /// The prefix family, when that family has more than one member.
    Group,
    Language,
    /// The literal topic, when the repo carries it.
    Topic(String),
}

impl Field {
    fn parse(name: &str) -> Result<Self, String> {
        match name.split_once(':') {
            Some(("topic", value)) if !value.is_empty() => Ok(Field::Topic(value.to_string())),
            Some(("topic", _)) => Err("{topic:} needs a topic name".to_string()),
            Some((other, _)) => Err(format!("unknown placeholder '{{{other}:…}}'")),
            None => match name {
                "repo" => Ok(Field::Repo),
                "owner" => Ok(Field::Owner),
                "project" => Ok(Field::Project),
                "group" => Ok(Field::Group),
                "language" | "lang" => Ok(Field::Language),
                other => Err(format!("unknown placeholder '{{{other}}}'")),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Literal(String),
    Field(Field),
}

/// A destination-path template, e.g. `{group}/{project}/{repo}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutTemplate {
    raw: String,
    /// One entry per `/`-separated path segment.
    segments: Vec<Vec<Part>>,
}

impl Default for LayoutTemplate {
    /// Flat — today's behaviour, and the only safe default when most repos cluster with nothing.
    fn default() -> Self {
        LayoutTemplate::parse("{repo}").expect("the default template is valid")
    }
}

impl LayoutTemplate {
    pub fn parse(raw: &str) -> Result<Self, String> {
        if raw.trim().is_empty() {
            return Err("layout template is empty".to_string());
        }
        let mut segments = Vec::new();
        for segment in raw.split('/').filter(|piece| !piece.is_empty()) {
            segments.push(parse_segment(segment)?);
        }
        if segments.is_empty() {
            return Err("layout template has no path segments".to_string());
        }
        let has_repo = segments
            .iter()
            .flatten()
            .any(|part| matches!(part, Part::Field(Field::Repo)));
        if !has_repo {
            return Err("layout template must contain {repo}".to_string());
        }
        Ok(Self { raw: raw.to_string(), segments })
    }

    pub fn source(&self) -> &str {
        &self.raw
    }

    /// The repo's path relative to the destination root. Segments whose placeholders all resolve to
    /// nothing are dropped rather than becoming empty directories.
    pub fn render(&self, repo: &RepoFacts, context: &LayoutContext, index: usize) -> String {
        let mut out: Vec<String> = Vec::new();
        for segment in &self.segments {
            let mut text = String::new();
            let mut had_field = false;
            let mut field_filled = false;
            for part in segment {
                match part {
                    Part::Literal(literal) => text.push_str(literal),
                    Part::Field(field) => {
                        had_field = true;
                        let value = context.resolve(field, repo, index);
                        if !value.is_empty() {
                            field_filled = true;
                            text.push_str(&value);
                        }
                    }
                }
            }
            let slug = slugify(&text);
            if slug.is_empty() || (had_field && !field_filled) {
                continue;
            }
            out.push(slug);
        }
        out.join("/")
    }
}

fn parse_segment(segment: &str) -> Result<Vec<Part>, String> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut rest = segment;
    while let Some(open) = rest.find('{') {
        literal.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close = after.find('}').ok_or_else(|| "unclosed '{' in layout".to_string())?;
        if !literal.is_empty() {
            parts.push(Part::Literal(std::mem::take(&mut literal)));
        }
        parts.push(Part::Field(Field::parse(&after[..close])?));
        rest = &after[close + 1..];
    }
    literal.push_str(rest);
    if !literal.is_empty() {
        parts.push(Part::Literal(literal));
    }
    Ok(parts)
}

/// Reduce arbitrary text to one safe path component: lowercase, non-alphanumerics folded to `-`,
/// runs collapsed, edges trimmed. Language names alone need this (`C#`, `Go Template`).
pub fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_dash = false;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    let trimmed = out.trim_matches(['-', '.']).to_string();
    if trimmed == ".." { String::new() } else { trimmed }
}

/// Derived facts a template needs: which cluster each repo belongs to, and which prefix families
/// are big enough to be worth a folder.
#[derive(Debug, Clone)]
pub struct LayoutContext {
    project_by_index: HashMap<usize, String>,
    group_by_index: HashMap<usize, String>,
}

impl LayoutContext {
    /// Build from the full listing. `prefix_depth` picks how many name tokens form a family key, and
    /// `min_family` is how many members a family needs before it earns a folder.
    pub fn build(
        repos: &[RepoFacts],
        prefix_depth: usize,
        min_family: usize,
        opts: &ClusterOpts,
    ) -> Self {
        let mut project_by_index = HashMap::new();
        for cluster in clusters(repos, opts) {
            for index in &cluster.members {
                project_by_index.insert(*index, cluster.core.clone());
            }
        }
        let families: HashMap<String, usize> =
            prefix_families(repos, prefix_depth).into_iter().collect();
        let mut group_by_index = HashMap::new();
        for (index, repo) in repos.iter().enumerate() {
            let key = prefix_key(&repo.name, prefix_depth);
            if families.get(&key).copied().unwrap_or(0) >= min_family.max(2) {
                group_by_index.insert(index, key);
            }
        }
        Self { project_by_index, group_by_index }
    }

    fn resolve(&self, field: &Field, repo: &RepoFacts, index: usize) -> String {
        match field {
            Field::Repo => repo.name.clone(),
            Field::Owner => repo.owner.clone(),
            Field::Project => self.project_by_index.get(&index).cloned().unwrap_or_default(),
            Field::Group => self.group_by_index.get(&index).cloned().unwrap_or_default(),
            Field::Language => repo.language.clone().unwrap_or_default(),
            Field::Topic(topic) => {
                let wanted = topic.to_lowercase();
                if repo.topics.iter().any(|owned| owned.to_lowercase() == wanted) {
                    topic.clone()
                } else {
                    String::new()
                }
            }
        }
    }
}

/// Why a selected repo produces no action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// Another selected repo wants the same destination. Both are skipped rather than one silently
    /// winning — two owners sharing a repo name is the case this catches.
    Collision(String),
    /// The template produced nothing usable for this repo.
    NoDestination,
}

impl SkipReason {
    pub fn label(&self) -> String {
        match self {
            SkipReason::Collision(other) => format!("collides with {other}"),
            SkipReason::NoDestination => "no destination".to_string(),
        }
    }
}

/// What will happen to one selected repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Not present locally — clone it here.
    Clone,
    /// Already exactly here.
    Keep,
    /// Present, but somewhere else.
    Move { from: PathBuf },
    Skip(SkipReason),
}

impl Action {
    pub fn label(&self) -> &'static str {
        match self {
            Action::Clone => "clone",
            Action::Keep => "keep",
            Action::Move { .. } => "move",
            Action::Skip(_) => "skip",
        }
    }
}

/// One row of the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRow {
    /// Index into the listing the plan was built from.
    pub index: usize,
    pub owner: String,
    pub name: String,
    /// Path relative to the plan root, `""` when skipped with no destination.
    pub rel_dest: String,
    pub dest: PathBuf,
    pub action: Action,
}

/// A resolved set of actions plus the root they are relative to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub root: PathBuf,
    pub rows: Vec<PlanRow>,
}

impl Plan {
    pub fn counts(&self) -> PlanCounts {
        let mut counts = PlanCounts::default();
        for row in &self.rows {
            match row.action {
                Action::Clone => counts.clone_rows += 1,
                Action::Keep => counts.keep += 1,
                Action::Move { .. } => counts.moves += 1,
                Action::Skip(_) => counts.skipped += 1,
            }
        }
        counts
    }

    /// The directory tree the plan describes, through the same builder the repo list uses. Indices
    /// in each node's `repos` are positions in `self.rows`.
    pub fn tree(&self) -> Vec<TreeNode> {
        let pairs: Vec<(usize, String)> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| !row.rel_dest.is_empty())
            .map(|(position, row)| (position, row.rel_dest.clone()))
            .collect();
        build_tree(&pairs)
    }
}

/// Tallies for the confirm dialog and the CLI summary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlanCounts {
    pub clone_rows: usize,
    pub keep: usize,
    pub moves: usize,
    pub skipped: usize,
}

impl PlanCounts {
    pub fn total(&self) -> usize {
        self.clone_rows + self.keep + self.moves + self.skipped
    }
}

/// Diff the selected repos against where they are now. `selected` holds indices into `repos`.
pub fn plan(
    repos: &[RepoFacts],
    selected: &[usize],
    layout: &LayoutTemplate,
    context: &LayoutContext,
    root: &Path,
) -> Plan {
    // Destinations first, so a collision is visible before any action is assigned.
    let mut wanted: Vec<(usize, String)> = Vec::new();
    for &index in selected {
        let Some(repo) = repos.get(index) else { continue };
        wanted.push((index, layout.render(repo, context, index)));
    }
    let mut claimants: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, rel) in &wanted {
        if !rel.is_empty() {
            claimants.entry(rel.as_str()).or_default().push(*index);
        }
    }

    let mut rows = Vec::with_capacity(wanted.len());
    for (index, rel) in &wanted {
        let repo = &repos[*index];
        let dest = if rel.is_empty() { PathBuf::new() } else { root.join(rel) };
        let action = if rel.is_empty() {
            Action::Skip(SkipReason::NoDestination)
        } else if let Some(others) = claimants.get(rel.as_str()).filter(|list| list.len() > 1) {
            let other = others
                .iter()
                .find(|&&candidate| candidate != *index)
                .map(|&candidate| repos[candidate].slug())
                .unwrap_or_default();
            Action::Skip(SkipReason::Collision(other))
        } else {
            match repo.local_path.as_ref() {
                None => Action::Clone,
                Some(current) if current == &dest => Action::Keep,
                Some(current) => Action::Move { from: current.clone() },
            }
        };
        rows.push(PlanRow {
            index: *index,
            owner: repo.owner.clone(),
            name: repo.name.clone(),
            rel_dest: rel.clone(),
            dest,
            action,
        });
    }
    Plan { root: root.to_path_buf(), rows }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(owner: &str, name: &str) -> RepoFacts {
        RepoFacts { owner: owner.into(), name: name.into(), ..Default::default() }
    }

    fn cloned(owner: &str, name: &str, path: &str) -> RepoFacts {
        RepoFacts { local_path: Some(PathBuf::from(path)), ..repo(owner, name) }
    }

    fn context(repos: &[RepoFacts]) -> LayoutContext {
        LayoutContext::build(repos, 1, 2, &ClusterOpts::default())
    }

    fn all(repos: &[RepoFacts]) -> Vec<usize> {
        (0..repos.len()).collect()
    }

    #[test]
    fn template_requires_repo_and_rejects_nonsense() {
        assert!(LayoutTemplate::parse("{owner}").unwrap_err().contains("{repo}"));
        assert!(LayoutTemplate::parse("").unwrap_err().contains("empty"));
        assert!(LayoutTemplate::parse("{repo").unwrap_err().contains("unclosed"));
        assert!(LayoutTemplate::parse("{bogus}/{repo}").unwrap_err().contains("unknown placeholder"));
        assert!(LayoutTemplate::parse("{topic:}/{repo}").unwrap_err().contains("needs a topic"));
        assert!(LayoutTemplate::parse("{group}/{project}/{repo}").is_ok());
    }

    #[test]
    fn default_layout_is_flat() {
        let repos = vec![repo("acme", "billing")];
        let rendered = LayoutTemplate::default().render(&repos[0], &context(&repos), 0);
        assert_eq!(rendered, "billing");
    }

    #[test]
    fn empty_placeholders_collapse_their_segment() {
        // Two clustered repos and one singleton, under {project}/{repo}: the cluster gets a folder,
        // the singleton sits flat rather than in a folder of its own.
        let repos = vec![repo("acme", "billing"), repo("acme", "tf-billing"), repo("acme", "solo")];
        let context = context(&repos);
        let template = LayoutTemplate::parse("{project}/{repo}").unwrap();
        assert_eq!(template.render(&repos[0], &context, 0), "billing/billing");
        assert_eq!(template.render(&repos[1], &context, 1), "billing/tf-billing");
        assert_eq!(template.render(&repos[2], &context, 2), "solo");
    }

    #[test]
    fn group_needs_a_family_to_earn_a_folder() {
        let repos =
            vec![repo("acme", "tf-a"), repo("acme", "tf-b"), repo("acme", "lonely-one")];
        let context = context(&repos);
        let template = LayoutTemplate::parse("{group}/{repo}").unwrap();
        assert_eq!(template.render(&repos[0], &context, 0), "tf/tf-a");
        assert_eq!(template.render(&repos[1], &context, 1), "tf/tf-b");
        assert_eq!(template.render(&repos[2], &context, 2), "lonely-one");
    }

    #[test]
    fn language_and_topic_are_slugified_and_optional() {
        let mut subject = repo("acme", "tool");
        subject.language = Some("Go Template".into());
        subject.topics = vec!["Data-Platform".into()];
        let repos = vec![subject];
        let context = context(&repos);
        assert_eq!(
            LayoutTemplate::parse("{language}/{repo}").unwrap().render(&repos[0], &context, 0),
            "go-template/tool"
        );
        assert_eq!(
            LayoutTemplate::parse("{topic:data-platform}/{repo}")
                .unwrap()
                .render(&repos[0], &context, 0),
            "data-platform/tool"
        );
        // A topic the repo does not carry collapses instead of inventing a folder.
        assert_eq!(
            LayoutTemplate::parse("{topic:absent}/{repo}").unwrap().render(&repos[0], &context, 0),
            "tool"
        );
    }

    #[test]
    fn slugify_refuses_traversal_and_separators() {
        assert_eq!(slugify(".."), "");
        assert_eq!(slugify("a/b"), "a-b");
        assert_eq!(slugify("  C#  "), "c");
        assert_eq!(slugify("---"), "");
    }

    #[test]
    fn plan_splits_clone_keep_and_move() {
        let root = Path::new("/root");
        let repos = vec![
            repo("acme", "absent"),
            cloned("acme", "here", "/root/here"),
            cloned("acme", "elsewhere", "/other/elsewhere"),
        ];
        let context = context(&repos);
        let plan = plan(&repos, &all(&repos), &LayoutTemplate::default(), &context, root);

        assert_eq!(plan.rows[0].action, Action::Clone);
        assert_eq!(plan.rows[0].dest, PathBuf::from("/root/absent"));
        assert_eq!(plan.rows[1].action, Action::Keep);
        assert_eq!(plan.rows[2].action, Action::Move { from: PathBuf::from("/other/elsewhere") });
        assert_eq!(plan.rows[2].dest, PathBuf::from("/root/elsewhere"));

        let counts = plan.counts();
        assert_eq!((counts.clone_rows, counts.keep, counts.moves, counts.skipped), (1, 1, 1, 0));
        assert_eq!(counts.total(), 3);
    }

    #[test]
    fn two_owners_sharing_a_name_collide_and_neither_wins() {
        // The case the current coverage clone hides: both would land on /root/shared.
        let root = Path::new("/root");
        let repos = vec![repo("one", "shared"), repo("two", "shared")];
        let context = context(&repos);
        let collided = plan(&repos, &all(&repos), &LayoutTemplate::default(), &context, root);
        assert_eq!(collided.rows[0].action, Action::Skip(SkipReason::Collision("two/shared".into())));
        assert_eq!(collided.rows[1].action, Action::Skip(SkipReason::Collision("one/shared".into())));
        assert_eq!(collided.counts().skipped, 2);

        // Adding {owner} to the layout resolves it.
        let template = LayoutTemplate::parse("{owner}/{repo}").unwrap();
        let owned = plan(&repos, &all(&repos), &template, &context, root);
        assert_eq!(owned.counts().skipped, 0);
        assert_eq!(owned.rows[0].dest, PathBuf::from("/root/one/shared"));
    }

    #[test]
    fn a_repo_keeps_its_place_only_when_the_path_matches_exactly() {
        let root = Path::new("/root");
        let repos = vec![cloned("acme", "billing", "/root/old-area/billing")];
        let context = context(&repos);
        let template = LayoutTemplate::parse("{repo}").unwrap();
        let plan = plan(&repos, &all(&repos), &template, &context, root);
        assert_eq!(plan.rows[0].action, Action::Move { from: "/root/old-area/billing".into() });
    }

    #[test]
    fn tree_is_built_from_planned_paths() {
        let root = Path::new("/root");
        let repos = vec![repo("acme", "billing"), repo("acme", "tf-billing"), repo("acme", "solo")];
        let context = context(&repos);
        let template = LayoutTemplate::parse("{project}/{repo}").unwrap();
        let plan = plan(&repos, &all(&repos), &template, &context, root);
        let nodes = plan.tree();
        // One folder node ("billing") holding the two clustered repos; the singleton is at the root.
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].rel_path, "billing");
        assert_eq!(nodes[0].repos.len(), 2);
    }
}
