<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
# Watcher spike — file-watching over OneDrive / SMB

SPEC §13 calls file-watching over OneDrive/SMB the project's **highest technical
risk**: native filesystem watchers are flaky-to-broken over network shares, and
OneDrive *Files-On-Demand* placeholders can turn indexing into a mass-hydration
event. This note records the Phase 2 spike, the design we shipped, and a manual
test script to run on a **real** OneDrive/SMB vault (which can't be exercised from
the Linux CI box — see "What was verified" below).

## Design we shipped

`src-tauri/src/watcher.rs` exposes one type, `VaultWatcher`, over two backends:

| Backend | Implementation | When to use |
|---|---|---|
| **Native** | `notify`'s recommended OS watcher (inotify / FSEvents / ReadDirectoryChangesW) wrapped by `notify-debouncer-full` | Local disks; most OneDrive setups where the sync client writes real files locally |
| **Polling** | `notify::PollWatcher` (periodic tree stat), same debouncer | Network/SMB shares and any vault where native events don't fire reliably |

Both backends feed an identical pipeline:

1. Raw `notify` events → `normalize_event()` — filters to `.md` files inside the
   vault, drops hidden paths (`.plainmark/`, `.git/`, our own
   `.note.md.plainmark.tmp` atomic-write temp files). Hidden-ness is judged on the
   path **relative to the vault root**, so a vault under a dotted directory (e.g.
   `~/.config/notes`) still indexes.
2. `coalesce()` — collapses a debounced burst so each path yields one logical
   change (editors and sync clients emit several events per save).
3. The resulting `Vec<IndexEvent>` is handed to a callback, which (in `main.rs`)
   re-emits each change to the webview as a `note://changed` event.

Both `normalize_event()` and `coalesce()` are **pure functions**, unit-tested by
constructing `notify::Event` values directly — no filesystem, no OS-timing
flakiness in CI.

### Configuration (`.plainmark/settings.json`)

```jsonc
{
  "watchMode": "auto",          // "auto" (= native) | "native" | "poll"
  "pollIntervalMs": 4000,       // how often poll mode stats the tree
  "debounceMs": 400,            // burst-settling window before emitting
  "pollCompareContents": true   // poll mode: hash contents to detect edits
}
```

Defaults: `auto`/native, 4 s poll, 400 ms debounce, content-compare on.

## What was verified (Linux CI box)

Runnable here and covered by `cargo test`:

- **Native (inotify)** compiles and the debouncer wiring is exercised by the
  build.
- **Polling** end-to-end: `poll_backend_detects_create_modify_delete` creates,
  modifies, and deletes a note in a temp dir and asserts the watcher emits
  `Created` / `Modified` / `Removed`.
- **Normalization & coalescing**: rename-pair correlation, `.md` filtering,
  hidden-path dropping (incl. the dotted-vault case), and burst-collapse rules.
- **Settings loading** from `.plainmark/settings.json`.

**Finding — poll detection sensitivity.** On the CI box (`/tmp`, tmpfs),
`PollWatcher` with the default mtime+size comparison did **not** reliably report
edits; enabling content comparison (`with_compare_contents(true)`) made detection
immediate and deterministic. So `pollCompareContents` defaults to **on**. This is
the right default for SMB shares (real files) but has a cost on OneDrive — see
below.

## OneDrive Files-On-Demand & hydration — the risk to verify on real hardware

OneDrive *Files-On-Demand* leaves cloud-only files as **placeholders** on disk.
Reading a placeholder's bytes forces the sync client to **hydrate** (download) it.
Two places could trigger mass hydration:

1. **The indexer** (Phase 2, `index.rs`/`cache.rs`) must read each `.md` to parse
   links/headings. Mitigation: the SQLite cache keyed by `(mtime, size)` skips
   re-reading unchanged files, so hydration is a one-time cost per file, not per
   launch. (Stat/metadata reads do **not** hydrate; content reads do.)
