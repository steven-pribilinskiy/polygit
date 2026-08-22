//! Headless report subcommands: `list`, `status`, `dirty`, `branches`, `sizes`.
//!
//! Each prints a one-shot overview of the repos under the scan roots and exits — no TUI.
//! Colors are emitted only when stdout is a TTY, so piped output stays clean.

use std::collections::HashSet;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use futures::stream::{self, StreamExt};

/// Concurrent per-repo git calls; deliberately modest so a big scan stays polite.
const REPORT_JOBS: usize = 8;

/// Concurrent repos for `sizes`. Each size walk is itself multi-threaded (saturates the cores),
/// so a handful in flight is optimal — more just oversubscribes. Benchmarked ~2x faster than the
/// old single-threaded walk on a tree with one dominant repo.
const SIZE_JOBS: usize = 3;

/// Width (in cells) of the `sizes` progress bar shown on a stderr TTY.
const PROGRESS_WIDTH: usize = 20;

const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const ITALIC: &str = "\x1b[3m";
const BOLD_CYAN: &str = "\x1b[1;36m";
const RESET: &str = "\x1b[0m";

/// Discovery args shared by the report subcommands (mirrors the top-level scan args, which
/// `args_conflicts_with_subcommands` makes unavailable to subcommands).
#[derive(clap::Args, Debug)]
pub struct ScanArgs {
    /// Directories to scan — each may itself be a single repo. With none, scans the
    /// current directory. (Use `-w <name>` to use a saved workspace instead.)
    pub dirs: Vec<PathBuf>,

    /// Use a saved workspace's folders as the scan roots.
    #[arg(short = 'w', long, value_name = "NAME")]
    pub workspace: Option<String>,

    /// Max directory depth to scan for repos (1 = immediate subdirs only)
    #[arg(long, value_name = "N", default_value = "16")]
    pub depth: usize,

    /// Scan only the immediate subdirectories (same as --depth 1)
    #[arg(long)]
    pub no_recursive: bool,
}

impl ScanArgs {
    /// Effective scan depth: `--no-recursive` forces 1; `--depth 0` is floored to 1.
    pub fn max_depth(&self) -> usize {
        if self.no_recursive { 1 } else { self.depth.max(1) }
    }
}

struct Repo {
    name: String,
    path: PathBuf,
}

/// Discover every repo under the roots (deduped across overlapping roots), named relative
/// to the root each was found under, sorted by name.
async fn discover(roots: &[PathBuf], max_depth: usize) -> Result<Vec<Repo>> {
    let mut repos: Vec<Repo> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for root in roots {
        for path in crate::git::discover_repos_recursive(root, max_depth).await? {
            if seen.insert(path.clone()) {
                repos.push(Repo { name: crate::git::relative_path(root, &path), path });
            }
        }
    }
    repos.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(repos)
}

fn stdout_color() -> bool {
    io::stdout().is_terminal()
}

/// Wrap `text` in an ANSI color code when `color` is on; pass through untouched otherwise.
fn paint(text: &str, code: &str, color: bool) -> String {
    if color { format!("{code}{text}{RESET}") } else { text.to_string() }
}

/// Repo-name column width. Formatting width counts chars, so measure chars (not bytes).
fn name_pad(repos: &[Repo]) -> usize {
    repos.iter().map(|repo| repo.name.chars().count()).max().unwrap_or(0)
}

