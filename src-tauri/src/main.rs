// SPDX-License-Identifier: GPL-3.0-or-later
// Hide the extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! plainmark backend entry point. Owns every filesystem operation and exposes a
//! small, typed Tauri command surface to the webview (SPEC §6, §7.1). Each
//! command that touches a path validates it is inside the active vault first.

mod config;
mod error;
mod fs_ops;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{Manager, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;

use error::{AppError, AppResult};
use fs_ops::{NoteFile, SavedAttachment, TreeNode};

/// The currently open vault root. `None` until the user opens a vault.
#[derive(Default)]
struct VaultState {
    root: Mutex<Option<PathBuf>>,
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

    Ok(VaultInfo {
        root: root.to_string_lossy().to_string(),
        tree,
    })
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
            read_image
        ])
        .run(tauri::generate_context!())
        .expect("error while running the plainmark application");
}
