//! Parse a unified git diff (ANSI-stripped) into structured rows, and a lightweight, language-aware
//! syntax highlighter — for the diff modal's unified / split (GitHub-PR-style) views. The raw view
//! keeps git's own `--color` output; these power the structured renders.

/// The kind of a parsed diff row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    /// File header / index / `\ No newline` etc. — shown dim, no line numbers.
    Meta,
    /// A `@@ … @@` hunk header.
    Hunk,
    /// An unchanged context line.
    Context,
    /// An added (`+`) line.
    Add,
    /// A removed (`-`) line.
    Del,
}

/// One parsed diff row: its kind, old/new line numbers (where applicable), and the code text with
/// the leading `+`/`-`/space marker removed (Meta/Hunk keep the full line).
#[derive(Debug, Clone)]
pub struct DiffRow {
    pub kind: DiffLineKind,
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub text: String,
}

fn strip_ansi(line: &str) -> String {
    String::from_utf8_lossy(&strip_ansi_escapes::strip(line.as_bytes())).into_owned()
}

/// Split a (possibly multi-file) unified diff into per-file segments, each paired with the file's
/// path — so the renderer can pick the right syntax lexer per file. Splits on `diff --git` headers;
/// any preamble before the first header becomes one pathless (`""`) segment. The path is taken from
/// `+++ b/<p>` (the new path), falling back to `--- a/<p>` (deletions, where `+++` is `/dev/null`),
/// then the `diff --git … b/<p>` line. ANSI is stripped before matching. Pure.
pub fn split_files(raw: &[String]) -> Vec<(String, Vec<String>)> {
    let mut segments: Vec<(String, Vec<String>)> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for line in raw {
        if strip_ansi(line).starts_with("diff --git ") && !current.is_empty() {
            let path = segment_path(&current);
            segments.push((path, std::mem::take(&mut current)));
        }
        current.push(line.clone());
    }
    if !current.is_empty() {
        let path = segment_path(&current);
        segments.push((path, current));
    }
    segments
}

/// The file path of one `split_files` segment (see its doc for the precedence). `""` if none found.
fn segment_path(segment: &[String]) -> String {
    let mut fallback = String::new();
    for line in segment {
        let text = strip_ansi(line);
        if let Some(rest) = text.strip_prefix("+++ ") {
            let path = rest.trim();
            if path != "/dev/null" {
                return path.strip_prefix("b/").unwrap_or(path).to_string();
            }
        } else if let Some(rest) = text.strip_prefix("--- ") {
            let path = rest.trim();
            if path != "/dev/null" {
                fallback = path.strip_prefix("a/").unwrap_or(path).to_string();
            }
        } else if fallback.is_empty() && text.starts_with("diff --git ") {
            if let Some((_, after)) = text.rsplit_once(" b/") {
                fallback = after.trim().to_string();
            }
        }
    }
    fallback
}

/// Parse the `@@ -a,b +c,d @@` header's two start line numbers.
fn parse_hunk_header(line: &str) -> Option<(usize, usize)> {
    let rest = line.strip_prefix("@@ ")?;
    let body = rest.split(" @@").next()?;
    let (old_part, new_part) = body.split_once(' ')?;
    let parse_start = |part: &str| -> Option<usize> {
        part.trim_start_matches(['-', '+']).split(',').next()?.parse().ok()
    };
    Some((parse_start(old_part)?, parse_start(new_part)?))
}