/// One command in the help screen: `(primary, comma-separated aliases, description)`.
type HelpRow = (&'static str, &'static str, &'static str);

/// The command list shown by `polygit --help`, grouped into sections. Hand-written, so
/// `help_lists_every_subcommand` (main.rs) holds it to the clap definition — without that guard a
/// new subcommand is simply absent from `polygit --help` and nobody notices.
pub(crate) const HELP_SECTIONS: &[(&str, &[HelpRow])] = &[
    ("Reports", &[
        ("list", "ls", "List every repo with its current branch"),
        ("status", "", "Show uncommitted changes for each dirty repo"),
        ("dirty", "", "Print the names of repos with uncommitted changes"),
        ("branches", "", "Branch + ahead/behind vs upstream, per repo"),
        ("sizes", "", "Disk usage per repo, largest first"),
        ("coverage", "missing, cov", "Which repos in each GitHub org aren't cloned locally"),
    ]),
    ("Layout", &[
        ("select", "sel", "Resolve a selector expression to the repos it picks"),
        ("plan", "", "Preview the directory layout a selector + template produce"),
        ("clone", "", "Clone the repos a selector picks, into the layout you choose"),
        ("orgs", "owners", "Your account, orgs and enterprises, with cloned counts"),
    ]),
    ("Workspaces", &[
        ("ws", "workspace, workspaces", "Manage & open saved workspaces (ws ls to list)"),
    ]),
    ("Maintenance", &[
        ("update", "upgrade", "Self-update to the latest release"),
    ]),
];

/// Visible width of a `primary, alias1, alias2` label (ANSI codes excluded — they're added later).
fn help_label_width(primary: &str, aliases: &str) -> usize {
    if aliases.is_empty() {
        primary.chars().count()
    } else {
        primary.chars().count() + 2 + aliases.chars().count()
    }
}

/// Print the top-level `--help`: a compact, sectioned command list with each primary name in cyan
/// and its aliases dimmed after it, descriptions aligned in a column. Colors only on a stdout TTY.
pub fn print_help() {
    let color = stdout_color();
    // Align descriptions to the widest label, capped so one long alias list can't push the whole
    // column far right; anything wider than the cap just gets a single space.
    let column = HELP_SECTIONS
        .iter()
        .flat_map(|(_, rows)| rows.iter())
        .map(|(primary, aliases, _)| help_label_width(primary, aliases))
        .max()
        .unwrap_or(0)
        .min(26);

    println!();
    println!(
        "  {} {} {}",
        paint("polygit", BOLD_CYAN, color),
        paint("—", DIM, color),
        paint("discover, status & pull many git repos", ITALIC, color),
    );
    println!();
    println!(
        "  {} polygit {}          scan & pull every repo (cwd if omitted)",
        paint("Usage:", BOLD, color),
        paint("[DIR...]", ITALIC, color),
    );
    println!("          polygit {} [args]", paint("<command>", ITALIC, color));

    for (title, rows) in HELP_SECTIONS {
        println!();
        println!("  {}", paint(title, BOLD, color));
        for (primary, aliases, desc) in *rows {
            let label = if aliases.is_empty() {
                paint(primary, CYAN, color)
            } else {
                format!("{}{}", paint(primary, CYAN, color), paint(&format!(", {aliases}"), DIM, color))
            };
            let gap = column.saturating_sub(help_label_width(primary, aliases)).max(1);
            println!("  {label}{}{}", " ".repeat(gap), paint(desc, DIM, color));
        }
    }

    println!();
    println!(
        "  {} polygit {} --help {}",
        paint("Run", BOLD, color),
        paint("<command>", ITALIC, color),
        paint("for details on any command.", DIM, color),
    );
    println!();
}

/// Render ahead/behind vs upstream: green `✓` when in sync, `↑N`/`↓N` for the nonzero
/// directions, or a dim `no upstream` when the branch tracks nothing.
fn format_track(ahead: Option<u32>, behind: Option<u32>, color: bool) -> String {
    match (ahead, behind) {
        (Some(0), Some(0)) => paint("✓", GREEN, color),
        (Some(ahead), Some(behind)) => {
            let mut parts = Vec::new();
            if ahead > 0 {
                parts.push(paint(&format!("↑{ahead}"), GREEN, color));
            }
            if behind > 0 {
                parts.push(paint(&format!("↓{behind}"), RED, color));
            }
            parts.join(" ")
        }
        _ => paint("no upstream", DIM, color),
    }
}

/// Largest first; ties break alphabetically so output is deterministic.
fn sort_sizes(rows: &mut [(String, u64)]) {
    rows.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
}

/// A fixed-width progress bar like `[████░░░░] 12/34 repos`. `total == 0` renders full
/// (nothing to do), avoiding a divide-by-zero.
fn progress_bar(done: usize, total: usize, width: usize) -> String {
    let filled = if total == 0 { width } else { (done * width / total).min(width) };
    let bar: String = "█".repeat(filled) + &"░".repeat(width - filled);
    format!("[{bar}] {done}/{total} repos")
}

/// `polygit list` — every repo with its current branch.
pub async fn run_list(roots: Vec<PathBuf>, max_depth: usize) -> Result<i32> {
    let repos = discover(&roots, max_depth).await?;
    if repos.is_empty() {
        println!("No git repositories found.");
        return Ok(0);
    }
    let color = stdout_color();
    let pad = name_pad(&repos);
    let rows: Vec<(String, String)> = stream::iter(repos)
        .map(|repo| async move {
            let branch = crate::git::get_branch(&repo.path).await.unwrap_or_else(|_| "?".into());
            (repo.name, branch)
        })
        .buffered(REPORT_JOBS)
        .collect()
        .await;
    for (name, branch) in rows {
        println!("{}  {branch}", paint(&format!("{name:<pad$}"), CYAN, color));
    }
    Ok(0)
}

/// `polygit status` — `git status --short` for each dirty repo.
pub async fn run_status(roots: Vec<PathBuf>, max_depth: usize) -> Result<i32> {
    let repos = discover(&roots, max_depth).await?;
    if repos.is_empty() {
        println!("No git repositories found.");
        return Ok(0);
    }
    let color = stdout_color();
    let rows: Vec<(String, String)> = stream::iter(repos)
        .map(|repo| async move {
            let status = crate::git::status_short(&repo.path, color).await;
            (repo.name, status)
        })
        .buffered(REPORT_JOBS)
        .collect()
        .await;
    let mut any_dirty = false;
    for (name, status) in rows {
        if status.is_empty() {
            continue;
        }
        if any_dirty {
            println!();
        }
        any_dirty = true;
        println!("{}", paint(&name, CYAN, color));
        for line in status.lines() {
            println!("  {line}");
        }
    }
    if !any_dirty {
        println!("All repos clean.");
    }
    Ok(0)
}

/// `polygit dirty` — names of repos with uncommitted changes (a grep-style filter:
/// silent when nothing is dirty, always exit 0).
pub async fn run_dirty(roots: Vec<PathBuf>, max_depth: usize) -> Result<i32> {
    let repos = discover(&roots, max_depth).await?;
    let color = stdout_color();
    let rows: Vec<(String, bool)> = stream::iter(repos)
        .map(|repo| async move {
            let dirty = crate::git::is_dirty(&repo.path).await.unwrap_or(false);
            (repo.name, dirty)
        })
        .buffered(REPORT_JOBS)
        .collect()
        .await;
    for (name, dirty) in rows {
        if dirty {
            println!("{}", paint(&name, CYAN, color));
        }
    }
    Ok(0)
}

/// `polygit branches` — branch plus ahead/behind vs upstream per repo.
pub async fn run_branches(roots: Vec<PathBuf>, max_depth: usize) -> Result<i32> {
    let repos = discover(&roots, max_depth).await?;
    if repos.is_empty() {
        println!("No git repositories found.");
        return Ok(0);
    }
    let color = stdout_color();
    let pad = name_pad(&repos);
    let rows: Vec<_> = stream::iter(repos)
        .map(|repo| async move {
            let (branch, track) = tokio::join!(
                crate::git::get_branch(&repo.path),
                crate::git::head_ahead_behind(&repo.path),
            );
            (repo.name, branch.unwrap_or_else(|_| "?".into()), track)
        })
        .buffered(REPORT_JOBS)
        .collect()
        .await;
    let branch_pad =
        rows.iter().map(|(_, branch, _)| branch.chars().count()).max().unwrap_or(0);
    for (name, branch, (ahead, behind)) in rows {
        println!(
            "{}  {branch:<branch_pad$}  {}",
            paint(&format!("{name:<pad$}"), CYAN, color),
            format_track(ahead, behind, color),
        );
    }
    Ok(0)
}

/// `polygit sizes` — disk usage per repo, largest first, plus a total.
pub async fn run_sizes(roots: Vec<PathBuf>, max_depth: usize) -> Result<i32> {
    let repos = discover(&roots, max_depth).await?;
    if repos.is_empty() {
        println!("No git repositories found.");
        return Ok(0);
    }
    let color = stdout_color();
    let total_repos = repos.len();
    // Sizing walks every repo in full (slow on huge trees), so show live progress on a stderr
    // TTY — stdout stays clean for pipes. Results are sorted afterward, so order-of-completion
    // (buffer_unordered) is fine and lets the bar advance as each repo finishes.
    let show_progress = io::stderr().is_terminal();
    let mut rows: Vec<(String, u64)> = Vec::with_capacity(total_repos);
    let mut sized = 0usize;
    if show_progress {
        eprint!("{}", paint(&progress_bar(0, total_repos, PROGRESS_WIDTH), DIM, true));
        let _ = io::stderr().flush();
    }
    let mut sizing = stream::iter(repos)
        .map(|repo| async move {
            let path = repo.path.clone();
            let bytes = tokio::task::spawn_blocking(move || crate::explorer::dir_size_parallel(&path))
                .await
                .unwrap_or(0);
            (repo.name, bytes)
        })
        .buffer_unordered(SIZE_JOBS);
    while let Some(row) = sizing.next().await {
        rows.push(row);
        sized += 1;
        if show_progress {
            eprint!("\r\x1b[2K{}", paint(&progress_bar(sized, total_repos, PROGRESS_WIDTH), DIM, true));
            let _ = io::stderr().flush();
        }
    }
    if show_progress {
        eprint!("\r\x1b[2K");
        let _ = io::stderr().flush();
    }
    sort_sizes(&mut rows);
    let total: u64 = rows.iter().map(|(_, bytes)| bytes).sum();
    for (name, bytes) in &rows {
        println!("{:>9}  {}", crate::explorer::human_size(*bytes), paint(name, CYAN, color));
    }
    println!("{:>9}  {}", crate::explorer::human_size(total), paint("total", DIM, color));
    Ok(0)
}

/// Options for `polygit coverage`.
pub struct CoverageOpts {
    pub json: bool,
    pub org: Option<String>,
    pub include_forks: bool,
    pub include_archived: bool,
    pub refresh: bool,
}

/// `polygit coverage` — for each GitHub owner/org found among the scan roots' remotes, print how
/// many of its repos are cloned locally and list the ones that aren't. Owner identity comes from the
/// remotes, so the root folder name is irrelevant.
pub async fn run_coverage(roots: Vec<PathBuf>, max_depth: usize, opts: CoverageOpts) -> Result<i32> {
    let coverage =
        crate::coverage::compute(&roots, max_depth, opts.org.as_deref(), opts.refresh, &[]).await?;

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&coverage)?);
        return Ok(0);
    }
    if coverage.is_empty() {
        println!("No GitHub repos found under the scan roots.");
        return Ok(0);
    }

    let color = stdout_color();
    let mut total_missing = 0usize;
    for owner in &coverage {
        let cloned = owner.cloned_count(opts.include_forks, opts.include_archived);
        let total = owner.badge_total(opts.include_forks, opts.include_archived);
        let tag = if owner.kind.is_partial() {
            paint("  (partial — local slice only)", DIM, color)
        } else {
            String::new()
        };
        println!();
        println!(
            "{}  {}{}",
            paint(&owner.owner, BOLD_CYAN, color),
            paint(&format!("{cloned}/{total}"), BOLD, color),
            tag,
        );

        let missing = owner.missing(opts.include_forks, opts.include_archived);
        total_missing += missing.len();
        if missing.is_empty() {
            if !owner.kind.is_partial() {
                println!("  {}", paint("✓ all cloned", GREEN, color));
            }
            continue;
        }
        for repo in missing {
            let mut flags = Vec::new();
            if repo.is_fork {
                flags.push("fork");
            }
            if repo.is_archived {
                flags.push("archived");
            }
            if repo.private {
                flags.push("private");
            }
            let suffix = if flags.is_empty() {
                String::new()
            } else {
                paint(&format!("  [{}]", flags.join(", ")), DIM, color)
            };
            println!("  {} {}{}", paint("✗", RED, color), repo.name, suffix);
        }
    }

    println!();
    println!(
        "{}",
        paint(
            &format!("{total_missing} repo(s) missing across {} owner(s)", coverage.len()),
            DIM,
            color,
        ),
    );
    Ok(0)
}

