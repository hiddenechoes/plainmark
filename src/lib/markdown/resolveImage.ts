// Resolve a preview image reference to an absolute vault path. The Rust side
// re-validates the path is inside the vault and reads the bytes, so this only
// has to produce a path string (it does not need to canonicalize `..`).
import { joinPath } from "../path";

export interface PreviewLocation {
  /** Absolute vault root. */
  vaultRoot: string;
  /** Absolute directory of the note being previewed. */
  noteDir: string;
}

/** Matches a leading URI scheme like `http:`, `https:`, `data:`. */
const HAS_SCHEME = /^[a-z][a-z0-9+.-]*:/i;

/** Resolve an image reference to an absolute path, or `null` if it carries a
 * URI scheme (remote/data URLs are handled separately, never read from disk).
 * Wiki embeds (`![[...]]`) resolve against the vault root; standard `![](...)`
 * images resolve against the note's own folder (SPEC §8.8/§8.9). */
export function resolveImagePath(
  loc: PreviewLocation,
  ref: string,
  isWiki: boolean,
): string | null {
  const trimmed = ref.trim();
  if (trimmed === "" || HAS_SCHEME.test(trimmed)) return null;
  const base = isWiki ? loc.vaultRoot : loc.noteDir;
  return joinPath(base, trimmed);
}
