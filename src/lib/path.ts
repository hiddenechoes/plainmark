// Small pure path helpers for displaying vault-relative names in the UI.
// Vault paths come from Rust as absolute OS paths (either `/` or `\` separated).

/** The final path segment (file or folder name). */
export function basename(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const idx = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  return idx === -1 ? trimmed : trimmed.slice(idx + 1);
}

/** A note's path relative to the vault root, for display. */
export function relativeTo(root: string, path: string): string {
  if (path.startsWith(root)) {
    return path.slice(root.length).replace(/^[\\/]+/, "");
  }
  return path;
}

/** The directory portion of a path (everything before the last separator). */
export function dirname(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const idx = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  return idx === -1 ? "" : trimmed.slice(0, idx);
}

/** Join a relative `ref` onto an absolute `base`, using whichever separator the
 * base already uses. `..`/`.` segments are left for the Rust side to canonicalize
 * (which also re-validates the result is inside the vault). */
export function joinPath(base: string, ref: string): string {
  const sep = base.includes("\\") && !base.includes("/") ? "\\" : "/";
  const cleanBase = base.replace(/[\\/]+$/, "");
  const cleanRef = ref.replace(/^[\\/]+/, "").replace(/[\\/]+/g, sep);
  return cleanBase ? `${cleanBase}${sep}${cleanRef}` : cleanRef;
}
