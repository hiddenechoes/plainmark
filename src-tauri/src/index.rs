// SPDX-License-Identifier: GPL-3.0-or-later
//! In-memory link index (SPEC §7, §8.8). Parses every note's outgoing
//! `[[wikilinks]]` (incl. `[[Note#Heading]]`) and headings, and answers link
//! resolution and backlink queries.
//!
//! Paths are keyed **vault-relative with forward slashes** (e.g.
//! `Projects/Plan.md`) so the graph is portable across platforms and the same
//! key works on Windows and Unix. The command layer (`main.rs`) translates to and
//! from the absolute paths the webview uses.
//!
//! Backlinks are computed **on demand** from the always-current outgoing-link
//! set rather than maintained as a separate inverted map: resolution can change
//! when notes are added or removed (an ambiguous `[[Note]]` may resolve
//! elsewhere), and recomputing on query avoids a class of incremental-staleness
//! bugs. At the ~10k-note target this is a handful of hashmap lookups per query,
//! and the query runs on note-switch / index-change, not per keystroke.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::Path;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde::Serialize;

use crate::cache::Cache;

/// A heading within a note, with its GitHub-style slug for `#Heading` links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Heading {
    pub text: String,
    pub slug: String,
    pub level: u8,
}

/// One outgoing `[[wikilink]]` occurrence. `target` is the note part (before any
/// `#heading` or `|alias`); `heading` is the optional `#` fragment. `snippet` is
/// the trimmed source line, captured once so backlink queries don't re-read files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkRef {
    pub target: String,
    pub heading: Option<String>,
    pub line: usize,
    pub snippet: String,
}

/// Everything the index knows about one note.
#[derive(Debug, Clone)]
pub struct NoteEntry {
    /// Vault-relative, forward-slash path (the index key).
    pub rel_path: String,
    /// Display title — the filename stem.
    pub title: String,
    pub headings: Vec<Heading>,
    pub outgoing: Vec<LinkRef>,
    /// Modification time (millis since epoch) and size, for the cache's
    /// skip-if-unchanged check.
    pub mtime: i64,
    pub size: u64,
}

/// One inbound link to a note (a backlink / linked mention).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Backlink {
    pub from: String,
    pub line: usize,
    pub snippet: String,
}

/// The in-memory index. `by_stem` maps a lowercased filename stem to the notes
/// that bear it, so bare-name resolution is a single hashmap lookup.
#[derive(Debug, Default)]
pub struct Index {
    notes: HashMap<String, NoteEntry>,
    by_stem: HashMap<String, Vec<String>>,
}

impl Index {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn get(&self, rel_path: &str) -> Option<&NoteEntry> {
        self.notes.get(rel_path)
    }

    /// All notes, for autocomplete / the link-target snapshot.
    pub fn entries(&self) -> impl Iterator<Item = &NoteEntry> {
        self.notes.values()
    }

    /// Insert or replace a note, keeping `by_stem` consistent.
    pub fn insert(&mut self, entry: NoteEntry) {
        let rel = entry.rel_path.clone();
        if self.notes.contains_key(&rel) {
            self.detach_stem(&rel);
        }
        let stem = stem_of(&rel).to_lowercase();
        self.by_stem.entry(stem).or_default().push(rel.clone());
        self.notes.insert(rel, entry);
    }

    /// Remove a note (e.g. on delete), keeping `by_stem` consistent.
    pub fn remove(&mut self, rel_path: &str) {
        if self.notes.remove(rel_path).is_some() {
            self.detach_stem(rel_path);
        }
    }

    fn detach_stem(&mut self, rel_path: &str) {
        let stem = stem_of(rel_path).to_lowercase();
        if let Some(paths) = self.by_stem.get_mut(&stem) {
            paths.retain(|p| p != rel_path);
            if paths.is_empty() {
                self.by_stem.remove(&stem);
            }
        }
    }

