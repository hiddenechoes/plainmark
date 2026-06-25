// SPDX-License-Identifier: GPL-3.0-or-later
//! Vault file watcher (SPEC §13 — the project's highest technical risk).
//!
//! The deployment target is a shared OneDrive/SMB folder, where native
//! filesystem events are flaky-to-broken. So this module exposes **two**
//! backends behind one type:
//!
//! - **Native** — `notify`'s recommended OS watcher (inotify/FSEvents/
//!   ReadDirectoryChangesW), wrapped by `notify-debouncer-full` which coalesces
//!   bursts and correlates rename pairs.
//! - **Polling** — `notify::PollWatcher`, which periodically stats the tree. Slow
//!   but reliable over network shares where native events don't fire.
//!
//! Both feed the same pipeline: raw `notify` events are normalized to a small
//! [`IndexEvent`] enum (filtered to `.md`, hidden paths dropped) and coalesced,
//! then handed to a caller-supplied callback. The translation functions
//! ([`normalize_event`], [`coalesce`]) are pure so they can be unit-tested
//! without touching the filesystem or waiting on OS timing.
//!
//! Files-On-Demand note: we never read file *contents* here — only paths. Reading
//! a cloud-only placeholder to index it would force hydration; that happens in
//! the indexer, which the mtime/size cache keeps to a minimum (see `cache.rs`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};
use notify::{Config, EventKind, PollWatcher, RecommendedWatcher};
use notify_debouncer_full::{new_debouncer, new_debouncer_opt, DebounceEventResult, Debouncer};
use notify_debouncer_full::{notify, RecommendedCache};
use serde::{Deserialize, Serialize};

/// Which watcher backend to use. `Auto` currently means native; users on a
/// OneDrive/SMB vault should select `Poll` (see `docs/watcher-spike.md`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WatchMode {
    #[default]
    Auto,
    Native,
    Poll,
}

/// Tunables for the watcher, sourced from `.plainmark/settings.json`.
#[derive(Debug, Clone, Copy)]
pub struct WatchConfig {
    pub mode: WatchMode,
    /// How long to wait for a burst to settle before emitting (debounce window).
    pub debounce: Duration,
    /// Poll interval for the polling backend.
    pub poll_interval: Duration,
    /// In poll mode, hash file contents to detect edits. More reliable than
    /// mtime/size alone (some filesystems and sync clients have coarse or
    /// preserved mtimes), but it reads every watched file each poll — which on
    /// OneDrive Files-On-Demand would force mass hydration. Default on; users on
    /// a OneDrive placeholder vault should set `"pollCompareContents": false`
    /// (or prefer native mode). See `docs/watcher-spike.md`.
    pub poll_compare_contents: bool,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            mode: WatchMode::Auto,
            debounce: Duration::from_millis(400),
            poll_interval: Duration::from_secs(4),
            poll_compare_contents: true,
        }
    }
}

/// A normalized, vault-relevant filesystem change. Only `.md` notes are reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum IndexEvent {
    Created { path: PathBuf },
    Modified { path: PathBuf },
    Removed { path: PathBuf },
    Renamed { from: PathBuf, to: PathBuf },
}

