// Pure decision logic for external-change handling (SPEC §4.1). Given a watcher
// change, the open note's path, and whether the buffer has unsaved edits, decide
// what the UI should do — never a silent clobber. Kept pure so it's unit-tested
// directly; App performs the resulting action.
import type { NoteChange } from "./tauri";

export type ExternalDecision =
  | { action: "ignore" }
  | { action: "reload" }
  | { action: "prompt"; removed: boolean }
  | { action: "navigate"; path: string };

export function decideExternalChange(
  change: NoteChange,
  openPath: string | null,
  dirty: boolean,
): ExternalDecision {
  if (!openPath) return { action: "ignore" };

  switch (change.kind) {
    case "created":
    case "modified":
      if (change.path !== openPath) return { action: "ignore" };
      // Clean buffer reloads; unsaved edits prompt (no-blind-clobber).
      return dirty ? { action: "prompt", removed: false } : { action: "reload" };
    case "removed":
      return change.path === openPath ? { action: "prompt", removed: true } : { action: "ignore" };
    case "renamed":
      if (change.from !== openPath) return { action: "ignore" };
      // Follow a clean buffer to the new path; otherwise prompt before losing it.
      return dirty ? { action: "prompt", removed: true } : { action: "navigate", path: change.to };
  }
}