    /// Resolve a `[[target]]` (the note part only) to a vault-relative path,
    /// from the perspective of the note at `from_rel` (SPEC §8.8): exact path
    /// match wins; otherwise shortest-unique-path by filename, preferring the
    /// same folder as `from_rel`, then fewest path segments, then lexicographic.
    pub fn resolve(&self, target: &str, from_rel: &str) -> Option<String> {
        let t = normalize_target(target);
        if t.is_empty() {
            return None;
        }

        // Exact path-qualified match, e.g. `[[Projects/Plan]]` or `[[a/b.md]]`.
        let with_md = if t.ends_with(".md") {
            t.clone()
        } else {
            format!("{t}.md")
        };
        if self.notes.contains_key(&with_md) {
            return Some(with_md);
        }

        // Bare-name resolution on the final path segment.
        let name = t.trim_end_matches(".md").rsplit('/').next().unwrap_or("");
        let candidates = self.by_stem.get(&name.to_lowercase())?;
        self.choose(candidates, from_rel)
    }

    /// Pick the best candidate per the §8.8 tiebreak order.
    fn choose(&self, candidates: &[String], from_rel: &str) -> Option<String> {
        let from_dir = dir_of(from_rel);
        candidates
            .iter()
            .min_by(|a, b| sort_key(a, from_dir).cmp(&sort_key(b, from_dir)))
            .cloned()
    }

    /// True if `target` resolves to a real note (used for resolved/unresolved
    /// rendering). Also reports whether the optional `#heading` exists.
    pub fn link_status(&self, target: &str, heading: Option<&str>, from_rel: &str) -> LinkStatus {
        match self.resolve(target, from_rel) {
            Some(path) => {
                let heading_ok = match heading {
                    None => true,
                    Some(h) => self
                        .notes
                        .get(&path)
                        .map(|n| heading_matches(&n.headings, h))
                        .unwrap_or(false),
                };
                LinkStatus {
                    path: Some(path),
                    heading_ok,
                }
            }
            None => LinkStatus {
                path: None,
                heading_ok: false,
            },
        }
    }

    /// Every note that links to `target_rel`, with context. Computed on demand.
    pub fn backlinks(&self, target_rel: &str) -> Vec<Backlink> {
        let mut out = Vec::new();
        for (from, note) in &self.notes {
            if from == target_rel {
                continue; // a note's links to itself aren't backlinks
            }
            for link in &note.outgoing {
                if self.resolve(&link.target, from).as_deref() == Some(target_rel) {
                    out.push(Backlink {
                        from: from.clone(),
                        line: link.line,
                        snippet: link.snippet.clone(),
                    });
                }
            }
        }
        out.sort_by(|a, b| a.from.cmp(&b.from).then(a.line.cmp(&b.line)));
        out
    }
}

/// The result of resolving a link: which note (if any) and whether its `#heading`
/// part (if given) exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkStatus {
    pub path: Option<String>,
    pub heading_ok: bool,
}

/// Sort key for §8.8 candidate selection: same-folder first, then fewest path
/// segments (shortest path), then lexicographic for determinism.
fn sort_key<'a>(path: &'a str, from_dir: &str) -> (u8, usize, &'a str) {
    let same_folder = if dir_of(path) == from_dir { 0 } else { 1 };
    (same_folder, path.matches('/').count(), path)
}

/// Directory portion of a vault-relative path (`""` for a top-level note).
fn dir_of(rel_path: &str) -> &str {
    match rel_path.rsplit_once('/') {
        Some((dir, _)) => dir,
        None => "",
    }
}

/// Filename stem (no directory, no `.md`).
fn stem_of(rel_path: &str) -> &str {
    let file = rel_path.rsplit('/').next().unwrap_or(rel_path);
    file.strip_suffix(".md").unwrap_or(file)
}

/// Tidy a raw `[[target]]` for resolution: trim, normalize separators, drop a
/// leading `./`, and trim surrounding slashes.
fn normalize_target(target: &str) -> String {
    target
        .trim()
        .replace('\\', "/")
        .trim_start_matches("./")
        .trim_matches('/')
        .to_string()
}

