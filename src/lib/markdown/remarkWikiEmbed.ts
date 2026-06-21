// A small remark transform for the plainmark `![[embed]]` extension (SPEC §8.7,
// §8.9). CommonMark leaves `![[path]]` as literal text (it isn't a valid image),
// so we walk text nodes and turn image embeds into mdast `image` nodes, tagging
// them so the preview's <img> renderer resolves them against the vault root.
//
// Phase 1 scope: only the image embed form `![[...]]`. Plain `[[wikilinks]]`
// stay as literal text — link resolution/backlinks are Phase 2.
import type { Image, Root, RootContent, Text } from "mdast";
import type { Node, Parent } from "unist";

// Inner = everything up to an optional `#heading` or `|alias`, which we drop for
// image resolution. `g` for iteration; reset `lastIndex` before each use.
const EMBED_RE = /!\[\[([^\]\n#|]+)(?:[#|][^\]\n]*)?\]\]/g;

function hasEmbed(value: string): boolean {
  return /!\[\[[^\]\n]+\]\]/.test(value);
}

function splitText(value: string): RootContent[] {
  const out: RootContent[] = [];
  let last = 0;
  EMBED_RE.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = EMBED_RE.exec(value)) !== null) {
    if (match.index > last) {
      out.push({ type: "text", value: value.slice(last, match.index) } as Text);
    }
    const ref = match[1].trim();
    out.push({
      type: "image",
      url: ref,
      alt: ref,
      // Marker read back by the preview's <img> component (hProperties are
      // applied verbatim to the hast element's properties).
      data: { hProperties: { "data-embed": "wiki" } },
    } as Image);
    last = match.index + match[0].length;
  }
  if (last < value.length) {
    out.push({ type: "text", value: value.slice(last) } as Text);
  }
  return out;
}

function isParent(node: Node): node is Parent {
  return Array.isArray((node as Parent).children);
}

function walk(node: Parent): void {
  const next: Node[] = [];
  for (const child of node.children) {
    if (child.type === "text" && hasEmbed((child as Text).value)) {
      next.push(...splitText((child as Text).value));
    } else {
      if (isParent(child)) walk(child);
      next.push(child);
    }
  }
  node.children = next as Parent["children"];
}

/** remark plugin: rewrite `![[path]]` image embeds into image nodes. */
export function remarkWikiEmbed() {
  return (tree: Root): void => {
    walk(tree);
  };
}
