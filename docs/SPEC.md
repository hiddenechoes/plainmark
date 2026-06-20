# plainmark — Product Spec (v0.5)

> **Name:** *plainmark* (npm + crates.io names confirmed free).
> **Status:** direction approved; ready to scaffold.
> **License:** GPL-3.0. **Repo:** public on GitHub from day one.

**Changes since v0.4:** added §16 (repository, CI & Claude Code conventions) and the committed `CLAUDE.md` / `.claude/` / `.github/` scaffolding. _(earlier)_ **v0.4:** sorted the remaining gaps into v1 must-haves vs. future
improvements (new §15). Added a data-integrity section (§7.1), markdown-flavor
and link-resolution decisions (§8.7–8.8), image attachments (§8.9), config split
+ scale target (§7), the split-vs-inline preview call (§8.1), and expanded risks
(§13) — notably the OneDrive/SMB watcher spike.

---

## 1. Vision

A local-first, plain-Markdown notes app in the spirit of Obsidian, built for
environments where an ungovernable third-party plugin ecosystem is a
non-starter. The defining design choice is what's **absent**: there is no
plugin runtime, no marketplace, and no user-scriptable code surface. Every
feature ships compiled into the app. "Governed by construction" rather than
"locked down after the fact."

Your notes are plain `.md` files in real folders. The app is a nice way to
read, write, link, and query them — never the owner of your data.

## 2. Goals

- Read/write standard Markdown files in ordinary filesystem folders.
- First-class wiki-links and backlinks.
- Daily notes from a template.
- Mermaid diagram rendering, fully offline.
- **Query Markdown tasks (`- [ ]`) across the entire vault** — the headline feature.
- Cross-platform desktop (Windows, macOS, Linux).
- Completely open source (GPL-3.0), reproducible builds via GitHub Actions.
- Sit cleanly under Microsoft Purview governance on managed endpoints (§11).

## 3. Non-goals (explicitly out of scope, especially for v1)

These are deliberate. Several are the whole point.

- **No plugin/extension API.** No marketplace. No remote or user-supplied code.
- **No embedded scripting** (no Lua/JS eval surface). This is the core governance guarantee.
- **No in-app application of Microsoft Purview labels** (no MIP SDK) in v1 — labeling/enforcement is handled by the Microsoft stack (§11).
- No built-in sync — the vault folder lives wherever the user puts it (OneDrive, network share, git).
- No mobile app in v1 (Tauri 2 leaves the door open later).
- **No real-time co-editing** (no CRDT/Yjs, no sync server or P2P transport). It would reintroduce a network/runtime surface, break "file over app," and is the biggest scope risk on the table — for low payoff in a one-author-per-note tool. See §4.1.
- No Notion-style block/WYSIWYG editor — Markdown source editor with live preview.

## 4. Target users & deployment

- **Primary:** individuals and small teams who want Obsidian-like PKM without the plugin attack surface.
- **v1 deployment model:** single-user desktop install. Team usage = everyone points the app at a **shared/synced folder** (OneDrive, SMB share, etc.). No accounts, no server. Conflicts are handled by the underlying sync layer.
- Fits a Microsoft/Azure-managed environment: nothing to govern at the app level beyond the binary itself, since there's no in-app extension surface, and Purview governs the files at the boundary (§11).

### 4.1 Collaboration model

**Stance: embrace async, file-based collaboration; consciously skip real-time co-editing.**

- **Async (embraced):** a shared OneDrive/SMB folder means the whole team works one vault and edits propagate to everyone — the same way shared Obsidian vaults work, and ~80% of what "collaboration" means for a knowledge base. Container-level Purview labeling (§11) keeps the files plain and syncing, so governance and collaboration coexist.
- **Real-time co-editing (out of scope):** see §3. The missing 20% (two people in the same note simultaneously) needs a CRDT + sync server or P2P — the exact network/runtime/plugin-shaped surface this project exists to avoid — and is the likeliest path into the "80% valley."
- **What we invest in instead** (makes the shared vault feel solid without a server):
  - Robust external-change handling — reload-on-disk-change and "this note changed underneath you" prompts. Required by the Phase 2 watcher anyway; doing it well is what keeps a shared vault from feeling broken.
  - A **conflict/merge view** (v1.x): when a note changed both locally and on disk, or sync drops a "conflicted copy," show a side-by-side diff and let the user merge. Small scope, stays pure-files.
  - Optional **git-backed vault** mode for power users — lean on git for history/merge rather than building sync.

