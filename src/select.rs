//! Selector engine: which repos a rule picks out of an owner's listing.
//!
//! Name-first by design. Across a real 991-repo org the name is populated for 100% of repos while
//! the description is empty for 35.8%, topics for 63.9% and the language is null for 9.2% — so
//! every other axis is a filter layered on top of a name-based selection, never the substrate.
//!
//! Glob matching delegates to [`crate::groups::wildcard_match`] so a `pattern` in `groups.json` and
//! a glob in a selector expression mean exactly the same thing.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use crate::groups::wildcard_match;

/// Role prefixes: the leading token says what the repo *is*, the rest names the project. Measured
/// at 30.8% of a real org, dominated by the IaC and GitOps pair.
pub const ROLE_PREFIXES: &[&str] = &[
    "tf", "terraform", "argocd", "gitops", "docker", "helm", "chart", "charts", "k8s", "kube",
    "kubernetes", "ansible",
];

/// Role suffixes: the trailing token says what the repo *is*. Compared after plural folding, since
/// `-service`/`-services` and `-integration`/`-integrations` both occur and treating them as
/// distinct silently misses ~30% of each family.
pub const ROLE_SUFFIXES: &[&str] = &[
    "service", "api", "lib", "library", "sdk", "client", "worker", "app", "front", "frontend",
    "backend", "ui", "web", "mfe", "infra", "chart", "helm", "tf", "terraform", "proto", "e2e",
    "test", "doc", "config", "migration", "cli", "deploy", "deployment", "integration", "job",
    "consumer", "producer", "exporter", "gateway", "proxy", "plugin", "tool", "util", "common",
];

/// Cores too generic to be a project. Half of all clusters found by naive stemming have a single
/// generic token as their core, and those absorb 55% of everything clustered — a fake "project"
/// merging unrelated platform repos. Rejecting them costs recall and buys precision.
pub const GENERIC_CORES: &[&str] = &[
    "infra", "module", "modules", "common", "shared", "core", "platform", "aws", "gcp", "azure",
    "k8s", "base", "test", "tests", "demo", "poc", "template", "templates", "utils", "tools",
    "example", "examples", "sandbox", "legacy", "misc",
];

/// Everything a selector can ask about one repo. Built from an owner listing plus the local scan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoFacts {
    pub owner: String,
    pub name: String,
    pub topics: Vec<String>,
    pub language: Option<String>,
    pub is_fork: bool,
    pub is_archived: bool,
    pub private: bool,
    /// GitHub's reported size in kilobytes; 0 when unknown.
    pub size_kb: u64,
    /// Absolute path of the local clone, when one exists under the scan roots.
    pub local_path: Option<std::path::PathBuf>,
}

impl RepoFacts {
    /// `owner/name`, the form an explicit list and a manifest both use.
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    /// Whether a local clone exists. Derived, so "cloned" and "where" can never disagree.
    pub fn cloned(&self) -> bool {
        self.local_path.is_some()
    }
}

/// A boolean property, matched by `is:<flag>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoFlag {
    Archived,
    Fork,
    Private,
    Cloned,
}

impl RepoFlag {
    fn parse(word: &str) -> Option<Self> {
        match word {
            "archived" => Some(RepoFlag::Archived),
            "fork" | "forked" => Some(RepoFlag::Fork),
            "private" => Some(RepoFlag::Private),
            "cloned" | "local" => Some(RepoFlag::Cloned),
            _ => None,
        }
    }
}

/// A compiled regex that stays `Clone`/`Eq` by comparing its source, so `Selector` derives cleanly.
#[derive(Debug, Clone)]
pub struct Pattern {
    source: String,
    regex: Arc<regex::Regex>,
}

impl Pattern {
    pub fn new(source: &str) -> Result<Self, String> {
        let regex = regex::Regex::new(source).map_err(|err| format!("bad regex: {err}"))?;
        Ok(Self { source: source.to_string(), regex: Arc::new(regex) })
    }

