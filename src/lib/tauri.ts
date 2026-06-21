// Typed wrappers around the Rust command surface. The webview never touches the
// filesystem directly (see .claude/rules/frontend.md) — every FS operation goes
// through one of these wrappers.
import { invoke } from "@tauri-apps/api/core";

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
}

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

/** Save a note atomically, preserving its line endings and BOM. */
export function saveNote(path: string, note: NoteFile): Promise<void> {
  return invoke<void>("save_note", {
    path,
    content: note.content,
    eol: note.eol,
    bom: note.bom,
  });
}

/** A saved attachment, located by a vault-relative, forward-slash path. */
export interface SavedAttachment {
  relativePath: string;
}

/** Save a pasted/dropped image (base64) into the vault's attachments folder. */
export function saveAttachment(dataBase64: string, ext: string): Promise<SavedAttachment> {
  return invoke<SavedAttachment>("save_attachment", { dataBase64, ext });
}

/** Read an image inside the vault as a `data:` URL for the preview pane. */
export function readImage(path: string): Promise<string> {
  return invoke<string>("read_image", { path });
}