/// Does any heading match the requested `#fragment`? Matches the GitHub slug or
/// the raw heading text, case-insensitively, so both `[[N#My Heading]]` and
/// `[[N#my-heading]]` resolve.
fn heading_matches(headings: &[Heading], requested: &str) -> bool {
    let req_slug = slugify(requested);
    let req_lower = requested.trim().to_lowercase();
    headings
        .iter()
        .any(|h| h.slug == req_slug || h.text.to_lowercase() == req_lower)
}

/// Build the whole index for a vault: load the SQLite cache, reuse entries whose
/// `(mtime, size)` still match, parse the rest, write changes back to the cache,
/// and prune notes that no longer exist on disk. Cache failures degrade to a
/// full parse — the cache is an optimization, never required.
pub fn build_index(vault_root: &Path) -> Index {
    let mut cache = Cache::open(vault_root).ok();
    let cached: HashMap<String, NoteEntry> = cache
        .as_ref()
        .and_then(|c| c.load_all().ok())
        .unwrap_or_default();

    let files = crate::fs_ops::list_md_files(vault_root).unwrap_or_default();
    let mut index = Index::new();
    let mut seen: HashSet<String> = HashSet::new();

    for abs in files {
        let Some(rel) = to_rel(vault_root, &abs) else {
            continue;
        };
        seen.insert(rel.clone());
        let (mtime, size) = file_stat(&abs);

        if let Some(cached_entry) = cached.get(&rel) {
            if cached_entry.mtime == mtime && cached_entry.size == size {
                index.insert(cached_entry.clone());
                continue;
            }
        }

        if let Ok(note) = crate::fs_ops::read_note(&abs) {
            let entry = build_entry(rel, mtime, size, &note.content);
            if let Some(c) = cache.as_mut() {
                let _ = c.upsert(&entry);
            }
            index.insert(entry);
        }
    }

    // Prune cache rows for notes that have since disappeared.
    if let Some(c) = cache.as_mut() {
        for rel in cached.keys() {
            if !seen.contains(rel) {
                let _ = c.remove(rel);
            }
        }
    }

    index
}

/// Rename/move a note and rewrite every inbound `[[link]]` so it still resolves
/// (SPEC §8.2 + §7.1). The scariest operation in the app, so it is precise and
/// batched: the file is moved, then for each note that linked to it we re-read
/// the current bytes, replace **only** the link-target text of the occurrences
/// that resolved to the old note (preserving any `#heading`/`|alias`), and save
/// via the atomic path — which preserves each file's own EOL/BOM. `[[links]]` in
/// code are left untouched. The in-memory index is updated to match.
///
/// Resolution is done against the index **before** it's updated (it still maps
/// the old name), which is why inbound links are collected and rewritten first.
pub fn perform_rename(
    vault_root: &Path,
    index: &mut Index,
    old_rel: &str,
    new_rel: &str,
) -> crate::error::AppResult<()> {
    let old_abs = vault_root.join(old_rel);
    let new_abs = vault_root.join(new_rel);

    // Collect the distinct notes that link to the old note, before any change.
    let froms: Vec<String> = {
        let mut set = HashSet::new();
        for b in index.backlinks(old_rel) {
            set.insert(b.from);
        }
        set.into_iter().collect()
    };

    if let Some(parent) = new_abs.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&old_abs, &new_abs)?;

    // Rewrite inbound links (index still resolves the old name).
    for from_rel in &froms {
        let from_abs = vault_root.join(from_rel);
        if let Ok(note) = crate::fs_ops::read_note(&from_abs) {
            if let Some(rewritten) =
                rewrite_inbound_links(&note.content, from_rel, index, old_rel, new_rel)
            {
                crate::fs_ops::save_note(&from_abs, &rewritten, &note.eol, note.bom)?;
            }
        }
    }

    // Update the index to match disk: drop the old, (re)index new + rewritten.
    index.remove(old_rel);
    reindex_path(index, vault_root, &new_abs);
    for from_rel in &froms {
        reindex_path(index, vault_root, &vault_root.join(from_rel));
    }
    Ok(())
}

