//! Versioned per-user persistence for invoice identity (design D5).
//!
//! The store persists ONLY identity: the [`Sender`] and [`Bank`] structs from
//! [`crate::model`]. Per-invoice content (client, meta, line items, VAT, notes)
//! is deliberately not part of this file; a future invoice store is the seam
//! for that (D5). Identity is restored on launch and retained as the default
//! for the next invoice.
//!
//! On-disk format (human-diffable JSON):
//!
//! ```json
//! {
//!   "schema_version": 1,
//!   "sender": {
//!     "name": "...", "street": "...", "postal_code_city": "...", "country": "...",
//!     "tax_number": "...", "vat_id": "...", "email": "...", "layout": "letter"
//!   },
//!   "bank": { "bank_name": "...", "beneficiary": "...", "iban": "..." }
//! }
//! ```
//!
//! Loading rules:
//! - A missing file is first-run: [`load_from`] returns an empty [`Identity`]
//!   without error.
//! - A file whose `schema_version` is not [`SCHEMA_VERSION`] (unknown or newer)
//!   returns [`IdentityError::UnsupportedVersion`]. It is never treated as
//!   empty, so a later save cannot silently overwrite data this build does not
//!   understand.
//! - A corrupt file returns [`IdentityError::Json`].

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::model::{Bank, Sender};

/// The schema version written by this build. Bump it and add a migration when
/// the on-disk shape changes.
pub const SCHEMA_VERSION: u32 = 1;

const APP_QUALIFIER: &str = "com";
const APP_ORGANIZATION: &str = "pravinbashyal";
const APP_NAME: &str = "billing";
const IDENTITY_FILE_NAME: &str = "identity.json";

/// Persisted identity: the sender who issues invoices and the bank that
/// receives payment. Holds no per-invoice content (D5).
///
/// The schema version is not a field here; it is written and checked by the
/// store around this struct, so the in-memory type stays version-agnostic.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Identity {
    pub sender: Sender,
    pub bank: Bank,
}

/// The full on-disk document: the schema version plus the flattened identity.
#[derive(Serialize, Deserialize)]
struct OnDisk {
    schema_version: u32,
    #[serde(flatten)]
    identity: Identity,
}

/// Reads only the schema version, so an unknown version is reported as such
/// even when the rest of the document has a shape this build cannot parse.
#[derive(Deserialize)]
struct VersionProbe {
    schema_version: u32,
}

/// Errors from identity persistence.
#[derive(Debug)]
pub enum IdentityError {
    /// Reading or writing the file failed.
    Io(io::Error),
    /// The file is not valid JSON, or does not match the expected shape.
    Json(serde_json::Error),
    /// The file declares a schema version this build does not understand.
    UnsupportedVersion { found: u32 },
    /// The OS did not provide a per-user config directory.
    NoConfigDir,
}

impl std::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IdentityError::Io(e) => write!(f, "identity file I/O error: {e}"),
            IdentityError::Json(e) => write!(f, "identity file is not valid: {e}"),
            IdentityError::UnsupportedVersion { found } => write!(
                f,
                "identity file schema version {found} is not supported (expected {SCHEMA_VERSION})"
            ),
            IdentityError::NoConfigDir => write!(f, "no per-user config directory is available"),
        }
    }
}

impl std::error::Error for IdentityError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            IdentityError::Io(e) => Some(e),
            IdentityError::Json(e) => Some(e),
            IdentityError::UnsupportedVersion { .. } | IdentityError::NoConfigDir => None,
        }
    }
}

impl From<io::Error> for IdentityError {
    fn from(e: io::Error) -> Self {
        IdentityError::Io(e)
    }
}

impl From<serde_json::Error> for IdentityError {
    fn from(e: serde_json::Error) -> Self {
        IdentityError::Json(e)
    }
}

/// The OS per-user config directory for this app, if one can be resolved.
///
/// On macOS this is `~/Library/Application Support/com.pravinbashyal.billing`.
pub fn default_config_dir() -> Option<PathBuf> {
    ProjectDirs::from(APP_QUALIFIER, APP_ORGANIZATION, APP_NAME)
        .map(|dirs| dirs.config_dir().to_path_buf())
}

