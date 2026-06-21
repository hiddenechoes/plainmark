// SPDX-License-Identifier: GPL-3.0-or-later
// Hide the extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! plainmark backend entry point. Owns every filesystem operation and exposes a
//! small, typed Tauri command surface to the webview (SPEC §6, §7.1). Each
//! command that touches a path validates it is inside the active vault first.

mod cache;
mod config;
mod error;
mod fs_ops;
mod index;
mod watcher;

use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{Emitter, Manager, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;

use error::{AppError, AppResult};
use fs_ops::{NoteFile, SavedAttachment, TreeNode};
use index::{Heading, Index};
use watcher::VaultWatcher;

/// The currently open vault root. `None` until the user opens a vault.
/// `watcher` keeps the active file watcher alive (dropping it stops watching);
/// re-opening a vault replaces it. `index` is the live link graph, shared with
/// the watcher thread (it applies incremental updates) via an `Arc`.
#[derive(Default)]
struct VaultState {
    root: Mutex<Option<PathBuf>>,
    watcher: Mutex<Option<VaultWatcher>>,
    index: Arc<RwLock<Index>>,
}

/// Returned to the frontend after opening a vault.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VaultInfo {
    root: String,
    tree: Vec<TreeNode>,
}

/// Set `root` as the active vault: build its tree, persist it as the last vault,
/// and record it for path-scoping subsequent commands.
fn open_vault_at(
    app: &tauri::AppHandle,
    state: &VaultState,
    root: PathBuf,
) -> AppResult<VaultInfo> {
    let tree = fs_ops::build_tree(&root)?;

    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|e| AppError::Io(e.to_string()))?;
    let mut cfg = config::load(&config_dir);
    cfg.last_vault = Some(root.to_string_lossy().to_string());
    config::save(&config_dir, &cfg)?;

    *state
        .root
        .lock()
        .map_err(|_| AppError::Io("vault state lock poisoned".into()))? = Some(root.clone());

    // Build the link index (cache-accelerated) before wiring the watcher so the
    // first events apply on top of a complete graph.
    let built = index::build_index(&root);
    eprintln!(
        "plainmark: indexed {} note(s){}",
        built.len(),
        if built.is_empty() {
            " (empty vault)"
        } else {
            ""
        }
    );
    *state
        .index
        .write()
        .map_err(|_| AppError::Io("index lock poisoned".into()))? = built;

    start_watcher(app, state, &root)?;

    Ok(VaultInfo {
        root: root.to_string_lossy().to_string(),
        tree,
    })
}

/// Start watching `root`. On each change the watcher thread updates the shared
/// index incrementally, then notifies the webview: `note://changed` per change
/// (drives external-change handling, §4.1) and a single `index://updated` so
/// panels refresh. The watcher is stored in `VaultState` so it lives as long as
/// the vault is open. A failure to start is non-fatal: the vault still opens,
/// just without live updates.
///
/// The watcher updates the in-memory index only; the SQLite cache is reconciled
/// on the next vault open (`build_index` reparses any file whose mtime/size no
/// longer matches its cached row), so an edited file's cache row self-heals
/// rather than being written through from this thread.
fn start_watcher(app: &tauri::AppHandle, state: &VaultState, root: &Path) -> AppResult<()> {
    let config = watcher::load_watch_config(root);
    let app_handle = app.clone();
    let index = Arc::clone(&state.index);
    let root_buf = root.to_path_buf();

    let started = VaultWatcher::start(root, config, move |batch| {
        if let Ok(mut idx) = index.write() {
            for ev in &batch {
                index::apply_event(&mut idx, &root_buf, ev);
            }
        }
        for ev in &batch {
            // If the webview has gone away the emit just fails; nothing to do.
            let _ = app_handle.emit("note://changed", ev);
        }
        let _ = app_handle.emit("index://updated", ());
    });

    match started {
        Ok(w) => {
            *state
                .watcher
                .lock()
                .map_err(|_| AppError::Io("watcher state lock poisoned".into()))? = Some(w);
        }
        Err(e) => {
            eprintln!("plainmark: file watcher failed to start: {e}");
        }
    }
    Ok(())
}

