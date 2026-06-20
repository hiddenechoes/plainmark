# plainmark — Claude Code project guide

plainmark is a local-first, plain-Markdown notes app (Tauri 2 + React/TS +
CodeMirror 6). The full design lives in `docs/SPEC.md` — **read it before any
non-trivial work**, especially §3 (non-goals), §6 (stack), §7 + §7.1 (data model
+ safe file mutations), §8 (features), §10 (phased roadmap), and §15 (scope).

## Non-negotiable constraints (do not violate, even if asked to "just add")
These *define* the product. If a request conflicts with one, stop and flag it
rather than complying.
- **No plugin/extension runtime, no marketplace, no user-supplied or remote
  code, no scripting/eval.** Every feature is compiled in.
- **No network calls at runtime, and no telemetry/analytics.** Bundle assets
  (Mermaid, KaTeX, fonts) locally; never fetch from a CDN.
- **Filesystem access is scoped to the user-selected vault folder only.** Keep
  the Tauri capability allowlist minimal and default-deny.
- **All filesystem I/O lives in Rust.** The webview never touches the FS
  directly — it calls typed Tauri commands.

## Data safety — the scariest surface (see §7.1)
Two operations edit user files: task write-back and rename-rewrites-links.
- **Atomic writes only:** write a temp file in the same directory, then rename
  over the target. Never partial-write a note in place.
- **Round-trip fidelity:** preserve original line endings (CRLF/LF), encoding
  (UTF-8 ±BOM), and any bytes not deliberately changed. A task toggle changes
  one character region and nothing else.
- **No blind clobber:** never overwrite a note whose on-disk version changed
  since it was last read without reconciling first.
- **Soft delete** to `.plainmark/.trash/`, never hard delete.

## Stack
- Tauri 2.x (Rust) · React + TypeScript + Vite · CodeMirror 6 · `pnpm`.
- Markdown flavor: CommonMark + GFM + KaTeX + footnotes + `[[wikilink]]` /
  `![[embed]]` extensions (§8.7).

## Commands
- Dev: `pnpm tauri dev` · Build: `pnpm tauri build`
- Frontend: `pnpm lint` · `pnpm typecheck` · `pnpm test`
- Rust (run inside `src-tauri/`): `cargo fmt` ·
  `cargo clippy --all-targets -- -D warnings` · `cargo test`
- Run fmt + clippy + typecheck before every commit.

## Repo conventions
- Layout: `src-tauri/` (Rust backend), `src/` (frontend; `components/`, `lib/`),
  `docs/` (spec + phase prompts), `.github/workflows/` (CI).
- Config split: vault-local in `.plainmark/`; app-level in the OS config dir (§7).
- License: **GPL-3.0**. Add an SPDX header to new source files:
  `SPDX-License-Identifier: GPL-3.0-or-later`.
- Branching: `feature/*` → `dev` → `main`, via PR. CI must be green to merge.
- Commits: Conventional Commits (`feat:`, `fix:`, `chore:`, `ci:`, `docs:`),
  small and reviewable.

## Working style
- **Respect the phased roadmap (§10): build one phase at a time and STOP for
  review before starting the next.** Do not implement future-phase features early.
- Propose file structure + dependencies before large additions.
- When a unit is done, print how to run/verify it and which acceptance checks
  it satisfies.
- If something is ambiguous or conflicts with the spec, ask — don't guess.

## Domain rules
Path-scoped detail lives in `.claude/rules/` (`rust.md`, `frontend.md`) and loads
automatically when you touch matching files.
