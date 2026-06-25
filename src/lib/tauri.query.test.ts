import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { runQuery, toggleTask } from "./tauri";

// The Tauri runtime isn't present under vitest; mock the command bridge so the
// wrappers can be tested in isolation. (`listen` is imported at module load by
// tauri.ts, so it must be mocked too.)
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const mockInvoke = vi.mocked(invoke);
afterEach(() => mockInvoke.mockReset());

describe("runQuery", () => {
  it("passes the source and the caller's local date (so `today` is local)", async () => {
    mockInvoke.mockResolvedValue({ error: null, tasks: [] });
    await runQuery("not done\ndue before today", { year: 2026, month: 6, day: 24 });
    expect(mockInvoke).toHaveBeenCalledWith("run_query", {
      source: "not done\ndue before today",
      year: 2026,
      month: 6,
      day: 24,
    });
  });

  it("surfaces an inline grammar error as data, not a rejection", async () => {
    mockInvoke.mockResolvedValue({ error: "unknown directive: group by file", tasks: [] });
    const res = await runQuery("group by file", { year: 2026, month: 6, day: 24 });
    expect(res.error).toContain("unknown directive");
    expect(res.tasks).toEqual([]);
  });
});

describe("toggleTask", () => {
  it("passes the expected text + status for backend re-verification (§7.1)", async () => {
    mockInvoke.mockResolvedValue(true);
    const done = await toggleTask("/vault/Work/Plan.md", 7, "Write the spec", false);
    expect(mockInvoke).toHaveBeenCalledWith("toggle_task", {
      path: "/vault/Work/Plan.md",
      line: 7,
      expectedText: "Write the spec",
      expectedDone: false,
    });
    expect(done).toBe(true);
  });
});
