// CodeMirror 6 extension wiring lives here, kept modular per
// .claude/rules/frontend.md. Phase 0 is a plain Markdown source editor with
// syntax highlighting; rendering/preview arrives in Phase 1.
import { autocompletion, completionKeymap } from "@codemirror/autocomplete";
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
import type { NoteMeta } from "../tauri";
import { wikiCompletionSource } from "./wikiComplete";

/** Build the editor extensions. `onChange` fires on every document edit;
 * `getTargets` supplies the live note snapshot for `[[` autocomplete. */
export function createEditorExtensions(
  onChange: (doc: string) => void,
  getTargets: () => NoteMeta[],
): Extension[] {
  return [
    lineNumbers(),
    history(),
    drawSelection(),
    highlightActiveLine(),
    EditorView.lineWrapping,
    markdown(),
    syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
    autocompletion({ override: [wikiCompletionSource(getTargets)] }),
    keymap.of([...completionKeymap, ...defaultKeymap, ...historyKeymap, indentWithTab]),
    EditorView.updateListener.of((update) => {
      if (update.docChanged) {
        onChange(update.state.doc.toString());
      }
    }),
  ];
}