## 5. Principles

1. **File over app.** Markdown files are the source of truth; the app is disposable.
2. **Offline by default.** No network calls in normal operation. Mermaid and all assets are bundled locally.
3. **Governed by construction.** Security comes from the absence of an extension runtime, not from config locks.
4. **Small, sharp scope.** Define tight feature boundaries (especially the query language) and stop there.

---

## 6. Tech stack

| Layer | Choice | Why |
|---|---|---|
| App shell | **Tauri 2.x** (Rust) | Local-first, tiny binaries, real filesystem access, cross-platform |
| Backend | Rust | File I/O, filesystem watching, indexing, query execution |
| Frontend | TypeScript + web | UI, panels, editor host |
| Editor | **CodeMirror 6** | Same engine Obsidian uses; don't hand-roll an editor |
| MD parsing | `markdown-it` / `remark` (frontend) + a Rust parser for indexing | Render + index |
| Diagrams | **mermaid** (npm, bundled locally) | Offline diagram rendering — no CDN |
| Index cache | SQLite (via Rust) — optional | Fast cold-start on large vaults; cache only, never source of truth |

**Security posture:** strict CSP, no remote content, no `eval`, no dynamic
module loading, Tauri allowlist scoped to the chosen vault folder only.

---

## 7. Data model

- **Vault** = a folder chosen by the user. All `.md` files within (recursively) are notes.
- **Config is split in two:**
  - *Vault-local* — `.plainmark/` inside the vault: `settings.json` (daily-note template path, etc.) and `index.sqlite` (optional rebuildable cache, safe to delete).
  - *App-level* — per-user OS config dir: theme, window state, **recent-vaults list**. v1 opens **one vault at a time** with quick switching between recent vaults; multiple simultaneous vault windows are a future improvement.
- **Index** (in memory, rebuilt on launch, kept live by a file watcher) holds:
  - Notes (path, title, headings, **frontmatter** incl. optional `classification`)
  - Links: outgoing `[[...]]` and the inverted backlink map
  - Tags (`#tag`)
  - **Tasks:** `{ text, status, tags[], due?, file, line }`

The index is derived. Deleting `.plainmark/` must lose nothing but a cache.

- **Scale target:** designed to stay snappy up to **~10k notes** (covers essentially all personal/team PKM vaults). In-memory index is fine at that size; SQLite cache is a cold-start optimization, not a requirement. 50k+ notes is an explicit future perf concern (see §13).

### 7.1 Data integrity & safe file mutations *(v1 must-have)*

plainmark programmatically edits user files in two places — task checkbox
write-back (§8.5) and rename-rewrites-inbound-links (§8.2). A bug here corrupts
notes, so this is the most safety-critical surface in the app. v1 requirements:

- **Atomic writes:** write to a temp file in the same directory, then rename over the target. Never partial-write a note in place.
- **Round-trip fidelity:** preserve bytes the app isn't deliberately changing — original line endings (CRLF/LF — relevant in a Windows shop), trailing whitespace, indentation, and encoding (UTF-8, with/without BOM). A task toggle changes one character region, nothing else.
- **No blind clobber:** never overwrite a note whose on-disk version changed since plainmark last read it without reconciling first (ties into external-change handling, §4.1 / Phase 2).
- **Soft delete:** deletions move to a `.plainmark/.trash/` rather than hard-deleting.
- **Precise write-back targeting:** task toggles must resolve to the exact file + line, re-verified against current content, to avoid editing the wrong line after concurrent edits.

---

## 8. Feature specs

### 8.1 Markdown editing
- Open a vault folder; browse a file tree (create/rename/move/delete files & folders).
- CodeMirror 6 editor with Markdown syntax highlighting.
- **Live preview = a split/rendered preview pane in v1** (editor + rendered view). Obsidian-style *inline* live preview (hides syntax and renders inside the editor) is a known big-effort item in CM6 and is a **future improvement**, not v1. Calling this out because "live preview" otherwise hides the single largest scope fork in the app.
- Save to disk (atomic, §7.1); external edits picked up via the watcher.
- **Acceptance:** edit a `.md` file, see it on disk unchanged except for the intended edit; edit externally, see the app update.