/// Loads identity from the per-user config directory. See [`load_from`].
pub fn load() -> Result<Identity, IdentityError> {
    let dir = default_config_dir().ok_or(IdentityError::NoConfigDir)?;
    load_from(&dir)
}

/// Saves identity to the per-user config directory. See [`save_to`].
pub fn save(identity: &Identity) -> Result<(), IdentityError> {
    let dir = default_config_dir().ok_or(IdentityError::NoConfigDir)?;
    save_to(&dir, identity)
}

/// Loads identity from `dir/identity.json`.
///
/// A missing file (first run) yields [`Identity::default`] with no error.
pub fn load_from(dir: &Path) -> Result<Identity, IdentityError> {
    let path = dir.join(IDENTITY_FILE_NAME);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Identity::default()),
        Err(e) => return Err(IdentityError::Io(e)),
    };

    let probe: VersionProbe = serde_json::from_str(&text)?;
    if probe.schema_version != SCHEMA_VERSION {
        return Err(IdentityError::UnsupportedVersion {
            found: probe.schema_version,
        });
    }

    let doc: OnDisk = serde_json::from_str(&text)?;
    Ok(doc.identity)
}

/// Saves identity to `dir/identity.json`, creating `dir` if needed.
///
/// The file is written to a temporary sibling and then renamed into place, so
/// an interrupted save does not leave a truncated identity file behind.
pub fn save_to(dir: &Path, identity: &Identity) -> Result<(), IdentityError> {
    fs::create_dir_all(dir)?;

    let doc = OnDisk {
        schema_version: SCHEMA_VERSION,
        identity: identity.clone(),
    };
    let text = serde_json::to_string_pretty(&doc)?;

    let path = dir.join(IDENTITY_FILE_NAME);
    let tmp_path = dir.join(format!("{IDENTITY_FILE_NAME}.tmp"));
    fs::write(&tmp_path, text.as_bytes())?;
    fs::rename(&tmp_path, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SenderLayout;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// A unique scratch directory under the OS temp dir, removed on drop.
    /// Tests never touch the real per-user config directory.
    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir().join(format!(
                "billing-identity-{label}-{}-{nanos}",
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

    fn reference_identity() -> Identity {
        Identity {
            sender: Sender {
                name: "Alexandra Morgan".to_string(),
                street: "Whitfield. 42".to_string(),
                postal_code_city: "10115 Musterstadt".to_string(),
                country: "Germany".to_string(),
                ..Sender::default()
            },
            bank: Bank {
                bank_name: "Ellery".to_string(),
                beneficiary: "Penhallow Morgan".to_string(),
                iban: "DE89 3704 0044 0532 0130 00".to_string(),
            },
        }
    }

    #[test]
    fn round_trip_preserves_reference_identity() {
        let scratch = ScratchDir::new("roundtrip");
        let original = reference_identity();

        save_to(scratch.path(), &original).expect("save should succeed");
        let loaded = load_from(scratch.path()).expect("load should succeed");

        assert_eq!(loaded, original);
    }

    #[test]
    fn saved_file_carries_schema_version_and_only_identity_keys() {
        let scratch = ScratchDir::new("shape");
        save_to(scratch.path(), &reference_identity()).expect("save should succeed");

        let text = fs::read_to_string(scratch.path().join(IDENTITY_FILE_NAME))
            .expect("identity file should exist");
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        let obj = value.as_object().expect("top level is an object");

        assert_eq!(obj["schema_version"], serde_json::json!(SCHEMA_VERSION));
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["bank", "schema_version", "sender"]);
    }

    #[test]
    fn absent_first_run_file_loads_as_empty_identity() {
        let scratch = ScratchDir::new("firstrun");
        // The directory does not exist at all: a true first run.
        let loaded = load_from(scratch.path()).expect("absent file must not be an error");

        assert_eq!(loaded, Identity::default());
        assert_eq!(loaded.sender, Sender::default());
        assert_eq!(loaded.bank, Bank::default());
    }

    #[test]
    fn unknown_schema_version_is_an_error_not_empty() {
        let scratch = ScratchDir::new("future");
        fs::create_dir_all(scratch.path()).expect("create dir");
        fs::write(
            scratch.path().join(IDENTITY_FILE_NAME),
            r#"{"schema_version": 99, "sender": {}, "bank": {}}"#,
        )
        .expect("write fixture");

        match load_from(scratch.path()) {
            Err(IdentityError::UnsupportedVersion { found }) => assert_eq!(found, 99),
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
    }

    #[test]
    fn corrupt_file_is_an_error() {
        let scratch = ScratchDir::new("corrupt");
        fs::create_dir_all(scratch.path()).expect("create dir");
        fs::write(scratch.path().join(IDENTITY_FILE_NAME), "{ not json").expect("write fixture");

        assert!(matches!(
            load_from(scratch.path()),
            Err(IdentityError::Json(_))
        ));
    }

    #[test]
    fn save_overwrites_previous_identity() {
        let scratch = ScratchDir::new("overwrite");
        save_to(scratch.path(), &reference_identity()).expect("first save");

        let mut edited = reference_identity();
        edited.sender.name = "Someone Else".to_string();
        save_to(scratch.path(), &edited).expect("second save");

        assert_eq!(load_from(scratch.path()).expect("load"), edited);
        assert!(!scratch.path().join("identity.json.tmp").exists());
    }

    #[test]
    fn identity_in_the_earlier_four_field_sender_shape_loads_with_defaults() {
        let scratch = ScratchDir::new("legacy");
        fs::create_dir_all(scratch.path()).expect("create dir");
        fs::write(
            scratch.path().join(IDENTITY_FILE_NAME),
            r#"{
                "schema_version": 1,
                "sender": {
                    "name": "Alexandra Morgan",
                    "street": "Whitfield. 42",
                    "postal_code_city": "10115 Musterstadt",
                    "country": "Germany"
                },
                "bank": {
                    "bank_name": "Ellery",
                    "beneficiary": "Penhallow Morgan",
                    "iban": "DE89 3704 0044 0532 0130 00"
                }
            }"#,
        )
        .expect("write fixture");

        let loaded = load_from(scratch.path()).expect("legacy identity must load");

        assert_eq!(loaded.sender.name, "Alexandra Morgan");
        assert_eq!(loaded.sender.country, "Germany");
        assert_eq!(loaded.sender.tax_number, "");
        assert_eq!(loaded.sender.vat_id, "");
        assert_eq!(loaded.sender.email, "");
        assert_eq!(loaded.sender.layout, SenderLayout::Letter);
        assert_eq!(loaded.bank, reference_identity().bank);
    }

    #[test]
    fn every_new_sender_field_and_each_layout_round_trip_through_the_store() {
        for layout in [
            SenderLayout::Letter,
            SenderLayout::Header,
            SenderLayout::Footer,
        ] {
            let scratch = ScratchDir::new("layout-roundtrip");
            let mut original = reference_identity();
            original.sender.tax_number = "12/345/67890".to_string();
            original.sender.vat_id = "DE123456789".to_string();
            original.sender.email = "Alexandra@example.com".to_string();
            original.sender.layout = layout;

            save_to(scratch.path(), &original).expect("save should succeed");
            let loaded = load_from(scratch.path()).expect("load should succeed");

            assert_eq!(loaded, original, "layout {layout:?}");
        }
    }

    #[test]
    fn layout_is_written_as_a_lowercase_string() {
        let scratch = ScratchDir::new("layout-json");
        let mut identity = reference_identity();
        identity.sender.layout = SenderLayout::Footer;
        save_to(scratch.path(), &identity).expect("save should succeed");

        let text = fs::read_to_string(scratch.path().join(IDENTITY_FILE_NAME))
            .expect("identity file should exist");
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(value["sender"]["layout"], serde_json::json!("footer"));
    }
}