fn is_md(path: &Path) -> bool {
    path.extension()
        .map(|e| e.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

/// Should this path participate in the index? It must be a `.md` file inside
/// `vault_root`, with no dot-prefixed component **below** the root — covering
/// `.plainmark/`, `.git/`, and our own `.note.md.plainmark.tmp` temp files.
///
/// Hidden-ness is judged on the path *relative to the vault*, not the absolute
/// path: the vault itself may legitimately sit under a dotted directory (e.g.
/// `~/.config/notes`), and that must not hide every note within it.
fn is_relevant(path: &Path, vault_root: &Path) -> bool {
    if !is_md(path) {
        return false;
    }
    match path.strip_prefix(vault_root) {
        Ok(rel) => !rel.components().any(|c| {
            c.as_os_str()
                .to_str()
                .map(|s| s.starts_with('.'))
                .unwrap_or(false)
        }),
        // A path outside the watched vault is never relevant.
        Err(_) => false,
    }
}

/// Translate one raw `notify` event into zero or more [`IndexEvent`]s, filtering
/// to relevant `.md` paths under `vault_root`. Pure — construct `notify::Event`s
/// in tests to drive it deterministically without the OS.
pub fn normalize_event(event: &notify::Event, vault_root: &Path) -> Vec<IndexEvent> {
    match &event.kind {
        EventKind::Create(CreateKind::File | CreateKind::Any) => event
            .paths
            .iter()
            .filter(|p| is_relevant(p, vault_root))
            .map(|p| IndexEvent::Created { path: p.clone() })
            .collect(),

        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            // The debouncer correlated a rename pair into [from, to].
            match (event.paths.first(), event.paths.get(1)) {
                (Some(from), Some(to)) => normalize_rename(from, to, vault_root),
                _ => Vec::new(),
            }
        }
        // Uncorrelated rename halves (e.g. polling, or events split across
        // batches): treat the source as removed and the destination as created.
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => event
            .paths
            .iter()
            .filter(|p| is_relevant(p, vault_root))
            .map(|p| IndexEvent::Removed { path: p.clone() })
            .collect(),
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => event
            .paths
            .iter()
            .filter(|p| is_relevant(p, vault_root))
            .map(|p| IndexEvent::Created { path: p.clone() })
            .collect(),

        EventKind::Modify(_) => event
            .paths
            .iter()
            .filter(|p| is_relevant(p, vault_root))
            .map(|p| IndexEvent::Modified { path: p.clone() })
            .collect(),

        EventKind::Remove(RemoveKind::File | RemoveKind::Any) => event
            .paths
            .iter()
            .filter(|p| is_relevant(p, vault_root))
            .map(|p| IndexEvent::Removed { path: p.clone() })
            .collect(),

        // Folder create/remove, access events, etc. don't move a note directly;
        // a directory rename surfaces as per-file events from the OS watcher.
        _ => Vec::new(),
    }
}

/// A correlated rename, accounting for the possibility that only one side is a
/// `.md` note (e.g. `note.md` → `note.txt`, or `draft.txt` → `note.md`).
fn normalize_rename(from: &Path, to: &Path, vault_root: &Path) -> Vec<IndexEvent> {
    match (is_relevant(from, vault_root), is_relevant(to, vault_root)) {
        (true, true) => vec![IndexEvent::Renamed {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
        }],
        (true, false) => vec![IndexEvent::Removed {
            path: from.to_path_buf(),
        }],
        (false, true) => vec![IndexEvent::Created {
            path: to.to_path_buf(),
        }],
        (false, false) => Vec::new(),
    }
}

/// Collapse a debounced batch so each path yields one logical change. Editors and
/// sync clients emit several events per save; the indexer only needs the net
/// effect. Order is preserved by first-seen path. Pure and unit-tested.
///
/// Rules per path: a later `Removed` wins outright; `Created` before a later
/// `Modified` stays `Created`; otherwise the last write wins. `Renamed` events
/// are kept as-is (they carry two distinct paths).
pub fn coalesce(events: Vec<IndexEvent>) -> Vec<IndexEvent> {
    let mut order: Vec<PathBuf> = Vec::new();
    let mut latest: std::collections::HashMap<PathBuf, IndexEvent> =
        std::collections::HashMap::new();
    let mut renames: Vec<IndexEvent> = Vec::new();

    for ev in events {
        let key = match &ev {
            IndexEvent::Created { path }
            | IndexEvent::Modified { path }
            | IndexEvent::Removed { path } => path.clone(),
            IndexEvent::Renamed { .. } => {
                renames.push(ev);
                continue;
            }
        };

        if !latest.contains_key(&key) {
            order.push(key.clone());
        }
        let merged = match (latest.get(&key), &ev) {
            // Once removed, stay removed.
            (_, IndexEvent::Removed { .. }) => ev,
            // Created then modified is still a create.
            (Some(IndexEvent::Created { .. }), IndexEvent::Modified { .. }) => {
                IndexEvent::Created { path: key.clone() }
            }
            // A modify/create after a remove resurrects the file (treat as create).
            (Some(IndexEvent::Removed { .. }), _) => IndexEvent::Created { path: key.clone() },
            _ => ev,
        };
        latest.insert(key, merged);
    }

    let mut out: Vec<IndexEvent> = order
        .into_iter()
        .filter_map(|p| latest.remove(&p))
        .collect();
    out.extend(renames);
    out
}

/// Watcher knobs read from `.plainmark/settings.json` (all optional).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WatchSettings {
    #[serde(default)]
    watch_mode: WatchMode,
    poll_interval_ms: Option<u64>,
    debounce_ms: Option<u64>,
    poll_compare_contents: Option<bool>,
}