/// Options shared by `select` and `plan`.
pub struct SelectOpts {
    pub expr: String,
    /// Owners to enumerate on top of whatever the scan roots reveal.
    pub owners: Vec<String>,
    pub with_siblings: bool,
    pub include_forks: bool,
    pub include_archived: bool,
    pub refresh: bool,
    pub json: bool,
}

/// Options for `plan`, which is `select` plus a destination.
pub struct PlanOpts {
    pub select: SelectOpts,
    pub layout: String,
    pub output: Option<PathBuf>,
    pub prefix_depth: usize,
}

/// Resolve the listing once: coverage for the scan roots plus any explicitly named owners, mapped
/// into the flat records the selector engine matches against.
async fn resolve_facts(
    roots: &[PathBuf],
    max_depth: usize,
    opts: &SelectOpts,
) -> Result<Vec<crate::select::RepoFacts>> {
    let coverage =
        crate::coverage::compute(roots, max_depth, None, opts.refresh, &opts.owners).await?;
    let mut facts = crate::coverage::repo_facts(&coverage);
    facts.retain(|repo| {
        repo.cloned()
            || ((opts.include_forks || !repo.is_fork)
                && (opts.include_archived || !repo.is_archived))
    });
    Ok(facts)
}

/// Apply the expression, then optionally widen to project siblings.
fn resolve_selection(
    facts: &[crate::select::RepoFacts],
    opts: &SelectOpts,
) -> Result<Vec<usize>> {
    let selector = crate::select::parse(&opts.expr).map_err(|err| anyhow::anyhow!(err))?;
    let mut chosen = crate::select::select(facts, &selector);
    if opts.with_siblings {
        chosen =
            crate::select::expand_siblings(&chosen, facts, &crate::select::ClusterOpts::default());
    }
    Ok(chosen)
}