    fn is_match(&self, text: &str) -> bool {
        self.regex.is_match(text)
    }
}

impl PartialEq for Pattern {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
    }
}

impl Eq for Pattern {}

/// One selectable dimension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Axis {
    /// Case-insensitive `*` glob over the name.
    Glob(String),
    /// Case-insensitive substring of the name.
    Contains(String),
    Regex(Pattern),
    /// The name's leading hyphen-tokens equal these tokens.
    Prefix(String),
    /// The name's trailing hyphen-token equals this one, after plural folding.
    Suffix(String),
    /// Any hyphen-token of the name equals this one.
    Token(String),
    Topic(String),
    Language(String),
    Owner(String),
    Flag(RepoFlag),
    /// Explicit membership by `name` or `owner/name`.
    List(Vec<String>),
}

/// A composed selection rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    Any(Vec<Selector>),
    All(Vec<Selector>),
    Not(Box<Selector>),
    Axis(Axis),
}

impl Selector {
    /// The rule that picks everything — an empty `All`, since every member must hold.
    pub fn everything() -> Self {
        Selector::All(Vec::new())
    }
}

// ---------------------------------------------------------------------------------------------
// Name tokenization
// ---------------------------------------------------------------------------------------------

/// Split a repo name into its lowercase parts. Hyphens dominate (95.1% of a real org) but
/// underscores occur, so both separate.
pub fn tokens(name: &str) -> Vec<String> {
    name.split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| part.to_lowercase())
        .collect()
}

/// Fold a trailing plural so `-service` and `-services` compare equal. Only `es`/`s`, and never
/// down to nothing.
pub fn fold_plural(token: &str) -> String {
    let lower = token.to_lowercase();
    // A single trailing `s`, and never off a `ss` word or down to nothing. Stripping `es` as a unit
    // is wrong here: `services` would fold to `servic`, which matches nothing.
    if lower.ends_with("ss") {
        return lower;
    }
    match lower.strip_suffix('s') {
        Some(stem) if !stem.is_empty() => stem.to_string(),
        _ => lower,
    }
}

/// The grouping key formed by a name's first `depth` tokens. Depth matters: 30 two-token prefixes
/// in a real org have 3+ members (the largest 26), so depth 1 over-merges those while depth 2
/// splits genuine single projects — which is why it is a live control rather than a constant.
pub fn prefix_key(name: &str, depth: usize) -> String {
    let parts = tokens(name);
    let take = depth.max(1).min(parts.len());
    parts[..take].join("-")
}

/// Strip one leading role prefix and one trailing role suffix, leaving the project stem. Never
/// strips down to nothing — a name that is entirely role words keeps its last token.
pub fn core(name: &str) -> String {
    let mut parts = tokens(name);
    if parts.len() > 1 && ROLE_PREFIXES.contains(&parts[0].as_str()) {
        parts.remove(0);
    }
    if parts.len() > 1 {
        let last = fold_plural(&parts[parts.len() - 1]);
        if ROLE_SUFFIXES.iter().any(|role| fold_plural(role) == last) {
            parts.pop();
        }
    }
    parts.join("-")
}

/// Whether a name carries a leading role prefix — i.e. it describes infrastructure for some project
/// rather than being the project itself.
pub fn has_role_prefix(name: &str) -> bool {
    let parts = tokens(name);
    parts.len() > 1 && ROLE_PREFIXES.contains(&parts[0].as_str())
}

// ---------------------------------------------------------------------------------------------
// Matching
// ---------------------------------------------------------------------------------------------