/// The active vault root, or `NoVault` if none is open.
fn vault_root(state: &VaultState) -> AppResult<PathBuf> {
    state
        .root
        .lock()
        .map_err(|_| AppError::Io("vault state lock poisoned".into()))?
        .clone()
        .ok_or(AppError::NoVault)
}

/// A note exposed to the webview for the link-target snapshot and autocomplete.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NoteMeta {
    /// Absolute path (matches the file-tree paths the webview already uses).
    path: String,
    /// Vault-relative, forward-slash path (used by the frontend link resolver).
    rel_path: String,
    title: String,
    headings: Vec<Heading>,
}

/// The result of resolving a `[[link]]` for the webview.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResolvedLink {
    /// Absolute path of the target note, if it resolves.
    path: Option<String>,
    exists: bool,
    /// Whether the optional `#heading` part exists in the target.
    heading_ok: bool,
}

/// One inbound link, for the backlinks panel.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BacklinkOut {
    /// Absolute path of the linking note.
    from: String,
    from_title: String,
    line: usize,
    snippet: String,
}

/// Split a raw link body into its note target and optional `#heading`, dropping
/// any `|alias` (aliases are out of scope this phase).
fn split_target(raw: &str) -> (String, Option<String>) {
    let before_alias = raw.split('|').next().unwrap_or("");
    let mut parts = before_alias.splitn(2, '#');
    let target = parts.next().unwrap_or("").trim().to_string();
    let heading = parts
        .next()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty());
    (target, heading)
}

/// Turn a link target into a safe vault-relative `.md` path, rejecting absolute
/// paths and any `..`/root component so a created note can't escape the vault.
fn safe_note_rel(target: &str) -> AppResult<String> {
    let cleaned = target.trim().replace('\\', "/");
    let cleaned = cleaned.trim_matches('/');
    let with_md = if cleaned.to_lowercase().ends_with(".md") {
        cleaned.to_string()
    } else {
        format!("{cleaned}.md")
    };
    let all_normal = !cleaned.is_empty()
        && Path::new(&with_md)
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
    if all_normal {
        Ok(with_md)
    } else {
        Err(AppError::InvalidPath(format!(
            "invalid note name: {target}"
        )))
    }
}

/// Resolve a `[[link]]` (note part + optional `#heading`) from the note at
/// `from` (absolute path). Returns the target's absolute path if it exists.
#[tauri::command]
fn resolve_link(
    target: String,
    from: String,
    state: State<'_, VaultState>,
) -> AppResult<ResolvedLink> {
    let root = vault_root(&state)?;
    let from_rel = index::to_rel(&root, Path::new(&from)).unwrap_or_default();
    let (note, heading) = split_target(&target);
    let idx = state
        .index
        .read()
        .map_err(|_| AppError::Io("index lock poisoned".into()))?;
    let status = idx.link_status(&note, heading.as_deref(), &from_rel);
    let path = status
        .path
        .as_ref()
        .map(|rel| root.join(rel).to_string_lossy().to_string());
    Ok(ResolvedLink {
        exists: status.path.is_some(),
        path,
        heading_ok: status.heading_ok,
    })
}

/// Snapshot of every note (title + headings + paths) for the frontend link
/// resolver and `[[` autocomplete.
#[tauri::command]
fn list_link_targets(state: State<'_, VaultState>) -> AppResult<Vec<NoteMeta>> {
    let root = vault_root(&state)?;
    let idx = state
        .index
        .read()
        .map_err(|_| AppError::Io("index lock poisoned".into()))?;
    let mut out: Vec<NoteMeta> = idx
        .entries()
        .map(|e| NoteMeta {
            path: root.join(&e.rel_path).to_string_lossy().to_string(),
            rel_path: e.rel_path.clone(),
            title: e.title.clone(),
            headings: e.headings.clone(),
        })
        .collect();
    out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok(out)
}