/// Build a [`WatchConfig`] from the vault's `.plainmark/settings.json`, falling
/// back to defaults for any missing/invalid field. A user on a OneDrive/SMB vault
/// sets `"watchMode": "poll"` here (see `docs/watcher-spike.md`).
pub fn load_watch_config(vault_root: &Path) -> WatchConfig {
    let path = vault_root.join(".plainmark").join("settings.json");
    let settings: WatchSettings = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();

    let mut cfg = WatchConfig {
        mode: settings.watch_mode,
        ..WatchConfig::default()
    };
    if let Some(ms) = settings.poll_interval_ms {
        cfg.poll_interval = Duration::from_millis(ms);
    }
    if let Some(ms) = settings.debounce_ms {
        cfg.debounce = Duration::from_millis(ms);
    }
    if let Some(compare) = settings.poll_compare_contents {
        cfg.poll_compare_contents = compare;
    }
    cfg
}

/// The live watcher. Dropping it stops watching. Holds the debouncer for whichever
/// backend was selected.
pub struct VaultWatcher {
    _backend: Backend,
}

// The debouncer is held purely for its lifetime: dropping it stops the watch.
// The variant payloads are never read, hence the allow.
#[allow(dead_code)]
enum Backend {
    Native(Debouncer<RecommendedWatcher, RecommendedCache>),
    Poll(Debouncer<PollWatcher, RecommendedCache>),
}

impl VaultWatcher {
    /// Start watching `root` recursively. `on_events` is called (off the main
    /// thread) with each coalesced, normalized batch. Empty batches are skipped.
    pub fn start<F>(root: &Path, config: WatchConfig, on_events: F) -> Result<Self, notify::Error>
    where
        F: Fn(Vec<IndexEvent>) + Send + 'static,
    {
        let root_buf = root.to_path_buf();
        let handler = move |result: DebounceEventResult| {
            if let Ok(events) = result {
                let normalized: Vec<IndexEvent> = events
                    .iter()
                    .flat_map(|e| normalize_event(&e.event, &root_buf))
                    .collect();
                let batch = coalesce(normalized);
                if !batch.is_empty() {
                    on_events(batch);
                }
            }
        };

        let backend = match config.mode {
            WatchMode::Poll => Backend::Poll(Self::start_poll(root, config, handler)?),
            WatchMode::Native | WatchMode::Auto => {
                Backend::Native(Self::start_native(root, config, handler)?)
            }
        };
        Ok(Self { _backend: backend })
    }

    fn start_native<F>(
        root: &Path,
        config: WatchConfig,
        handler: F,
    ) -> Result<Debouncer<RecommendedWatcher, RecommendedCache>, notify::Error>
    where
        F: FnMut(DebounceEventResult) + Send + 'static,
    {
        let mut debouncer = new_debouncer(config.debounce, None, handler)?;
        debouncer.watch(root, notify::RecursiveMode::Recursive)?;
        Ok(debouncer)
    }

