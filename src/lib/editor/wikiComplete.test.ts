import { describe, expect, it } from "vitest";
import { CompletionContext } from "@codemirror/autocomplete";
import { EditorState } from "@codemirror/state";
import { wikiCompletionSource } from "./wikiComplete";
import type { Heading, NoteMeta } from "../tauri";

function meta(rel: string, headings: Heading[] = []): NoteMeta {
  const stem = rel.slice(rel.lastIndexOf("/") + 1).replace(/\.md$/, "");
  return { path: `/vault/${rel}`, relPath: rel, title: stem, headings };
}

// Build a CompletionContext with the cursor at the end of `doc`.
function complete(doc: string, targets: NoteMeta[]) {
  const state = EditorState.create({ doc, selection: { anchor: doc.length } });
  const context = new CompletionContext(state, doc.length, false);
  return wikiCompletionSource(() => targets)(context);
}

describe("wikiCompletionSource", () => {
  it("suggests note titles after [[", () => {
    const result = complete("see [[Pl", [meta("Plan.md"), meta("Inbox.md")]);
    expect(result).not.toBeNull();
    // `from` points just after the `[[` so the typed prefix is replaced.
    expect(result?.from).toBe(6);
    expect(result?.options.map((o) => o.label).sort()).toEqual(["Inbox", "Plan"]);
  });

  it("does not trigger outside an open [[", () => {
    expect(complete("just text", [meta("Plan.md")])).toBeNull();
    // A closed link before the cursor must not re-trigger.
    expect(complete("[[Plan]] ", [meta("Plan.md")])).toBeNull();
  });

  it("suggests the target note's headings after #", () => {
    const plan = meta("Plan.md", [
      { text: "Goals", slug: "goals", level: 2 },
      { text: "Risks", slug: "risks", level: 2 },
    ]);
    const result = complete("[[Plan#", [plan]);
    expect(result).not.toBeNull();
    expect(result?.from).toBe(7); // just after the `#`
    expect(result?.options.map((o) => o.label)).toEqual(["Goals", "Risks"]);
  });

  it("returns nothing for headings of an unknown note", () => {
    expect(complete("[[Ghost#", [meta("Plan.md")])).toBeNull();
  });
});