fn axis_matches(repo: &RepoFacts, axis: &Axis) -> bool {
    match axis {
        Axis::Glob(pattern) => wildcard_match(pattern, &repo.name),
        Axis::Contains(needle) => repo.name.to_lowercase().contains(&needle.to_lowercase()),
        Axis::Regex(pattern) => pattern.is_match(&repo.name),
        Axis::Prefix(prefix) => {
            let wanted = tokens(prefix);
            let actual = tokens(&repo.name);
            !wanted.is_empty() && actual.len() >= wanted.len() && actual[..wanted.len()] == wanted[..]
        }
        Axis::Suffix(suffix) => {
            let wanted = fold_plural(suffix);
            tokens(&repo.name).last().map(|last| fold_plural(last) == wanted).unwrap_or(false)
        }
        Axis::Token(token) => {
            let wanted = token.to_lowercase();
            tokens(&repo.name).contains(&wanted)
        }
        Axis::Topic(topic) => {
            let wanted = topic.to_lowercase();
            repo.topics.iter().any(|owned| owned.to_lowercase() == wanted)
        }
        Axis::Language(language) => repo
            .language
            .as_deref()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(language)),
        Axis::Owner(owner) => repo.owner.eq_ignore_ascii_case(owner),
        Axis::Flag(flag) => match flag {
            RepoFlag::Archived => repo.is_archived,
            RepoFlag::Fork => repo.is_fork,
            RepoFlag::Private => repo.private,
            RepoFlag::Cloned => repo.cloned(),
        },
        Axis::List(entries) => entries.iter().any(|entry| {
            entry.eq_ignore_ascii_case(&repo.name) || entry.eq_ignore_ascii_case(&repo.slug())
        }),
    }
}

/// Whether a repo satisfies the rule. An empty `All` matches everything; an empty `Any` matches
/// nothing — the usual identities, so a rule built up from no terms selects the whole listing.
pub fn matches(repo: &RepoFacts, selector: &Selector) -> bool {
    match selector {
        Selector::Any(parts) => parts.iter().any(|part| matches(repo, part)),
        Selector::All(parts) => parts.iter().all(|part| matches(repo, part)),
        Selector::Not(inner) => !matches(repo, inner),
        Selector::Axis(axis) => axis_matches(repo, axis),
    }
}

/// Indices of the repos a rule selects, in listing order.
pub fn select(repos: &[RepoFacts], selector: &Selector) -> Vec<usize> {
    repos
        .iter()
        .enumerate()
        .filter(|(_, repo)| matches(repo, selector))
        .map(|(index, _)| index)
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Expression parsing
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    LParen,
    RParen,
    Word(String),
}

