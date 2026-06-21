import { useEffect, useRef } from "react";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { createEditorExtensions } from "../lib/editor/setup";
import { fileToBase64, imageExt, imageFilesFrom } from "../lib/image";

interface EditorProps {
  /** Initial document. The editor is remounted (via React `key`) per file, so
   * this is read once at mount and not treated as a live prop afterwards. */
  doc: string;
  onChange: (doc: string) => void;
  /** Persist a pasted/dropped image; returns the vault-relative path to embed. */
  onSaveImage?: (dataBase64: string, ext: string) => Promise<string>;
  /** Surface a backend error (e.g. an image write that failed). */
  onError?: (message: string) => void;
}

export function Editor({ doc, onChange, onSaveImage, onError }: EditorProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  // Capture the initial doc and keep the latest callbacks without re-creating
  // the editor (which would reset the cursor on every keystroke).
  const initialDocRef = useRef(doc);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const onSaveImageRef = useRef(onSaveImage);
  onSaveImageRef.current = onSaveImage;
  const onErrorRef = useRef(onError);
  onErrorRef.current = onError;

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

    // Image paste/drop: hand bytes to Rust, then insert an `![[...]]` embed at
    // the selection (SPEC §8.9). The dispatch flows through the normal change
    // path, so the note becomes dirty and saves atomically like any edit.
    const insertImages = async (files: File[]) => {
      const save = onSaveImageRef.current;
      if (!save) return;
      for (const file of files) {
        try {
          const relativePath = await save(await fileToBase64(file), imageExt(file));
          view.dispatch(view.state.replaceSelection(`![[${relativePath}]]`));
        } catch (e) {
          onErrorRef.current?.(e instanceof Error ? e.message : String(e));
        }
      }
    };

    const onPaste = (e: ClipboardEvent) => {
      const files = imageFilesFrom(e.clipboardData);
      if (files.length === 0) return;
      e.preventDefault();
      void insertImages(files);
    };
    const onDrop = (e: DragEvent) => {
      const files = imageFilesFrom(e.dataTransfer);
      if (files.length === 0) return;
      e.preventDefault();
      void insertImages(files);
    };
    const onDragOver = (e: DragEvent) => {
      if (Array.from(e.dataTransfer?.items ?? []).some((i) => i.kind === "file")) {
        e.preventDefault();
      }
    };

    view.dom.addEventListener("paste", onPaste);
    view.dom.addEventListener("drop", onDrop);
    view.dom.addEventListener("dragover", onDragOver);

    return () => {
      view.dom.removeEventListener("paste", onPaste);
      view.dom.removeEventListener("drop", onDrop);
      view.dom.removeEventListener("dragover", onDragOver);
      view.destroy();
    };
  }, []);

  return <div className="editor-host" ref={hostRef} />;
}