/// Rewrite `[[links]]` in `content` (from the note at `from_rel`) that resolve to
/// `old_rel`, pointing them at `new_rel`. Bare targets become the new filename
/// stem (or the new path if that stem is ambiguous); path-qualified targets
/// become the new path. `#heading`/`|alias` and all other bytes are preserved;
/// links in code spans/blocks and `![[embeds]]` are skipped. Returns `None` if
/// nothing changed.
pub fn rewrite_inbound_links(
    content: &str,
    from_rel: &str,
    index: &Index,
    old_rel: &str,
    new_rel: &str,
) -> Option<String> {
    let new_stem = stem_of(new_rel);
    let new_path_ref = new_rel.strip_suffix(".md").unwrap_or(new_rel);
    let ambiguous = index
        .entries()
        .any(|e| e.rel_path != old_rel && stem_of(&e.rel_path).eq_ignore_ascii_case(new_stem));
    let new_bare = if ambiguous { new_path_ref } else { new_stem };

    let (_, code_ranges) = headings_and_code_ranges(content);
    let bytes = content.as_bytes();
    let mut out = String::with_capacity(content.len());
    let mut i = 0;
    let mut last = 0;
    let mut changed = false;

    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            if let Some(close) = find_close(bytes, i + 2) {
                let inner = &content[i + 2..close];
                let is_embed = i > 0 && bytes[i - 1] == b'!';
                let in_code = code_ranges.iter().any(|r| r.contains(&i));
                if !is_embed && !in_code {
                    let (before_alias, alias) = match inner.split_once('|') {
                        Some((b, a)) => (b, Some(a)),
                        None => (inner, None),
                    };
                    let (target_raw, heading) = match before_alias.split_once('#') {
                        Some((t, h)) => (t, Some(h)),
                        None => (before_alias, None),
                    };
                    let target = target_raw.trim();
                    if !target.is_empty()
                        && index.resolve(target, from_rel).as_deref() == Some(old_rel)
                    {
                        let replacement = if target.contains('/') {
                            new_path_ref
                        } else {
                            new_bare
                        };
                        out.push_str(&content[last..i]);
                        out.push_str("[[");
                        out.push_str(replacement);
                        if let Some(h) = heading {
                            out.push('#');
                            out.push_str(h);
                        }
                        if let Some(a) = alias {
                            out.push('|');
                            out.push_str(a);
                        }
                        out.push_str("]]");
                        last = close + 2;
                        changed = true;
                    }
                }
                i = close + 2;
                continue;
            }
        }
        i += 1;
    }

    if !changed {
        return None;
    }
    out.push_str(&content[last..]);
    Some(out)
}

/// Apply one watcher [`IndexEvent`](crate::watcher::IndexEvent) to the live
/// index, keeping it in step with the filesystem. (Rename *link-rewriting* is a
/// separate, deliberate command; this only keeps the graph current.)
pub fn apply_event(index: &mut Index, vault_root: &Path, event: &crate::watcher::IndexEvent) {
    use crate::watcher::IndexEvent;
    match event {
        IndexEvent::Created { path } | IndexEvent::Modified { path } => {
            reindex_path(index, vault_root, path);
        }
        IndexEvent::Removed { path } => {
            if let Some(rel) = to_rel(vault_root, path) {
                index.remove(&rel);
            }
        }
        IndexEvent::Renamed { from, to } => {
            if let Some(rel) = to_rel(vault_root, from) {
                index.remove(&rel);
            }
            reindex_path(index, vault_root, to);
        }
    }
}

/// Reparse a single file (by absolute path) and upsert it into the index.
pub fn reindex_path(index: &mut Index, vault_root: &Path, abs: &Path) {
    if let Some(rel) = to_rel(vault_root, abs) {
        let (mtime, size) = file_stat(abs);
        if let Ok(note) = crate::fs_ops::read_note(abs) {
            index.insert(build_entry(rel, mtime, size, &note.content));
        }
    }
}