fn tokenize(expr: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in expr.chars() {
        match ch {
            '(' | ')' => {
                if !current.is_empty() {
                    out.push(Token::Word(std::mem::take(&mut current)));
                }
                out.push(if ch == '(' { Token::LParen } else { Token::RParen });
            }
            _ if ch.is_whitespace() => {
                if !current.is_empty() {
                    out.push(Token::Word(std::mem::take(&mut current)));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        out.push(Token::Word(current));
    }
    out
}

/// One `key:value` or bare term. A bare word containing `*` is a glob, otherwise a substring —
/// matching the filter grammar the coverage panel already taught.
fn parse_term(word: &str) -> Result<Selector, String> {
    if let Some(rest) = word.strip_prefix('-') {
        if rest.is_empty() {
            return Err("bare '-' is not a term".to_string());
        }
        return Ok(Selector::Not(Box::new(parse_term(rest)?)));
    }
    let axis = match word.split_once(':') {
        Some(("topic", value)) => Axis::Topic(require(value, "topic")?),
        Some(("lang" | "language", value)) => Axis::Language(require(value, "language")?),
        Some(("owner" | "org", value)) => Axis::Owner(require(value, "owner")?),
        Some(("prefix", value)) => Axis::Prefix(require(value, "prefix")?),
        Some(("suffix", value)) => Axis::Suffix(require(value, "suffix")?),
        Some(("token" | "word", value)) => Axis::Token(require(value, "token")?),
        Some(("re" | "regex", value)) => Axis::Regex(Pattern::new(&require(value, "regex")?)?),
        Some(("is", value)) => {
            let value = require(value, "is")?.to_lowercase();
            match RepoFlag::parse(&value) {
                Some(flag) => Axis::Flag(flag),
                None if value == "missing" => {
                    return Ok(Selector::Not(Box::new(Selector::Axis(Axis::Flag(RepoFlag::Cloned)))));
                }
                None => return Err(format!("unknown is:{value}")),
            }
        }
        Some(("list", value)) => Axis::List(
            require(value, "list")?.split(',').filter(|entry| !entry.is_empty()).map(String::from).collect(),
        ),
        Some((key, _)) => return Err(format!("unknown selector '{key}:'")),
        None if word.contains('*') => Axis::Glob(word.to_string()),
        None => Axis::Contains(word.to_string()),
    };
    Ok(Selector::Axis(axis))
}

fn require(value: &str, key: &str) -> Result<String, String> {
    if value.is_empty() {
        Err(format!("{key}: needs a value"))
    } else {
        Ok(value.to_string())
    }
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn keyword(&self) -> Option<String> {
        match self.peek() {
            Some(Token::Word(word)) => Some(word.to_uppercase()),
            _ => None,
        }
    }

    fn parse_or(&mut self) -> Result<Selector, String> {
        let mut parts = vec![self.parse_and()?];
        while matches!(self.keyword().as_deref(), Some("OR" | "|" | "||")) {
            self.position += 1;
            parts.push(self.parse_and()?);
        }
        Ok(if parts.len() == 1 { parts.remove(0) } else { Selector::Any(parts) })
    }

    fn parse_and(&mut self) -> Result<Selector, String> {
        let mut parts = vec![self.parse_unary()?];
        loop {
            match self.keyword().as_deref() {
                Some("AND" | "&" | "&&") => {
                    self.position += 1;
                    parts.push(self.parse_unary()?);
                }
                Some("OR" | "|" | "||") | None => break,
                _ if matches!(self.peek(), Some(Token::RParen)) => break,
                // Adjacent terms are an implicit AND.
                _ => parts.push(self.parse_unary()?),
            }
        }
        Ok(if parts.len() == 1 { parts.remove(0) } else { Selector::All(parts) })
    }

    fn parse_unary(&mut self) -> Result<Selector, String> {
        if matches!(self.keyword().as_deref(), Some("NOT" | "!")) {
            self.position += 1;
            return Ok(Selector::Not(Box::new(self.parse_unary()?)));
        }
        match self.peek().cloned() {
            Some(Token::LParen) => {
                self.position += 1;
                let inner = self.parse_or()?;
                match self.peek() {
                    Some(Token::RParen) => {
                        self.position += 1;
                        Ok(inner)
                    }
                    _ => Err("unclosed '('".to_string()),
                }
            }
            Some(Token::RParen) => Err("unexpected ')'".to_string()),
            Some(Token::Word(word)) => {
                self.position += 1;
                parse_term(&word)
            }
            None => Err("expression ends early".to_string()),
        }
    }
}

/// Parse a selector expression. `OR`/`AND`/`NOT` (and `|`/`&`/`!`) with parentheses; adjacent terms
/// are an implicit AND; a leading `-` negates one term. An empty expression selects everything.
pub fn parse(expr: &str) -> Result<Selector, String> {
    let tokens = tokenize(expr);
    if tokens.is_empty() {
        return Ok(Selector::everything());
    }
    let mut parser = Parser { tokens, position: 0 };
    let selector = parser.parse_or()?;
    if parser.position != parser.tokens.len() {
        return Err("trailing input after the expression".to_string());
    }
    Ok(selector)
}

// ---------------------------------------------------------------------------------------------
// Project clustering
// ---------------------------------------------------------------------------------------------

/// Knobs for stem clustering. The stopword list is the precision/recall dial.
#[derive(Debug, Clone)]
pub struct ClusterOpts {
    pub stopwords: Vec<String>,
    /// Reject a cluster whose core is a single token shorter than this (a 2-3 letter core collides).
    pub min_short_core: usize,
}

impl Default for ClusterOpts {
    fn default() -> Self {
        Self {
            stopwords: GENERIC_CORES.iter().map(|word| word.to_string()).collect(),
            min_short_core: 4,
        }
    }
}

/// A set of repos sharing a project stem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cluster {
    pub core: String,
    /// Indices into the slice passed to [`clusters`], in listing order.
    pub members: Vec<usize>,
    /// At least one member has no role prefix, so the project exists as a repo and not only as
    /// infrastructure naming. 54% of role-prefixed repos in a real org have no such anchor.
    pub anchored: bool,
}

/// Whether a core is too generic to name a project.
fn core_rejected(core: &str, opts: &ClusterOpts) -> bool {
    if core.is_empty() {
        return true;
    }
    let parts = tokens(core);
    if parts.len() == 1 {
        let single = &parts[0];
        if single.len() < opts.min_short_core {
            return true;
        }
        if opts.stopwords.iter().any(|word| word == single) {
            return true;
        }
    }
    false
}

/// Group repos into project clusters by stem, dropping singletons and generic cores. Sorted by
/// descending size then core, so the biggest real projects lead.
pub fn clusters(repos: &[RepoFacts], opts: &ClusterOpts) -> Vec<Cluster> {
    let mut by_core: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, repo) in repos.iter().enumerate() {
        by_core.entry(core(&repo.name)).or_default().push(index);
    }
    let mut out: Vec<Cluster> = by_core
        .into_iter()
        .filter(|(stem, members)| members.len() > 1 && !core_rejected(stem, opts))
        .map(|(stem, members)| {
            let anchored = members.iter().any(|&index| !has_role_prefix(&repos[index].name));
            Cluster { core: stem, members, anchored }
        })
        .collect();
    out.sort_by(|left, right| {
        right.members.len().cmp(&left.members.len()).then_with(|| left.core.cmp(&right.core))
    });
    out
}

/// Expand a seed selection to every repo sharing a project stem with it. This is the shape the data
/// supports: 71.7% of repos are cluster singletons, so clustering works as an operator over a seed
/// and would be a poor primary grouping.
pub fn expand_siblings(seed: &[usize], repos: &[RepoFacts], opts: &ClusterOpts) -> Vec<usize> {
    let wanted: HashSet<String> = seed
        .iter()
        .filter_map(|&index| repos.get(index))
        .map(|repo| core(&repo.name))
        .filter(|stem| !core_rejected(stem, opts))
        .collect();
    let mut chosen: HashSet<usize> = seed.iter().copied().collect();
    for (index, repo) in repos.iter().enumerate() {
        if wanted.contains(&core(&repo.name)) {
            chosen.insert(index);
        }
    }
    let mut out: Vec<usize> = chosen.into_iter().collect();
    out.sort_unstable();
    out
}

// ---------------------------------------------------------------------------------------------
// Axis usefulness
// ---------------------------------------------------------------------------------------------

/// How much signal the topic axis carries for an owner. Two real orgs sit at opposite ends — 36%
/// tagged over 38 distinct topics versus 100% tagged over 208 — so a fixed default is wrong for one
/// of them and the UI orders its axes by what actually discriminates here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Regime {
    /// Widely tagged with a varied vocabulary — topic is a good primary axis.
    Rich,
    /// Mostly untagged — topic is a fallback.
    Sparse,
    /// Tagged, but with a handful of machine-generated markers — useful as a flag, not a selector.
    Degenerate,
}