/// `polygit select` — print the repos an expression picks, with why each axis is worth using.
pub async fn run_select(roots: Vec<PathBuf>, max_depth: usize, opts: SelectOpts) -> Result<i32> {
    let facts = resolve_facts(&roots, max_depth, &opts).await?;
    let chosen = resolve_selection(&facts, &opts)?;

    if opts.json {
        let rows: Vec<serde_json::Value> = chosen
            .iter()
            .map(|&index| {
                let repo = &facts[index];
                serde_json::json!({
                    "owner": repo.owner,
                    "name": repo.name,
                    "cloned": repo.cloned(),
                    "local_path": repo.local_path,
                    "topics": repo.topics,
                    "language": repo.language,
                    "archived": repo.is_archived,
                    "fork": repo.is_fork,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(0);
    }

    let color = stdout_color();
    if chosen.is_empty() {
        println!("{}", paint("No repos match.", DIM, color));
        print_axis_hints(&facts, color);
        return Ok(0);
    }
    let pad = chosen.iter().map(|&index| facts[index].name.chars().count()).max().unwrap_or(0);
    for &index in &chosen {
        let repo = &facts[index];
        let mark = if repo.cloned() {
            paint("\u{2713}", GREEN, color)
        } else {
            paint("\u{2717}", RED, color)
        };
        let name = paint(&format!("{:<pad$}", repo.name), CYAN, color);
        let mut tail = vec![repo.owner.clone()];
        if let Some(language) = &repo.language {
            tail.push(language.clone());
        }
        if !repo.topics.is_empty() {
            tail.push(repo.topics.join(" "));
        }
        println!("{mark} {name}  {}", paint(&tail.join("  \u{b7}  "), DIM, color));
    }
    println!();
    let cloned = chosen.iter().filter(|&&index| facts[index].cloned()).count();
    println!(
        "{}",
        paint(
            &format!("{} selected \u{b7} {cloned} cloned \u{b7} {} missing", chosen.len(), chosen.len() - cloned),
            DIM,
            color,
        )
    );
    print_axis_hints(&facts, color);
    Ok(0)
}

/// Report how much signal each axis actually carries here, so the ranking is legible rather than
/// magic — topics are a strong primary axis in one org and near-useless in another.
fn print_axis_hints(facts: &[crate::select::RepoFacts], color: bool) {
    if facts.is_empty() {
        return;
    }
    let (topics, regime) = crate::select::topic_stats(facts);
    let languages = crate::select::language_stats(facts);
    let families = crate::select::prefix_families(facts, 1);
    let covered: usize = families.iter().map(|(_, count)| count).sum();
    let regime_label = match regime {
        crate::select::Regime::Rich => "rich",
        crate::select::Regime::Sparse => "sparse",
        crate::select::Regime::Degenerate => "degenerate",
    };
    println!(
        "{}",
        paint(
            &format!(
                "axes over {} repos \u{b7} prefix: {}% in {} families \u{b7} topic: {}% tagged, {} distinct ({regime_label}) \u{b7} language: {}% in {} distinct",
                facts.len(),
                covered * 100 / facts.len().max(1),
                families.len(),
                topics.percent(),
                topics.distinct,
                languages.percent(),
                languages.distinct,
            ),
            DIM,
            color,
        )
    );
}

/// `polygit plan` — resolve a selection, lay it out, and print the resulting directory tree plus
/// what each repo would do. Touches nothing.
pub async fn run_plan(roots: Vec<PathBuf>, max_depth: usize, opts: PlanOpts) -> Result<i32> {
    let facts = resolve_facts(&roots, max_depth, &opts.select).await?;
    let chosen = resolve_selection(&facts, &opts.select)?;
    let layout = crate::layout::LayoutTemplate::parse(&opts.layout)
        .map_err(|err| anyhow::anyhow!(err))?;
    let context = crate::layout::LayoutContext::build(
        &facts,
        opts.prefix_depth,
        2,
        &crate::select::ClusterOpts::default(),
    );
    let root = match opts.output {
        Some(path) => path,
        None => roots.first().cloned().unwrap_or_else(|| PathBuf::from(".")),
    };
    let plan = crate::layout::plan(&facts, &chosen, &layout, &context, &root);

    if opts.select.json {
        let rows: Vec<serde_json::Value> = plan
            .rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "owner": row.owner,
                    "name": row.name,
                    "action": row.action.label(),
                    "dest": row.dest,
                    "from": match &row.action {
                        crate::layout::Action::Move { from } => Some(from.clone()),
                        _ => None,
                    },
                    "skip_reason": match &row.action {
                        crate::layout::Action::Skip(reason) => Some(reason.label()),
                        _ => None,
                    },
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "root": plan.root,
                "layout": layout.source(),
                "rows": rows,
            }))?
        );
        return Ok(0);
    }

    let color = stdout_color();
    println!("{}  {}", paint("root", DIM, color), plan.root.display());
    println!("{}  {}", paint("layout", DIM, color), layout.source());
    println!();
    print_plan_tree(&plan, color);
    println!();
    for row in &plan.rows {
        if let crate::layout::Action::Skip(reason) = &row.action {
            println!(
                "  {} {}  {}",
                paint("!", RED, color),
                row.name,
                paint(&reason.label(), DIM, color)
            );
        }
    }
    let counts = plan.counts();
    println!(
        "{}",
        paint(
            &format!(
                "{} selected \u{2014} {} clone \u{b7} {} move \u{b7} {} keep \u{b7} {} skip",
                counts.total(),
                counts.clone_rows,
                counts.moves,
                counts.keep,
                counts.skipped
            ),
            BOLD,
            color,
        )
    );
    Ok(0)
}

