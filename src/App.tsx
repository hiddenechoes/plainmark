import { useCallback, useEffect, useState } from "react";
import { BacklinksPanel } from "./components/BacklinksPanel";
import { Editor } from "./components/Editor";
import { FileTree } from "./components/FileTree";
import { Preview } from "./components/Preview";
import { basename, dirname, joinPath, relativeTo } from "./lib/path";
import {
  createNote,
  importAttachment,
  listLinkTargets,
  loadLastVault,
  onIndexUpdated,
  pickVault,
  readNote,
  renameNote,
  saveClipboardImage,
  saveNote,
  type NoteFile,
  type NoteMeta,
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
  const [targets, setTargets] = useState<NoteMeta[]>([]);
  const [renaming, setRenaming] = useState(false);
  const [renameValue, setRenameValue] = useState("");

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

  // Keep the link-target snapshot fresh: load it when a vault opens and refresh
  // whenever the index changes (create/edit/delete/rename, in-app or external).
  useEffect(() => {
    if (!vault) return;
    let active = true;
    const load = () => {
      listLinkTargets()
        .then((t) => {
          if (active) setTargets(t);
        })
        .catch(() => {});
    };
    load();
    const unlisten = onIndexUpdated(load);
    return () => {
      active = false;
      void unlisten.then((off) => off());
    };
  }, [vault]);

  const handleNavigate = useCallback(
    (path: string) => {
      void handleSelect(path);
    },
    [handleSelect],
  );

  // Rename the open note (keeping it in the same folder). The backend rewrites
  // inbound links across the vault atomically (§8.2); we then open the new path.
  const startRename = useCallback(() => {
    if (!open) return;
    setRenameValue(basename(open.path).replace(/\.md$/i, ""));
    setRenaming(true);
  }, [open]);

  const submitRename = useCallback(() => {
    if (!open) return;
    const name = renameValue.trim();
    if (name === "" || `${name}.md` === basename(open.path)) {
      setRenaming(false);
      return;
    }
    void (async () => {
      setError(null);
      try {
        const newPath = joinPath(dirname(open.path), `${name}.md`);
        const finalPath = await renameNote(open.path, newPath);
        setRenaming(false);
        setTargets(await listLinkTargets());
        await handleSelect(finalPath);
      } catch (e) {
        setError(String(e));
      }
    })();
  }, [open, renameValue, handleSelect]);

  // Click-to-create on an unresolved link: create the note, refresh the snapshot,
  // then open it.
  const handleCreate = useCallback(
    (target: string) => {
      void (async () => {
        setError(null);
        try {
          const path = await createNote(target);
          setTargets(await listLinkTargets());
          await handleSelect(path);
        } catch (e) {
          setError(String(e));
        }
      })();
    },
    [handleSelect],
  );

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

  // Paste: read an image from the system clipboard; null means "no image, let
  // the text paste proceed". The editor inserts the embed at the cursor.
  const handlePasteImage = useCallback(async () => {
    const saved = await saveClipboardImage();
    return saved ? saved.relativePath : null;
  }, []);

  // Drop: copy the dropped image file into the vault and report its path.
  const handleImportImage = useCallback(async (sourcePath: string) => {
    const { relativePath } = await importAttachment(sourcePath);
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
            <BacklinksPanel notePath={open?.path ?? null} onNavigate={handleNavigate} />
          </aside>
          <main className="editor-pane">
            {open ? (
              <>
                <div className="editor-status">
                  {renaming ? (
                    <input
                      className="rename-input"
                      autoFocus
                      value={renameValue}
                      onChange={(e) => setRenameValue(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") submitRename();
                        else if (e.key === "Escape") setRenaming(false);
                      }}
                      onBlur={() => setRenaming(false)}
                    />
                  ) : (
                    <button
                      type="button"
                      className="note-name"
                      title="Rename note"
                      onClick={startRename}
                    >
                      {relativeTo(vault.root, open.path)}
                    </button>
                  )}
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
                    onPasteImage={handlePasteImage}
                    onImportImage={handleImportImage}
                    linkTargets={targets}
                    onError={setError}
                  />
                  {showPreview && (
                    <Preview
                      content={previewContent}
                      vaultRoot={vault.root}
                      notePath={open.path}
                      targets={targets}
                      onNavigate={handleNavigate}
                      onCreate={handleCreate}
                    />
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