/// Coverage of one axis over a listing, for ordering the axis menu and labelling it honestly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisStats {
    pub populated: usize,
    pub total: usize,
    pub distinct: usize,
}

impl AxisStats {
    pub fn percent(&self) -> u32 {
        (self.populated * 100).checked_div(self.total).unwrap_or(0) as u32
    }
}

/// Topic coverage plus the regime it implies.
pub fn topic_stats(repos: &[RepoFacts]) -> (AxisStats, Regime) {
    let mut distinct: HashSet<String> = HashSet::new();
    let mut populated = 0usize;
    for repo in repos {
        if !repo.topics.is_empty() {
            populated += 1;
        }
        for topic in &repo.topics {
            distinct.insert(topic.to_lowercase());
        }
    }
    let stats = AxisStats { populated, total: repos.len(), distinct: distinct.len() };
    let regime = if stats.percent() >= 80 && stats.distinct >= 40 {
        Regime::Rich
    } else if stats.percent() >= 50 && stats.distinct < 10 {
        Regime::Degenerate
    } else {
        Regime::Sparse
    };
    (stats, regime)
}

/// Language coverage. Reported alongside the prefix families because in a real org the two are ~86%
/// redundant — selecting by language mostly re-derives a prefix selection.
pub fn language_stats(repos: &[RepoFacts]) -> AxisStats {
    let mut distinct: HashSet<String> = HashSet::new();
    let mut populated = 0usize;
    for repo in repos {
        if let Some(language) = repo.language.as_deref() {
            populated += 1;
            distinct.insert(language.to_lowercase());
        }
    }
    AxisStats { populated, total: repos.len(), distinct: distinct.len() }
}

