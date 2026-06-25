// Local-date helpers for daily notes (SPEC §8.3).
//
// "Today" must be the user's *local* calendar date: a note opened at 23:00 local
// must land on today, not tomorrow-UTC. We read the local components straight off
// `Date` (`getFullYear`/`getMonth`/`getDate`, never the UTC getters) and pass
// them to the backend, so the timezone is resolved exactly once, here, where it
// is unambiguous. The backend never reads a clock.

/** A local calendar date as plain components (`month` is 1-based). */
export interface LocalDate {
  year: number;
  month: number;
  day: number;
}

/** Extract the *local* calendar date from a `Date`, using the local getters so
 * the result is the date on the user's wall clock (correct near midnight). */
export function localDateParts(d: Date): LocalDate {
  return { year: d.getFullYear(), month: d.getMonth() + 1, day: d.getDate() };
}

/** Today's local date. */
export function today(): LocalDate {
  return localDateParts(new Date());
}

/** Format a local date as `YYYY-MM-DD`, e.g. for an `<input type="date">` value. */
export function toDateInputValue(d: LocalDate): string {
  const y = String(d.year).padStart(4, "0");
  const m = String(d.month).padStart(2, "0");
  const day = String(d.day).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/** Parse a `YYYY-MM-DD` value (from `<input type="date">`) into a LocalDate, or
 * `null` if it isn't a well-formed, in-range date string. */
export function parseDateInput(value: string): LocalDate | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!m) return null;
  const year = Number(m[1]);
  const month = Number(m[2]);
  const day = Number(m[3]);
  if (month < 1 || month > 12 || day < 1 || day > 31) return null;
  return { year, month, day };
}