/// Inbound links to the note at `path` (absolute), with context snippets.
#[tauri::command]
fn backlinks(path: String, state: State<'_, VaultState>) -> AppResult<Vec<BacklinkOut>> {
    let root = vault_root(&state)?;
    let rel = index::to_rel(&root, Path::new(&path)).ok_or(AppError::OutsideVault)?;
    let idx = state
        .index
        .read()
        .map_err(|_| AppError::Io("index lock poisoned".into()))?;
    Ok(idx
        .backlinks(&rel)
        .into_iter()
        .map(|b| {
            let from_title = idx
                .get(&b.from)
                .map(|e| e.title.clone())
                .unwrap_or_else(|| b.from.clone());
            BacklinkOut {
                from: root.join(&b.from).to_string_lossy().to_string(),
                from_title,
                line: b.line,
                snippet: b.snippet,
            }
        })
        .collect())
}

/// Create a note for an unresolved link (click-to-create). Idempotent: returns
/// the existing note's path if it already exists. Returns the absolute path.
#[tauri::command]
fn create_note(
    app: tauri::AppHandle,
    target: String,
    state: State<'_, VaultState>,
) -> AppResult<String> {
    let root = vault_root(&state)?;
    let rel = safe_note_rel(&split_target(&target).0)?;
    let unchecked = root.join(&rel);
    if let Some(parent) = unchecked.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let abs = fs_ops::ensure_within(&root, &unchecked)?;

    if !abs.exists() {
        let title = Path::new(&rel)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled");
        fs_ops::save_note(&abs, &format!("# {title}\n"), "lf", false)?;
    }

    // Update the index immediately so the link resolves without waiting for the
    // watcher; the watcher's later event is harmless (insert is idempotent).
    let (mtime, size) = index::file_stat(&abs);
    if let Ok(note) = fs_ops::read_note(&abs) {
        let mut idx = state
            .index
            .write()
            .map_err(|_| AppError::Io("index lock poisoned".into()))?;
        idx.insert(index::build_entry(rel, mtime, size, &note.content));
    }
    let _ = app.emit("index://updated", ());

    Ok(abs.to_string_lossy().to_string())
}

fn resolve_in_vault(state: &VaultState, path: &str) -> AppResult<PathBuf> {
    let guard = state
        .root
        .lock()
        .map_err(|_| AppError::Io("vault state lock poisoned".into()))?;
    let root = guard.as_ref().ok_or(AppError::NoVault)?;
    fs_ops::ensure_within(root, Path::new(path))
}

/// Open the native folder picker; on selection, open it as the vault.
/// Returns `None` if the user cancels.
#[tauri::command]
async fn pick_vault(
    app: tauri::AppHandle,
    state: State<'_, VaultState>,
) -> AppResult<Option<VaultInfo>> {
    // `blocking_pick_folder` must run off the main thread; an `async` command
    // already runs on the async runtime, so this is safe.
    let Some(folder) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None);
    };
    let root = folder
        .into_path()
        .map_err(|e| AppError::InvalidPath(e.to_string()))?;
    let info = open_vault_at(&app, &state, root)?;
    Ok(Some(info))
}

/// Reopen the last vault on launch, if it still exists. Returns `None` if no
/// vault was remembered or it has since moved.
#[tauri::command]
fn load_last_vault(
    app: tauri::AppHandle,
    state: State<'_, VaultState>,
) -> AppResult<Option<VaultInfo>> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|e| AppError::Io(e.to_string()))?;
    let Some(last) = config::load(&config_dir).last_vault else {
        return Ok(None);
    };
    let root = PathBuf::from(last);
    if !root.is_dir() {
        return Ok(None);
    }
    let info = open_vault_at(&app, &state, root)?;
    Ok(Some(info))
}

