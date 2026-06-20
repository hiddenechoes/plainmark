// CodeMirror 6 extension wiring lives here, kept modular per
// .claude/rules/frontend.md. Phase 0 is a plain Markdown source editor with
// syntax highlighting; rendering/preview arrives in Phase 1.
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import { defaultHighlightStyle, syntaxHighlighting } from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import {
  EditorView,
  drawSelection,
  highlightActiveLine,
  keymap,
  lineNumbers,
} from "@codemirror/view";

/** Build the editor extensions. `onChange` fires on every document edit. */
export function createEditorExtensions(onChange: (doc: string) => void): Extension[] {
  return [
    lineNumbers(),
    history(),
    drawSelection(),
    highlightActiveLine(),
    EditorView.lineWrapping,
    markdown(),
    syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
    keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
    EditorView.updateListener.of((update) => {
      if (update.docChanged) {
        onChange(update.state.doc.toString());
      }
    }),
  ];
}