/// Render the planned directory tree, badging each repo with what will happen to it.
fn print_plan_tree(plan: &crate::layout::Plan, color: bool) {
    let nodes = plan.tree();
    // Repos that sit directly at the root have no folder node, so print them first.
    let mut in_folder: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for node in &nodes {
        for position in &node.repos {
            in_folder.insert(*position);
        }
    }
    for (position, row) in plan.rows.iter().enumerate() {
        if !in_folder.contains(&position) && !row.rel_dest.is_empty() {
            println!("{}", plan_row_line(row, 0, color));
        }
    }
    for node in &nodes {
        let indent = "  ".repeat(node.depth as usize);
        println!(
            "{indent}{}{}",
            paint(&node.name, BOLD_CYAN, color),
            paint(&format!("/  ({})", node.repos.len()), DIM, color)
        );
        for position in &node.repos {
            println!("{}", plan_row_line(&plan.rows[*position], node.depth as usize + 1, color));
        }
    }
}

fn plan_row_line(row: &crate::layout::PlanRow, depth: usize, color: bool) -> String {
    let indent = "  ".repeat(depth);
    let (glyph, code) = match &row.action {
        crate::layout::Action::Clone => ("+", GREEN),
        crate::layout::Action::Keep => ("\u{b7}", DIM),
        crate::layout::Action::Move { .. } => ("\u{2192}", CYAN),
        crate::layout::Action::Skip(_) => ("!", RED),
    };
    let detail = match &row.action {
        crate::layout::Action::Move { from } => format!("  from {}", from.display()),
        _ => String::new(),
    };
    format!("{indent}{} {}{}", paint(glyph, code, color), row.name, paint(&detail, DIM, color))
}