/// Parse `content` and assemble a [`NoteEntry`] for the note at `rel`.
pub fn build_entry(rel: String, mtime: i64, size: u64, content: &str) -> NoteEntry {
    let (headings, outgoing) = parse_note(content);
    let title = stem_of(&rel).to_string();
    NoteEntry {
        rel_path: rel,
        title,
        headings,
        outgoing,
        mtime,
        size,
    }
}

/// Vault-relative, forward-slash path for `abs`, or `None` if it's outside.
pub fn to_rel(vault_root: &Path, abs: &Path) -> Option<String> {
    abs.strip_prefix(vault_root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
}

/// `(mtime_millis, size)` for a file; `(0, 0)` if it can't be stat'd.
pub fn file_stat(path: &Path) -> (i64, u64) {
    match std::fs::metadata(path) {
        Ok(meta) => {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            (mtime, meta.len())
        }
        Err(_) => (0, 0),
    }
}

/// Parse a note's LF-normalized `content` into its headings and outgoing links.
/// `[[...]]` inside code spans/blocks and `![[embeds]]` are excluded.
pub fn parse_note(content: &str) -> (Vec<Heading>, Vec<LinkRef>) {
    let (headings, code_ranges) = headings_and_code_ranges(content);
    let links = scan_links(content, &code_ranges);
    (headings, links)
}

/// One pulldown-cmark pass to collect heading text (+ slug + level) and the byte
/// ranges of code spans and fenced code blocks (so links there are ignored).
fn headings_and_code_ranges(src: &str) -> (Vec<Heading>, Vec<Range<usize>>) {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_MATH);

    let mut headings = Vec::new();
    let mut code_ranges = Vec::new();
    let mut current: Option<(u8, String)> = None;
    let mut code_depth: u32 = 0;

    for (event, range) in Parser::new_ext(src, options).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                current = Some((heading_level(level), String::new()));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, text)) = current.take() {
                    let trimmed = text.trim().to_string();
                    let slug = slugify(&trimmed);
                    headings.push(Heading {
                        text: trimmed,
                        slug,
                        level,
                    });
                }
            }
            Event::Start(Tag::CodeBlock(_)) => {
                code_depth += 1;
                code_ranges.push(range);
            }
            Event::End(TagEnd::CodeBlock) => {
                code_depth = code_depth.saturating_sub(1);
            }
            Event::Code(ref text) => {
                code_ranges.push(range);
                if let Some((_, acc)) = current.as_mut() {
                    acc.push_str(text);
                }
            }
            Event::Text(ref text) => {
                if code_depth > 0 {
                    code_ranges.push(range);
                }
                if let Some((_, acc)) = current.as_mut() {
                    acc.push_str(text);
                }
            }
            _ => {}
        }
    }

    (headings, code_ranges)
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Scan raw source for `[[target#heading|alias]]`, skipping embeds (`![[...]]`)
/// and any match inside a code range. Aliases are parsed off and ignored (out of
/// scope this phase); the alias text never affects resolution.
fn scan_links(src: &str, code_ranges: &[Range<usize>]) -> Vec<LinkRef> {
    let bytes = src.as_bytes();
    let mut links = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            if let Some(close) = find_close(bytes, i + 2) {
                let inner = &src[i + 2..close];
                let is_embed = i > 0 && bytes[i - 1] == b'!';
                let in_code = code_ranges.iter().any(|r| r.contains(&i));
                if !is_embed && !in_code {
                    if let Some(link) = parse_link(inner, src, i) {
                        links.push(link);
                    }
                }
                i = close + 2;
                continue;
            }
        }
        i += 1;
    }
    links
}

