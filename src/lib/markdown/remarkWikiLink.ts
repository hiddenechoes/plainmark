// A small remark transform for plain `[[wikilinks]]` (SPEC §8.2, §8.8). It runs
// *after* remarkWikiEmbed (which has already consumed `![[embeds]]`), and walks
// text nodes turning `[[target#heading|alias]]` into mdast `link` nodes tagged
// with data attributes. The preview's `a` renderer reads those tags and renders
// a WikiLink that resolves against the index (resolved → clickable; unresolved →
// muted, offers to create).
//
// `[[...]]` inside code spans/blocks is untouched: react-markdown puts code in
// `inlineCode`/`code` nodes, not `text` nodes, so walking text nodes skips it.
import type { Link, Root, RootContent, Text } from "mdast";
import type { Node, Parent } from "unist";

// `[[...]]` not preceded by `!` (that's an embed). `g` for iteration; reset
// `lastIndex` before each use.
const LINK_RE = /(?<!!)\[\[([^\]\n]+)\]\]/g;

function hasLink(value: string): boolean {
  return /(?<!!)\[\[[^\]\n]+\]\]/.test(value);
}

function splitOnce(value: string, sep: string): [string, string | null] {
  const idx = value.indexOf(sep);
  return idx === -1 ? [value, null] : [value.slice(0, idx), value.slice(idx + 1)];
}

/** Parse the body between `[[` and `]]` into target, optional heading, and the
 * display text (alias if present, else the target + heading as written). */
export function parseWikiLink(inner: string): {
  target: string;
  heading: string | null;
  display: string;
} {
  const [beforeAlias, alias] = splitOnce(inner, "|");
  const [target, heading] = splitOnce(beforeAlias, "#");
  const headingTrimmed = heading?.trim() ?? "";
  const display = (alias ?? beforeAlias).trim();
  return {
    target: target.trim(),
    heading: headingTrimmed === "" ? null : headingTrimmed,
    display,
  };
}

function splitText(value: string): RootContent[] {
  const out: RootContent[] = [];
  let last = 0;
  LINK_RE.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = LINK_RE.exec(value)) !== null) {
    if (match.index > last) {
      out.push({ type: "text", value: value.slice(last, match.index) } as Text);
    }
    const { target, heading, display } = parseWikiLink(match[1]);
    if (target === "") {
      // e.g. `[[#Heading]]` (same-note links are out of scope) — keep literal.
      out.push({ type: "text", value: match[0] } as Text);
    } else {
      out.push({
        type: "link",
        // href is irrelevant: the WikiLink renderer handles navigation. A "#"
        // keeps it a valid mdast link node.
        url: "#",
        data: {
          hProperties: {
            "data-wikilink": "true",
            "data-target": target,
            "data-heading": heading ?? "",
          },
        },
        children: [{ type: "text", value: display || target }],
      } as Link);
    }
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
    if (child.type === "text" && hasLink((child as Text).value)) {
      next.push(...splitText((child as Text).value));
    } else {
      if (isParent(child)) walk(child);
      next.push(child);
    }
  }
  node.children = next as Parent["children"];
}

/** remark plugin: rewrite `[[wikilinks]]` into tagged link nodes. */
export function remarkWikiLink() {
  return (tree: Root): void => {
    walk(tree);
  };
}
