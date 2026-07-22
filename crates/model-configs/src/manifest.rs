use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use crate::{
    ConfigError, Diagnostic, DocumentKind, ModelRepository, ModelRepositoryConfig,
    NORMALIZATION_PROFILE,
};

/// Current compatibility-manifest schema version.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Deterministic compatibility description of a repository snapshot.
#[derive(Clone, Debug, Serialize)]
pub struct CompatibilityManifest {
    /// Manifest schema version.
    pub schema_version: u32,
    /// Versioned rule profile used to produce normalized values and defaults.
    pub normalization_profile: String,
    /// Lossless source document identities and fingerprints.
    pub documents: Vec<ManifestDocument>,
    /// Normalized repository configuration, if normalization succeeded.
    pub normalized: Option<ModelRepositoryConfig>,
    /// Stable diagnostics produced for the snapshot.
    pub diagnostics: Vec<Diagnostic>,
}

impl CompatibilityManifest {
    pub(crate) fn from_repository(repository: &ModelRepository) -> Result<Self, ConfigError> {
        let mut documents = repository
            .documents()
            .iter()
            .map(|document| {
                Ok(ManifestDocument {
                    path: portable_path(document.relative_path())?,
                    kind: *document.kind(),
                    sha256: document.sha256_hex(),
                    size: document.original().len() as u64,
                })
            })
            .collect::<Result<Vec<_>, ConfigError>>()?;
        documents.sort_by(|left, right| left.path.cmp(&right.path));
        let normalized = repository
            .normalized()
            .ok()
            .and_then(crate::normalize::manifest_safe);
        let mut diagnostics = repository.diagnostics();
        for diagnostic in &mut diagnostics {
            if crate::normalize::manifest_sensitive_text(&diagnostic.message) {
                diagnostic.message =
                    "diagnostic message redacted because it contained source-sensitive text".into();
            }
            if diagnostic
                .json_path
                .as_ref()
                .is_some_and(|path| crate::normalize::manifest_sensitive_text(path))
            {
                diagnostic.json_path = None;
            }
        }
        Ok(Self {
            schema_version: MANIFEST_SCHEMA_VERSION,
            normalization_profile: NORMALIZATION_PROFILE.to_owned(),
            documents,
            normalized,
            diagnostics,
        })
    }

    /// Serializes this manifest as deterministic pretty-printed JSON.
    ///
    /// # Errors
    ///
    /// Returns an error only if a future manifest field cannot be serialized.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Decodes a compatibility manifest with explicit schema validation.
    ///
    /// Unknown fields are ignored within schema version 1. Unknown diagnostic
    /// codes decode as [`crate::DiagnosticCode::Unknown`].
    ///
    /// # Errors
    ///
    /// Returns an error for malformed JSON, a missing or invalid schema version,
    /// an unsupported schema version, or a non-portable repository path.
    pub fn from_json(source: &str) -> Result<Self, ManifestReadError> {
        let value: serde_json::Value = serde_json::from_str(source)?;
        let schema_version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or(ManifestReadError::InvalidSchemaVersion)?;
        if schema_version != u64::from(MANIFEST_SCHEMA_VERSION) {
            return Err(ManifestReadError::UnsupportedSchemaVersion {
                found: schema_version,
            });
        }
        let wire: CompatibilityManifestWire = serde_json::from_value(value)?;
        let mut documents = Vec::with_capacity(wire.documents.len());
        for document in wire.documents {
            let path = Path::new(&document.path);
            if crate::path_serde::validate(path).is_err() {
                return Err(ManifestReadError::UnsafeDocumentPath {
                    path: document.path,
                });
            }
            if document.sha256.len() != 64
                || !document.sha256.bytes().all(|byte| {
                    byte.is_ascii_digit() || (byte.is_ascii_hexdigit() && byte.is_ascii_lowercase())
                })
            {
                return Err(ManifestReadError::InvalidDocumentDigest {
                    path: document.path.clone(),
                });
            }
            documents.push(ManifestDocument {
                path: document.path,
                kind: document.kind,
                sha256: document.sha256,
                size: document.size,
            });
        }
        Ok(Self {
            schema_version: wire.schema_version,
            normalization_profile: wire.normalization_profile,
            documents,
            normalized: wire.normalized,
            diagnostics: wire.diagnostics,
        })
    }
}

#[derive(Deserialize)]
struct CompatibilityManifestWire {
    schema_version: u32,
    normalization_profile: String,
    documents: Vec<ManifestDocumentWire>,
    normalized: Option<ModelRepositoryConfig>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Deserialize)]
struct ManifestDocumentWire {
    path: String,
    kind: DocumentKind,
    sha256: String,
    size: u64,
}

/// Error decoding a schema-versioned compatibility manifest.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ManifestReadError {
    /// The input is not valid manifest JSON.
    #[error("invalid compatibility manifest JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// `schema_version` is absent or not a non-negative integer.
    #[error("compatibility manifest requires an integer schema_version")]
    InvalidSchemaVersion,
    /// The manifest uses a schema newer or otherwise unsupported by this crate.
    #[error("unsupported compatibility manifest schema version {found}")]
    UnsupportedSchemaVersion {
        /// Unrecognized source schema version.
        found: u64,
    },
    /// A document entry contains a non-portable repository path.
    #[error("compatibility manifest document path is not portable: {path}")]
    UnsafeDocumentPath {
        /// Invalid source path spelling.
        path: String,
    },
    /// A document entry does not contain a lowercase SHA-256 digest.
    #[error("compatibility manifest document has an invalid SHA-256 digest: {path}")]
    InvalidDocumentDigest {
        /// Document whose digest is invalid.
        path: String,
    },
}

/// Source document entry in a [`CompatibilityManifest`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ManifestDocument {
    /// Portable slash-separated repository path.
    path: String,
    /// Recognized document kind.
    kind: DocumentKind,
    /// SHA-256 of the exact source bytes.
    sha256: String,
    /// Exact source size in bytes.
    size: u64,
}

impl ManifestDocument {
    /// Returns the portable slash-separated repository path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the recognized source-document kind.
    #[must_use]
    pub const fn kind(&self) -> DocumentKind {
        self.kind
    }

    /// Returns the lowercase SHA-256 of the exact source bytes.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Returns the exact source byte length.
    #[must_use]
    pub const fn size(&self) -> u64 {
        self.size
    }
}

pub(crate) fn portable_path(path: &Path) -> Result<String, ConfigError> {
    let mut output = String::new();
    for component in path.components() {
        let Component::Normal(value) = component else {
            return Err(ConfigError::UnsafePath(path.to_path_buf()));
        };
        let Some(value) = value.to_str() else {
            return Err(ConfigError::NonUtf8Path(path.to_path_buf()));
        };
        if !output.is_empty() {
            output.push('/');
        }
        output.push_str(value);
    }
    if output.is_empty() {
        return Err(ConfigError::UnsafePath(path.to_path_buf()));
    }
    Ok(output)
}
