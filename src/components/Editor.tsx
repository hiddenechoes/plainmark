import { useEffect, useRef } from "react";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { createEditorExtensions } from "../lib/editor/setup";

interface EditorProps {
  /** Initial document. The editor is remounted (via React `key`) per file, so
   * this is read once at mount and not treated as a live prop afterwards. */
  doc: string;
  onChange: (doc: string) => void;
}

export function Editor({ doc, onChange }: EditorProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  // Capture the initial doc and keep the latest onChange without re-creating
  // the editor (which would reset the cursor on every keystroke).
  const initialDocRef = useRef(doc);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const view = new EditorView({
      state: EditorState.create({
        doc: initialDocRef.current,
        extensions: createEditorExtensions((d) => onChangeRef.current(d)),
      }),
      parent: host,
    });
    view.focus();
    return () => view.destroy();
  }, []);

  return <div className="editor-host" ref={hostRef} />;
}
