# plainmark — Claude Code Prompt: Phase 4 (Tasks — the headline feature)

## Before you start
- Standing constraints, stack, and conventions are in `CLAUDE.md` and
  `.claude/rules/` (already loaded) — follow them.
- Read `docs/SPEC.md` §8.5 (task query — read it carefully, the v1 grammar is
  fixed), §7 (index/data model includes tasks), §7.1 (safe writes — **task
  write-back is the second file-mutation surface**), and §11 (the `classification`
  frontmatter field).
- Build **only Phase 4**. Do **not** implement full-text search or a settings UI
  (Phase 5), and do **not** expand the query grammar beyond the v1 set below.
  **Stop for review** when the acceptance criteria pass.
- Work on branch `feature/phase-4-tasks`; small Conventional Commits; PR into `dev`.
- Ride existing infrastructure: extend the `index.rs` pulldown-cmark indexer +
  SQLite cache; reuse the atomic-write + content-token + `ensure_within` path from
  the Phase 2 rename/save work for write-back; reuse the Phase 3 local-date
  resolution for `today`. Add a `` ```query `` component alongside the existing
  Mermaid code-block component.

## Phase 4 deliverable
1. **Task index.** Extend the indexer to parse every note for task lines —
   `- [ ]` (open) and `- [x]`/`- [X]` (done) — capturing for each task:
   `{ text, status, tags[] (inline #tag), due? (inline 📅 YYYY-MM-DD), file, line }`
   plus the note's **frontmatter `classification`**. Skip tasks inside fenced code
   blocks (as links already skip code). Keep it incremental via the watcher and in
   the SQLite cache.
2. **Query grammar parser — the FIXED v1 set (§8.5).** A fenced `` ```query ``
   block contains line-oriented directives. Implement exactly:
   - `done` / `not done`
   - `path startswith "<folder>/"` · `path includes "<substr>"`
   - `tag #<tag>`
   - `text includes "<substr>"`
   - `due before|after|on <YYYY-MM-DD>` · `due before|after|on today` · `due today`
   - `no due` · `has due`
   - `classification is <Label>` (matches frontmatter `classification`)
   - `sort by (due|path) (asc|desc)`
   - `limit <n>`
   Deliberately **out of scope**: priority, start/scheduled dates, group-by,
   OR-logic. An unknown/malformed directive renders a **clear inline error** in
   place of results — never a crash.
3. **Query execution + live results.** The `` ```query `` block renders a live,
   filtered/sorted/limited checklist over the task index. Each result shows its
   text and **links to its source file + line** (clicking opens the note at that
   line). Results re-run on `index://updated` so they stay live.
4. **Task write-back (the scariest surface — §7.1).** Toggling a result's checkbox
   writes `[ ]`↔`[x]` back to the **source file**:
   - **line-precise** and **re-verified** against the current file content (match
     the expected task text before flipping, so a shifted line is never mis-edited),
   - **atomic**, preserving the file's original **EOL/BOM**,
   - **no blind clobber** (reuse the content-token check); if the file changed
     underneath, surface it rather than overwrite.
   If the target is the currently-open note with unsaved edits, handle it
   gracefully (apply to the buffer or warn) rather than racing the save.
5. **`today` = local date**, resolved the same way as Phase 3 (never UTC).

## Constraints specific to this phase
- Task indexing, query execution, and write-back live in **Rust**; the query
  component calls typed commands. Stay **offline** and **vault-scoped**.
- The grammar is **frozen** at the v1 set — resist adding "just one more" filter.
- `classification is` reads the in-file frontmatter field only — it is not a real
  Purview label; don't over-engineer it.
- Query over the in-memory index; debounce live re-render; stay snappy at ~10k
  notes with many tasks.

## Tests / CI (keep it green)
- Rust: task parsing (markers, inline `#tags`, `📅` due, code-block skipping,
  correct line numbers); **every** grammar filter incl. `due … today` and
  `classification is`; sort + limit; grammar parse-error surfacing; **toggle
  write-back: correct line flipped, re-verify mismatch handled, atomic, EOL/BOM
  preserved, no-blind-clobber**; `today` = local date.
- Frontend: query block parse→render, result source links, checkbox toggle
  round-trip, invalid-query inline error, live update on index change.
- `pnpm lint`/`typecheck`/`format:check`/`test` and Rust `fmt --check`/
  `clippy -D warnings`/`test` green on Windows, macOS, Linux.

## When done, print
- How to verify each item, each acceptance check and whether it passes, and
  anything you deferred or assumed.

## Acceptance criteria (Phase 4 done)
- Open tasks scattered across **≥3 folders** all appear in one `` ```query `` block.
- Each v1 directive filters/sorts/limits correctly; `due today` / `due before
  today` use the **local** date; `classification is X` matches frontmatter.
- Each result links to its source file + line; clicking opens the note there.
- Toggling a result checkbox flips `[ ]`↔`[x]` in the source file — atomic,
  EOL/BOM preserved, line-precise, no clobber — and the query refreshes live.
- An invalid/unknown directive shows a clear inline error and never crashes the
  preview.
- No network; vault-scoped.
- CI is green on all three OSes.

**Stop here for review before Phase 5.**
