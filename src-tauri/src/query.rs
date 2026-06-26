// SPDX-License-Identifier: GPL-3.0-or-later
//! Task-query language (SPEC §8.5) — the **frozen v1 grammar**. A fenced
//! ` ```query ` block holds line-oriented directives; this module parses them
//! into a [`Query`] and runs it over the in-memory task index.
//!
//! The grammar is deliberately small and fixed. Filters combine with AND only;
//! there is no OR, no grouping, no priority, and no start/scheduled dates. An
//! unknown or malformed directive yields a clear `Err(String)` that the command
//! layer turns into an inline error in the preview — it never panics.
//!
//! Dates are the fixed-width `YYYY-MM-DD` form, so lexical string comparison is
//! also chronological comparison. `today` is resolved from the caller's *local*
//! date (passed in by the frontend, never read from a clock here) so a query run
//! at 23:00 local uses today's date, not tomorrow-UTC (mirrors daily notes, §8.3).

use crate::index::{is_iso_date, Index, Task};

/// A parsed, ready-to-run query.
#[derive(Debug, Default, PartialEq)]
pub struct Query {
    filters: Vec<Filter>,
    sort: Option<(SortKey, SortDir)>,
    limit: Option<usize>,
}

#[derive(Debug, PartialEq)]
enum Filter {
    Done(bool),
    PathStartsWith(String),
    PathIncludes(String),
    Tag(String),
    TextIncludes(String),
    Due(DueCmp, DateSpec),
    NoDue,
    HasDue,
    Classification(String),
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum DueCmp {
    Before,
    After,
    On,
}

#[derive(Debug, PartialEq)]
enum DateSpec {
    Date(String),
    Today,
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum SortKey {
    Due,
    Path,
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum SortDir {
    Asc,
    Desc,
}

/// One task that matched a query, with the note it lives in. Owned so the caller
/// can release the index lock before serializing.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskHit {
    pub rel_path: String,
    pub title: String,
    pub text: String,
    pub done: bool,
    pub due: Option<String>,
    pub tags: Vec<String>,
    pub line: usize,
}

/// Parse a ` ```query ` block body into a [`Query`]. Blank lines are ignored.
/// Returns a human-readable error on the first unknown or malformed directive.
pub fn parse(source: &str) -> Result<Query, String> {
    let mut query = Query::default();

    for raw in source.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        match parse_line(line)? {
            Directive::Filter(f) => query.filters.push(f),
            // Later `sort`/`limit` directives override earlier ones rather than
            // erroring — a small, forgiving convenience.
            Directive::Sort(key, dir) => query.sort = Some((key, dir)),
            Directive::Limit(n) => query.limit = Some(n),
        }
    }

    Ok(query)
}

enum Directive {
    Filter(Filter),
    Sort(SortKey, SortDir),
    Limit(usize),
}

fn parse_line(line: &str) -> Result<Directive, String> {
    match line {
        "done" => return Ok(Directive::Filter(Filter::Done(true))),
        "not done" => return Ok(Directive::Filter(Filter::Done(false))),
        "no due" => return Ok(Directive::Filter(Filter::NoDue)),
        "has due" => return Ok(Directive::Filter(Filter::HasDue)),
        "due today" => {
            return Ok(Directive::Filter(Filter::Due(DueCmp::On, DateSpec::Today)));
        }
        _ => {}
    }

    if let Some(rest) = line.strip_prefix("path startswith ") {
        return Ok(Directive::Filter(Filter::PathStartsWith(parse_quoted(
            rest,
        )?)));
    }
    if let Some(rest) = line.strip_prefix("path includes ") {
        return Ok(Directive::Filter(Filter::PathIncludes(parse_quoted(rest)?)));
    }
    if let Some(rest) = line.strip_prefix("text includes ") {
        return Ok(Directive::Filter(Filter::TextIncludes(parse_quoted(rest)?)));
    }
    if let Some(rest) = line.strip_prefix("tag ") {
        let tag = rest.trim();
        let name = tag
            .strip_prefix('#')
            .ok_or_else(|| format!("tag must start with '#': {line}"))?;
        if name.is_empty() || name.chars().any(|c| c.is_whitespace()) {
            return Err(format!("invalid tag: {line}"));
        }
        return Ok(Directive::Filter(Filter::Tag(name.to_string())));
    }
    if let Some(rest) = line.strip_prefix("classification is ") {
        let label = rest.trim();
        if label.is_empty() {
            return Err(format!("classification needs a label: {line}"));
        }
        return Ok(Directive::Filter(Filter::Classification(label.to_string())));
    }
    if let Some(rest) = line.strip_prefix("due ") {
        return parse_due(rest).map(Directive::Filter);
    }
    if let Some(rest) = line.strip_prefix("sort by ") {
        return parse_sort(rest);
    }
    if let Some(rest) = line.strip_prefix("limit ") {
        let n = rest
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("limit needs a non-negative number: {line}"))?;
        return Ok(Directive::Limit(n));
    }

