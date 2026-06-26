// SPDX-License-Identifier: GPL-3.0-or-later
//! Rebuildable index cache (SPEC §7) at `.plainmark/index.sqlite`.
//!
//! This is a **cache, never a source of truth**: deleting it (or all of
//! `.plainmark/`) loses nothing — the index rebuilds by parsing the notes. On
//! vault open we load cached entries and reuse any whose `(mtime, size)` still
//! matches the file on disk, so cold start stays fast on large vaults and we
//! avoid re-reading (and, on OneDrive, re-hydrating) unchanged notes.
//!
//! Every cache operation returns `rusqlite::Result`; callers treat a failure as
//! "no cache" and fall back to parsing, so a corrupt or unwritable cache can
//! never break indexing.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection};

use crate::index::{Heading, LinkRef, NoteEntry, Task};

/// Bump when the schema changes; a mismatch drops and recreates the tables.
/// v2 added the `tasks` table and the `classification` column (SPEC §8.5, §11).
const SCHEMA_VERSION: i64 = 2;

pub struct Cache {
    conn: Connection,
}

impl Cache {
    /// Open (creating if needed) the cache db under `vault_root/.plainmark/`.
    pub fn open(vault_root: &Path) -> rusqlite::Result<Self> {
        let dir = vault_root.join(".plainmark");
        // Best-effort: if this fails, Connection::open will surface the error.
        let _ = std::fs::create_dir_all(&dir);
        let conn = Connection::open(dir.join("index.sqlite"))?;
        let cache = Self { conn };
        cache.ensure_schema()?;
        Ok(cache)
    }

