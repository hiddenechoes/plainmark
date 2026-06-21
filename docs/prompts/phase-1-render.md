# plainmark — Claude Code Prompt: Phase 1 (Render)

## Before you start
- Standing constraints, stack, and conventions are in `CLAUDE.md` and
  `.claude/rules/` (already loaded) — follow them.
- Read `docs/SPEC.md` §8.1 (preview mode), §8.4 (Mermaid), §8.7 (markdown
  flavor), §8.9 (attachments), and §7.1 (safe writes — applies to image files too).
- Build **only Phase 1**. Do **not** implement wiki-link resolution/backlinks,
  the indexer/file-watcher, daily notes, tasks, or search yet (Phase 2+).
  **Stop for review** when the acceptance criteria pass.
- Work on branch `feature/phase-1-render`; small Conventional Commits; PR into `dev`.

## Phase 1 deliverable
1. **Split live preview.** A rendered-Markdown pane alongside the CodeMirror
   editor, updating as you type (debounced), with a toggle to show/hide it. This
   is the **split-pane** form from §8.1 — *not* inline/hide-syntax live preview,
   which is deferred.
2. **Markdown flavor (§8.7).** CommonMark + GFM (tables, strikethrough, GFM task
   lists) + GFM footnotes; parse YAML frontmatter (don't render the frontmatter
   block as body text).
3. **Math — KaTeX, bundled.** `$inline$` and `$$block$$` render via a locally
   bundled KaTeX.
4. **Diagrams — Mermaid, bundled.** Fenced ` ```mermaid ` blocks render via a
   locally bundled mermaid, working with the network disabled.
5. **Image paste/embed (§8.9).** Pasting an image from the clipboard (and
   drag-dropping an image file) writes it into an attachments folder (default
   `Attachments/`, configurable in vault-local settings), inserts an embed
   referencing it, and renders it in preview. Generate a collision-safe filename
   (e.g. timestamp + short hash). Standard `![](path)` images also render.

## Constraints specific to this phase
- **Everything renders offline.** Mermaid, KaTeX, and any fonts/assets are
  bundled npm dependencies — **never fetched from a CDN at runtime**. This is the
  single most important check for this phase.
- **The preview must not execute arbitrary HTML or scripts.** Do not enable
  raw-HTML passthrough without sanitization; keep the renderer safe by default —
  this is the no-code-execution guarantee applied to rendering.
- **Image writes go through Rust** (binary write) using the same atomic approach
  as note writes (§7.1); the frontend hands bytes to a typed Tauri command, and
  the path stays scoped to the vault.
- **Wiki-link rendering is out of scope here** — link parsing/resolution/
  backlinks belong to Phase 2. Leave `[[...]]` text as-is for now; only the image
  embed form `![[...]]` needs to work, to support paste.

## Recommended approach (change it if the constraints still hold)
- Render pipeline: `react-markdown` + `remark-gfm` (tables/strikethrough/task
  lists/footnotes) + `remark-math` + `rehype-katex`, plus a custom component for
  ` ```mermaid ` code blocks that calls `mermaid.render`. Bundle the KaTeX CSS
  locally.
- Debounce preview rendering and keep re-render cost bounded on large notes.

## Tests / CI (keep it green)
- Add a unit test for the image-save command: correct `Attachments/` path,
  collision-safe filename, byte round-trip, vault-scoped.
- Optional: a render smoke test (GFM + math + a mermaid block produce expected output).
- `pnpm lint`, `pnpm typecheck`, `pnpm format:check`, `pnpm test` pass; Rust
  `fmt --check` / `clippy -D warnings` / `cargo test` pass on Windows, macOS, Linux.

## When done, print
- How to verify each item, each acceptance check and whether it passes, and
  anything you deferred or assumed.

## Acceptance criteria (Phase 1 done)
- Toggle a split preview; typing updates the rendered pane.
- GFM tables, strikethrough, task-list checkboxes, and footnotes render.
- `$…$` and `$$…$$` render as math, and a ` ```mermaid ` block renders as a
  diagram — both with the **network disabled**.
- Pasting an image saves it under `Attachments/`, inserts an embed, and shows it
  in preview; the image file and the note edit are both written atomically via Rust.
- No network requests occur during editing, rendering, or paste.
- The renderer does not execute injected HTML/JS.
- CI is green on all three OSes.

**Stop here for review before Phase 2.**