/// Rebuild the file tree for the active vault (e.g. after external changes).
#[tauri::command]
fn refresh_tree(state: State<'_, VaultState>) -> AppResult<Vec<TreeNode>> {
    let guard = state
        .root
        .lock()
        .map_err(|_| AppError::Io("vault state lock poisoned".into()))?;
    let root = guard.as_ref().ok_or(AppError::NoVault)?;
    fs_ops::build_tree(root)
}

/// Read a note (path must be inside the active vault).
#[tauri::command]
fn read_note(path: String, state: State<'_, VaultState>) -> AppResult<NoteFile> {
    let target = resolve_in_vault(&state, &path)?;
    fs_ops::read_note(&target)
}

/// Save a note atomically, preserving its original line endings and BOM.
#[tauri::command]
fn save_note(
    path: String,
    content: String,
    eol: String,
    bom: bool,
    state: State<'_, VaultState>,
) -> AppResult<()> {
    let target = resolve_in_vault(&state, &path)?;
    fs_ops::save_note(&target, &content, &eol, bom)
}

/// Write image `bytes` into the active vault's attachments folder. Shared by the
/// clipboard-paste and file-drop paths.
fn save_image_bytes(state: &VaultState, bytes: &[u8], ext: &str) -> AppResult<SavedAttachment> {
    let guard = state
        .root
        .lock()
        .map_err(|_| AppError::Io("vault state lock poisoned".into()))?;
    let root = guard.as_ref().ok_or(AppError::NoVault)?;

    let now_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    fs_ops::save_attachment(root, bytes, ext, now_millis)
}

/// Encode raw RGBA pixels as a PNG (the clipboard hands us pixels, not a file).
fn encode_png(rgba: &[u8], width: u32, height: u32) -> AppResult<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| AppError::Io(e.to_string()))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| AppError::Io(e.to_string()))?;
    }
    Ok(out)
}

/// Save an image from the system clipboard into the vault (the reliable
/// cross-platform paste path; the webview's clipboard often lacks image bytes,
/// especially on Linux/WebKitGTK). Returns `None` when the clipboard holds no
/// image, so a plain-text paste falls through to the editor unchanged.
#[tauri::command]
fn save_clipboard_image(
    app: tauri::AppHandle,
    state: State<'_, VaultState>,
) -> AppResult<Option<SavedAttachment>> {
    let Ok(image) = app.clipboard().read_image() else {
        return Ok(None);
    };
    let png = encode_png(image.rgba(), image.width(), image.height())?;
    Ok(Some(save_image_bytes(&state, &png, "png")?))
}

/// Copy a dropped image file into the vault's attachments folder (§8.9). The
/// source path comes from Tauri's native drag-drop event; we read it once and
/// write a vault-scoped copy atomically.
#[tauri::command]
fn import_attachment(
    source_path: String,
    state: State<'_, VaultState>,
) -> AppResult<SavedAttachment> {
    let bytes = std::fs::read(&source_path)?;
    let ext = Path::new(&source_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png");
    save_image_bytes(&state, &bytes, ext)
}

/// Read an image inside the active vault and return it as a `data:` URL for the
/// preview pane (path is validated to be inside the vault first).
#[tauri::command]
fn read_image(path: String, state: State<'_, VaultState>) -> AppResult<String> {
    let target = resolve_in_vault(&state, &path)?;
    fs_ops::read_image_data_url(&target)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(VaultState::default())
        .invoke_handler(tauri::generate_handler![
            pick_vault,
            load_last_vault,
            refresh_tree,
            read_note,
            save_note,
            save_clipboard_image,
            import_attachment,
            read_image,
            resolve_link,
            list_link_targets,
            backlinks,
            create_note
        ])
        .run(tauri::generate_context!())
        .expect("error while running the plainmark application");
}
