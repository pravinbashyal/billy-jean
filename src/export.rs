//! PDF export helpers (task 9.2).
//!
//! Pure, headless-testable pieces used by the Export PDF control:
//! - [`default_filename`] / [`default_filename_for`] — save-dialog suggested name
//!   from client name + invoice number (e.g. `Ostrander Quennell invoice 4.pdf`);
//! - last-used folder load/save under the per-user config dir;
//! - [`write_invoice_pdf`] — writes a PDF via [`crate::pdf::render_to_path`], the
//!   same projection the live preview uses.
//!
//! The native save dialog itself lives in the editor; these helpers keep the
//! filename, folder preference, and write path unit-testable without UI.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::UserDirs;
use serde::{Deserialize, Serialize};

use crate::model::Invoice;
use crate::pdf::{self, PdfError, Projection};

const EXPORT_PREFS_FILE: &str = "export_prefs.json";

/// On-disk export preferences (last-used save folder). Kept separate from the
/// identity store so invoice-identity schema changes do not touch this file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct ExportPrefs {
    #[serde(default)]
    last_export_dir: Option<PathBuf>,
}

/// Errors from reading or writing export preferences.
#[derive(Debug)]
pub enum ExportPrefsError {
    Io(io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for ExportPrefsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportPrefsError::Io(e) => write!(f, "export prefs I/O error: {e}"),
            ExportPrefsError::Json(e) => write!(f, "export prefs are not valid: {e}"),
        }
    }
}

impl std::error::Error for ExportPrefsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ExportPrefsError::Io(e) => Some(e),
            ExportPrefsError::Json(e) => Some(e),
        }
    }
}

impl From<io::Error> for ExportPrefsError {
    fn from(e: io::Error) -> Self {
        ExportPrefsError::Io(e)
    }
}

impl From<serde_json::Error> for ExportPrefsError {
    fn from(e: serde_json::Error) -> Self {
        ExportPrefsError::Json(e)
    }
}

/// Default save-dialog filename: `{client name} invoice {number}.pdf`.
///
/// Path-unsafe characters in either part are replaced so the name is usable on
/// macOS. Empty parts are omitted rather than leaving a stray "invoice".
pub fn default_filename(client_name: &str, invoice_number: &str) -> String {
    let name = sanitize_filename_part(client_name.trim());
    let number = sanitize_filename_part(invoice_number.trim());
    match (name.is_empty(), number.is_empty()) {
        (false, false) => format!("{name} invoice {number}.pdf"),
        (true, false) => format!("invoice {number}.pdf"),
        (false, true) => format!("{name} invoice.pdf"),
        (true, true) => "invoice.pdf".to_string(),
    }
}

/// [`default_filename`] from an [`Invoice`]'s client name and invoice number.
pub fn default_filename_for(invoice: &Invoice) -> String {
    default_filename(&invoice.client.name, &invoice.meta.number)
}

fn sanitize_filename_part(part: &str) -> String {
    part.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '\0' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect()
}

/// First-run / missing-prefs directory for the save dialog: Documents if
/// available, otherwise the home directory, otherwise the process cwd.
pub fn fallback_directory() -> PathBuf {
    if let Some(dirs) = UserDirs::new() {
        if let Some(docs) = dirs.document_dir() {
            return docs.to_path_buf();
        }
        return dirs.home_dir().to_path_buf();
    }
    PathBuf::from(".")
}

/// Directory to open in the save dialog: the last-used folder when it still
/// exists on disk, otherwise [`fallback_directory`].
pub fn dialog_directory(prefs_dir: Option<&Path>) -> PathBuf {
    if let Some(dir) = prefs_dir {
        if let Ok(Some(last)) = load_last_export_dir(dir) {
            if last.is_dir() {
                return last;
            }
        }
    }
    fallback_directory()
}

/// Loads the last-used export folder from `prefs_dir/export_prefs.json`.
///
/// A missing file is first-run and yields `Ok(None)`.
pub fn load_last_export_dir(prefs_dir: &Path) -> Result<Option<PathBuf>, ExportPrefsError> {
    let path = prefs_dir.join(EXPORT_PREFS_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(ExportPrefsError::Io(e)),
    };
    let prefs: ExportPrefs = serde_json::from_str(&text)?;
    Ok(prefs.last_export_dir)
}