/// Options for `clone`, which is `plan` plus permission to act on its clone rows.
pub struct CloneCmdOpts {
    pub plan: PlanOpts,
    pub jobs: Option<usize>,
    pub blobless: bool,
    pub depth: Option<u32>,
    pub max_size: Option<String>,
    pub yes: bool,
    pub dry_run: bool,
}

/// `polygit clone` — resolve a selection, lay it out, and clone every repo that is missing. Repos
/// already present are left alone; a destination that is occupied is reported, never overwritten.
pub async fn run_clone_command(
    roots: Vec<PathBuf>,
    max_depth: usize,
    opts: CloneCmdOpts,
) -> Result<i32> {
    let color = stdout_color();
    let facts = resolve_facts(&roots, max_depth, &opts.plan.select).await?;
    let chosen = resolve_selection(&facts, &opts.plan.select)?;
    let layout = crate::layout::LayoutTemplate::parse(&opts.plan.layout)
        .map_err(|err| anyhow::anyhow!(err))?;
    let context = crate::layout::LayoutContext::build(
        &facts,
        opts.plan.prefix_depth,
        2,
        &crate::select::ClusterOpts::default(),
    );
    let root = match &opts.plan.output {
        Some(path) => path.clone(),
        None => roots.first().cloned().unwrap_or_else(|| PathBuf::from(".")),
    };
    let plan = crate::layout::plan(&facts, &chosen, &layout, &context, &root);

    let max_size_kb = match &opts.max_size {
        Some(raw) => Some(crate::clone::parse_size_to_kb(raw).map_err(|err| anyhow::anyhow!(err))?),
        None => None,
    };
    let clone_options = crate::clone::CloneOptions {
        blobless: opts.blobless,
        depth: opts.depth,
        max_size_kb,
    };

    let targets: Vec<crate::clone::CloneTarget> = plan
        .rows
        .iter()
        .filter(|row| matches!(row.action, crate::layout::Action::Clone))
        .map(|row| crate::clone::CloneTarget {
            owner: row.owner.clone(),
            name: row.name.clone(),
            dest: row.dest.clone(),
            size_kb: facts[row.index].size_kb,
        })
        .collect();

    let counts = plan.counts();
    let total_kb: u64 = targets.iter().map(|target| target.size_kb).sum();
    println!(
        "{}",
        paint(
            &format!(
                "{} to clone into {} \u{b7} {} \u{b7} {} already there, {} elsewhere, {} skipped",
                targets.len(),
                root.display(),
                crate::clone::format_size(total_kb),
                counts.keep,
                counts.moves,
                counts.skipped,
            ),
            BOLD,
            color,
        )
    );
    if counts.moves > 0 {
        println!(
            "{}",
            paint(
                "repos that exist elsewhere are left where they are — `polygit plan` shows where they would move to",
                DIM,
                color,
            )
        );
    }
    if targets.is_empty() {
        println!("{}", paint("Nothing to clone.", DIM, color));
        return Ok(0);
    }
    if opts.dry_run {
        for target in &targets {
            println!("  {} {}", paint("+", GREEN, color), target.dest.display());
        }
        return Ok(0);
    }
    if !opts.yes && !confirm(&format!("Clone {} repo(s)?", targets.len()))? {
        println!("{}", paint("Aborted.", DIM, color));
        return Ok(0);
    }

    let progress = Arc::new(std::sync::Mutex::new(crate::clone::CloneProgress::new(targets.len())));
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let jobs = opts.jobs.unwrap_or_else(num_cpus::get).max(1);
    let control = crate::app::ThrottleControl::new(jobs);

    let reporter = Arc::clone(&progress);
    let total = targets.len();
    let ticker = tokio::spawn(async move {
        let stderr_tty = io::stderr().is_terminal();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            let (done, running) = {
                let state = reporter.lock().unwrap();
                (state.done, state.running.join(", "))
            };
            if stderr_tty {
                eprint!("\r\x1b[2K{} {running}", progress_bar(done, total, PROGRESS_WIDTH));
                let _ = io::stderr().flush();
            }
            if done >= total {
                break;
            }
        }
        if stderr_tty {
            eprint!("\r\x1b[2K");
            let _ = io::stderr().flush();
        }
    });

    crate::clone::run_clone(
        targets,
        clone_options,
        Arc::clone(&progress),
        cancel,
        control,
        crate::clone::gh_clone_fn(),
    )
    .await;
    ticker.abort();

    let state = progress.lock().unwrap();
    for (target, outcome) in &state.results {
        match outcome {
            crate::clone::CloneOutcome::Cloned => {}
            other => println!(
                "  {} {}  {}",
                paint("!", RED, color),
                target.slug(),
                paint(&other.label(), DIM, color)
            ),
        }
    }
    println!("{}", paint(&state.summary(), BOLD, color));
    Ok(if state.failed() > 0 { 1 } else { 0 })
}