    Err(format!("unknown directive: {line}"))
}

/// Parse a `"..."` argument; the whole remainder must be a single quoted string.
fn parse_quoted(rest: &str) -> Result<String, String> {
    let trimmed = rest.trim();
    let inner = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .filter(|_| trimmed.len() >= 2);
    match inner {
        Some(value) if !value.contains('"') => Ok(value.to_string()),
        _ => Err(format!("expected a quoted \"value\", got: {rest}")),
    }
}

fn parse_due(rest: &str) -> Result<Filter, String> {
    let mut parts = rest.split_whitespace();
    let cmp = match parts.next() {
        Some("before") => DueCmp::Before,
        Some("after") => DueCmp::After,
        Some("on") => DueCmp::On,
        _ => return Err(format!("due needs before|after|on: due {rest}")),
    };
    let date = match parts.next() {
        Some("today") => DateSpec::Today,
        Some(d) if is_iso_date(d) => DateSpec::Date(d.to_string()),
        Some(d) => return Err(format!("invalid date '{d}' (use YYYY-MM-DD or today)")),
        None => return Err(format!("due needs a date: due {rest}")),
    };
    if parts.next().is_some() {
        return Err(format!("too many words in: due {rest}"));
    }
    Ok(Filter::Due(cmp, date))
}

fn parse_sort(rest: &str) -> Result<Directive, String> {
    let mut parts = rest.split_whitespace();
    let key = match parts.next() {
        Some("due") => SortKey::Due,
        Some("path") => SortKey::Path,
        _ => return Err(format!("sort by needs due|path: sort by {rest}")),
    };
    let dir = match parts.next() {
        Some("asc") => SortDir::Asc,
        Some("desc") => SortDir::Desc,
        None => SortDir::Asc,
        Some(other) => return Err(format!("sort direction must be asc|desc: {other}")),
    };
    if parts.next().is_some() {
        return Err(format!("too many words in: sort by {rest}"));
    }
    Ok(Directive::Sort(key, dir))
}

/// Run `query` over every task in `index`. `today` is the caller's local date as
/// `YYYY-MM-DD`, substituted wherever the grammar says `today`. Results are always
/// returned in a deterministic order (the requested sort, or path+line) so the
/// HashMap iteration order never leaks through, then truncated to `limit`.
pub fn execute(index: &Index, query: &Query, today: &str) -> Vec<TaskHit> {
    let mut hits: Vec<TaskHit> = Vec::new();

    for note in index.entries() {
        for task in &note.tasks {
            if query.filters.iter().all(|f| {
                matches_filter(
                    f,
                    note.rel_path.as_str(),
                    note.classification.as_deref(),
                    task,
                    today,
                )
            }) {
                hits.push(TaskHit {
                    rel_path: note.rel_path.clone(),
                    title: note.title.clone(),
                    text: task.text.clone(),
                    done: task.done,
                    due: task.due.clone(),
                    tags: task.tags.clone(),
                    line: task.line,
                });
            }
        }
    }

    sort_hits(&mut hits, query.sort);

    if let Some(limit) = query.limit {
        hits.truncate(limit);
    }
    hits
}

fn matches_filter(
    filter: &Filter,
    rel_path: &str,
    classification: Option<&str>,
    task: &Task,
    today: &str,
) -> bool {
    match filter {
        Filter::Done(want) => task.done == *want,
        Filter::PathStartsWith(prefix) => rel_path.starts_with(prefix.as_str()),
        Filter::PathIncludes(sub) => rel_path.contains(sub.as_str()),
        Filter::Tag(tag) => task.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)),
        Filter::TextIncludes(sub) => task.text.to_lowercase().contains(&sub.to_lowercase()),
        Filter::NoDue => task.due.is_none(),
        Filter::HasDue => task.due.is_some(),
        Filter::Classification(label) => {
            classification.is_some_and(|c| c.eq_ignore_ascii_case(label))
        }
        Filter::Due(cmp, spec) => {
            let target = match spec {
                DateSpec::Today => today,
                DateSpec::Date(d) => d.as_str(),
            };
            // A task with no due date satisfies no due-comparison.
            match task.due.as_deref() {
                None => false,
                Some(due) => match cmp {
                    DueCmp::Before => due < target,
                    DueCmp::After => due > target,
                    DueCmp::On => due == target,
                },
            }
        }
    }
}

