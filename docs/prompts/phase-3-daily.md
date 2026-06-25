# plainmark — Claude Code Prompt: Phase 3 (Daily notes)

## Before you start
- Standing constraints, stack, and conventions are in `CLAUDE.md` and
  `.claude/rules/` (already loaded) — follow them.
- Read `docs/SPEC.md` §8.3 (daily notes), and note §7.1 (creation is an atomic
  write) and §7 (vault-local settings live in `.plainmark/settings.json`).
- Build **only Phase 3**. Do **not** implement tasks/queries (Phase 4),
  full-text search or a settings UI (Phase 5). **Stop for review** when the
  acceptance criteria pass.
- Work on branch `feature/phase-3-daily`; small Conventional Commits; PR into `dev`.
- This phase rides on existing infrastructure: reuse the atomic write +
  `ensure_within` path and the create flow from Phase 0/2, and rely on the
  watcher/index to pick up the new file (`index://updated`) — don't rebuild any
  of that.

## Phase 3 deliverable
1. **"Open today's daily note" command.** A visible button **and** a keyboard
   shortcut. It resolves *today* (local date — see the watch-item below) to
   `<dailyFolder>/<YYYY-MM-DD>.md`, then:
   - if the file exists, just open it;
   - if not, create it from the template (below), then open it.
2. **Template, applied on creation only.** On creation, read the template file
   (default `Templates/Daily.md`); use its contents as the new note's body. If
   the template is missing, create a sensible empty note. **Re-running the
   command the same day opens the existing file untouched** — never re-apply the
   template, never clobber edits.
3. **Configurable via `.plainmark/settings.json`** (read-only for now; the
   settings UI is Phase 5):
   - `dailyNotes.folder` (default `Daily/`)
   - `dailyNotes.filenameFormat` (default `YYYY-MM-DD`)
   - `dailyNotes.templatePath` (default `Templates/Daily.md`)
   Validate the folder and template paths stay **vault-relative** (reuse the same
   constraint already applied to the attachments folder).
4. **Optional (include if cheap): jump to a date.** A small date picker or
   prev/next-day controls that open (or create) that date's daily note via the
   same path. Clearly optional per §8.3.

## Constraints specific to this phase
- **No templating/expression engine.** At most, support a tiny fixed set of
  literal date tokens (e.g. a `{{date}}` / `{{date:FORMAT}}` substitution) — and
  only if you want to. No arbitrary code, no scripting (governance rule).
- Creation is an **atomic write** through the existing path; the new file must
  not overwrite an existing same-day note.
- Stay **offline** and **vault-scoped**.

## Watch-item: local-date correctness
"Today" must be the user's **local** date, not UTC — a note created at 23:00
local should land on today's local date, not tomorrow's. Be explicit about the
timezone handling and test a near-midnight case.

## Tests / CI (keep it green)
- Rust: date→path resolution; create-from-template writes the template content
  atomically; **idempotency** (second invocation opens the existing file and
  neither re-applies the template nor clobbers edits); missing-template fallback;
  configurable folder/format honored; folder/template vault-scoping; local-date
  handling incl. a near-midnight case.
- Frontend: the command/button (and shortcut) opens-or-creates; opening twice in
  a day yields the same file; (optional) date jump.
- `pnpm lint`/`typecheck`/`format:check`/`test` and Rust `fmt --check`/
  `clippy -D warnings`/`test` green on Windows, macOS, Linux.

## When done, print
- How to verify each item, each acceptance check and whether it passes, and
  anything you deferred or assumed.

## Acceptance criteria (Phase 3 done)
- A command/button (and shortcut) opens today's daily note: on first use it
  creates `Daily/YYYY-MM-DD.md` from `Templates/Daily.md` and opens it; a second
  use the same day opens the **same file, unchanged**.
- The template is applied only on creation; editing then re-invoking does not
  overwrite.
- Folder, filename format, and template path are configurable via
  `.plainmark/settings.json`; a missing template yields a sensible empty note.
- The new daily note appears in the file tree and index automatically (existing
  watcher) — no restart.
- "Today" uses the local date (near-midnight case handled).
- (Optional) jumping to a specific date opens/creates that date's note.
- No network; vault-scoped; atomic writes.
- CI is green on all three OSes.

**Stop here for review before Phase 4.**