/// Ask on a TTY; without one, refuse rather than assuming yes.
fn confirm(question: &str) -> Result<bool> {
    if !io::stdin().is_terminal() {
        println!("{question} (no TTY — pass -y to proceed)");
        return Ok(false);
    }
    print!("{question} [y/N] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// `polygit orgs` — your account, your orgs and your enterprises, with how much of each is cloned.
pub async fn run_orgs(roots: Vec<PathBuf>, max_depth: usize, refresh: bool) -> Result<i32> {
    let color = stdout_color();
    let owners = crate::coverage::list_my_owners().await;
    if owners.is_empty() {
        println!("{}", paint("No GitHub account resolved \u{2014} is `gh` authenticated?", DIM, color));
        return Ok(1);
    }
    let names: Vec<String> = owners.iter().map(|owner| owner.login.clone()).collect();
    let coverage = crate::coverage::compute(&roots, max_depth, None, refresh, &names).await?;

    let pad = owners.iter().map(|owner| owner.login.chars().count()).max().unwrap_or(0);
    println!("{}", paint("Owners", BOLD, color));
    for owner in &owners {
        let found = coverage.iter().find(|entry| entry.owner.eq_ignore_ascii_case(&owner.login));
        let summary = match found {
            Some(entry) => format!("{}/{}", entry.cloned_count(true, true), entry.badge_total(true, true)),
            None => "-".to_string(),
        };
        let kind = match owner.kind {
            crate::coverage::OwnerKind::User => "you",
            crate::coverage::OwnerKind::MemberOrg => "org",
            crate::coverage::OwnerKind::Partial => "partial",
        };
        println!(
            "  {}  {:>9}  {}",
            paint(&format!("{:<pad$}", owner.login), CYAN, color),
            summary,
            paint(kind, DIM, color)
        );
    }

    println!();
    println!("{}", paint("Enterprises", BOLD, color));
    match crate::coverage::list_enterprises().await {
        Ok(enterprises) if enterprises.is_empty() => {
            println!("  {}", paint("none", DIM, color));
        }
        Ok(enterprises) => {
            for enterprise in &enterprises {
                println!(
                    "  {}  {}",
                    paint(&enterprise.slug, BOLD_CYAN, color),
                    paint(&enterprise.name, DIM, color)
                );
                for org in &enterprise.orgs {
                    println!(
                        "    {}  {}",
                        org.login,
                        paint(
                            &format!("{} repos, {} archived", org.total, org.archived),
                            DIM,
                            color
                        )
                    );
                }
            }
        }
        Err(err) => {
            // A scope failure reads as "no enterprises" unless the remedy is named here.
            println!("  {}", paint(&err.message(), DIM, color));
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_track_all_states() {
        assert_eq!(format_track(None, None, false), "no upstream");
        assert_eq!(format_track(Some(1), None, false), "no upstream");
        assert_eq!(format_track(Some(0), Some(0), false), "✓");
        assert_eq!(format_track(Some(3), Some(0), false), "↑3");
        assert_eq!(format_track(Some(0), Some(2), false), "↓2");
        assert_eq!(format_track(Some(1), Some(4), false), "↑1 ↓4");
    }

    #[test]
    fn format_track_colored() {
        assert_eq!(format_track(Some(0), Some(0), true), "\x1b[32m✓\x1b[0m");
        assert_eq!(format_track(None, None, true), "\x1b[2mno upstream\x1b[0m");
        assert_eq!(format_track(Some(2), Some(1), true), "\x1b[32m↑2\x1b[0m \x1b[31m↓1\x1b[0m");
    }

    #[test]
    fn paint_wraps_only_when_colored() {
        assert_eq!(paint("x", CYAN, true), "\x1b[36mx\x1b[0m");
        assert_eq!(paint("x", CYAN, false), "x");
    }

    #[test]
    fn help_label_width_counts_primary_and_aliases() {
        assert_eq!(help_label_width("status", ""), 6);
        assert_eq!(help_label_width("list", "ls"), 8); // "list, ls"
        assert_eq!(help_label_width("ws", "workspace, workspaces"), 25);
    }

    #[test]
    fn help_sections_are_populated() {
        assert!(!HELP_SECTIONS.is_empty());
        assert!(HELP_SECTIONS.iter().all(|(_, rows)| !rows.is_empty()));
    }

    #[test]
    fn progress_bar_fills_proportionally() {
        assert_eq!(progress_bar(0, 4, 4), "[░░░░] 0/4 repos");
        assert_eq!(progress_bar(2, 4, 4), "[██░░] 2/4 repos");
        assert_eq!(progress_bar(4, 4, 4), "[████] 4/4 repos");
        // Guard: no repos renders full rather than dividing by zero.
        assert_eq!(progress_bar(0, 0, 4), "[████] 0/0 repos");
    }

    #[test]
    fn sort_sizes_largest_first_ties_alphabetical() {
        let mut rows = vec![
            ("beta".to_string(), 10),
            ("alpha".to_string(), 10),
            ("gamma".to_string(), 99),
        ];
        sort_sizes(&mut rows);
        let names: Vec<&str> = rows.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["gamma", "alpha", "beta"]);
    }

    #[test]
    fn scan_args_depth_floors_and_no_recursive_wins() {
        let base = ScanArgs { dirs: vec![], workspace: None, depth: 16, no_recursive: false };
        assert_eq!(base.max_depth(), 16);
        let flat = ScanArgs { no_recursive: true, ..base };
        assert_eq!(flat.max_depth(), 1);
        let floored = ScanArgs { depth: 0, no_recursive: false, dirs: vec![], workspace: None };
        assert_eq!(floored.max_depth(), 1);
    }
}
