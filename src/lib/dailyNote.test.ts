import { describe, expect, it } from "vitest";
import { localDateParts, parseDateInput, toDateInputValue, today } from "./dailyNote";

describe("localDateParts", () => {
  // The watch-item: a daily note opened at 23:00 *local* must land on today's
  // local date, not tomorrow-UTC. `new Date(2026, 5, 24, 23, ...)` constructs a
  // local time, so the local getters report 2026-06-24 regardless of the host
  // timezone — whereas a UTC reading could roll to the 25th east of UTC. We use
  // the local getters, so the result stays correct near midnight.
  it("reads the local calendar date, not UTC (near-midnight case)", () => {
    const nearMidnight = new Date(2026, 5, 24, 23, 0, 0);
    expect(localDateParts(nearMidnight)).toEqual({ year: 2026, month: 6, day: 24 });
  });

  it("uses 1-based months at the year boundaries", () => {
    expect(localDateParts(new Date(2026, 0, 1, 9))).toEqual({ year: 2026, month: 1, day: 1 });
    expect(localDateParts(new Date(2026, 11, 31, 9))).toEqual({ year: 2026, month: 12, day: 31 });
  });
});

describe("today", () => {
  it("returns the current local date components", () => {
    const now = new Date();
    expect(today()).toEqual(localDateParts(now));
  });
});

describe("date-input round-trip", () => {
  it("formats and parses YYYY-MM-DD, zero-padding single digits", () => {
    const d = { year: 2026, month: 3, day: 5 };
    expect(toDateInputValue(d)).toBe("2026-03-05");
    expect(parseDateInput("2026-03-05")).toEqual(d);
  });

  it("rejects malformed or out-of-range values", () => {
    expect(parseDateInput("nope")).toBeNull();
    expect(parseDateInput("2026-3-5")).toBeNull();
    expect(parseDateInput("2026-13-01")).toBeNull();
    expect(parseDateInput("2026-00-10")).toBeNull();
    expect(parseDateInput("2026-06-00")).toBeNull();
  });
});
