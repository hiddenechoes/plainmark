import { describe, expect, it } from "vitest";
import { basename, relativeTo } from "./path";

describe("basename", () => {
  it("returns the last segment of a unix path", () => {
    expect(basename("/home/user/vault/note.md")).toBe("note.md");
  });

  it("returns the last segment of a windows path", () => {
    expect(basename("C:\\Users\\me\\vault\\note.md")).toBe("note.md");
  });

  it("ignores a trailing separator", () => {
    expect(basename("/home/user/vault/")).toBe("vault");
  });
});

describe("relativeTo", () => {
  it("strips the vault root and leading separator", () => {
    expect(relativeTo("/home/user/vault", "/home/user/vault/Projects/a.md")).toBe("Projects/a.md");
  });

  it("returns the path unchanged when it is not under the root", () => {
    expect(relativeTo("/home/user/vault", "/etc/passwd")).toBe("/etc/passwd");
  });
});
