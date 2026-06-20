<!-- Update <your-org> to your GitHub org/user once the repo exists. -->
![CI](https://github.com/hiddenechoes/plainmark/actions/workflows/ci.yml/badge.svg)

# plainmark

A local-first, plain-Markdown notes app — Obsidian-like, **without** a
third-party plugin runtime. Built for people and teams who want linked notes,
daily notes, diagrams, and cross-vault task queries, but can't accept an
ungovernable extension surface.

> **Status:** early development. The product spec is complete and the project is
> being built phase by phase (see [`docs/SPEC.md`](docs/SPEC.md)). Phase 0 is the
> editor skeleton.

## Why plainmark

- **Governed by construction.** No plugin/marketplace, no scripting/eval, no
  remote or user-supplied code. Every feature is compiled in.
- **Your files, not ours.** Plain `.md` files in ordinary folders are the source
  of truth; the app is disposable.
- **Offline by default.** No network calls at runtime, no telemetry. Diagrams and
  math render from bundled libraries.

## Roadmap (v1)

Markdown editing with split live preview · wiki-links & backlinks · daily notes ·
Mermaid + KaTeX (offline) · image paste/embed · **cross-vault Markdown task
queries** · full-text search. Async, file-based collaboration via a shared folder;
no real-time co-editing. Full detail and the must-have vs. future split are in
[`docs/SPEC.md`](docs/SPEC.md) (§15).

## Tech stack

[Tauri 2](https://tauri.app) (Rust) · React + TypeScript + [Vite](https://vitejs.dev) ·
[CodeMirror 6](https://codemirror.net) · pnpm.

## Getting started

**Prerequisites:** Node 20+ with pnpm (`corepack enable`), Rust (`rustup`), and
the Tauri 2 system dependencies for your OS — see
<https://tauri.app/start/prerequisites/>.

```bash
pnpm install
pnpm tauri dev      # run the app in development
pnpm tauri build    # produce a release build for your platform
```

Checks:

```bash
pnpm lint && pnpm typecheck && pnpm format:check && pnpm test
cargo test --manifest-path src-tauri/Cargo.toml
```

## Project layout

```
src-tauri/   Rust backend — owns ALL filesystem I/O
src/         React + TypeScript frontend (editor, panels)
docs/        SPEC.md (source of truth) + phase prompts
.claude/     Claude Code config (rules + permissions)
.github/     CI/CD workflows
```

## Contributing

Branching is `feature/*` → `dev` → `main` via pull request; CI must be green to
merge. Commits follow [Conventional Commits](https://www.conventionalcommits.org).
Project conventions and guardrails live in [`CLAUDE.md`](CLAUDE.md) and
`.claude/rules/`.

## License

[GPL-3.0](LICENSE) © contributors.
