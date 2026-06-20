# plainmark — Claude Code Prompt: Phase 0 (Skeleton)

## Before you start
- Your standing constraints, stack, commands, and conventions are in `CLAUDE.md`
  and `.claude/rules/` (already loaded) — follow them.
- Read `docs/SPEC.md` before coding, especially §3 (non-goals), §6 (stack),
  §7 + §7.1 (data model + *safe file mutations*), and §10 (roadmap).
- Build **only Phase 0**. Do **not** implement links, backlinks, indexing, daily
  notes, tasks, search, attachments, or a preview pane yet. **Stop for review**
  when Phase 0's acceptance criteria pass.
- Work on branch `feature/phase-0-scaffold`; small Conventional Commits.

## Phase 0 deliverable (a runnable skeleton)
1. A Tauri 2 desktop app that launches to a clean window.
2. **Open vault:** pick a folder via the native dialog; persist the last-opened
   vault path in app-level config and reopen it on launch.
3. **File tree:** list `.md` files in the vault recursively (show folders);
   clicking a file opens it. Read-only tree is fine for Phase 0.
4. **Editor:** open the selected `.md` in a CodeMirror 6 editor with Markdown
   syntax highlighting.
5. **Save:** Cmd/Ctrl+S writes via an **atomic write** (temp file in the same
   dir → rename), preserving original line endings + encoding (§7.1). No preview yet.
6. Sensible empty/error states; surface filesystem errors, don't swallow them.

## Make the EXISTING CI pass (do not create or edit workflows)
`.github/workflows/ci.yml`, `.gitattributes`, `CLAUDE.md`, and `.claude/` are
already committed. Make CI's **checks** and **build-and-test** jobs pass by
wiring up the toolchain they invoke:
- `package.json` must include a **`"packageManager": "pnpm@<version>"`** field
  (the CI's `pnpm/action-setup@v4` reads the version from it — without it the
  workflow errors).
- `package.json` scripts: `lint` (eslint), `typecheck` (`tsc --noEmit`),
  `test` (**`vitest run`** — not watch mode), `format` (`prettier --write .`),
  `format:check` (`prettier --check .`), and `tauri`.
- Minimal eslint + prettier configs; strict `tsconfig.json`. At least one trivial
  frontend test so `pnpm test` passes.
- A Rust crate under `src-tauri/` that is `cargo fmt`-clean and
  `cargo clippy --all-targets -- -D warnings`-clean, with **a unit test that
  round-trips a CRLF file and a UTF-8-BOM file through the atomic-write helper
  unchanged.** Construct those inputs as **raw byte literals in the test**
  (e.g. `b"a\r\nb\r\n"`, BOM `\xEF\xBB\xBF`) so git can't normalize them. This
  test runs on Windows/macOS/Linux in CI and is the §7.1 guarantee.
- Configure `tauri.conf.json` `beforeBuildCommand` to build the frontend so
  `pnpm tauri build` works in CI.
- Use `pnpm`; commit `pnpm-lock.yaml` (CI installs with `--frozen-lockfile`).

## Engineering requirements
- Rust owns all filesystem I/O (open dialog, read tree, read file, atomic write);
  the frontend calls typed Tauri commands — no direct FS from the webview.
- **All note writes go through a single atomic-write helper.**
- Set a **strict CSP** in `tauri.conf.json` and a **minimal capability allowlist
  scoped to the active vault** (no broad FS or network capabilities).
- Layout: `src-tauri/` (Rust), `src/` (frontend; `components/`, `lib/`);
  vault-local config in `.plainmark/`, app-level config in the OS config dir.
- Ensure `.gitignore` covers `node_modules`, `dist`, `src-tauri/target`, and
  `.env`. Add `LICENSE` (GPL-3.0) and `README.md` if not present.

## When done, print
- How to run locally (`pnpm tauri dev`).
- Each acceptance check below and whether it passes.
- Anything you deferred or assumed.

## Acceptance criteria (Phase 0 done)
- `pnpm tauri dev` launches the app.
- Open a folder → see `.md` files in a tree → click → edit → Cmd/Ctrl+S persists
  to disk, with line endings/encoding unchanged.
- Re-launching reopens the last vault.
- No network requests occur; filesystem access is limited to the chosen vault.
- `pnpm lint`, `pnpm typecheck`, `pnpm format:check`, `pnpm test` pass;
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  and `cargo test` pass.
- On a PR into `dev`, `ci.yml` is green on Windows, macOS, and Linux —
  including the CRLF/BOM round-trip test on all three.

**Stop here for review before starting Phase 1.**
