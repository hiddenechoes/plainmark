---
paths:
  - "src/**/*.ts"
  - "src/**/*.tsx"
---

# Frontend (React + TypeScript) rules

- **TypeScript strict mode.** Functional components only; named exports, no
  default exports. Tests live next to source (`foo.ts` → `foo.test.ts`).
- **No direct filesystem or network access from the webview.** All FS goes
  through typed Tauri command wrappers in `src/lib/` — one wrapper per command,
  with explicit input/output types. No `fetch`/XHR to remote hosts at runtime.
- **No CDN imports.** Mermaid, KaTeX, and fonts are bundled as local deps.
- **No browser storage** (`localStorage`/`sessionStorage`) for vault data — the
  markdown files and `.plainmark/` are the source of truth.
- Editor is **CodeMirror 6**; keep editor extensions modular under
  `src/lib/editor/`.
- Keep state local/component-scoped; lift only when shared. Avoid a global store
  until a feature actually needs it.
- Surface backend errors to the user (empty/error states), never swallow them.
- Must pass `pnpm lint` and `pnpm typecheck` before commit.
