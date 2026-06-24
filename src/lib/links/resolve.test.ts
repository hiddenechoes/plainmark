import { describe, expect, it } from "vitest";
import { createResolver, slugify } from "./resolve";
import type { Heading, NoteMeta } from "../tauri";

// Mirror the Rust resolution tests (src-tauri/src/index.rs) so both sides agree.
function meta(rel: string, headings: Heading[] = []): NoteMeta {
  const stem = rel.slice(rel.lastIndexOf("/") + 1).replace(/\.md$/, "");
  return { path: `/vault/${rel}`, relPath: rel, title: stem, headings };
}

describe("slugify", () => {
  it("matches the GitHub-style slug rules", () => {
    expect(slugify("My Heading: Part 2!")).toBe("my-heading-part-2");
    expect(slugify("snake_case and-dash")).toBe("snake-case-and-dash");
  });
});

describe("createResolver", () => {
  it("resolves a bare name case-insensitively", () => {
    const r = createResolver([meta("Plan.md")]);
    expect(r.resolve("plan", "Inbox.md")?.relPath).toBe("Plan.md");
    expect(r.resolve("Missing", "Inbox.md")).toBeNull();
  });

  it("prefers the same folder on a name collision", () => {
    const r = createResolver([meta("Work/Plan.md"), meta("Personal/Plan.md")]);
    expect(r.resolve("Plan", "Work/Tasks.md")?.relPath).toBe("Work/Plan.md");
    // From elsewhere: same segment count, lexicographic tiebreak → Personal.
    expect(r.resolve("Plan", "Inbox.md")?.relPath).toBe("Personal/Plan.md");
  });

  it("prefers the shortest path", () => {
    const r = createResolver([meta("Plan.md"), meta("Archive/Old/Plan.md")]);
    expect(r.resolve("Plan", "Notes/X.md")?.relPath).toBe("Plan.md");
  });

  it("resolves an exact path-qualified target", () => {
    const r = createResolver([meta("Work/Plan.md"), meta("Personal/Plan.md")]);
    expect(r.resolve("Personal/Plan", "Work/X.md")?.relPath).toBe("Personal/Plan.md");
  });

  it("reports heading existence via slug or text", () => {
    const r = createResolver([meta("Plan.md", [{ text: "Goals", slug: "goals", level: 2 }])]);
    expect(r.status("Plan", "Goals", "X.md")).toMatchObject({ headingOk: true });
    expect(r.status("Plan", "goals", "X.md")).toMatchObject({ headingOk: true });
    expect(r.status("Plan", "Nope", "X.md")).toMatchObject({ headingOk: false });
    // No heading requested → trivially ok when the note resolves.
    expect(r.status("Plan", null, "X.md")).toMatchObject({ headingOk: true });
  });

  it("returns no meta for an unresolved target", () => {
    const r = createResolver([meta("Plan.md")]);
    expect(r.status("Ghost", null, "X.md").meta).toBeNull();
  });
});