/// Persists `dir` as the last-used export folder under `prefs_dir`.
pub fn save_last_export_dir(prefs_dir: &Path, dir: &Path) -> Result<(), ExportPrefsError> {
    fs::create_dir_all(prefs_dir)?;
    let prefs = ExportPrefs {
        last_export_dir: Some(dir.to_path_buf()),
    };
    let text = serde_json::to_string_pretty(&prefs)?;
    let path = prefs_dir.join(EXPORT_PREFS_FILE);
    let tmp = prefs_dir.join(format!("{EXPORT_PREFS_FILE}.tmp"));
    fs::write(&tmp, text.as_bytes())?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// After a successful write, remember the chosen file's parent folder. No-op
/// when there is no prefs dir or the path has no parent. Failures are logged
/// and never block the export itself.
pub fn remember_export_path(prefs_dir: Option<&Path>, exported_path: &Path) {
    let Some(prefs_dir) = prefs_dir else {
        return;
    };
    let Some(parent) = exported_path.parent() else {
        return;
    };
    if parent.as_os_str().is_empty() {
        return;
    }
    if let Err(e) = save_last_export_dir(prefs_dir, parent) {
        eprintln!("warning: could not save last export folder: {e}");
    }
}

/// Writes the invoice PDF to `path` from the same [`project`](pdf::project)
/// snapshot the live preview uses. Returns that projection so callers (and
/// tests) can assert export and preview agree without parsing PDF bytes.
pub fn write_invoice_pdf(invoice: &Invoice, path: &Path) -> Result<Projection, PdfError> {
    let projection = pdf::project(invoice);
    let bytes = pdf::render_projection(&projection)?;
    std::fs::write(path, bytes).map_err(PdfError::from)?;
    Ok(projection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Bank, Client, InvoiceMeta, LineItem, Money, Quantity};
    use crate::pdf::project;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir().join(format!(
                "billing-export-{label}-{}-{nanos}",
                std::process::id()
            ));
            ScratchDir(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn reference_invoice() -> Invoice {
        Invoice {
            client: Client {
                name: "Ostrander Quennell".to_string(),
                street: "Rayburn Str. 42".to_string(),
                postal_code_city: "80331 Musterstadt".to_string(),
                country: "Germany".to_string(),
            },
            meta: InvoiceMeta {
                number: "4".to_string(),
                invoice_date: "03/10/2026".to_string(),
                service_date: "11/09/2026".to_string(),
                due_date: "15/10/2026".to_string(),
            },
            line_items: vec![LineItem {
                description: "Consulting".to_string(),
                quantity: Quantity::whole(1),
                rate: Money::from_euros(200),
            }],
            bank: Bank {
                bank_name: "Ellery".to_string(),
                beneficiary: "Penhallow Morgan".to_string(),
                iban: "DE89 3704 0044 0532 0130 00".to_string(),
            },
            ..Invoice::default()
        }
    }

    #[test]
    fn default_filename_matches_spec_example() {
        assert_eq!(
            default_filename("Ostrander Quennell", "4"),
            "Ostrander Quennell invoice 4.pdf"
        );
        assert_eq!(
            default_filename_for(&reference_invoice()),
            "Ostrander Quennell invoice 4.pdf"
        );
    }

    #[test]
    fn default_filename_trims_and_sanitizes_path_separators() {
        assert_eq!(
            default_filename("  Acme/Corp  ", " 12 "),
            "Acme-Corp invoice 12.pdf"
        );
        assert_eq!(default_filename("", "4"), "invoice 4.pdf");
        assert_eq!(default_filename("Acme", ""), "Acme invoice.pdf");
        assert_eq!(default_filename("", ""), "invoice.pdf");
    }

    #[test]
    fn last_export_dir_round_trips_and_missing_is_none() {
        let scratch = ScratchDir::new("prefs");
        assert_eq!(
            load_last_export_dir(scratch.path()).expect("missing is ok"),
            None
        );

        let folder = scratch.path().join("exports");
        fs::create_dir_all(&folder).expect("mkdir");
        save_last_export_dir(scratch.path(), &folder).expect("save");
        assert_eq!(
            load_last_export_dir(scratch.path()).expect("load"),
            Some(folder)
        );
    }

    #[test]
    fn dialog_directory_prefers_existing_last_used_folder() {
        let scratch = ScratchDir::new("dialog");
        let last = scratch.path().join("last");
        fs::create_dir_all(&last).expect("mkdir");
        save_last_export_dir(scratch.path(), &last).expect("save");

        assert_eq!(dialog_directory(Some(scratch.path())), last);
    }

    #[test]
    fn dialog_directory_falls_back_when_last_used_is_gone() {
        let scratch = ScratchDir::new("gone");
        let missing = scratch.path().join("was-here");
        save_last_export_dir(scratch.path(), &missing).expect("save prefs for missing dir");

        let chosen = dialog_directory(Some(scratch.path()));
        assert_ne!(chosen, missing);
        assert!(chosen.is_dir() || chosen == PathBuf::from("."));
    }

    #[test]
    fn write_invoice_pdf_matches_preview_projection() {
        let scratch = ScratchDir::new("write");
        fs::create_dir_all(scratch.path()).expect("mkdir");
        let path = scratch.path().join("Ostrander Quennell invoice 4.pdf");
        let invoice = reference_invoice();

        let preview = project(&invoice);
        assert_eq!(preview.billed_to[0], "Ostrander Quennell");
        assert_eq!(
            preview
                .meta
                .iter()
                .find(|(l, _)| *l == "Invoice number")
                .map(|(_, v)| v.as_str()),
            Some("4")
        );

        let exported = write_invoice_pdf(&invoice, &path).expect("write");
        assert_eq!(
            exported, preview,
            "export must write from the same projection the preview shows"
        );

        let written = fs::read(&path).expect("read back");
        assert!(path.is_file());
        assert!(written.starts_with(b"%PDF"), "must be a PDF");
        // Rendered bytes are not reproducible (printpdf stamps the creation time
        // and a random document id), so check the written file's text instead.
        assert_pdf_shows_projection(&path, &preview);
    }

    /// Text of a PDF via `pdftotext` (Poppler), with whitespace collapsed.
    fn pdf_text(path: &Path) -> String {
        let output = Command::new("pdftotext")
            .arg(path)
            .arg("-")
            .output()
            .expect("pdftotext must be installed to verify exported PDF text");
        assert!(
            output.status.success(),
            "pdftotext failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        normalise(&String::from_utf8_lossy(&output.stdout))
    }

    fn normalise(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Asserts every field group of `projection` appears in the PDF at `path`.
    /// Labels and values are checked separately: the metadata grid and the bank
    /// footer print each label above its value, so `pdftotext` may split them.
    fn assert_pdf_shows_projection(path: &Path, projection: &Projection) {
        let text = pdf_text(path);
        let mut needles: Vec<String> = projection.billed_to.clone();
        for (label, value) in projection.meta.iter().chain(projection.bank.iter()) {
            needles.push(label.to_string());
            needles.push(value.clone());
        }
        for row in &projection.line_items {
            needles.push(row.description.clone());
            needles.push(row.quantity.clone());
            needles.push(row.rate.clone());
            needles.push(row.total.clone());
        }
        for total in &projection.totals {
            needles.push(total.label.clone());
            needles.push(total.amount.clone());
        }
        assert!(!needles.is_empty(), "projection has no fields to check");
        for needle in needles {
            let needle = normalise(&needle);
            assert!(
                text.contains(&needle),
                "exported PDF is missing {needle:?}; text was:\n{text}"
            );
        }
    }

    #[test]
    fn remember_export_path_stores_the_parent_folder() {
        let scratch = ScratchDir::new("remember");
        let out_dir = scratch.path().join("out");
        fs::create_dir_all(&out_dir).expect("mkdir");
        let file = out_dir.join("invoice.pdf");
        fs::write(&file, b"%PDF").expect("touch");

        remember_export_path(Some(scratch.path()), &file);
        assert_eq!(
            load_last_export_dir(scratch.path()).expect("load"),
            Some(out_dir)
        );
    }
}