### 8.2 Wiki-links & backlinks
- `[[Note Name]]` links with autocomplete against existing notes.
- Clicking a link navigates (creating the note if missing, optionally).
- **Backlinks / linked-mentions** panel listing every note that links to the current one, with context line.
- Renaming a note updates inbound links (rename-safe).
- **Acceptance:** create A linking to B; B's backlinks show A; rename B and A's link still resolves.

### 8.3 Daily notes
- Command "Open today's daily note" → creates/opens the daily note from a template.
- **Defaults (all configurable):** location `Daily/`, filename `YYYY-MM-DD.md` (ISO, sorts correctly), template `Templates/Daily.md` applied **only on creation**.
- Optional: a small calendar / "jump to date".
- **Acceptance:** invoking the command twice in a day opens the same file; template applied only on creation.

### 8.4 Mermaid
- Fenced ` ```mermaid ` blocks render in preview using the **bundled** mermaid lib.
- No network access required; works fully offline.
- **Acceptance:** a flowchart renders with the network disabled.

### 8.5 Task query — *headline feature*
Tasks are standard Markdown checkboxes, kept portable:
```
- [ ] Write the spec #work 📅 2026-07-01
- [x] Done thing
```
- Inline metadata: `#tags` and an optional due date `📅 YYYY-MM-DD` (Obsidian-Tasks-compatible so files stay portable).
- A fenced ` ```query ` block renders a live, filtered task list.

**v1 grammar** (line-oriented, deliberately small):
```
```query
not done
path startswith "Projects/"
tag #work
due before today
sort by due asc
limit 50
```
```

Supported v1 directives:
- `done` / `not done`
- `path startswith "<folder>/"` · `path includes "<substr>"`
- `tag #<tag>`
- `text includes "<substr>"`
- `due before|after|on <YYYY-MM-DD>` · `due before|after|on today` · `due today`
- `no due` · `has due`
- `classification is <Label>` (matches frontmatter `classification:`; see §11)
- `sort by (due|path) (asc|desc)`
- `limit <n>`

Deliberately deferred (keep v1 small): priority, start/scheduled dates, group-by, OR-logic.

Behavior:
- Results render as a checklist; each item links to its source file + line.
- Toggling a result checkbox writes `[ ]`↔`[x]` back to the source file.
- **Acceptance:** open tasks across 3 folders appear in one query block; checking one updates the underlying file; an invalid directive shows a clear inline error, never crashes.

### 8.6 Search & navigation
- Quick switcher (fuzzy note open).
- Full-text vault search.

### 8.7 Markdown flavor *(decision — v1)*
- **Base:** CommonMark + GFM (tables, strikethrough, GFM task lists).
- **Math:** KaTeX (`$…$` / `$$…$$`) — yes in v1; the target user is technical.
- **plainmark extensions:** `[[wikilinks]]` and `![[embeds]]` (non-standard; the parser/indexer must handle them explicitly).
- **Footnotes:** v1. **Frontmatter:** YAML, parsed (drives `classification`, etc.).
- **Future:** Obsidian-style callouts, custom block syntaxes.

### 8.8 Link resolution *(decision — v1)*
Backlink correctness depends on pinning this down:
- **Name resolution:** `[[Note]]` resolves by shortest unique path; if ambiguous (same filename in two folders), prefer same-folder, then prompt/disambiguate.
- **Heading links:** `[[Note#Heading]]` supported in v1.
- **Unresolved links:** rendered distinctly (e.g. muted), click offers to create.
- **Future:** block references (`[[Note^blockid]]`) and link aliases (`[[Note|alias]]`).

### 8.9 Attachments & images *(partial — v1)*
Pasting a screenshot into a note is table stakes for technical docs.
- **v1:** paste-from-clipboard saves the image into an attachments folder (configurable, default `Attachments/`), inserts an `![[image.png]]` embed, and renders it in preview. Drag-drop of image files does the same.
- **Future:** richer attachment management (PDF/audio embeds, attachment browser, orphan cleanup).

