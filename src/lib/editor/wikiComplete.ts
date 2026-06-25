// CodeMirror 6 autocomplete for `[[wikilinks]]` (SPEC §8.2). Typing `[[`
// suggests note titles; after a `#` it suggests the target note's headings.
// Selecting a suggestion inserts a working link, adding the closing `]]` if it
// isn't already there. Completions are sourced from the live link-target
// snapshot via a getter so the editor never has to be re-created when it changes.
import {
  type Completion,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete";
import type { EditorView } from "@codemirror/view";
import type { NoteMeta } from "../tauri";

/** Insert `text`, appending `]]` unless the document already has it next, and
 * leave the cursor after the link. */
function applyLink(text: string) {
  return (view: EditorView, _completion: Completion, from: number, to: number) => {
    const closing = view.state.sliceDoc(to, to + 2) === "]]" ? "" : "]]";
    view.dispatch({
      changes: { from, to, insert: text + closing },
      selection: { anchor: from + text.length + closing.length },
    });
  };
}

/** Find a note for the `[[Note#...]]` heading phase: by title, else by stem,
 * case-insensitively (first match wins — good enough for suggestions). */
function findNote(targets: NoteMeta[], notePart: string): NoteMeta | undefined {
  const wanted = notePart.trim().toLowerCase();
  if (wanted === "") return undefined;
  return targets.find((m) => {
    const stem = m.relPath.slice(m.relPath.lastIndexOf("/") + 1).replace(/\.md$/, "");
    return m.title.toLowerCase() === wanted || stem.toLowerCase() === wanted;
  });
}

/** A completion source for `[[` links, reading the current targets each call. */
export function wikiCompletionSource(getTargets: () => NoteMeta[]) {
  return (context: CompletionContext): CompletionResult | null => {
    // Match an unclosed `[[` up to the cursor on the current line.
    const open = context.matchBefore(/\[\[[^\]\n]*/);
    if (!open) return null;

    const typed = open.text.slice(2); // drop the leading `[[`
    const targets = getTargets();
    const hashIdx = typed.indexOf("#");

    if (hashIdx >= 0) {
      // Heading phase: suggest the resolved note's headings.
      const note = findNote(targets, typed.slice(0, hashIdx));
      if (!note || note.headings.length === 0) return null;
      const options: Completion[] = note.headings.map((h) => ({
        label: h.text,
        type: "property",
        apply: applyLink(h.text),
      }));
      return { from: open.from + 2 + hashIdx + 1, options, filter: true };
    }

    // Note phase: suggest note titles (detail shows the path for disambiguation).
    if (targets.length === 0) return null;
    const options: Completion[] = targets.map((m) => ({
      label: m.title,
      detail: m.relPath,
      type: "class",
      apply: applyLink(m.title),
    }));
    return { from: open.from + 2, options, filter: true };
  };
}
