import { useEffect, useRef } from "react";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { createEditorExtensions } from "../lib/editor/setup";
import { isImagePath } from "../lib/image";
import type { NoteMeta } from "../lib/tauri";

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
  /** Live note snapshot for `[[` autocomplete (read on demand, so updates don't
   * re-create the editor). */
  linkTargets?: NoteMeta[];
  /** 1-based line to scroll to + select (e.g. opening a query result at its
   * source line). Applied on mount and whenever `gotoNonce` changes. */
  gotoLine?: number;
  /** Bumped by the caller to request a jump to `gotoLine` even when the note is
   * already open (so repeat clicks on the same line re-scroll). */
  gotoNonce?: number;
  /** Surface a backend error (e.g. an image write that failed). */
  onError?: (message: string) => void;
}

export function Editor({
  doc,
  onChange,
  onPasteImage,
  onImportImage,
  linkTargets,
  gotoLine,
  gotoNonce,
  onError,
}: EditorProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  // Latest goto line in a ref, so the jump effect (keyed on the nonce) reads the
  // current target without re-running on every line change.
  const gotoLineRef = useRef(gotoLine);
  useEffect(() => {
    gotoLineRef.current = gotoLine;
  });
  // Capture the initial doc and keep the latest callbacks without re-creating
  // the editor (which would reset the cursor on every keystroke).
  const initialDocRef = useRef(doc);
  const onChangeRef = useRef(onChange);
  const onPasteImageRef = useRef(onPasteImage);
  const onImportImageRef = useRef(onImportImage);
  const linkTargetsRef = useRef(linkTargets);
  const onErrorRef = useRef(onError);
  // Keep the latest callbacks in refs without re-creating the editor. Writing
  // refs in an effect (rather than during render) runs after every render and
  // before any editor event handler can read them.
  useEffect(() => {
    onChangeRef.current = onChange;
    onPasteImageRef.current = onPasteImage;
    onImportImageRef.current = onImportImage;
    linkTargetsRef.current = linkTargets;
    onErrorRef.current = onError;
  });

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
        extensions: [
          ...createEditorExtensions(
            (d) => onChangeRef.current(d),
            () => linkTargetsRef.current ?? [],
          ),
          pasteHandler,
        ],
      }),
      parent: host,
    });
    viewRef.current = view;
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
      viewRef.current = null;
      view.destroy();
    };
  }, []);

  // Jump to a 1-based line on mount and whenever the caller bumps `gotoNonce`.
  // Runs after the editor-creation effect (declared above), so `viewRef` is set
  // even on the first mount of a freshly-opened note.
  useEffect(() => {
    const view = viewRef.current;
    const line = gotoLineRef.current;
    if (!view || line == null) return;
    const clamped = Math.min(Math.max(Math.trunc(line), 1), view.state.doc.lines);
    const target = view.state.doc.line(clamped);
    view.dispatch({
      selection: { anchor: target.from },
      effects: EditorView.scrollIntoView(target.from, { y: "center" }),
    });
    view.focus();
  }, [gotoNonce]);

  return <div className="editor-host" ref={hostRef} />;
}
