use std::collections::BTreeSet;
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
        let (normalized, sensitive_identity_omitted) =
            repository
                .normalized()
                .ok()
                .map_or((None, None), |normalized| {
                    let source_path = normalized.source_path().to_path_buf();
                    (
                        crate::normalize::manifest_safe(normalized),
                        Some(source_path),
                    )
                });
        let sensitive_identity_omitted =
            sensitive_identity_omitted.filter(|_| normalized.is_none());
        let mut diagnostics = repository.diagnostics();
        if let Some(source_path) = sensitive_identity_omitted {
            let mut diagnostic = Diagnostic::warning(
                crate::DiagnosticCode::ManifestSensitiveDataOmitted,
                "normalized manifest projection was omitted because an identity field contained source-sensitive text",
            );
            diagnostic.document_path = Some(source_path);
            crate::diagnostic::push_bounded(&mut diagnostics, diagnostic);
        }
        for diagnostic in &mut diagnostics {
            if crate::normalize::manifest_sensitive_text(&diagnostic.message) {
                diagnostic.message =
                    "diagnostic message redacted because it contained source-sensitive text".into();
            }
            if diagnostic
                .json_path
                .as_ref()
                .is_some_and(|path| crate::normalize::manifest_sensitive_json_pointer(path))
            {
                diagnostic.json_path = None;
            }
        }
        crate::diagnostic::sort_diagnostics(&mut diagnostics);
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
    /// an unsupported schema version, resource-limit violations, or invalid
    /// document identity metadata.
    pub fn from_json(source: &str) -> Result<Self, ManifestReadError> {
        if source.len() > crate::MAX_SOURCE_DOCUMENT_BYTES {
            return Err(ManifestReadError::SourceTooLarge {
                size: source.len(),
                limit: crate::MAX_SOURCE_DOCUMENT_BYTES,
            });
        }
        let duplicate_scan = crate::json_scan::duplicate_keys(source.as_bytes())?;
        if !duplicate_scan.pointers.is_empty() || duplicate_scan.truncated {
            return Err(ManifestReadError::DuplicateObjectMember {
                path: duplicate_scan
                    .pointers
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| "<location omitted>".into()),
            });
        }
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
        if wire.documents.len() > crate::MAX_REPOSITORY_DOCUMENTS {
            return Err(ManifestReadError::DocumentLimit {
                count: wire.documents.len(),
                limit: crate::MAX_REPOSITORY_DOCUMENTS,
            });
        }
        if wire.diagnostics.len() > crate::MAX_REPOSITORY_DIAGNOSTICS {
            return Err(ManifestReadError::DiagnosticLimit {
                count: wire.diagnostics.len(),
                limit: crate::MAX_REPOSITORY_DIAGNOSTICS,
            });
        }
        let mut documents = Vec::with_capacity(wire.documents.len());
        let mut paths = BTreeSet::new();
        for document in wire.documents {
            let path = Path::new(&document.path);
            if crate::path_serde::validate(path).is_err() {
                return Err(ManifestReadError::UnsafeDocumentPath {
                    path: document.path,
                });
            }
            let Some(expected_kind) = DocumentKind::for_path(path) else {
                return Err(ManifestReadError::UnsupportedDocumentPath {
                    path: document.path,
                });
            };
            if document.kind != expected_kind {
                return Err(ManifestReadError::DocumentKindMismatch {
                    path: document.path,
                    expected: expected_kind,
                    found: document.kind,
                });
            }
            if !paths.insert(document.path.clone()) {
                return Err(ManifestReadError::DuplicateDocumentPath {
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
    /// An object repeats a member and therefore has ambiguous semantics.
    #[error("compatibility manifest repeats an object member at {path}")]
    DuplicateObjectMember {
        /// First retained duplicate-key JSON Pointer.
        path: String,
    },
    /// `schema_version` is absent or not a non-negative integer.
    #[error("compatibility manifest requires an integer schema_version")]
    InvalidSchemaVersion,
    /// The manifest uses a schema newer or otherwise unsupported by this crate.
    #[error("unsupported compatibility manifest schema version {found}")]
    UnsupportedSchemaVersion {
        /// Unrecognized source schema version.
        found: u64,
    },
    /// The serialized manifest exceeds the bounded parser input size.
    #[error("compatibility manifest is {size} bytes, exceeding the {limit}-byte limit")]
    SourceTooLarge {
        /// Observed UTF-8 byte length.
        size: usize,
        /// Maximum accepted UTF-8 byte length.
        limit: usize,
    },
    /// A manifest lists more source documents than a repository may retain.
    #[error(
        "compatibility manifest contains {count} documents, exceeding the {limit}-document limit"
    )]
    DocumentLimit {
        /// Observed document count.
        count: usize,
        /// Maximum accepted document count.
        limit: usize,
    },
    /// A manifest carries more diagnostics than one validation pass may emit.
    #[error(
        "compatibility manifest contains {count} diagnostics, exceeding the {limit}-diagnostic limit"
    )]
    DiagnosticLimit {
        /// Observed diagnostic count.
        count: usize,
        /// Maximum accepted diagnostic count.
        limit: usize,
    },
    /// A document entry contains a non-portable repository path.
    #[error("compatibility manifest document path is not portable: {path}")]
    UnsafeDocumentPath {
        /// Invalid source path spelling.
        path: String,
    },
    /// A path does not identify a supported configuration document.
    #[error("compatibility manifest path is not a supported document: {path}")]
    UnsupportedDocumentPath {
        /// Unsupported repository-relative path.
        path: String,
    },
    /// A document's declared kind does not match its path.
    #[error(
        "compatibility manifest document kind mismatch for {path}: expected {expected:?}, found {found:?}"
    )]
    DocumentKindMismatch {
        /// Repository-relative source path.
        path: String,
        /// Kind implied by the supported filename.
        expected: DocumentKind,
        /// Kind declared in the manifest.
        found: DocumentKind,
    },
    /// Two fingerprint entries use the same logical repository path.
    #[error("compatibility manifest repeats document path {path}")]
    DuplicateDocumentPath {
        /// Repeated repository-relative path.
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
