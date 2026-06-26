// Typed wrappers around the Rust command surface. The webview never touches the
// filesystem directly (see .claude/rules/frontend.md) — every FS operation goes
// through one of these wrappers.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface TreeNode {
  name: string;
  path: string;
  isDir: boolean;
  children: TreeNode[];
}

export interface VaultInfo {
  root: string;
  tree: TreeNode[];
}

/** A note loaded for editing, carrying the metadata needed for a faithful save. */
export interface NoteFile {
  /** LF-normalized content for CodeMirror. */
  content: string;
  /** Original on-disk line-ending style. */
  eol: "lf" | "crlf";
  /** Whether the file had a UTF-8 BOM. */
  bom: boolean;
  /** Hash of the on-disk bytes at read time, for the no-blind-clobber check. */
  token: string;
}

/** The marker (Rust `AppError::ChangedOnDisk`) used to detect a no-blind-clobber
 * rejection, so the UI can prompt instead of treating it as a generic error. */
export const CHANGED_ON_DISK = "changed-on-disk";

/** Open the native folder picker and load the chosen vault. `null` if cancelled. */
export function pickVault(): Promise<VaultInfo | null> {
  return invoke<VaultInfo | null>("pick_vault");
}

/** Reopen the last vault on launch, if it still exists. */
export function loadLastVault(): Promise<VaultInfo | null> {
  return invoke<VaultInfo | null>("load_last_vault");
}

/** Rebuild the active vault's file tree. */
export function refreshTree(): Promise<TreeNode[]> {
  return invoke<TreeNode[]>("refresh_tree");
}

/** Read a note from within the active vault. */
export function readNote(path: string): Promise<NoteFile> {
  return invoke<NoteFile>("read_note", { path });
}

/** Save a note atomically, preserving its line endings and BOM. Passes the
 * note's read-time token so the backend rejects a blind clobber (§7.1), and
 * returns the new on-disk token to refresh the buffer. */
export function saveNote(path: string, note: NoteFile): Promise<string> {
  return invoke<string>("save_note", {
    path,
    content: note.content,
    eol: note.eol,
    bom: note.bom,
    baseToken: note.token,
  });
}

/** Rename/move a note (absolute paths), rewriting inbound links across the vault.
 * Returns the new absolute path. */
export function renameNote(oldPath: string, newPath: string): Promise<string> {
  return invoke<string>("rename_note", { oldPath, newPath });
}

/** A saved attachment, located by a vault-relative, forward-slash path. */
export interface SavedAttachment {
  relativePath: string;
}

/** Save an image from the system clipboard into the vault's attachments folder.
 * Resolves to `null` if the clipboard holds no image (so a text paste is left
 * untouched). Reading the clipboard in Rust is far more reliable than the
 * webview's `paste` event, especially on Linux/WebKitGTK. */
export function saveClipboardImage(): Promise<SavedAttachment | null> {
  return invoke<SavedAttachment | null>("save_clipboard_image");
}

/** Copy a dropped image file (by absolute source path) into the vault. */
export function importAttachment(sourcePath: string): Promise<SavedAttachment> {
  return invoke<SavedAttachment>("import_attachment", { sourcePath });
}

/** Read an image inside the vault as a `data:` URL for the preview pane. */
export function readImage(path: string): Promise<string> {
  return invoke<string>("read_image", { path });
}

/** A heading within a note (for `#heading` links and autocomplete). */
export interface Heading {
  text: string;
  slug: string;
  level: number;
}

/** A note in the link-target snapshot (Phase 2 graph). `path` is absolute (for
 * navigation); `relPath` is vault-relative with forward slashes (for resolution). */
export interface NoteMeta {
  path: string;
  relPath: string;
  title: string;
  headings: Heading[];
}

/** The result of resolving a `[[link]]` via the backend. */
export interface ResolvedLink {
  /** Absolute path of the target note, if it resolves. */
  path: string | null;
  exists: boolean;
  /** Whether the optional `#heading` part exists in the target. */
  headingOk: boolean;
}

