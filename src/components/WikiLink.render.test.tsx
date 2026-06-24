import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { LinkContext, type LinkContextValue } from "../lib/links/context";
import { WikiLink } from "./WikiLink";
import { createResolver } from "../lib/links/resolve";
import type { Heading, NoteMeta } from "../lib/tauri";

function meta(rel: string, headings: Heading[] = []): NoteMeta {
  const stem = rel.slice(rel.lastIndexOf("/") + 1).replace(/\.md$/, "");
  return { path: `/vault/${rel}`, relPath: rel, title: stem, headings };
}

function renderLink(
  targets: NoteMeta[],
  target: string,
  heading: string | null,
  overrides: Partial<LinkContextValue> = {},
): string {
  const ctx: LinkContextValue = {
    resolver: createResolver(targets),
    fromRel: "Inbox.md",
    onNavigate: () => {},
    onCreate: () => {},
    ...overrides,
  };
  return renderToStaticMarkup(
    <LinkContext.Provider value={ctx}>
      <WikiLink target={target} heading={heading}>
        {heading ? `${target}#${heading}` : target}
      </WikiLink>
    </LinkContext.Provider>,
  );
}

describe("WikiLink rendering", () => {
  it("renders a resolved link as a plain wikilink", () => {
    const html = renderLink([meta("Plan.md")], "Plan", null);
    expect(html).toContain('class="wikilink"');
    expect(html).not.toContain("wikilink-unresolved");
    expect(html).toContain("Plan");
  });

  it("renders an unresolved link distinctly (offers create)", () => {
    const html = renderLink([], "Ghost", null);
    expect(html).toContain("wikilink-unresolved");
    expect(html).toContain('role="button"');
    expect(html).toContain("Ghost");
  });

  it("flags a resolved link whose heading is missing", () => {
    const html = renderLink(
      [meta("Plan.md", [{ text: "Goals", slug: "goals", level: 2 }])],
      "Plan",
      "Nonexistent",
    );
    expect(html).toContain("wikilink-bad-heading");
  });
});
