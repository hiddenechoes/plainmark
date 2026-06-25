import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { openDailyNote } from "./tauri";

// The Tauri runtime isn't present under vitest; mock the command bridge so the
// wrapper can be tested in isolation. (`listen` is imported at module load by
// tauri.ts, so it must be mocked too.)
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const mockInvoke = vi.mocked(invoke);

afterEach(() => mockInvoke.mockReset());

describe("openDailyNote", () => {
  it("invokes the backend command with the local date components", async () => {
    mockInvoke.mockResolvedValue("/vault/Daily/2026-06-24.md");
    const path = await openDailyNote(2026, 6, 24);
    expect(mockInvoke).toHaveBeenCalledWith("open_daily_note", {
      year: 2026,
      month: 6,
      day: 24,
    });
    expect(path).toBe("/vault/Daily/2026-06-24.md");
  });

  it("opening twice the same day resolves to the same file", async () => {
    // The backend is create-or-open and idempotent (proven in daily.rs); two
    // invocations for the same date return the same path.
    mockInvoke.mockResolvedValue("/vault/Daily/2026-06-24.md");
    const first = await openDailyNote(2026, 6, 24);
    const second = await openDailyNote(2026, 6, 24);
    expect(second).toBe(first);
    expect(mockInvoke).toHaveBeenNthCalledWith(1, "open_daily_note", {
      year: 2026,
      month: 6,
      day: 24,
    });
    expect(mockInvoke).toHaveBeenNthCalledWith(2, "open_daily_note", {
      year: 2026,
      month: 6,
      day: 24,
    });
  });
});
