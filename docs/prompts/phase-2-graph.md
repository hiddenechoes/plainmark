# plainmark — Claude Code Prompt: Phase 2 (Graph)

## Before you start
- Standing constraints, stack, and conventions are in `CLAUDE.md` and
  `.claude/rules/` (already loaded) — follow them.
- Read `docs/SPEC.md` §8.2 (wiki-links & backlinks), §8.8 (link resolution),
  §7 (index / data model), §7.1 (safe writes — rename rewrites inbound links),
  §4.1 (external-change handling), and §13 (the OneDrive/SMB watcher risk).
- Build **only Phase 2**. Do **not** implement tasks/queries (Phase 4), daily
  notes (Phase 3), full-text search (Phase 5), block references or link aliases
  (future), or inline live preview (future). **Stop for review** when the
  acceptance criteria pass.
- Work on branch `feature/phase-2-graph`; small Conventional Commits; PR into `dev`.

## Sequencing — spike the risky part FIRST
SPEC §13 flags file-watching over OneDrive/SMB as the project's highest technical
risk. Do this **before** building features on top of the watcher:
- Spike the Rust file-watcher (`notify`) against a **real OneDrive-synced vault**
  (and an SMB share if you can). Verify create/modify/rename/delete events
  actually fire and are timely.
- Watch for OneDrive **Files-On-Demand** placeholders — confirm indexing does
  **not** force mass hydration of cloud-only files.
- Implement a **polling fallback** (configurable interval) for when native events
  are unreliable, with auto-detection or a setting to choose.
- Write up the findings (a short `docs/` note or the PR body) so the watcher
  strategy is on record.

## Phase 2 deliverable
1. **Index (Rust, in-memory, watcher-maintained).** On vault open, parse every
   `.md` for outgoing `[[wikilinks]]` (including `[[Note#Heading]]`) and build the
   inverted **backlink map**; capture note paths/titles/headings. Keep it live via
   the watcher with **incremental** updates (no full re-index per event; debounce).
   Target: snappy at ~10k notes. (SQLite persistence from §7 is optional — defer
   it if it adds risk.)
2. **Link resolution (§8.8).** Resolve `[[Note]]` by shortest unique path; on a
   same-name collision prefer the same folder, then disambiguate deterministically.
   Support `[[Note#Heading]]`.
3. **Link rendering + navigation.** Extend the existing remark wiki transform so
   `[[…]]` renders as a **clickable link** (resolved via the index) rather than
   literal text. Clicking a resolved link navigates to the note. **Unresolved**
   links render distinctly (muted) and clicking offers to create the note.
4. **Autocomplete.** Typing `[[` in the editor suggests existing notes (and
   headings after `#`); selecting inserts a working link.
5. **Backlinks / linked-mentions panel.** For the active note, list every note
   linking to it with a context snippet; updates live as the index changes.
6. **Rename-safety (§8.2 + §7.1).** Renaming or moving a note rewrites inbound
   `[[links]]` across the vault. These writes MUST go through the atomic path and
   preserve each file's original EOL/BOM (reuse `save_note`), touch only the link
   target text, and never blind-clobber a file that changed on disk.
7. **External-change handling (§4.1).** When a note changes on disk: reload it if
   the buffer is clean; if there are unsaved edits, surface a "changed underneath
   you" prompt rather than silently overwriting (the no-blind-clobber rule). The
   watcher drives this.

## Constraints specific to this phase
- Indexer + watcher live in **Rust**; the frontend subscribes to index updates via
  Tauri events and resolves links through a typed command (or a pushed lightweight
  name→path map). No FS from the webview.
- Stay **offline** and **vault-scoped** (reuse `ensure_within`).
- Rename link-rewriting is the scariest operation in this phase — make it a
  precise, batched, well-tested operation.

## Recommended approach (change it if the constraints still hold)
- Debounce and coalesce watcher events (editors and sync clients emit bursts).
- Key the index by normalized path; store display title + headings for resolution
  and autocomplete.
- Optional nice-to-have: show a brief summary of what a rename will change before
  applying it.

## Tests / CI (keep it green)
- Rust: link parsing incl. heading links; backlink-map correctness; resolution
  rules (shortest-unique-path + same-name ambiguity); **rename rewrite across
  multiple files preserving EOL/BOM**; incremental watcher update; polling fallback.
- Frontend: autocomplete, resolved-vs-unresolved rendering, click-to-navigate,
  click-to-create, backlinks panel.
- `pnpm lint`/`typecheck`/`format:check`/`test` and Rust `fmt --check`/
  `clippy -D warnings`/`test` green on Windows, macOS, Linux.

## When done, print
- How to verify each item, each acceptance check and whether it passes, the
  watcher-spike findings, and anything you deferred or assumed.

## Acceptance criteria (Phase 2 done)
- Opening a vault builds the index; the backlinks panel shows inbound links (with
  context) for the active note.
- `[[` autocompletes; `[[Note]]` and `[[Note#Heading]]` render as clickable links;
  clicking navigates; unresolved links look distinct and offer to create.
- Creating / editing / deleting / **renaming** a note updates links, backlinks, and
  the index **live**, with no restart.
- Renaming a note rewrites inbound links across the vault atomically, preserving
  each file's EOL/BOM (proven by a multi-file test).
- An external change to the open note reloads it (clean buffer) or prompts (unsaved
  edits) — never a silent clobber.
- The OneDrive/SMB watcher spike is documented and a polling fallback works.
- CI is green on all three OSes.

**Stop here for review before Phase 3.**
