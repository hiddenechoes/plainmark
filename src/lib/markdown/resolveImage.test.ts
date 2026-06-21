import { describe, expect, it } from "vitest";
import { resolveImagePath, type PreviewLocation } from "./resolveImage";

const loc: PreviewLocation = { vaultRoot: "/vault", noteDir: "/vault/Notes" };

describe("resolveImagePath", () => {
  it("resolves wiki embeds against the vault root", () => {
    expect(resolveImagePath(loc, "Attachments/x.png", true)).toBe("/vault/Attachments/x.png");
  });

  it("resolves standard images against the note's folder", () => {
    expect(resolveImagePath(loc, "img/y.png", false)).toBe("/vault/Notes/img/y.png");
  });

  it("returns null for remote and data URLs (never read from disk)", () => {
    expect(resolveImagePath(loc, "https://example.com/a.png", false)).toBeNull();
    expect(resolveImagePath(loc, "data:image/png;base64,AAAA", true)).toBeNull();
    expect(resolveImagePath(loc, "  ", false)).toBeNull();
  });

  it("uses the base path's separator for Windows vaults", () => {
    const win: PreviewLocation = { vaultRoot: "C:\\vault", noteDir: "C:\\vault\\Notes" };
    expect(resolveImagePath(win, "Attachments/x.png", true)).toBe("C:\\vault\\Attachments\\x.png");
  });
});
