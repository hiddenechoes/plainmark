import { useEffect, useRef } from "react";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { createEditorExtensions } from "../lib/editor/setup";
import { isImagePath } from "../lib/image";

interface EditorProps {
  /** Initial document. The editor is remounted (via React `key`) per file, so
   * this is read once at mount and not treated as a live prop afterwards. */
  doc: string;
  onChange: (doc: string) => void;
  /** Read an image from the system clipboard; resolves to the vault-relative
   * path to embed, or `null` if the clipboard holds no image. */
  onPasteImage?: () => Promise<string | null>;
  /** Copy a dropped image file (absolute source path) into the vault; resolves
   * to the vault-relative path to embed. */
  onImportImage?: (sourcePath: string) => Promise<string>;
  /** Surface a backend error (e.g. an image write that failed). */
  onError?: (message: string) => void;
}

export function Editor({ doc, onChange, onPasteImage, onImportImage, onError }: EditorProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  // Capture the initial doc and keep the latest callbacks without re-creating
  // the editor (which would reset the cursor on every keystroke).
  const initialDocRef = useRef(doc);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const onPasteImageRef = useRef(onPasteImage);
  onPasteImageRef.current = onPasteImage;
  const onImportImageRef = useRef(onImportImage);
  onImportImageRef.current = onImportImage;
  const onErrorRef = useRef(onError);
  onErrorRef.current = onError;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
    const insertEmbed = (view: EditorView, relativePath: string) => {
      view.dispatch(view.state.replaceSelection(`![[${relativePath}]]`));
    };

    // Paste: the webview's clipboard rarely carries image bytes (notably on
    // Linux/WebKitGTK), so we ask Rust to read the system clipboard. A text
    // paste returns null and proceeds through CodeMirror unchanged (§8.9).
    const pasteHandler = EditorView.domEventHandlers({
      paste: (_event, view) => {
        const read = onPasteImageRef.current;
        if (!read) return false;
        void read()
          .then((relativePath) => {
            if (relativePath) insertEmbed(view, relativePath);
          })
          .catch((e) => onErrorRef.current?.(message(e)));
        return false;
      },
    });

    const view = new EditorView({
      state: EditorState.create({
        doc: initialDocRef.current,
        extensions: [...createEditorExtensions((d) => onChangeRef.current(d)), pasteHandler],
      }),
      parent: host,
    });
    view.focus();

    // Drag-drop: Tauri intercepts OS file drops at the window level, so the
    // HTML5 `drop` event never fires. Use the native drag-drop event, which
    // gives file paths, and copy each image into the vault via Rust (§8.9).
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type !== "drop") return;
        const importImage = onImportImageRef.current;
        if (!importImage) return;
        for (const path of event.payload.paths) {
          if (!isImagePath(path)) continue;
          void importImage(path)
            .then((relativePath) => insertEmbed(view, relativePath))
            .catch((e) => onErrorRef.current?.(message(e)));
        }
      })
      .then((un) => {
        if (cancelled) un();
        else unlisten = un;
      })
      .catch((e) => onErrorRef.current?.(message(e)));

    return () => {
      cancelled = true;
      unlisten?.();
      view.destroy();
    };
  }, []);

  return <div className="editor-host" ref={hostRef} />;
}
