import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import remarkBreaks from "remark-breaks";
import remarkFrontmatter from "remark-frontmatter";
import rehypeKatex from "rehype-katex";
import { remarkWikiEmbed } from "../lib/markdown/remarkWikiEmbed";

// Mirrors Preview's plugin pipeline. (Preview itself pulls in Mermaid + the
// Tauri bridge, so we exercise the markdown pipeline directly here.)
function render(md: string): string {
  return renderToStaticMarkup(
    <Markdown
      remarkPlugins={[remarkFrontmatter, remarkGfm, remarkBreaks, remarkMath, remarkWikiEmbed]}
      rehypePlugins={[rehypeKatex]}
    >
      {md}
    </Markdown>,
  );
}

describe("markdown render pipeline", () => {
  it("renders GFM tables, strikethrough, task lists and footnotes", () => {
    const html = render(
      [
        "| a | b |",
        "|---|---|",
        "| 1 | 2 |",
        "",
        "~~gone~~",
        "",
        "- [x] done",
        "- [ ] todo",
        "",
        "Note[^1]",
        "",
        "[^1]: a footnote",
      ].join("\n"),
    );
    expect(html).toContain("<table>");
    expect(html).toContain("<del>");
    expect(html).toContain('type="checkbox"');
    expect(html).toContain("checked");
    expect(html).toContain("data-footnotes");
  });

  it("renders single newlines as line breaks (Obsidian-style)", () => {
    const html = render("first line\nsecond line");
    expect(html).toContain("<br");
  });

  it("renders inline and block math via bundled KaTeX", () => {
    const html = render("inline $a^2$ and\n\n$$\n\\sum_{i=1}^n i\n$$");
    expect(html).toContain("katex");
    expect(html).toContain("katex-display"); // block math wrapper
  });

  it("does not render frontmatter as body text", () => {
    const html = render("---\ntitle: Hello\nclassification: Secret\n---\n\nBody here");
    expect(html).not.toContain("classification");
    expect(html).toContain("Body here");
  });

  it("does not execute injected HTML or scripts (escaped, not passed through)", () => {
    const html = render('text <img src=x onerror="alert(1)"> and <script>alert(2)</script>');
    expect(html).not.toContain("<script>");
    expect(html).not.toMatch(/<img[^>]*onerror/);
    expect(html).toContain("&lt;script&gt;");
  });
});