/// The prefix families at a given token depth, largest first, excluding one-member groups.
pub fn prefix_families(repos: &[RepoFacts], depth: usize) -> Vec<(String, usize)> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for repo in repos {
        *counts.entry(prefix_key(&repo.name, depth)).or_insert(0) += 1;
    }
    let mut out: Vec<(String, usize)> =
        counts.into_iter().filter(|(_, count)| *count > 1).collect();
    out.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(name: &str) -> RepoFacts {
        RepoFacts { owner: "acme".into(), name: name.into(), ..Default::default() }
    }

    fn tagged(name: &str, topics: &[&str]) -> RepoFacts {
        RepoFacts {
            topics: topics.iter().map(|topic| topic.to_string()).collect(),
            ..repo(name)
        }
    }

    #[test]
    fn tokens_split_on_both_separators() {
        assert_eq!(tokens("tf-billing_core"), vec!["tf", "billing", "core"]);
        assert_eq!(tokens("Solo"), vec!["solo"]);
        assert_eq!(tokens("--a--"), vec!["a"]);
    }

    #[test]
    fn plural_folding_is_conservative() {
        assert_eq!(fold_plural("services"), "service");
        assert_eq!(fold_plural("service"), "service");
        assert_eq!(fold_plural("protos"), "proto");
        assert_eq!(fold_plural("charts"), "chart");
        assert_eq!(fold_plural("integrations"), "integration");
        // Never folds a word away entirely, and leaves a genuine `ss` ending alone.
        assert_eq!(fold_plural("s"), "s");
        assert_eq!(fold_plural("class"), "class");
    }

    #[test]
    fn prefix_key_depth_changes_the_grouping() {
        // The over-merge / over-split boundary: at depth 1 these share a family, at depth 2 they
        // split into two — which is why depth is a live control and not a constant.
        assert_eq!(prefix_key("data-ingest-worker", 1), "data");
        assert_eq!(prefix_key("data-export-worker", 1), "data");
        assert_eq!(prefix_key("data-ingest-worker", 2), "data-ingest");
        assert_eq!(prefix_key("data-export-worker", 2), "data-export");
        // Depth beyond the token count is clamped, and depth 0 behaves as 1.
        assert_eq!(prefix_key("solo", 9), "solo");
        assert_eq!(prefix_key("data-ingest", 0), "data");
    }

    #[test]
    fn core_strips_one_affix_from_each_end() {
        assert_eq!(core("billing"), "billing");
        assert_eq!(core("billing-service"), "billing");
        assert_eq!(core("tf-billing"), "billing");
        assert_eq!(core("argocd-billing"), "billing");
        assert_eq!(core("tf-billing-service"), "billing");
        // Plural suffix folds to the same core.
        assert_eq!(core("billing-services"), "billing");
        // Only ONE prefix and ONE suffix come off.
        assert_eq!(core("tf-tf-billing"), "tf-billing");
        // Never strips to nothing.
        assert_eq!(core("tf-service"), "service");
        assert_eq!(core("service"), "service");
    }

    #[test]
    fn the_scattered_family_clusters_together() {
        // The exact family a role-bucket classifier splits four ways.
        let repos = vec![
            repo("billing"),
            repo("billing-service"),
            repo("argocd-billing"),
            repo("tf-billing"),
            repo("unrelated"),
        ];
        let found = clusters(&repos, &ClusterOpts::default());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].core, "billing");
        assert_eq!(found[0].members, vec![0, 1, 2, 3]);
        assert!(found[0].anchored);
    }

    #[test]
    fn generic_cores_are_rejected() {
        let repos = vec![repo("tf-infra"), repo("argocd-infra"), repo("helm-infra")];
        assert!(clusters(&repos, &ClusterOpts::default()).is_empty());
        // With the stopword removed the same input does cluster — the list is the whole dial.
        let opts = ClusterOpts { stopwords: Vec::new(), ..ClusterOpts::default() };
        assert_eq!(clusters(&repos, &opts).len(), 1);
    }

    #[test]
    fn short_single_token_cores_are_rejected() {
        let repos = vec![repo("tf-abc"), repo("argocd-abc")];
        assert!(clusters(&repos, &ClusterOpts::default()).is_empty());
    }

    #[test]
    fn unanchored_clusters_are_marked() {
        let repos = vec![repo("tf-billing"), repo("argocd-billing")];
        let found = clusters(&repos, &ClusterOpts::default());
        assert_eq!(found.len(), 1);
        assert!(!found[0].anchored, "no repo without a role prefix means no anchor");
    }

    #[test]
    fn expand_siblings_pulls_in_the_family_and_nothing_else() {
        let repos = vec![
            repo("billing"),
            repo("tf-billing"),
            repo("argocd-billing"),
            repo("shipping"),
        ];
        assert_eq!(expand_siblings(&[0], &repos, &ClusterOpts::default()), vec![0, 1, 2]);
        // A seed whose core is generic expands to itself only.
        let generic = vec![repo("tf-infra"), repo("argocd-infra")];
        assert_eq!(expand_siblings(&[0], &generic, &ClusterOpts::default()), vec![0]);
    }

    #[test]
    fn axis_matching() {
        let mut subject = tagged("tf-billing-service", &["Terraform", "iac"]);
        subject.language = Some("HCL".into());
        subject.is_archived = true;

        assert!(axis_matches(&subject, &Axis::Glob("tf-*".into())));
        assert!(!axis_matches(&subject, &Axis::Glob("argocd-*".into())));
        assert!(axis_matches(&subject, &Axis::Contains("BILL".into())));
        assert!(axis_matches(&subject, &Axis::Prefix("tf".into())));
        assert!(axis_matches(&subject, &Axis::Prefix("tf-billing".into())));
        assert!(!axis_matches(&subject, &Axis::Prefix("billing".into())));
        // Suffix folds plurals in both directions.
        assert!(axis_matches(&subject, &Axis::Suffix("services".into())));
        assert!(axis_matches(&subject, &Axis::Token("billing".into())));
        assert!(!axis_matches(&subject, &Axis::Token("bill".into())));
        assert!(axis_matches(&subject, &Axis::Topic("terraform".into())));
        assert!(axis_matches(&subject, &Axis::Language("hcl".into())));
        assert!(axis_matches(&subject, &Axis::Owner("ACME".into())));
        assert!(axis_matches(&subject, &Axis::Flag(RepoFlag::Archived)));
        assert!(!axis_matches(&subject, &Axis::Flag(RepoFlag::Cloned)));
        assert!(axis_matches(&subject, &Axis::List(vec!["acme/tf-billing-service".into()])));
        assert!(axis_matches(&subject, &Axis::List(vec!["tf-billing-service".into()])));
        assert!(axis_matches(&subject, &Axis::Regex(Pattern::new("^tf-").unwrap())));
    }

    #[test]
    fn empty_expression_selects_everything() {
        let repos = vec![repo("a"), repo("b")];
        let selector = parse("").unwrap();
        assert_eq!(select(&repos, &selector), vec![0, 1]);
    }

    #[test]
    fn parses_boolean_composition() {
        let repos = vec![
            RepoFacts { is_archived: true, ..repo("tf-billing") },
            repo("tf-shipping"),
            tagged("web-app", &["frontend"]),
        ];
        let selector = parse("tf-* NOT is:archived").unwrap();
        assert_eq!(select(&repos, &selector), vec![1]);

        let selector = parse("tf-* OR topic:frontend").unwrap();
        assert_eq!(select(&repos, &selector), vec![0, 1, 2]);

        let selector = parse("(tf-* OR topic:frontend) AND -is:archived").unwrap();
        assert_eq!(select(&repos, &selector), vec![1, 2]);
    }

    #[test]
    fn adjacent_terms_are_an_implicit_and() {
        let repos = vec![repo("tf-billing"), repo("tf-shipping")];
        let selector = parse("tf-* billing").unwrap();
        assert_eq!(select(&repos, &selector), vec![0]);
    }

    #[test]
    fn is_missing_is_the_negation_of_cloned() {
        let repos =
            vec![RepoFacts { local_path: Some("/tmp/here".into()), ..repo("here") }, repo("absent")];
        assert_eq!(select(&repos, &parse("is:missing").unwrap()), vec![1]);
        assert_eq!(select(&repos, &parse("is:cloned").unwrap()), vec![0]);
    }

    #[test]
    fn parse_errors_are_specific() {
        assert!(parse("topic:").unwrap_err().contains("needs a value"));
        assert!(parse("bogus:x").unwrap_err().contains("unknown selector"));
        assert!(parse("is:nonsense").unwrap_err().contains("unknown is:"));
        assert!(parse("(tf-*").unwrap_err().contains("unclosed"));
        assert!(parse("tf-*)").unwrap_err().contains("trailing input"));
        assert!(parse("re:[").unwrap_err().contains("bad regex"));
        assert!(parse("NOT").unwrap_err().contains("ends early"));
    }

    #[test]
    fn topic_regimes_separate_the_two_real_shapes() {
        // Sparse: most repos untagged, a tiny vocabulary.
        let sparse: Vec<RepoFacts> = (0..10)
            .map(|index| if index < 4 { tagged("a", &["iac"]) } else { repo("b") })
            .collect();
        let (stats, regime) = topic_stats(&sparse);
        assert_eq!(stats.percent(), 40);
        assert_eq!(regime, Regime::Sparse);

        // Degenerate: widely tagged, but with a handful of machine-generated markers.
        let degenerate: Vec<RepoFacts> = (0..10).map(|_| tagged("a", &["iac", "code"])).collect();
        assert_eq!(topic_stats(&degenerate).1, Regime::Degenerate);

        // Rich: widely tagged with a varied vocabulary.
        let rich: Vec<RepoFacts> = (0..60)
            .map(|index| {
                let owned = format!("topic{index}");
                RepoFacts { topics: vec![owned], ..repo("a") }
            })
            .collect();
        assert_eq!(topic_stats(&rich).1, Regime::Rich);
    }

    #[test]
    fn prefix_families_rank_by_size_and_drop_singletons() {
        let repos =
            vec![repo("tf-a"), repo("tf-b"), repo("tf-c"), repo("web-a"), repo("web-b"), repo("solo")];
        let families = prefix_families(&repos, 1);
        assert_eq!(families, vec![("tf".to_string(), 3), ("web".to_string(), 2)]);
    }
}