2. **Poll mode with `pollCompareContents: true`** reads every watched file each
   poll to hash it → it would re-hydrate the whole vault repeatedly. **On a
   OneDrive Files-On-Demand vault, set `"pollCompareContents": false`, or prefer
   native mode.** The watcher itself never reads file contents in native mode or
   in poll mode with compare-contents off — it only stats paths.

**Windows placeholder detection (future hardening).** Windows marks cloud-only
placeholders with `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` (0x00400000) and
`FILE_ATTRIBUTE_OFFLINE` (0x1000). A future pass can check these (via
`std::os::windows::fs::MetadataExt::file_attributes()`) and **skip** hydrating
cloud-only notes during indexing, indexing them lazily on first open instead. Not
implemented this phase; recorded here so the strategy is on record.

## Manual test script — run on a REAL OneDrive / SMB vault

This is the part the Linux CI box cannot do. Run it on a Windows machine with the
OneDrive client (and, separately, against a mounted SMB share).

### A. Native events on a OneDrive-synced folder
1. Put a vault inside your OneDrive folder; open it in plainmark (`watchMode:
   "auto"`).
2. Open `note-A.md` in plainmark. In a **second** editor (e.g. Notepad/VS Code),
   edit and save `note-A.md`. → plainmark should reload it (clean buffer) or show
   the "changed underneath you" prompt (unsaved edits). **Note the latency.**
3. From the second editor: **create** `note-B.md`, **rename** it to `note-C.md`,
   then **delete** it. → the file tree / index should reflect each within a few
   seconds. Record which events fired and how fast.
4. **Files-On-Demand:** mark the vault "Free up space" so notes become cloud-only
   placeholders. Open plainmark and watch the OneDrive tray icon. → confirm
   indexing does **not** download every file (no mass sync-down). If it does,
   switch to `pollCompareContents: false` and/or lazy indexing.

### B. Polling fallback over SMB
1. Put a vault on an SMB/network share. Set `"watchMode": "poll"` in
   `.plainmark/settings.json` (keep `pollCompareContents: true` for real files).
2. Repeat steps A.2–A.3 from a second machine/editor writing to the share. →
   changes should appear within ~`pollIntervalMs`. Tune the interval for your
   share's latency.
3. Try `"watchMode": "native"` on the same share to confirm whether native events
   fire at all over SMB (often they don't — this is exactly why poll exists).

### What to record
For each of native-local, native-OneDrive, native-SMB, poll-SMB: did
create/modify/rename/delete fire? Latency? Any missed events? Any unexpected
hydration? Drop the results back into this file under a "Field results" heading.

## Field results (2026-06-24, Windows)

First real-hardware run, on the `v0.0.0-phase2.3` build:

- **OneDrive-synced vault, `watchMode: auto` (native):** create / modify / rename
  / delete all fired and were reflected (file tree, preview, backlinks) within a
  second or two, no manual refresh. External edit of the open note reloaded
  correctly. No mass hydration observed.
- **SMB share, default settings (`auto` = native):** create / modify / rename /
  delete all worked correctly with native events — the polling fallback was not
  needed for this share. (Native over SMB is environment-dependent; poll remains
  the escape hatch for shares where native doesn't fire — see below.)

Net: the §13 OneDrive/SMB watcher risk is validated for these environments with
default settings. Two UI-wiring bugs found and fixed during this pass (the editor
not remounting on external reload; the file tree not auto-refreshing on index
updates) — both were frontend gaps, not watcher-reliability problems.

## Recommendations (current)

- **Local disk / typical OneDrive:** `watchMode: "auto"` (native).
- **SMB / network share:** `watchMode: "poll"`, `pollCompareContents: true`,
  tune `pollIntervalMs` to the share's latency.
- **OneDrive Files-On-Demand (placeholders):** native mode; if you must poll, set
  `pollCompareContents: false` to avoid re-hydrating the vault each poll.
- Auto-detection of "native is silently dropping events" is **not** implemented;
  the setting is the escape hatch. Revisit if field results show native failing
  on shares without a clear signal.