/// Parse raw (ANSI-colored) git-diff lines into structured rows with line numbers assigned.
pub fn parse(raw: &[String]) -> Vec<DiffRow> {
    let mut rows = Vec::with_capacity(raw.len());
    let mut old_no = 0usize;
    let mut new_no = 0usize;
    for line in raw {
        let text = strip_ansi(line);
        if text.starts_with("@@") {
            if let Some((old, new)) = parse_hunk_header(&text) {
                old_no = old;
                new_no = new;
            }
            rows.push(DiffRow { kind: DiffLineKind::Hunk, old: None, new: None, text });
        } else if text.starts_with("+++") || text.starts_with("---") {
            rows.push(DiffRow { kind: DiffLineKind::Meta, old: None, new: None, text });
        } else if let Some(code) = text.strip_prefix('+') {
            rows.push(DiffRow {
                kind: DiffLineKind::Add,
                old: None,
                new: Some(new_no),
                text: code.to_string(),
            });
            new_no += 1;
        } else if let Some(code) = text.strip_prefix('-') {
            rows.push(DiffRow {
                kind: DiffLineKind::Del,
                old: Some(old_no),
                new: None,
                text: code.to_string(),
            });
            old_no += 1;
        } else if let Some(code) = text.strip_prefix(' ') {
            rows.push(DiffRow {
                kind: DiffLineKind::Context,
                old: Some(old_no),
                new: Some(new_no),
                text: code.to_string(),
            });
            old_no += 1;
            new_no += 1;
        } else {
            // `diff --git`, `index …`, `\ No newline …`, blank — metadata.
            rows.push(DiffRow { kind: DiffLineKind::Meta, old: None, new: None, text });
        }
    }
    rows
}

/// A side-by-side row for the split view: an optional left (old) and right (new) row. Meta/Hunk
/// rows span both sides (returned as `left == right` clones).
#[derive(Debug, Clone)]
pub struct SplitRow {
    pub left: Option<DiffRow>,
    pub right: Option<DiffRow>,
    /// A full-width header (Meta/Hunk) rather than a paired code row.
    pub full: bool,
}

/// Pair parsed rows into side-by-side split rows: within a change block, removed lines align with
/// added lines positionally; leftover removals/additions get a blank on the opposite side.
pub fn to_split(rows: &[DiffRow]) -> Vec<SplitRow> {
    let mut out = Vec::new();
    let mut dels: Vec<DiffRow> = Vec::new();
    let mut adds: Vec<DiffRow> = Vec::new();
    let flush = |dels: &mut Vec<DiffRow>, adds: &mut Vec<DiffRow>, out: &mut Vec<SplitRow>| {
        let pairs = dels.len().max(adds.len());
        for index in 0..pairs {
            out.push(SplitRow {
                left: dels.get(index).cloned(),
                right: adds.get(index).cloned(),
                full: false,
            });
        }
        dels.clear();
        adds.clear();
    };
    for row in rows {
        match row.kind {
            DiffLineKind::Del => dels.push(row.clone()),
            DiffLineKind::Add => adds.push(row.clone()),
            DiffLineKind::Context => {
                flush(&mut dels, &mut adds, &mut out);
                out.push(SplitRow { left: Some(row.clone()), right: Some(row.clone()), full: false });
            }
            DiffLineKind::Meta | DiffLineKind::Hunk => {
                flush(&mut dels, &mut adds, &mut out);
                out.push(SplitRow { left: Some(row.clone()), right: Some(row.clone()), full: true });
            }
        }
    }
    flush(&mut dels, &mut adds, &mut out);
    out
}

// ---- Lightweight syntax highlighting --------------------------------------------------------

/// A highlight token class (mapped to a palette color by the renderer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tok {
    Plain,
    Keyword,
    Str,
    Num,
    Comment,
    Punct,
}

/// A language's lexical rules for the generic highlighter.
struct Syntax {
    keywords: &'static [&'static str],
    line_comment: &'static [&'static str],
    block_comment: bool,
    /// Backtick is a string delimiter (JS/Go/Markdown-ish).
    backtick_strings: bool,
}