fn sort_hits(hits: &mut [TaskHit], sort: Option<(SortKey, SortDir)>) {
    match sort {
        Some((SortKey::Due, dir)) => {
            // Natural Option ordering: None sorts before Some (so undated tasks
            // come first when ascending). Tie-break by path+line for stability.
            hits.sort_by(|a, b| a.due.cmp(&b.due).then(path_line(a).cmp(&path_line(b))));
            if dir == SortDir::Desc {
                hits.reverse();
            }
        }
        Some((SortKey::Path, dir)) => {
            hits.sort_by(|a, b| path_line(a).cmp(&path_line(b)));
            if dir == SortDir::Desc {
                hits.reverse();
            }
        }
        // No sort directive: still deterministic (path, line), since the index is
        // a HashMap with no inherent order.
        None => hits.sort_by(|a, b| path_line(a).cmp(&path_line(b))),
    }
}

fn path_line(hit: &TaskHit) -> (&str, usize) {
    (hit.rel_path.as_str(), hit.line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::build_entry;

    /// Build an index from `(rel_path, content)` pairs.
    fn index_of(notes: &[(&str, &str)]) -> Index {
        let mut idx = Index::new();
        for (rel, content) in notes {
            idx.insert(build_entry(
                rel.to_string(),
                0,
                content.len() as u64,
                content,
            ));
        }
        idx
    }

    fn texts(hits: &[TaskHit]) -> Vec<&str> {
        hits.iter().map(|h| h.text.as_str()).collect()
    }

    #[test]
    fn done_and_not_done() {
        let idx = index_of(&[("a.md", "- [ ] open\n- [x] closed\n")]);
        let q = parse("not done").unwrap();
        assert_eq!(texts(&execute(&idx, &q, "2026-06-24")), ["open"]);
        let q = parse("done").unwrap();
        assert_eq!(texts(&execute(&idx, &q, "2026-06-24")), ["closed"]);
    }

    #[test]
    fn tasks_across_three_folders() {
        let idx = index_of(&[
            ("Projects/a.md", "- [ ] alpha\n"),
            ("Work/b.md", "- [ ] beta\n"),
            ("Inbox/c.md", "- [ ] gamma\n"),
        ]);
        let q = parse("not done").unwrap();
        let hits = execute(&idx, &q, "2026-06-24");
        assert_eq!(hits.len(), 3);
        // Deterministic (path) order despite the HashMap index:
        // Inbox/ < Projects/ < Work/.
        assert_eq!(texts(&hits), ["gamma", "alpha", "beta"]);
    }

    #[test]
    fn path_startswith_and_includes() {
        let idx = index_of(&[
            ("Projects/a.md", "- [ ] one\n"),
            ("Work/b.md", "- [ ] two\n"),
        ]);
        let q = parse("path startswith \"Projects/\"").unwrap();
        assert_eq!(texts(&execute(&idx, &q, "2026-06-24")), ["one"]);
        let q = parse("path includes \"Work\"").unwrap();
        assert_eq!(texts(&execute(&idx, &q, "2026-06-24")), ["two"]);
    }

    #[test]
    fn tag_filter_is_case_insensitive() {
        let idx = index_of(&[("a.md", "- [ ] x #Work\n- [ ] y #home\n")]);
        let q = parse("tag #work").unwrap();
        assert_eq!(texts(&execute(&idx, &q, "2026-06-24")), ["x #Work"]);
    }

    #[test]
    fn text_includes_is_case_insensitive() {
        let idx = index_of(&[("a.md", "- [ ] Write the SPEC\n- [ ] other\n")]);
        let q = parse("text includes \"spec\"").unwrap();
        assert_eq!(texts(&execute(&idx, &q, "2026-06-24")), ["Write the SPEC"]);
    }

    #[test]
    fn due_comparisons_and_today() {
        let idx = index_of(&[(
            "a.md",
            "- [ ] past 📅 2026-06-01\n- [ ] today 📅 2026-06-24\n- [ ] future 📅 2026-12-31\n- [ ] undated\n",
        )]);
        let today = "2026-06-24";
        // `text` carries the full body incl. the inline `📅`, so match on that.
        assert_eq!(
            texts(&execute(&idx, &parse("due before today").unwrap(), today)),
            ["past 📅 2026-06-01"]
        );
        assert_eq!(
            texts(&execute(&idx, &parse("due after today").unwrap(), today)),
            ["future 📅 2026-12-31"]
        );
        assert_eq!(
            texts(&execute(&idx, &parse("due today").unwrap(), today)),
            ["today 📅 2026-06-24"]
        );
        assert_eq!(
            texts(&execute(&idx, &parse("due on 2026-12-31").unwrap(), today)),
            ["future 📅 2026-12-31"]
        );
    }

    #[test]
    fn no_due_and_has_due() {
        let idx = index_of(&[("a.md", "- [ ] dated 📅 2026-01-01\n- [ ] bare\n")]);
        assert_eq!(
            texts(&execute(&idx, &parse("no due").unwrap(), "2026-06-24")),
            ["bare"]
        );
        assert_eq!(
            texts(&execute(&idx, &parse("has due").unwrap(), "2026-06-24")),
            ["dated 📅 2026-01-01"]
        );
    }

    #[test]
    fn classification_matches_frontmatter() {
        let idx = index_of(&[
            (
                "secret.md",
                "---\nclassification: Secret\n---\n- [ ] hush\n",
            ),
            ("plain.md", "- [ ] open\n"),
        ]);
        let q = parse("classification is Secret").unwrap();
        assert_eq!(texts(&execute(&idx, &q, "2026-06-24")), ["hush"]);
        // Case-insensitive label match.
        let q = parse("classification is secret").unwrap();
        assert_eq!(texts(&execute(&idx, &q, "2026-06-24")), ["hush"]);
    }

    #[test]
    fn sort_by_due_and_path() {
        let idx = index_of(&[
            ("b.md", "- [ ] later 📅 2026-12-01\n"),
            ("a.md", "- [ ] earlier 📅 2026-01-01\n"),
        ]);
        let hits = execute(&idx, &parse("sort by due asc").unwrap(), "2026-06-24");
        assert_eq!(
            texts(&hits),
            ["earlier 📅 2026-01-01", "later 📅 2026-12-01"]
        );
        let hits = execute(&idx, &parse("sort by due desc").unwrap(), "2026-06-24");
        assert_eq!(
            texts(&hits),
            ["later 📅 2026-12-01", "earlier 📅 2026-01-01"]
        );
        let hits = execute(&idx, &parse("sort by path asc").unwrap(), "2026-06-24");
        // a.md before b.md.
        assert_eq!(
            texts(&hits),
            ["earlier 📅 2026-01-01", "later 📅 2026-12-01"]
        );
    }

    #[test]
    fn limit_truncates() {
        let idx = index_of(&[("a.md", "- [ ] one\n- [ ] two\n- [ ] three\n")]);
        let q = parse("sort by path asc\nlimit 2").unwrap();
        assert_eq!(execute(&idx, &q, "2026-06-24").len(), 2);
    }

    #[test]
    fn filters_combine_with_and() {
        let idx = index_of(&[(
            "Projects/a.md",
            "- [ ] hit #work 📅 2026-06-01\n- [ ] miss #work\n- [ ] other 📅 2026-06-01\n",
        )]);
        let q = parse("not done\ntag #work\ndue before today").unwrap();
        assert_eq!(
            texts(&execute(&idx, &q, "2026-06-24")),
            ["hit #work 📅 2026-06-01"]
        );
    }

    #[test]
    fn unknown_and_malformed_directives_error() {
        assert!(parse("group by file").is_err());
        assert!(parse("priority high").is_err());
        assert!(parse("limit abc").is_err());
        assert!(parse("due before yesterday").is_err());
        assert!(parse("due on 2026-13-40").is_err());
        assert!(parse("tag work").is_err()); // missing '#'
        assert!(parse("path startswith Projects/").is_err()); // unquoted
        assert!(parse("sort by name asc").is_err());
        assert!(parse("classification is ").is_err());
    }

    #[test]
    fn blank_lines_are_ignored() {
        let q = parse("\n  \ndone\n\n").unwrap();
        assert_eq!(
            q,
            Query {
                filters: vec![Filter::Done(true)],
                sort: None,
                limit: None
            }
        );
    }
}