    #[cfg(test)]
    fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let cache = Self { conn };
        cache.ensure_schema()?;
        Ok(cache)
    }

    fn ensure_schema(&self) -> rusqlite::Result<()> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap_or(0);
        if version != SCHEMA_VERSION {
            self.recreate()?;
        }
        Ok(())
    }

    fn recreate(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
            "DROP TABLE IF EXISTS notes;
             DROP TABLE IF EXISTS headings;
             DROP TABLE IF EXISTS links;
             DROP TABLE IF EXISTS tasks;
             CREATE TABLE notes (
                 path           TEXT PRIMARY KEY,
                 title          TEXT NOT NULL,
                 mtime          INTEGER NOT NULL,
                 size           INTEGER NOT NULL,
                 classification TEXT
             );
             CREATE TABLE headings (
                 path  TEXT NOT NULL,
                 ord   INTEGER NOT NULL,
                 level INTEGER NOT NULL,
                 text  TEXT NOT NULL,
                 slug  TEXT NOT NULL
             );
             CREATE TABLE links (
                 path    TEXT NOT NULL,
                 ord     INTEGER NOT NULL,
                 target  TEXT NOT NULL,
                 heading TEXT,
                 line    INTEGER NOT NULL,
                 snippet TEXT NOT NULL
             );
             CREATE TABLE tasks (
                 path TEXT NOT NULL,
                 ord  INTEGER NOT NULL,
                 text TEXT NOT NULL,
                 done INTEGER NOT NULL,
                 due  TEXT,
                 line INTEGER NOT NULL,
                 tags TEXT NOT NULL
             );
             CREATE INDEX idx_headings_path ON headings(path);
             CREATE INDEX idx_links_path ON links(path);
             CREATE INDEX idx_tasks_path ON tasks(path);",
        )?;
        self.conn
            .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }

    /// Load every cached note into a map keyed by vault-relative path.
    pub fn load_all(&self) -> rusqlite::Result<HashMap<String, NoteEntry>> {
        let mut map: HashMap<String, NoteEntry> = HashMap::new();

        let mut notes = self
            .conn
            .prepare("SELECT path, title, mtime, size, classification FROM notes")?;
        let rows = notes.query_map([], |r| {
            Ok(NoteEntry {
                rel_path: r.get(0)?,
                title: r.get(1)?,
                headings: Vec::new(),
                outgoing: Vec::new(),
                tasks: Vec::new(),
                mtime: r.get(2)?,
                size: r.get::<_, i64>(3)? as u64,
                classification: r.get(4)?,
            })
        })?;
        for row in rows {
            let entry = row?;
            map.insert(entry.rel_path.clone(), entry);
        }

        let mut headings = self
            .conn
            .prepare("SELECT path, level, text, slug FROM headings ORDER BY path, ord")?;
        let hrows = headings.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                Heading {
                    level: r.get::<_, i64>(1)? as u8,
                    text: r.get(2)?,
                    slug: r.get(3)?,
                },
            ))
        })?;
        for row in hrows {
            let (path, heading) = row?;
            if let Some(entry) = map.get_mut(&path) {
                entry.headings.push(heading);
            }
        }

        let mut links = self
            .conn
            .prepare("SELECT path, target, heading, line, snippet FROM links ORDER BY path, ord")?;
        let lrows = links.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                LinkRef {
                    target: r.get(1)?,
                    heading: r.get(2)?,
                    line: r.get::<_, i64>(3)? as usize,
                    snippet: r.get(4)?,
                },
            ))
        })?;
        for row in lrows {
            let (path, link) = row?;
            if let Some(entry) = map.get_mut(&path) {
                entry.outgoing.push(link);
            }
        }

        let mut tasks = self
            .conn
            .prepare("SELECT path, text, done, due, line, tags FROM tasks ORDER BY path, ord")?;
        let trows = tasks.query_map([], |r| {
            let tags_json: String = r.get(5)?;
            Ok((
                r.get::<_, String>(0)?,
                Task {
                    text: r.get(1)?,
                    done: r.get::<_, i64>(2)? != 0,
                    due: r.get(3)?,
                    line: r.get::<_, i64>(4)? as usize,
                    // Tags are stored as a JSON array; a corrupt value degrades to
                    // no tags rather than failing the whole load.
                    tags: serde_json::from_str(&tags_json).unwrap_or_default(),
                },
            ))
        })?;
        for row in trows {
            let (path, task) = row?;
            if let Some(entry) = map.get_mut(&path) {
                entry.tasks.push(task);
            }
        }

        Ok(map)
    }

    /// Insert or replace all rows for one note.
    pub fn upsert(&mut self, entry: &NoteEntry) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO notes (path, title, mtime, size, classification) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![entry.rel_path, entry.title, entry.mtime, entry.size as i64, entry.classification],
        )?;
        tx.execute(
            "DELETE FROM headings WHERE path = ?1",
            params![entry.rel_path],
        )?;
        tx.execute("DELETE FROM links WHERE path = ?1", params![entry.rel_path])?;
        tx.execute("DELETE FROM tasks WHERE path = ?1", params![entry.rel_path])?;
        for (i, h) in entry.headings.iter().enumerate() {
            tx.execute(
                "INSERT INTO headings (path, ord, level, text, slug) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![entry.rel_path, i as i64, h.level as i64, h.text, h.slug],
            )?;
        }
        for (i, l) in entry.outgoing.iter().enumerate() {
            tx.execute(
                "INSERT INTO links (path, ord, target, heading, line, snippet) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![entry.rel_path, i as i64, l.target, l.heading, l.line as i64, l.snippet],
            )?;
        }
        for (i, t) in entry.tasks.iter().enumerate() {
            let tags_json = serde_json::to_string(&t.tags).unwrap_or_else(|_| "[]".to_string());
            tx.execute(
                "INSERT INTO tasks (path, ord, text, done, due, line, tags) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![entry.rel_path, i as i64, t.text, t.done as i64, t.due, t.line as i64, tags_json],
            )?;
        }
        tx.commit()
    }

    /// Drop all rows for a note (e.g. when it's deleted on disk).
    pub fn remove(&mut self, rel_path: &str) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM notes WHERE path = ?1", params![rel_path])?;
        tx.execute("DELETE FROM headings WHERE path = ?1", params![rel_path])?;
        tx.execute("DELETE FROM links WHERE path = ?1", params![rel_path])?;
        tx.execute("DELETE FROM tasks WHERE path = ?1", params![rel_path])?;
        tx.commit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(rel: &str) -> NoteEntry {
        NoteEntry {
            rel_path: rel.to_string(),
            title: "Plan".to_string(),
            headings: vec![Heading {
                text: "Goals".to_string(),
                slug: "goals".to_string(),
                level: 2,
            }],
            outgoing: vec![LinkRef {
                target: "Other".to_string(),
                heading: Some("Section".to_string()),
                line: 4,
                snippet: "see [[Other#Section]]".to_string(),
            }],
            tasks: vec![Task {
                text: "Ship it #work".to_string(),
                done: false,
                tags: vec!["work".to_string()],
                due: Some("2026-07-01".to_string()),
                line: 6,
            }],
            classification: Some("Secret".to_string()),
            mtime: 1234,
            size: 99,
        }
    }

    #[test]
    fn round_trips_a_note() {
        let mut cache = Cache::open_in_memory().unwrap();
        let entry = sample("Work/Plan.md");
        cache.upsert(&entry).unwrap();

        let loaded = cache.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        let got = &loaded["Work/Plan.md"];
        assert_eq!(got.title, "Plan");
        assert_eq!(got.mtime, 1234);
        assert_eq!(got.size, 99);
        assert_eq!(got.headings, entry.headings);
        assert_eq!(got.outgoing, entry.outgoing);
        assert_eq!(got.tasks, entry.tasks);
        assert_eq!(got.classification.as_deref(), Some("Secret"));
    }

    #[test]
    fn upsert_replaces_prior_rows() {
        let mut cache = Cache::open_in_memory().unwrap();
        cache.upsert(&sample("Plan.md")).unwrap();

        let mut updated = sample("Plan.md");
        updated.headings.clear();
        updated.outgoing.clear();
        updated.tasks.clear();
        updated.classification = None;
        updated.mtime = 5678;
        cache.upsert(&updated).unwrap();

        let loaded = cache.load_all().unwrap();
        let got = &loaded["Plan.md"];
        assert_eq!(got.mtime, 5678);
        assert!(got.headings.is_empty());
        assert!(got.outgoing.is_empty());
        assert!(got.tasks.is_empty());
        assert!(got.classification.is_none());
    }

    #[test]
    fn remove_drops_all_rows() {
        let mut cache = Cache::open_in_memory().unwrap();
        cache.upsert(&sample("Plan.md")).unwrap();
        cache.remove("Plan.md").unwrap();
        assert!(cache.load_all().unwrap().is_empty());
    }

    #[test]
    fn stale_schema_version_recreates_empty() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path();
        {
            let mut cache = Cache::open(vault).unwrap();
            cache.upsert(&sample("Plan.md")).unwrap();
            // Simulate an older/newer schema by forcing a different user_version.
            cache
                .conn
                .pragma_update(None, "user_version", 99i64)
                .unwrap();
        }
        // Reopening detects the mismatch and rebuilds the tables empty.
        let cache = Cache::open(vault).unwrap();
        assert!(cache.load_all().unwrap().is_empty());
    }
}
