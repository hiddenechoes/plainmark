// Frontend wiki-link resolver. Mirrors the Rust resolution rules (SPEC §8.8) in
// `src-tauri/src/index.rs::resolve` so links render and navigate correctly
// without an async round-trip per link. It is fed by the `list_link_targets`
// snapshot and kept in sync with the backend by the resolution unit tests on
// both sides. All paths are vault-relative with forward slashes.

import type { NoteMeta } from "../tauri";

/** GitHub-style heading slug — mirror of `index.rs::slugify`. */
export function slugify(text: string): string {
  let out = "";
  for (const ch of text.trim()) {
    if (/[\p{L}\p{N}]/u.test(ch)) {
      out += ch.toLowerCase();
    } else if (ch === " " || ch === "-" || ch === "_") {
      out += "-";
    }
  }
  return out;
}

function normalizeTarget(target: string): string {
  return target
    .trim()
    .replace(/\\/g, "/")
    .replace(/^(\.\/)+/, "")
    .replace(/^\/+|\/+$/g, "");
}

function dirOf(relPath: string): string {
  const idx = relPath.lastIndexOf("/");
  return idx === -1 ? "" : relPath.slice(0, idx);
}

function stemOf(relPath: string): string {
  const file = relPath.slice(relPath.lastIndexOf("/") + 1);
  return file.endsWith(".md") ? file.slice(0, -3) : file;
}

function countSegments(relPath: string): number {
  return (relPath.match(/\//g) ?? []).length;
}

/** §8.8 tiebreak: same folder first, then fewest segments, then lexicographic. */
function compare(a: NoteMeta, b: NoteMeta, fromDir: string): number {
  const sameA = dirOf(a.relPath) === fromDir ? 0 : 1;
  const sameB = dirOf(b.relPath) === fromDir ? 0 : 1;
  if (sameA !== sameB) return sameA - sameB;
  const segA = countSegments(a.relPath);
  const segB = countSegments(b.relPath);
  if (segA !== segB) return segA - segB;
  if (a.relPath < b.relPath) return -1;
  if (a.relPath > b.relPath) return 1;
  return 0;
}

/** The outcome of resolving a `[[link]]`: the target note (if any) and whether
 * the optional `#heading` exists in it. */
export interface LinkResolution {
  meta: NoteMeta | null;
  headingOk: boolean;
}

export interface Resolver {
  /** Resolve a bare note target (no `#heading`) to a note, or `null`. */
  resolve(target: string, fromRel: string): NoteMeta | null;
  /** Resolve a target plus optional `#heading`, reporting heading existence. */
  status(target: string, heading: string | null, fromRel: string): LinkResolution;
}

/** Build a resolver from the link-target snapshot. Cheap to call per render. */
export function createResolver(targets: NoteMeta[]): Resolver {
  const byRel = new Map<string, NoteMeta>();
  const byStem = new Map<string, NoteMeta[]>();
  for (const meta of targets) {
    byRel.set(meta.relPath, meta);
    const stem = stemOf(meta.relPath).toLowerCase();
    const bucket = byStem.get(stem);
    if (bucket) bucket.push(meta);
    else byStem.set(stem, [meta]);
  }

  function resolve(target: string, fromRel: string): NoteMeta | null {
    const t = normalizeTarget(target);
    if (t === "") return null;

    // Exact path-qualified match (case-sensitive, like the backend).
    const withMd = /\.md$/i.test(t) ? t : `${t}.md`;
    const exact = byRel.get(withMd);
    if (exact) return exact;

    // Bare-name resolution on the final segment.
    const bare = t.replace(/\.md$/i, "");
    const name = bare.slice(bare.lastIndexOf("/") + 1).toLowerCase();
    const candidates = byStem.get(name);
    if (!candidates || candidates.length === 0) return null;

    const fromDir = dirOf(fromRel);
    return [...candidates].sort((a, b) => compare(a, b, fromDir))[0] ?? null;
  }

  function status(target: string, heading: string | null, fromRel: string): LinkResolution {
    const meta = resolve(target, fromRel);
    if (!meta) return { meta: null, headingOk: false };
    if (!heading) return { meta, headingOk: true };
    const reqSlug = slugify(heading);
    const reqLower = heading.trim().toLowerCase();
    const headingOk = meta.headings.some(
      (h) => h.slug === reqSlug || h.text.toLowerCase() === reqLower,
    );
    return { meta, headingOk };
  }

  return { resolve, status };
}