const C_LIKE_KW: &[&str] = &[
    "if", "else", "for", "while", "return", "break", "continue", "switch", "case", "default",
    "struct", "enum", "union", "class", "public", "private", "protected", "static", "const", "void",
    "int", "long", "char", "float", "double", "bool", "true", "false", "null", "new", "delete",
    "import", "export", "function", "var", "let", "this", "typeof", "async", "await", "try", "catch",
    "throw", "type", "interface", "extends", "implements", "namespace",
];
const RUST_KW: &[&str] = &[
    "fn", "let", "mut", "pub", "struct", "enum", "trait", "impl", "for", "while", "loop", "if",
    "else", "match", "return", "break", "continue", "use", "mod", "crate", "self", "Self", "super",
    "where", "as", "ref", "move", "async", "await", "dyn", "const", "static", "type", "unsafe",
    "true", "false", "Some", "None", "Ok", "Err", "Option", "Result", "Vec", "String", "str",
];
const PY_KW: &[&str] = &[
    "def", "class", "if", "elif", "else", "for", "while", "return", "import", "from", "as", "try",
    "except", "finally", "with", "lambda", "yield", "pass", "break", "continue", "and", "or", "not",
    "in", "is", "None", "True", "False", "self", "global", "nonlocal", "raise", "async", "await",
];
const GO_KW: &[&str] = &[
    "func", "package", "import", "var", "const", "type", "struct", "interface", "map", "chan", "go",
    "defer", "if", "else", "for", "range", "return", "switch", "case", "default", "select", "nil",
    "true", "false", "make", "new", "string", "int", "error", "bool",
];

fn syntax_for(path: &str) -> Syntax {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "rs" => Syntax { keywords: RUST_KW, line_comment: &["//"], block_comment: true, backtick_strings: false },
        "py" => Syntax { keywords: PY_KW, line_comment: &["#"], block_comment: false, backtick_strings: false },
        "go" => Syntax { keywords: GO_KW, line_comment: &["//"], block_comment: true, backtick_strings: true },
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => {
            Syntax { keywords: C_LIKE_KW, line_comment: &["//"], block_comment: true, backtick_strings: true }
        }
        "c" | "h" | "cpp" | "cc" | "hpp" | "java" | "cs" | "kt" | "swift" => {
            Syntax { keywords: C_LIKE_KW, line_comment: &["//"], block_comment: true, backtick_strings: false }
        }
        "sh" | "bash" | "zsh" | "yaml" | "yml" | "toml" | "ini" | "conf" => {
            Syntax { keywords: &[], line_comment: &["#"], block_comment: false, backtick_strings: false }
        }
        _ => Syntax { keywords: C_LIKE_KW, line_comment: &["//", "#"], block_comment: true, backtick_strings: true },
    }
}

