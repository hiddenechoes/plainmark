import { describe, expect, it } from "vitest";
import { decideExternalChange } from "./externalChange";

const OPEN = "/v/Note.md";

describe("decideExternalChange", () => {
  it("reloads a clean buffer when the open note changes on disk", () => {
    expect(decideExternalChange({ kind: "modified", path: OPEN }, OPEN, false)).toEqual({
      action: "reload",
    });
  });

  it("prompts (never clobbers) when the open note changes with unsaved edits", () => {
    expect(decideExternalChange({ kind: "modified", path: OPEN }, OPEN, true)).toEqual({
      action: "prompt",
      removed: false,
    });
  });

  it("prompts when the open note is removed", () => {
    expect(decideExternalChange({ kind: "removed", path: OPEN }, OPEN, false)).toEqual({
      action: "prompt",
      removed: true,
    });
  });

  it("follows a clean buffer to the new path on external rename", () => {
    expect(
      decideExternalChange({ kind: "renamed", from: OPEN, to: "/v/Renamed.md" }, OPEN, false),
    ).toEqual({ action: "navigate", path: "/v/Renamed.md" });
  });

  it("prompts on external rename when there are unsaved edits", () => {
    expect(
      decideExternalChange({ kind: "renamed", from: OPEN, to: "/v/Renamed.md" }, OPEN, true),
    ).toEqual({ action: "prompt", removed: true });
  });

  it("ignores changes to other notes and when nothing is open", () => {
    expect(decideExternalChange({ kind: "modified", path: "/v/Other.md" }, OPEN, false)).toEqual({
      action: "ignore",
    });
    expect(decideExternalChange({ kind: "modified", path: OPEN }, null, false)).toEqual({
      action: "ignore",
    });
  });
});