---

## 9. Architecture sketch

```
+---------------------------- Tauri app ----------------------------+
|  Frontend (TS / web)                Backend (Rust)                |
|  - CodeMirror 6 editor       <-->   - Vault file I/O (scoped)     |
|  - Live preview + mermaid           - Filesystem watcher          |
|  - Panels: tree, backlinks,         - Indexer (links/tags/tasks)  |
|    search, query results            - Query engine                |
|  - classification badge             - SQLite cache (optional)     |
+-------------------------------------------------------------------+
            |                                   |
            +------------ plain .md files in a folder ---------------+
```

No process in this diagram loads third-party code. That is the security model.

---

## 10. Phased roadmap

CI/CD is set up early — good practice and a deliberate skill-building goal.
The phases below *are* the v1 must-have scope; deferred items live in §15.

- **Phase 0 — Skeleton:** Tauri shell, open a vault folder, file tree, CodeMirror editor, open/save `.md` (atomic, §7.1). GitHub Actions building on all 3 OSes.
- **Phase 1 — Render:** Markdown live preview (split pane), the §8.7 markdown flavor (GFM + KaTeX + footnotes), bundled Mermaid, and image paste/embed (§8.9).
- **Phase 2 — Graph:** `[[links]]` + autocomplete + backlinks panel + rename-safety + link-resolution rules (§8.8). Indexer + watcher land here, with robust **external-change handling** (§4.1). **Includes an early spike on file-watching over a real OneDrive/SMB vault, with a polling fallback** (§13).
- **Phase 3 — Daily notes:** template + command (+ optional date jump).
- **Phase 4 — Tasks (headline):** task indexing + `query` block + write-back toggles. Write-back must meet §7.1 (atomic, line-precise, no blind clobber).
- **Phase 5 — Polish & ship:** quick switcher, full-text search, settings UI (vault-local + app-level config), theming, frontmatter `classification` badge/filter; **no telemetry**; **code signing + macOS notarization** (strongly recommended for the enterprise target — may slip to v1.x if certs aren't ready); update strategy (Tauri updater *or* IT-controlled MSI/winget); tagged GitHub Releases per OS.
- **v1.x — Collaboration polish:** conflict/merge view (side-by-side diff on local-vs-disk or "conflicted copy"); optional git-backed vault mode. Async-only; no real-time co-editing (§4.1).

---

## 11. Sensitivity labeling (Microsoft Purview) — design

**Decision: in v1, plainmark is label-agnostic. Labeling and enforcement live in
the Microsoft stack, not in the app.** This keeps "file over app" intact and
avoids dragging the MIP SDK into v1.

**Why this works on managed endpoints.** A sensitivity label is fundamentally
plaintext classification metadata, and labeling need not mean encryption.
Historically, labeling a non-Office file wrapped it into an encrypted `.pfile`
(viewer-only) — which would break the plain-md model. **Advanced Label-Based
Protection** (Endpoint DLP, GA mid-2025) resolves this: on a managed Windows
device, a labeled `.md` keeps its original extension and stays editable in its
normal app, with Endpoint DLP enforcing protection; it only converts to a
protected file when it leaves the device (USB, network share, cloud). The MPIP
File Labeler supports text files, so `.md` qualifies.

**Optional app-native marker (v1, low cost):** support a `classification:` field
in YAML frontmatter. The app can:
- show a UI badge for the current note's classification,
- filter on it in queries (`classification is Secret`),
- act as an export guard (warn/block on configurable actions).

> This frontmatter field is a UX convenience, **not** a Microsoft Purview label.
> It does not classify, encrypt, or enforce anything at the tenant level.

**Caveats (go in eyes open):**
- Plain `.md` has no Office-style metadata container, so an *unencrypted* label
  doesn't durably embed/travel the way it does in a Word doc — persistence leans
  on Endpoint DLP at the boundary. A label that must travel = an *encrypting* label.
- Off the managed/DLP boundary, an encrypted note becomes a protected file
  plainmark cannot read (only the Purview viewer can). The plain-md experience
  holds *inside* the boundary.
- Requires appropriate Purview / Endpoint-DLP licensing.

**Post-v1 (optional):**
- MIP SDK integration so the app can read/apply real labels (Entra app
  registration, entitlement; C++/.NET/Python).
- A Microsoft Graph / PowerShell job to reconcile frontmatter `classification`
  ↔ real tenant labels.

---

## 12. Definition of done (v1)

A single binary per OS that opens a folder of Markdown, edits with a split live
preview, renders Mermaid + KaTeX offline, pastes/embeds images, resolves
wiki-links and backlinks, opens daily notes, and answers cross-vault task
queries with **safe, atomic** write-back (§7.1) — with **zero extension/runtime
surface**, an optional frontmatter `classification` badge/filter, no telemetry,
and a green CI pipeline producing public releases.

---

## 13. Risks & watch-items

- **File-watching over OneDrive/SMB — highest risk.** The deployment target is the watcher's worst case: filesystem watchers (Rust `notify`) are flaky-to-broken over network/SMB shares, and OneDrive Files-On-Demand placeholders can mean indexing triggers mass hydration. Mitigation: an early Phase-2 spike on a real OneDrive vault, with a polling fallback ready. This is the risk most likely to dent the shared-vault story.
- **Inline live-preview effort (schedule):** CM6 is the right engine, but true Obsidian-style inline preview is the longest pole — which is why v1 ships a split pane (§8.1) and inline is deferred (§15).
- **Purview behavior on `.md` (validate, don't assume):** the encryption-breaks-sync / container-labeling-is-the-fix direction holds, but the exact behavior of an encrypted non-Office file through the OneDrive *sync client* is tenant/config-dependent. Action: test with a throwaway label on a test `.md` before betting the model on it (§11).
- **Scale beyond target:** the in-memory-index design is sized for ~10k notes (§7). 50k+ needs a perf pass (incremental indexing, watcher tuning) — out of v1 scope.
- **Data corruption surface:** task write-back and rename-rewrite edit user files — see §7.1; treat as the most carefully-tested modules.
- **Markdown portability:** keep task/link conventions standard so files stay usable in any editor.
- **The 80% valley:** editor polish and edge cases are the long tail. Keep scope tight; defer aggressively to §15.

---

## 14. Decisions log

1. **Name:** plainmark (npm + crates.io confirmed free; repo under your own GitHub account).
2. **License:** GPL-3.0 (deps Tauri/CodeMirror/mermaid are permissive — no conflict; AGPL only if ever hosted as a service).
3. **Multi-user:** single-user desktop on a shared/synced folder. No server/CRDT collaboration.
4. **Sensitivity labels:** Microsoft stack governs (§11); optional frontmatter `classification` marker in-app; MIP SDK deferred post-v1.
5. **Task grammar:** small fixed v1 set (§8.5), tweakable later.
6. **Daily notes:** `Daily/`, `YYYY-MM-DD.md`, `Templates/Daily.md` (§8.3).
7. **Collaboration:** async file-based collaboration embraced (§4.1); real-time co-editing is a deliberate non-goal; invest in external-change handling (Phase 2) + conflict/merge view (v1.x) instead.
8. **Live preview:** split/rendered pane in v1; inline live preview deferred (§8.1, §15).
9. **Markdown flavor:** CommonMark + GFM + KaTeX + footnotes + wikilink/embed extensions (§8.7).
10. **Link resolution:** shortest-unique-path + heading links in v1; block-refs and aliases deferred (§8.8).
11. **Attachments:** image paste/embed in v1; richer attachment management deferred (§8.9).
12. **Data safety:** atomic writes, round-trip fidelity, soft-delete, no blind clobber are v1 must-haves (§7.1).
13. **Scale target:** ~10k notes for v1; 50k+ is a future perf concern (§7, §13).
14. **Distribution:** no telemetry; code signing + notarization strongly recommended at ship (may slip to v1.x); update strategy chosen at Phase 5 (§10).

---

## 15. Scope at a glance — v1 must-haves vs. future improvements

**Principle for sorting:** anything that protects user data or forces a
parser/architecture decision is a v1 must-have; polish and big effort-multipliers
with a tractable simpler version are deferred.

### v1 must-haves
- Plain-`.md` editing, file tree, **atomic/safe file writes** (§7.1)
- Split live preview; GFM + KaTeX + footnotes; bundled Mermaid (offline)
- **Image paste → attachments folder → embed → render**
- Wiki-links, backlinks, rename-safety, **link-resolution rules** (incl. heading links)
- Daily notes from template
- **Cross-vault task query + safe write-back** (headline)
- Quick switcher + full-text search; one-vault-at-a-time + recent-vaults switch
- Robust external-change handling + **OneDrive/SMB watcher spike with polling fallback**
- Optional frontmatter `classification` badge/filter; container-label-compatible (§11)
- No telemetry; cross-platform CI releases; signing/notarization (target, may slip)

### Future improvements (post-v1 backlog)
- **Inline** (Obsidian-style) live preview
- Block references (`[[Note^id]]`) and link aliases (`[[Note|alias]]`)
- Callouts / custom block syntaxes
- Richer attachments (PDF/audio embeds, attachment browser, orphan cleanup)
- Simultaneous multi-vault windows
- Conflict/merge view + git-backed vault mode (v1.x)
- MIP SDK integration / Graph-PowerShell label reconciliation (§11)
- 50k+ note performance pass (incremental indexing, watcher tuning)
- Mobile (Tauri 2 path); richer theming
- Real-time co-editing — remains a non-goal unless real demand appears (§3, §4.1)

---

## 16. Repository, CI & Claude Code conventions

The project is built primarily with **Claude Code**, so the repo carries the
guardrails as committed config rather than relying on prompt discipline alone.

**Layout**
```
plainmark/
├─ CLAUDE.md                  # loaded every session: constraints, commands, conventions
├─ .gitattributes             # LF normalization; protects §7.1 test fixtures from rewrite
├─ .claude/
│  ├─ settings.json           # ENFORCED permissions + auto-format hook (committed)
│  └─ rules/
│     ├─ rust.md              # loads when touching src-tauri/**
│     └─ frontend.md          # loads when touching src/**
├─ .github/
│  ├─ workflows/ci.yml        # checks + cross-platform build on push/PR
│  ├─ workflows/release.yml   # tag (v*) → installers + draft release
│  └─ dependabot.yml          # weekly cargo / npm / actions updates
├─ docs/
│  ├─ SPEC.md                 # this document — source of truth
│  └─ prompts/                # one prompt per phase
├─ src-tauri/                 # Rust backend (owns all FS I/O)
└─ src/                       # React + TS frontend
```

**Why the split:** `CLAUDE.md` is *guidance* loaded every session (kept <200
lines); `.claude/settings.json` permissions and hooks are *enforced* by Claude
Code regardless of model behavior — so the "no network, no plugins, atomic
writes, scoped FS" constraints live partly there (and in CI) where they can't
silently drift. Path-scoped `rules/` keep domain detail out of the always-loaded
CLAUDE.md and load only when relevant files are touched.

**CI/CD**
- `ci.yml` runs on push to `main`/`dev` and PRs: a fast checks job (eslint,
  tsc, vitest, `cargo fmt --check`, `clippy -D warnings`, `cargo test`) plus a
  build matrix (Windows/macOS/Linux). Green CI is required to merge.
- `release.yml` triggers on `v*` tags via `tauri-apps/tauri-action`, producing
  per-OS installers and a **draft** GitHub Release. Code signing + notarization
  are wired in here at Phase 5 (§10).
- Tauri 2 Linux deps: `libwebkit2gtk-4.1-dev libgtk-3-dev libappindicator3-dev
  librsvg2-dev patchelf`.
- Dependabot keeps Rust, npm, and Actions current (supply-chain hygiene, in
  keeping with the project's governance posture).

**Decisions log addendum**
15. **Repo guardrails:** constraints encoded in `CLAUDE.md` + enforced
    `.claude/settings.json` + CI, not prompt discipline alone.
16. **CI/CD:** `ci.yml` (checks + build matrix) on push/PR; `release.yml` on
    `v*` tags; Dependabot weekly. Signing deferred to Phase 5.