/** One inbound link to the active note, for the backlinks panel. */
export interface Backlink {
  /** Absolute path of the linking note. */
  from: string;
  fromTitle: string;
  line: number;
  snippet: string;
}

/** Snapshot of every note for the link resolver and `[[` autocomplete. */
export function listLinkTargets(): Promise<NoteMeta[]> {
  return invoke<NoteMeta[]>("list_link_targets");
}

/** Resolve a `[[link]]` (note + optional `#heading`) from the note at `from`. */
export function resolveLink(target: string, from: string): Promise<ResolvedLink> {
  return invoke<ResolvedLink>("resolve_link", { target, from });
}

/** Inbound links to the note at `path` (absolute), with context snippets. */
export function getBacklinks(path: string): Promise<Backlink[]> {
  return invoke<Backlink[]>("backlinks", { path });
}

/** Create (or open, if it exists) a note for an unresolved link. Returns the
 * note's absolute path. */
export function createNote(target: string): Promise<string> {
  return invoke<string>("create_note", { target });
}

/** Open (creating on first use that day) the daily note for a *local* calendar
 * date. The frontend resolves "today" in local time and passes the components;
 * the backend never reads a clock, so the date is correct near midnight. The
 * template is applied on creation only — re-invoking the same day returns the
 * existing file untouched. Returns the note's absolute path. */
export function openDailyNote(year: number, month: number, day: number): Promise<string> {
  return invoke<string>("open_daily_note", { year, month, day });
}

/** One task result for a ` ```query ` block (SPEC §8.5). `path` is absolute (for
 * navigation); `relPath` is vault-relative; `line` is 1-based. */
export interface TaskResult {
  path: string;
  relPath: string;
  title: string;
  line: number;
  text: string;
  done: boolean;
  due: string | null;
  tags: string[];
}

/** The outcome of running a query: either an inline grammar `error` or `tasks`.
 * A malformed query is reported here (not thrown), so the block renders a clear
 * inline message instead of crashing the preview. */
export interface QueryResponse {
  error: string | null;
  tasks: TaskResult[];
}

/** The marker (Rust `AppError::TaskMismatch`) for a write-back whose target line
 * no longer matches the indexed task — the live results have drifted; refresh. */
export const TASK_MISMATCH = "task-mismatch";

/** Run a ` ```query ` block against the live task index. `year`/`month`/`day` are
 * the user's *local* date so `today` is resolved in local time, never UTC. */
export function runQuery(
  source: string,
  date: { year: number; month: number; day: number },
): Promise<QueryResponse> {
  return invoke<QueryResponse>("run_query", {
    source,
    year: date.year,
    month: date.month,
    day: date.day,
  });
}

/** Toggle a task checkbox `[ ]`↔`[x]` in its source file (atomic, EOL/BOM-
 * preserving, no-blind-clobber — §7.1). The edit is re-verified against the
 * `expectedText`/`expectedDone` the query showed, so a shifted line is never
 * mis-edited. Returns the task's new done-state. */
export function toggleTask(
  path: string,
  line: number,
  expectedText: string,
  expectedDone: boolean,
): Promise<boolean> {
  return invoke<boolean>("toggle_task", { path, line, expectedText, expectedDone });
}

/** A filesystem change to a note, pushed by the Rust watcher (SPEC §4.1). */
export type NoteChange =
  | { kind: "created"; path: string }
  | { kind: "modified"; path: string }
  | { kind: "removed"; path: string }
  | { kind: "renamed"; from: string; to: string };

/** Subscribe to "the index changed" (rebuild panels/snapshots). */
export function onIndexUpdated(callback: () => void): Promise<UnlistenFn> {
  return listen("index://updated", () => callback());
}

/** Subscribe to per-note filesystem changes (external-change handling). */
export function onNoteChanged(callback: (change: NoteChange) => void): Promise<UnlistenFn> {
  return listen<NoteChange>("note://changed", (event) => callback(event.payload));
}