    fn start_poll<F>(
        root: &Path,
        config: WatchConfig,
        handler: F,
    ) -> Result<Debouncer<PollWatcher, RecommendedCache>, notify::Error>
    where
        F: FnMut(DebounceEventResult) + Send + 'static,
    {
        let poll_config = Config::default()
            .with_poll_interval(config.poll_interval)
            .with_compare_contents(config.poll_compare_contents);
        let mut debouncer = new_debouncer_opt::<F, PollWatcher, RecommendedCache>(
            config.debounce,
            None,
            handler,
            RecommendedCache::new(),
            poll_config,
        )?;
        debouncer.watch(root, notify::RecursiveMode::Recursive)?;
        Ok(debouncer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::EventAttributes;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    const VAULT: &str = "/v";

    fn event(kind: EventKind, paths: &[&str]) -> notify::Event {
        notify::Event {
            kind,
            paths: paths.iter().map(PathBuf::from).collect(),
            attrs: EventAttributes::default(),
        }
    }

    fn normalize(ev: &notify::Event) -> Vec<IndexEvent> {
        normalize_event(ev, Path::new(VAULT))
    }

    #[test]
    fn normalize_keeps_md_and_drops_others() {
        let create_md = event(EventKind::Create(CreateKind::File), &["/v/a.md"]);
        assert_eq!(
            normalize(&create_md),
            vec![IndexEvent::Created {
                path: PathBuf::from("/v/a.md")
            }]
        );

        let create_txt = event(EventKind::Create(CreateKind::File), &["/v/a.txt"]);
        assert!(normalize(&create_txt).is_empty());
    }

    #[test]
    fn normalize_drops_hidden_and_temp_paths() {
        // Our own atomic-write temp file must never re-enter the index.
        let tmp = event(
            EventKind::Create(CreateKind::File),
            &["/v/.a.md.plainmark.tmp"],
        );
        assert!(normalize(&tmp).is_empty());

        let dotdir = event(
            EventKind::Modify(ModifyKind::Data(notify::event::DataChange::Any)),
            &["/v/.plainmark/x.md"],
        );
        assert!(normalize(&dotdir).is_empty());
    }

    #[test]
    fn normalize_keeps_md_even_when_vault_sits_under_a_dotted_dir() {
        // The vault itself may live under e.g. `~/.config/notes`; that must not
        // hide every note. Hidden-ness is relative to the vault root.
        let ev = event(
            EventKind::Create(CreateKind::File),
            &["/home/u/.config/notes/a.md"],
        );
        assert_eq!(
            normalize_event(&ev, Path::new("/home/u/.config/notes")),
            vec![IndexEvent::Created {
                path: PathBuf::from("/home/u/.config/notes/a.md")
            }]
        );
    }

    #[test]
    fn normalize_correlated_rename_becomes_single_event() {
        let ev = event(
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            &["/v/old.md", "/v/new.md"],
        );
        assert_eq!(
            normalize(&ev),
            vec![IndexEvent::Renamed {
                from: PathBuf::from("/v/old.md"),
                to: PathBuf::from("/v/new.md"),
            }]
        );
    }

    #[test]
    fn normalize_rename_to_non_md_is_a_removal() {
        let ev = event(
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            &["/v/note.md", "/v/note.txt"],
        );
        assert_eq!(
            normalize(&ev),
            vec![IndexEvent::Removed {
                path: PathBuf::from("/v/note.md")
            }]
        );
    }

    #[test]
    fn coalesce_collapses_modify_burst() {
        let burst = vec![
            IndexEvent::Modified {
                path: PathBuf::from("/v/a.md"),
            },
            IndexEvent::Modified {
                path: PathBuf::from("/v/a.md"),
            },
            IndexEvent::Modified {
                path: PathBuf::from("/v/a.md"),
            },
        ];
        assert_eq!(
            coalesce(burst),
            vec![IndexEvent::Modified {
                path: PathBuf::from("/v/a.md")
            }]
        );
    }

    #[test]
    fn coalesce_create_then_modify_stays_create() {
        let burst = vec![
            IndexEvent::Created {
                path: PathBuf::from("/v/a.md"),
            },
            IndexEvent::Modified {
                path: PathBuf::from("/v/a.md"),
            },
        ];
        assert_eq!(
            coalesce(burst),
            vec![IndexEvent::Created {
                path: PathBuf::from("/v/a.md")
            }]
        );
    }

    #[test]
    fn coalesce_modify_then_remove_is_remove() {
        let burst = vec![
            IndexEvent::Modified {
                path: PathBuf::from("/v/a.md"),
            },
            IndexEvent::Removed {
                path: PathBuf::from("/v/a.md"),
            },
        ];
        assert_eq!(
            coalesce(burst),
            vec![IndexEvent::Removed {
                path: PathBuf::from("/v/a.md")
            }]
        );
    }

    #[test]
    fn coalesce_preserves_distinct_paths_in_order() {
        let burst = vec![
            IndexEvent::Modified {
                path: PathBuf::from("/v/b.md"),
            },
            IndexEvent::Modified {
                path: PathBuf::from("/v/a.md"),
            },
        ];
        let out = coalesce(burst);
        assert_eq!(out.len(), 2);
        assert_eq!(
            out[0],
            IndexEvent::Modified {
                path: PathBuf::from("/v/b.md")
            }
        );
    }

    // Live integration test: the polling backend must actually pick up a create,
    // a modify, and a delete in a real directory. Polling is the reliable
    // fallback for OneDrive/SMB, so it's the one we can verify deterministically
    // in CI on every OS.
    #[test]
    fn poll_backend_detects_create_modify_delete() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let (tx, rx) = mpsc::channel::<IndexEvent>();
        let seen = Arc::new(Mutex::new(Vec::<IndexEvent>::new()));

        let config = WatchConfig {
            mode: WatchMode::Poll,
            debounce: Duration::from_millis(120),
            poll_interval: Duration::from_millis(120),
            poll_compare_contents: true,
        };
        let sink = seen.clone();
        let _watcher = VaultWatcher::start(&root, config, move |batch| {
            for ev in batch {
                sink.lock().unwrap().push(ev.clone());
                let _ = tx.send(ev);
            }
        })
        .unwrap();

        // Let the poll backend take its baseline snapshot of the (empty) dir
        // before we create the file, otherwise the file is part of the baseline
        // and never reported as a creation.
        std::thread::sleep(Duration::from_millis(400));

        let note = root.join("note.md");
        std::fs::write(&note, b"hello\n").unwrap();
        // PollWatcher reports a new file as a create; accept a modify too in case
        // the baseline scan happened to include the brand-new file.
        wait_for(
            &rx,
            &seen,
            "create",
            |e| matches!(e, IndexEvent::Created { path } | IndexEvent::Modified { path } if path.ends_with("note.md")),
        );

        std::fs::write(&note, b"hello world, a longer body\n").unwrap();
        wait_for(
            &rx,
            &seen,
            "modify",
            |e| matches!(e, IndexEvent::Modified { path } | IndexEvent::Created { path } if path.ends_with("note.md")),
        );

        std::fs::remove_file(&note).unwrap();
        wait_for(
            &rx,
            &seen,
            "delete",
            |e| matches!(e, IndexEvent::Removed { path } if path.ends_with("note.md")),
        );
    }

    #[test]
    fn load_watch_config_reads_poll_mode_from_settings() {
        let dir = tempdir().unwrap();
        let vault = dir.path();
        std::fs::create_dir_all(vault.join(".plainmark")).unwrap();
        std::fs::write(
            vault.join(".plainmark/settings.json"),
            br#"{"watchMode": "poll", "pollIntervalMs": 1000}"#,
        )
        .unwrap();

        let cfg = load_watch_config(vault);
        assert_eq!(cfg.mode, WatchMode::Poll);
        assert_eq!(cfg.poll_interval, Duration::from_millis(1000));
    }

    #[test]
    fn load_watch_config_defaults_when_absent() {
        let dir = tempdir().unwrap();
        let cfg = load_watch_config(dir.path());
        assert_eq!(cfg.mode, WatchMode::Auto);
    }

    fn wait_for<F>(
        rx: &mpsc::Receiver<IndexEvent>,
        seen: &Arc<Mutex<Vec<IndexEvent>>>,
        label: &str,
        pred: F,
    ) where
        F: Fn(&IndexEvent) -> bool,
    {
        let deadline = Duration::from_secs(10);
        let start = std::time::Instant::now();
        while start.elapsed() < deadline {
            if let Ok(ev) = rx.recv_timeout(Duration::from_millis(500)) {
                if pred(&ev) {
                    return;
                }
            }
        }
        let all = seen.lock().unwrap();
        panic!(
            "watcher did not emit the expected '{label}' event within {deadline:?}; saw: {all:#?}"
        );
    }
}