/// Index of the first `]]` at or after `from`, but not past a newline (a wiki
/// link never spans lines). Returns the offset of the first `]`.
fn find_close(bytes: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i + 1 < bytes.len() {
        match bytes[i] {
            b'\n' => return None,
            b']' if bytes[i + 1] == b']' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// Split `target#heading|alias` and build a [`LinkRef`] anchored at byte `offset`.
fn parse_link(inner: &str, src: &str, offset: usize) -> Option<LinkRef> {
    let before_alias = inner.split('|').next().unwrap_or("");
    let mut parts = before_alias.splitn(2, '#');
    let target = parts.next().unwrap_or("").trim().to_string();
    let heading = parts
        .next()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty());
    if target.is_empty() {
        return None; // e.g. `[[#Heading]]` (same-note links are out of scope)
    }
    Some(LinkRef {
        target,
        heading,
        line: line_of(src, offset),
        snippet: line_text(src, offset).trim().to_string(),
    })
}

/// 1-based line number of byte `offset`.
fn line_of(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count()
        + 1
}

/// The full source line containing byte `offset`.
fn line_text(src: &str, offset: usize) -> &str {
    let start = src[..offset.min(src.len())]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let end = src[start..]
        .find('\n')
        .map(|i| start + i)
        .unwrap_or(src.len());
    &src[start..end]
}

/// GitHub-style heading slug: lowercase, drop punctuation other than spaces,
/// hyphens, and underscores, then map spaces to hyphens.
fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.trim().chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else if ch == ' ' || ch == '-' || ch == '_' {
            out.push('-');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(rel: &str, content: &str) -> NoteEntry {
        let (headings, outgoing) = parse_note(content);
        NoteEntry {
            rel_path: rel.to_string(),
            title: stem_of(rel).to_string(),
            headings,
            outgoing,
            mtime: 0,
            size: content.len() as u64,
        }
    }

    #[test]
    fn parses_basic_links_and_headings() {
        let (headings, links) = parse_note("# Title\n\nSee [[Other Note]] and [[Plan#Goals]].\n");
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].text, "Title");
        assert_eq!(headings[0].slug, "title");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].target, "Other Note");
        assert_eq!(links[0].heading, None);
        assert_eq!(links[0].line, 3);
        assert_eq!(links[1].target, "Plan");
        assert_eq!(links[1].heading.as_deref(), Some("Goals"));
    }

    #[test]
    fn ignores_embeds_and_aliases() {
        let (_, links) = parse_note("![[image.png]] but [[Real|shown text]] counts\n");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "Real");
        // Alias is parsed off; it must not leak into the target.
        assert!(links[0].heading.is_none());
    }

    #[test]
    fn ignores_links_in_code() {
        let content =
            "Inline `[[NotALink]]` and a block:\n\n```\n[[AlsoNot]]\n```\n\n[[RealOne]]\n";
        let (_, links) = parse_note(content);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "RealOne");
    }

    #[test]
    fn heading_slug_handles_punctuation() {
        let (headings, _) = parse_note("## My Heading: Part 2!\n");
        assert_eq!(headings[0].slug, "my-heading-part-2");
    }

    #[test]
    fn resolves_bare_name_case_insensitively() {
        let mut idx = Index::new();
        idx.insert(entry("Plan.md", ""));
        assert_eq!(idx.resolve("plan", "Inbox.md").as_deref(), Some("Plan.md"));
        assert_eq!(idx.resolve("Missing", "Inbox.md"), None);
    }

    #[test]
    fn resolves_prefers_same_folder_on_collision() {
        let mut idx = Index::new();
        idx.insert(entry("Work/Plan.md", ""));
        idx.insert(entry("Personal/Plan.md", ""));
        // From a Work note, `[[Plan]]` should resolve to Work/Plan.md.
        assert_eq!(
            idx.resolve("Plan", "Work/Tasks.md").as_deref(),
            Some("Work/Plan.md")
        );
        // From elsewhere, the tiebreak is fewest segments then lexicographic;
        // both have one segment, so "Personal/..." wins lexicographically.
        assert_eq!(
            idx.resolve("Plan", "Inbox.md").as_deref(),
            Some("Personal/Plan.md")
        );
    }

    #[test]
    fn resolves_shortest_path_over_deeper() {
        let mut idx = Index::new();
        idx.insert(entry("Plan.md", ""));
        idx.insert(entry("Archive/Old/Plan.md", ""));
        // From an unrelated folder, the top-level (fewest segments) wins.
        assert_eq!(
            idx.resolve("Plan", "Notes/X.md").as_deref(),
            Some("Plan.md")
        );
    }

    #[test]
    fn resolves_exact_path_qualified() {
        let mut idx = Index::new();
        idx.insert(entry("Work/Plan.md", ""));
        idx.insert(entry("Personal/Plan.md", ""));
        assert_eq!(
            idx.resolve("Personal/Plan", "Work/X.md").as_deref(),
            Some("Personal/Plan.md")
        );
    }

    #[test]
    fn link_status_reports_heading_existence() {
        let mut idx = Index::new();
        idx.insert(entry("Plan.md", "# Goals\n\nbody\n"));
        let ok = idx.link_status("Plan", Some("Goals"), "X.md");
        assert_eq!(ok.path.as_deref(), Some("Plan.md"));
        assert!(ok.heading_ok);

        let bad = idx.link_status("Plan", Some("Nonexistent"), "X.md");
        assert_eq!(bad.path.as_deref(), Some("Plan.md"));
        assert!(!bad.heading_ok);
    }

    #[test]
    fn backlinks_are_computed_and_sorted() {
        let mut idx = Index::new();
        idx.insert(entry("Target.md", "# Target\n"));
        idx.insert(entry("B.md", "links to [[Target]] here\n"));
        idx.insert(entry("A.md", "also [[Target]]\nand again [[Target]]\n"));
        idx.insert(entry("Unrelated.md", "nothing\n"));

        let backs = idx.backlinks("Target.md");
        assert_eq!(backs.len(), 3);
        // Sorted by from-path then line: A.md (l1), A.md (l2), B.md (l1).
        assert_eq!(backs[0].from, "A.md");
        assert_eq!(backs[0].line, 1);
        assert_eq!(backs[1].from, "A.md");
        assert_eq!(backs[1].line, 2);
        assert_eq!(backs[2].from, "B.md");
        assert!(backs[0].snippet.contains("[[Target]]"));
    }

    #[test]
    fn incremental_remove_updates_backlinks_and_resolution() {
        let mut idx = Index::new();
        idx.insert(entry("Target.md", ""));
        idx.insert(entry("B.md", "[[Target]]\n"));
        assert_eq!(idx.backlinks("Target.md").len(), 1);

        idx.remove("B.md");
        assert_eq!(idx.backlinks("Target.md").len(), 0);
        assert_eq!(idx.len(), 1);

        // Removing the last note of a stem clears it from resolution.
        idx.remove("Target.md");
        assert_eq!(idx.resolve("Target", "X.md"), None);
        assert!(idx.is_empty());
    }

    #[test]
    fn build_index_walks_vault_and_reuses_cache() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path();
        std::fs::create_dir_all(vault.join("Work")).unwrap();
        std::fs::write(vault.join("Work/Plan.md"), "# Plan\n\n[[Inbox]]\n").unwrap();
        std::fs::write(vault.join("Inbox.md"), "inbox\n").unwrap();

        let idx = build_index(vault);
        assert_eq!(idx.len(), 2);
        assert_eq!(
            idx.resolve("Inbox", "Work/Plan.md").as_deref(),
            Some("Inbox.md")
        );
        assert_eq!(idx.backlinks("Inbox.md").len(), 1);

        // A second build should reuse the cache (entries unchanged) and produce
        // the same graph. The .plainmark/index.sqlite file now exists.
        assert!(vault.join(".plainmark/index.sqlite").exists());
        let idx2 = build_index(vault);
        assert_eq!(idx2.len(), 2);
        assert_eq!(idx2.backlinks("Inbox.md").len(), 1);
    }

    #[test]
    fn rewrite_inbound_links_changes_only_resolving_targets() {
        let mut idx = Index::new();
        idx.insert(entry("Old.md", ""));
        let from = "note.md";
        // Bare link resolves to Old → rewritten; a code span and a non-matching
        // link are left alone.
        let content = "see [[Old]] and `[[Old]]` and [[Other]]\n";
        let out = rewrite_inbound_links(content, from, &idx, "Old.md", "New.md").unwrap();
        assert_eq!(out, "see [[New]] and `[[Old]]` and [[Other]]\n");
    }

    #[test]
    fn rewrite_preserves_heading_and_alias() {
        let mut idx = Index::new();
        idx.insert(entry("Old.md", "# Goals\n"));
        let out =
            rewrite_inbound_links("[[Old#Goals|My Plan]]\n", "n.md", &idx, "Old.md", "New.md")
                .unwrap();
        assert_eq!(out, "[[New#Goals|My Plan]]\n");
    }

    #[test]
    fn rewrite_path_qualified_target_uses_new_path() {
        let mut idx = Index::new();
        idx.insert(entry("Work/Old.md", ""));
        let out =
            rewrite_inbound_links("[[Work/Old]]\n", "n.md", &idx, "Work/Old.md", "Work/New.md")
                .unwrap();
        assert_eq!(out, "[[Work/New]]\n");
    }

    // The headline §8.2 + §7.1 test: a rename rewrites inbound links across
    // multiple files, atomically, preserving each file's EOL/BOM byte-for-byte,
    // and never touches `[[links]]` inside code.
    #[test]
    fn perform_rename_rewrites_across_files_preserving_eol_bom() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path();

        std::fs::write(vault.join("Old.md"), b"# Old\n").unwrap();

        // Linker A: UTF-8 BOM + CRLF.
        let a_original = {
            let mut v = vec![0xEF, 0xBB, 0xBF];
            v.extend_from_slice(b"see [[Old]] here\r\n");
            v
        };
        std::fs::write(vault.join("A.md"), &a_original).unwrap();

        // Linker B: plain LF, with a real link in prose and a fenced-code link
        // that must NOT be rewritten.
        std::fs::write(vault.join("B.md"), b"link [[Old]]\n\n```\n[[Old]]\n```\n").unwrap();

        let mut idx = build_index(vault);
        assert_eq!(idx.backlinks("Old.md").len(), 2);

        perform_rename(vault, &mut idx, "Old.md", "New.md").unwrap();

        // File moved on disk.
        assert!(!vault.join("Old.md").exists());
        assert!(vault.join("New.md").exists());

        // A: BOM + CRLF preserved, only the target text changed.
        let a_expected = {
            let mut v = vec![0xEF, 0xBB, 0xBF];
            v.extend_from_slice(b"see [[New]] here\r\n");
            v
        };
        assert_eq!(std::fs::read(vault.join("A.md")).unwrap(), a_expected);

        // B: prose link rewritten; the fenced-code link untouched.
        assert_eq!(
            std::fs::read(vault.join("B.md")).unwrap(),
            b"link [[New]]\n\n```\n[[Old]]\n```\n"
        );

        // Index now resolves the new name and reports both backlinks.
        assert_eq!(idx.resolve("New", "A.md").as_deref(), Some("New.md"));
        assert_eq!(idx.resolve("Old", "A.md"), None);
        assert_eq!(idx.backlinks("New.md").len(), 2);
    }

    #[test]
    fn reinsert_replaces_outgoing_links() {
        let mut idx = Index::new();
        idx.insert(entry("Target.md", ""));
        idx.insert(entry("B.md", "[[Target]]\n"));
        assert_eq!(idx.backlinks("Target.md").len(), 1);

        // Re-index B with the link removed.
        idx.insert(entry("B.md", "no links now\n"));
        assert_eq!(idx.backlinks("Target.md").len(), 0);
    }
}