/// Tokenize `text` into highlighted runs for the file at `path`. A best-effort, single-line lexer
/// (no cross-line block-comment state) — enough for a readable, GitHub-PR-like diff.
pub fn highlight(text: &str, path: &str) -> Vec<(String, Tok)> {
    let syntax = syntax_for(path);
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<(String, Tok)> = Vec::new();
    let mut index = 0;
    let push = |out: &mut Vec<(String, Tok)>, text: String, tok: Tok| {
        if !text.is_empty() {
            out.push((text, tok));
        }
    };
    while index < chars.len() {
        let rest: String = chars[index..].iter().collect();
        // Line comment to end of line.
        if let Some(marker) = syntax.line_comment.iter().find(|marker| rest.starts_with(**marker)) {
            let _ = marker;
            push(&mut out, rest, Tok::Comment);
            break;
        }
        // Block comment `/* … */` (single-line best effort).
        if syntax.block_comment && rest.starts_with("/*") {
            let end = rest.find("*/").map(|pos| index + pos + 2).unwrap_or(chars.len());
            push(&mut out, chars[index..end].iter().collect(), Tok::Comment);
            index = end;
            continue;
        }
        let ch = chars[index];
        // Strings.
        if ch == '"' || ch == '\'' || (ch == '`' && syntax.backtick_strings) {
            let quote = ch;
            let start = index;
            index += 1;
            while index < chars.len() {
                if chars[index] == '\\' {
                    index += 2;
                    continue;
                }
                if chars[index] == quote {
                    index += 1;
                    break;
                }
                index += 1;
            }
            push(&mut out, chars[start..index.min(chars.len())].iter().collect(), Tok::Str);
            continue;
        }
        // Numbers.
        if ch.is_ascii_digit() {
            let start = index;
            while index < chars.len()
                && (chars[index].is_ascii_alphanumeric() || chars[index] == '.' || chars[index] == '_')
            {
                index += 1;
            }
            push(&mut out, chars[start..index].iter().collect(), Tok::Num);
            continue;
        }
        // Identifiers / keywords.
        if ch.is_alphabetic() || ch == '_' {
            let start = index;
            while index < chars.len() && (chars[index].is_alphanumeric() || chars[index] == '_') {
                index += 1;
            }
            let word: String = chars[start..index].iter().collect();
            let tok = if syntax.keywords.contains(&word.as_str()) { Tok::Keyword } else { Tok::Plain };
            push(&mut out, word, tok);
            continue;
        }
        // Punctuation / whitespace runs.
        if ch.is_whitespace() {
            let start = index;
            while index < chars.len() && chars[index].is_whitespace() {
                index += 1;
            }
            push(&mut out, chars[start..index].iter().collect(), Tok::Plain);
            continue;
        }
        push(&mut out, ch.to_string(), Tok::Punct);
        index += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hunk_and_numbers() {
        let raw: Vec<String> = [
            "diff --git a/x.rs b/x.rs",
            "@@ -10,3 +10,4 @@ fn main() {",
            " context",
            "-removed",
            "+added one",
            "+added two",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let rows = parse(&raw);
        assert_eq!(rows[0].kind, DiffLineKind::Meta);
        assert_eq!(rows[1].kind, DiffLineKind::Hunk);
        // context at old 10 / new 10
        assert_eq!((rows[2].old, rows[2].new), (Some(10), Some(10)));
        // removed at old 11, added at new 11 then 12
        assert_eq!(rows[3].old, Some(11));
        assert_eq!(rows[4].new, Some(11));
        assert_eq!(rows[5].new, Some(12));
        assert_eq!(rows[3].text, "removed");
    }

    #[test]
    fn split_files_tracks_path_per_file() {
        let raw: Vec<String> = [
            "diff --git a/src/app.rs b/src/app.rs",
            "--- a/src/app.rs",
            "+++ b/src/app.rs",
            "@@ -1 +1 @@",
            "-old",
            "+new",
            "diff --git a/README.md b/README.md",
            "--- a/README.md",
            "+++ b/README.md",
            "@@ -1 +1 @@",
            "-a",
            "+b",
        ]
        .iter()
        .map(|line| line.to_string())
        .collect();
        let segments = split_files(&raw);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].0, "src/app.rs");
        assert_eq!(segments[1].0, "README.md");
        // Each segment carries its own file's lines (6 each here).
        assert_eq!(segments[0].1.len(), 6);
        assert_eq!(segments[1].1.len(), 6);
    }

    #[test]
    fn split_files_added_and_deleted_paths() {
        // Addition: `--- /dev/null`, path from `+++ b/`. Deletion: `+++ /dev/null`, path from `--- a/`.
        let raw: Vec<String> = [
            "diff --git a/new.rs b/new.rs",
            "--- /dev/null",
            "+++ b/new.rs",
            "+hello",
            "diff --git a/gone.rs b/gone.rs",
            "--- a/gone.rs",
            "+++ /dev/null",
            "-bye",
        ]
        .iter()
        .map(|line| line.to_string())
        .collect();
        let segments = split_files(&raw);
        assert_eq!(segments[0].0, "new.rs");
        assert_eq!(segments[1].0, "gone.rs");
    }

    #[test]
    fn split_pairs_changes() {
        let rows = parse(
            &["@@ -1,2 +1,2 @@", "-old a", "-old b", "+new a", "+new b"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
        );
        let split = to_split(&rows);
        // hunk header (full) + two paired change rows.
        assert!(split[0].full);
        assert_eq!(split[1].left.as_ref().unwrap().text, "old a");
        assert_eq!(split[1].right.as_ref().unwrap().text, "new a");
        assert_eq!(split[2].right.as_ref().unwrap().text, "new b");
    }

    #[test]
    fn highlights_keywords_and_strings() {
        let toks = highlight("let x = \"hi\"; // c", "f.rs");
        assert!(toks.iter().any(|(text, tok)| text == "let" && *tok == Tok::Keyword));
        assert!(toks.iter().any(|(text, tok)| text == "\"hi\"" && *tok == Tok::Str));
        assert!(toks.iter().any(|(_, tok)| *tok == Tok::Comment));
    }
}
