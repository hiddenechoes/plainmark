import { describe, expect, it } from "vitest";
import type { Link, Paragraph, Root, Text } from "mdast";
import { parseWikiLink, remarkWikiLink } from "./remarkWikiLink";

function paragraph(value: string): Root {
  return {
    type: "root",
    children: [{ type: "paragraph", children: [{ type: "text", value }] }],
  };
}

function children(tree: Root) {
  return (tree.children[0] as Paragraph).children;
}

function attrs(node: Link): Record<string, unknown> {
  const data = node.data as { hProperties?: Record<string, unknown> } | undefined;
  return data?.hProperties ?? {};
}

describe("parseWikiLink", () => {
  it("splits target, heading, and alias", () => {
    expect(parseWikiLink("Note")).toEqual({ target: "Note", heading: null, display: "Note" });
    expect(parseWikiLink("Note#Goals")).toEqual({
      target: "Note",
      heading: "Goals",
      display: "Note#Goals",
    });
    expect(parseWikiLink("Note#Goals|Shown")).toEqual({
      target: "Note",
      heading: "Goals",
      display: "Shown",
    });
  });
});

describe("remarkWikiLink", () => {
  it("turns [[Note]] into a tagged link node split out of text", () => {
    const tree = paragraph("see [[Other Note]] end");
    remarkWikiLink()(tree);

    const kids = children(tree);
    expect(kids.map((k) => k.type)).toEqual(["text", "link", "text"]);
    const link = kids[1] as Link;
    expect(attrs(link)["data-wikilink"]).toBe("true");
    expect(attrs(link)["data-target"]).toBe("Other Note");
    expect((link.children[0] as Text).value).toBe("Other Note");
  });

  it("captures the #heading and shows the alias as display text", () => {
    const tree = paragraph("[[Plan#Goals|Roadmap]]");
    remarkWikiLink()(tree);

    const link = children(tree)[0] as Link;
    expect(attrs(link)["data-target"]).toBe("Plan");
    expect(attrs(link)["data-heading"]).toBe("Goals");
    expect((link.children[0] as Text).value).toBe("Roadmap");
  });

  it("leaves image embeds untouched (the ! lookbehind)", () => {
    const tree = paragraph("![[image.png]]");
    remarkWikiLink()(tree);
    const kids = children(tree);
    expect(kids).toHaveLength(1);
    expect(kids[0].type).toBe("text");
    expect((kids[0] as Text).value).toBe("![[image.png]]");
  });

  it("leaves a same-note [[#heading]] (empty target) as literal text", () => {
    const tree = paragraph("[[#Goals]]");
    remarkWikiLink()(tree);
    const kids = children(tree);
    expect(kids[0].type).toBe("text");
    expect((kids[0] as Text).value).toBe("[[#Goals]]");
  });
});
