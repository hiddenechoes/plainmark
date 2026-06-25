import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { BacklinksList } from "./BacklinksPanel";
import type { Backlink } from "../lib/tauri";

function back(from: string, fromTitle: string, line: number, snippet: string): Backlink {
  return { from, fromTitle, line, snippet };
}

describe("BacklinksList", () => {
  it("renders each inbound link with its title, line, and snippet", () => {
    const html = renderToStaticMarkup(
      <BacklinksList
        backlinks={[
          back("/v/A.md", "A", 3, "see [[Target]] here"),
          back("/v/B.md", "B", 1, "and [[Target]] again"),
        ]}
        onNavigate={() => {}}
      />,
    );
    expect(html).toContain("A");
    expect(html).toContain(":3");
    expect(html).toContain("see [[Target]] here");
    expect(html).toContain("B");
    expect(html).toContain("and [[Target]] again");
    // One clickable item per backlink.
    expect(html.match(/backlink-item/g)).toHaveLength(2);
  });

  it("shows an empty state when there are no backlinks", () => {
    const html = renderToStaticMarkup(<BacklinksList backlinks={[]} onNavigate={() => {}} />);
    expect(html).toContain("No backlinks.");
  });
});
