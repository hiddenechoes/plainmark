// SPDX-License-Identifier: GPL-3.0-or-later
//! Daily notes (SPEC §8.3): resolve `<dailyFolder>/<YYYY-MM-DD>.md` for a given
//! *local* date and, on first use that day, create it from a template.
//!
//! **Timezone handling.** The local calendar date is computed by the frontend
//! (where the user's timezone is unambiguous — `Date` exposes local getters) and
//! passed in as explicit `(year, month, day)` components. This module never reads
//! a wall clock, so "today" stays correct near midnight (a note created at 23:00
//! local lands on today, not tomorrow-UTC) and the whole flow is deterministic
//! and unit-testable.
//!
//! **Safety.** Creation goes through the same atomic write as every other note
//! ([`fs_ops::save_note`]), the folder and template paths are validated to stay
//! inside the vault ([`fs_ops::safe_vault_rel`]), and an existing same-day note
//! is opened untouched — the template is applied on creation only, never
//! re-applied, never clobbering edits.

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::fs_ops::{
    self, default_daily_folder, load_vault_settings, safe_vault_rel, VaultSettings,
};

/// A local calendar date supplied by the frontend (`month`/`day` are 1-based).
#[derive(Clone, Copy, Debug)]
pub struct LocalDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl LocalDate {
    /// ISO `YYYY-MM-DD` rendering, used for the `{{date}}` token and the
    /// empty-note fallback title.
    fn iso(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// Outcome of opening today's daily note.
pub struct DailyNote {
    /// Absolute path to the daily note (guaranteed to exist on disk on return).
    pub abs_path: PathBuf,
    /// `true` if this call created the file; `false` if it already existed.
    pub created: bool,
}

/// Apply the configured filename format. Only a tiny fixed set of literal date
/// tokens is supported (no expression engine, per the governance rule): `YYYY`,
/// `MM`, `DD`. Any other text in the format string is kept verbatim.
fn format_filename(format: &str, date: LocalDate) -> String {
    format
        .replace("YYYY", &format!("{:04}", date.year))
        .replace("MM", &format!("{:02}", date.month))
        .replace("DD", &format!("{:02}", date.day))
}

/// Substitute the single supported literal token in a template body: `{{date}}`
/// becomes the ISO date. This is a fixed substitution, not a templating engine.
fn expand_template(body: &str, date: LocalDate) -> String {
    body.replace("{{date}}", &date.iso())
}

/// Resolve the vault-relative path of the daily note for `date`, honoring the
/// configured folder + filename format and keeping the result vault-scoped. A
/// folder that would escape the vault falls back to the default; a filename
/// format that composes into an escaping path is rejected outright.
fn daily_rel_path(settings: &VaultSettings, date: LocalDate) -> AppResult<String> {
    let folder = safe_vault_rel(&settings.daily_notes.folder).unwrap_or_else(default_daily_folder);
    let filename = format!(
        "{}.md",
        format_filename(&settings.daily_notes.filename_format, date)
    );
    let rel = format!("{folder}/{filename}");
    // Re-validate the composed path: a hostile filename format must not be able
    // to introduce a `..` or an absolute component via the filename.
    safe_vault_rel(&rel)
        .ok_or_else(|| AppError::InvalidPath(format!("invalid daily note path: {rel}")))
}

/// The body for a freshly created daily note: the configured template with its
/// `{{date}}` token expanded, or — when the template is missing, unreadable, not
/// valid UTF-8, or would escape the vault — a sensible empty note titled with the
/// date.
fn template_body(vault_root: &Path, settings: &VaultSettings, date: LocalDate) -> String {
    let template = safe_vault_rel(&settings.daily_notes.template_path)
        .map(|rel| vault_root.join(rel))
        .and_then(|p| std::fs::read_to_string(p).ok());
    match template {
        Some(body) => expand_template(&body, date),
        None => format!("# {}\n", date.iso()),
    }
}

/// Open today's daily note, creating it from the template on first use that day.
/// Idempotent: an existing same-day note is opened untouched.
pub fn open_or_create_daily(vault_root: &Path, date: LocalDate) -> AppResult<DailyNote> {
    let settings = load_vault_settings(vault_root);
    let rel = daily_rel_path(&settings, date)?;

    let unchecked = vault_root.join(&rel);
    // Create the (possibly nested) daily folder up front so the vault-scoping
    // check below can canonicalize the parent, matching the create-note flow.
    if let Some(parent) = unchecked.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let abs = fs_ops::ensure_within(vault_root, &unchecked)?;

    // Idempotent: never re-apply the template, never clobber edits (SPEC §8.3).
    if abs.exists() {
        return Ok(DailyNote {
            abs_path: abs,
            created: false,
        });
    }

    let body = template_body(vault_root, &settings, date);
    // Atomic write through the shared helper; new daily notes are LF, no BOM.
    fs_ops::save_note(&abs, &body, "lf", false)?;
    Ok(DailyNote {
        abs_path: abs,
        created: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn date(year: i32, month: u32, day: u32) -> LocalDate {
        LocalDate { year, month, day }
    }

    fn write_settings(vault: &Path, json: &str) {
        fs::create_dir_all(vault.join(".plainmark")).unwrap();
        fs::write(vault.join(".plainmark/settings.json"), json).unwrap();
    }

    // date -> path resolution with the defaults (folder `Daily/`, ISO filename).
    #[test]
    fn resolves_default_folder_and_iso_filename() {
        let dir = tempdir().unwrap();
        let note = open_or_create_daily(dir.path(), date(2026, 6, 24)).unwrap();
        assert!(note.created);
        assert!(
            note.abs_path.ends_with("Daily/2026-06-24.md"),
            "{:?}",
            note.abs_path
        );
        assert!(note.abs_path.exists());
    }

    // create-from-template writes the template's content (with `{{date}}`
    // expanded) atomically.
    #[test]
    fn creates_from_template_with_date_token() {
        let dir = tempdir().unwrap();
        let vault = dir.path();
        fs::create_dir_all(vault.join("Templates")).unwrap();
        fs::write(
            vault.join("Templates/Daily.md"),
            "## Log for {{date}}\n\n- \n",
        )
        .unwrap();

        let note = open_or_create_daily(vault, date(2026, 1, 2)).unwrap();
        assert!(note.created);
        assert_eq!(
            fs::read_to_string(&note.abs_path).unwrap(),
            "## Log for 2026-01-02\n\n- \n"
        );
    }

    // Idempotency: a second invocation the same day opens the existing file and
    // neither re-applies the template nor clobbers edits (SPEC §8.3 acceptance).
    #[test]
    fn second_invocation_opens_existing_without_clobbering() {
        let dir = tempdir().unwrap();
        let vault = dir.path();
        fs::create_dir_all(vault.join("Templates")).unwrap();
        fs::write(vault.join("Templates/Daily.md"), "TEMPLATE BODY\n").unwrap();

        let first = open_or_create_daily(vault, date(2026, 6, 24)).unwrap();
        assert!(first.created);
        // The user edits the note after creation.
        fs::write(&first.abs_path, "my own edits\n").unwrap();

        let second = open_or_create_daily(vault, date(2026, 6, 24)).unwrap();
        assert!(!second.created);
        assert_eq!(second.abs_path, first.abs_path);
        // Edits preserved; template not re-applied.
        assert_eq!(
            fs::read_to_string(&second.abs_path).unwrap(),
            "my own edits\n"
        );
    }

    // A missing template yields a sensible empty note (titled with the date).
    #[test]
    fn missing_template_yields_sensible_empty_note() {
        let dir = tempdir().unwrap();
        let note = open_or_create_daily(dir.path(), date(2026, 12, 31)).unwrap();
        assert_eq!(
            fs::read_to_string(&note.abs_path).unwrap(),
            "# 2026-12-31\n"
        );
    }

    // Configurable folder + filename format are honored.
    #[test]
    fn honors_configured_folder_and_format() {
        let dir = tempdir().unwrap();
        let vault = dir.path();
        write_settings(
            vault,
            r#"{"dailyNotes":{"folder":"Journal/Days","filenameFormat":"YYYY_MM_DD"}}"#,
        );

        let note = open_or_create_daily(vault, date(2026, 6, 24)).unwrap();
        assert!(
            note.abs_path.ends_with("Journal/Days/2026_06_24.md"),
            "{:?}",
            note.abs_path
        );
        assert!(note.abs_path.exists());
    }

    // A folder that escapes the vault falls back to the default `Daily/` and
    // writes nothing outside the vault.
    #[test]
    fn escaping_folder_falls_back_to_default() {
        let dir = tempdir().unwrap();
        let vault = dir.path().join("vault");
        write_settings(&vault, r#"{"dailyNotes":{"folder":"../escape"}}"#);

        let note = open_or_create_daily(&vault, date(2026, 6, 24)).unwrap();
        assert!(
            note.abs_path.ends_with("Daily/2026-06-24.md"),
            "{:?}",
            note.abs_path
        );
        assert!(!dir.path().join("escape").exists());
    }

    // A template path that escapes the vault is refused: it must not read a file
    // outside the vault, falling back to the empty note instead.
    #[test]
    fn escaping_template_uses_empty_fallback() {
        let dir = tempdir().unwrap();
        let vault = dir.path().join("vault");
        fs::create_dir_all(&vault).unwrap();
        // A secret outside the vault that a hostile templatePath tries to read.
        fs::write(dir.path().join("secret.md"), "SECRET CONTENTS\n").unwrap();
        write_settings(&vault, r#"{"dailyNotes":{"templatePath":"../secret.md"}}"#);

        let note = open_or_create_daily(&vault, date(2026, 6, 24)).unwrap();
        let body = fs::read_to_string(&note.abs_path).unwrap();
        assert_eq!(body, "# 2026-06-24\n");
        assert!(!body.contains("SECRET"));
    }

    // Local-date handling: the backend uses the injected local date verbatim,
    // with no UTC reinterpretation. The frontend resolves 23:00-local to these
    // components (see dailyNote.test.ts for the near-midnight conversion); here
    // we prove the backend honors them exactly — no off-by-one.
    #[test]
    fn uses_injected_local_date_verbatim() {
        let dir = tempdir().unwrap();
        // 2026-06-24 23:00 local -> (2026, 6, 24) from the frontend.
        let note = open_or_create_daily(dir.path(), date(2026, 6, 24)).unwrap();
        assert!(
            note.abs_path.ends_with("Daily/2026-06-24.md"),
            "{:?}",
            note.abs_path
        );
    }

    #[test]
    fn format_filename_pads_and_substitutes_tokens() {
        let d = date(2026, 3, 5);
        assert_eq!(format_filename("YYYY-MM-DD", d), "2026-03-05");
        assert_eq!(format_filename("DD.MM.YYYY", d), "05.03.2026");
        // Surrounding literal text is preserved (no expression engine).
        assert_eq!(format_filename("daily-YYYYMMDD", d), "daily-20260305");
    }

    #[test]
    fn expand_template_substitutes_only_the_date_token() {
        let d = date(2026, 6, 24);
        assert_eq!(
            expand_template("# {{date}}\n\n{{notatoken}}\n", d),
            "# 2026-06-24\n\n{{notatoken}}\n"
        );
    }
}
