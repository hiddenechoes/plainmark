import { describe, expect, it } from "vitest";
import type { Image, Paragraph, Root, Text } from "mdast";
import { remarkWikiEmbed } from "./remarkWikiEmbed";

function paragraph(value: string): Root {
  return {
    type: "root",
    children: [{ type: "paragraph", children: [{ type: "text", value }] }],
  };
}

function children(tree: Root) {
  return (tree.children[0] as Paragraph).children;
}

function embedMarker(node: Image): string | undefined {
  const data = node.data as { hProperties?: Record<string, string> } | undefined;
  return data?.hProperties?.["data-embed"];
}

describe("remarkWikiEmbed", () => {
  it("converts an image embed into an image node split out of surrounding text", () => {
    const tree = paragraph("see ![[Attachments/a.png]] end");
    remarkWikiEmbed()(tree);

    const kids = children(tree);
    expect(kids.map((k) => k.type)).toEqual(["text", "image", "text"]);
    expect((kids[0] as Text).value).toBe("see ");
    const img = kids[1] as Image;
    expect(img.url).toBe("Attachments/a.png");
    expect(embedMarker(img)).toBe("wiki");
    expect((kids[2] as Text).value).toBe(" end");
  });

  it("strips a #heading or |alias from the embed path", () => {
    const tree = paragraph("![[b.png|alias]] ![[c.png#frag]]");
    remarkWikiEmbed()(tree);

    const imgs = children(tree).filter((k): k is Image => k.type === "image");
    expect(imgs.map((i) => i.url)).toEqual(["b.png", "c.png"]);
  });

  it("leaves plain [[wikilinks]] as literal text (Phase 2 scope)", () => {
    const tree = paragraph("a [[Note]] b");
    remarkWikiEmbed()(tree);

    const kids = children(tree);
    expect(kids).toHaveLength(1);
    expect(kids[0].type).toBe("text");
    expect((kids[0] as Text).value).toBe("a [[Note]] b");
  });
});
