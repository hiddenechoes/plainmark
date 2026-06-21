// SPDX-License-Identifier: GPL-3.0-or-later
//! App-level config (SPEC §7): per-user OS config dir. Phase 0 stores only the
//! last-opened vault so the app can reopen it on launch. Vault-local config in
//! `.plainmark/` is a separate concern handled later.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::fs_ops::atomic_write;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    /// Absolute path of the most recently opened vault, if any.
    #[serde(default)]
    pub last_vault: Option<String>,
}

fn config_file(config_dir: &Path) -> PathBuf {
    config_dir.join("config.json")
}

/// Load config, tolerating a missing or corrupt file by returning defaults —
/// app-level config is a convenience, never a source of truth.
pub fn load(config_dir: &Path) -> AppConfig {
    match std::fs::read(config_file(config_dir)) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => AppConfig::default(),
    }
}

/// Persist config via the same atomic-write helper used for notes.
pub fn save(config_dir: &Path, config: &AppConfig) -> AppResult<()> {
    let bytes = serde_json::to_vec_pretty(config).map_err(|e| AppError::Io(e.to_string()))?;
    atomic_write(&config_file(config_dir), &bytes)?;
    Ok(())
}
