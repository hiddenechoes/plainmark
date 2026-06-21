import { useCallback, useEffect, useState } from "react";
import { Editor } from "./components/Editor";
import { FileTree } from "./components/FileTree";
import { Preview } from "./components/Preview";
import { basename, relativeTo } from "./lib/path";
import {
  loadLastVault,
  pickVault,
  readNote,
  saveAttachment,
  saveNote,
  type NoteFile,
  type VaultInfo,
} from "./lib/tauri";
import "./styles.css";

interface OpenNote {
  path: string;
  note: NoteFile;
}

/** Debounce a value so the preview re-renders at most every `delay` ms while
 * typing, keeping render cost bounded on large notes. */
function useDebounced<T>(value: T, delay: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const id = setTimeout(() => setDebounced(value), delay);
    return () => clearTimeout(id);
  }, [value, delay]);
  return debounced;
}

export function App() {
  const [vault, setVault] = useState<VaultInfo | null>(null);
  const [open, setOpen] = useState<OpenNote | null>(null);
  const [dirty, setDirty] = useState(false);
  const [status, setStatus] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [showPreview, setShowPreview] = useState(true);

  const previewContent = useDebounced(open?.note.content ?? "", 200);

  // Reopen the last vault on launch (SPEC §7 recent-vault behavior).
  useEffect(() => {
    loadLastVault()
      .then((info) => {
        if (info) setVault(info);
      })
      .catch((e: unknown) => setError(String(e)));
  }, []);

  const handleOpenVault = useCallback(async () => {
    setError(null);
    try {
      const info = await pickVault();
      if (info) {
        setVault(info);
        setOpen(null);
        setDirty(false);
        setStatus("");
      }
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const handleSelect = useCallback(async (path: string) => {
    setError(null);
    try {
      const note = await readNote(path);
      setOpen({ path, note });
      setDirty(false);
      setStatus("");
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const handleChange = useCallback((content: string) => {
    setOpen((prev) => (prev ? { ...prev, note: { ...prev.note, content } } : prev));
    setDirty(true);
  }, []);

  const handleSave = useCallback(async () => {
    if (!open) return;
    setError(null);
    try {
      await saveNote(open.path, open.note);
      setDirty(false);
      setStatus(`Saved ${basename(open.path)}`);
    } catch (e) {
      setError(String(e));
    }
  }, [open]);

  // Persist a pasted/dropped image and report its vault-relative path; the
  // editor inserts the embed and the note becomes dirty (saved on Cmd/Ctrl+S).
  const handleSaveImage = useCallback(async (dataBase64: string, ext: string) => {
    const { relativePath } = await saveAttachment(dataBase64, ext);
    return relativePath;
  }, []);

  // Cmd/Ctrl+S saves, regardless of which pane has focus.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        void handleSave();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [handleSave]);

  return (
    <div className="app">
      <header className="toolbar">
        <span className="brand">plainmark</span>
        <button type="button" onClick={() => void handleOpenVault()}>
          Open vault…
        </button>
        {vault && <span className="vault-path">{vault.root}</span>}
      </header>

      {error && (
        <div className="banner error" role="alert">
          {error}
        </div>
      )}

      {!vault ? (
        <main className="empty-state">
          <p>Open a vault folder to start editing your Markdown notes.</p>
          <button type="button" onClick={() => void handleOpenVault()}>
            Open vault…
          </button>
        </main>
      ) : (
        <div className="workspace">
          <aside className="sidebar">
            <FileTree
              nodes={vault.tree}
              selectedPath={open?.path ?? null}
              onSelect={(path) => void handleSelect(path)}
            />
          </aside>
          <main className="editor-pane">
            {open ? (
              <>
                <div className="editor-status">
                  <span>{relativeTo(vault.root, open.path)}</span>
                  <span className="spacer" />
                  {dirty && <span className="dirty">● unsaved</span>}
                  {!dirty && status && <span className="saved">{status}</span>}
                  <button
                    type="button"
                    className="preview-toggle"
                    aria-pressed={showPreview}
                    onClick={() => setShowPreview((s) => !s)}
                  >
                    {showPreview ? "Hide preview" : "Show preview"}
                  </button>
                </div>
                <div className={showPreview ? "split split-both" : "split"}>
                  <Editor
                    key={open.path}
                    doc={open.note.content}
                    onChange={handleChange}
                    onSaveImage={handleSaveImage}
                    onError={setError}
                  />
                  {showPreview && (
                    <Preview content={previewContent} vaultRoot={vault.root} notePath={open.path} />
                  )}
                </div>
              </>
            ) : (
              <p className="editor-placeholder">Select a note from the file tree.</p>
            )}
          </main>
        </div>
      )}
    </div>
  );
}
