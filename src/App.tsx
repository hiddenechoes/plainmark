import { useCallback, useEffect, useState } from "react";
import { Editor } from "./components/Editor";
import { FileTree } from "./components/FileTree";
import { basename, relativeTo } from "./lib/path";
import {
  loadLastVault,
  pickVault,
  readNote,
  saveNote,
  type NoteFile,
  type VaultInfo,
} from "./lib/tauri";
import "./styles.css";

interface OpenNote {
  path: string;
  note: NoteFile;
}

export function App() {
  const [vault, setVault] = useState<VaultInfo | null>(null);
  const [open, setOpen] = useState<OpenNote | null>(null);
  const [dirty, setDirty] = useState(false);
  const [status, setStatus] = useState("");
  const [error, setError] = useState<string | null>(null);

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
                </div>
                <Editor key={open.path} doc={open.note.content} onChange={handleChange} />
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
