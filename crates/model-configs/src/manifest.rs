use std::collections::BTreeSet;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use crate::{
    ConfigError, Diagnostic, DocumentKind, ModelRepository, ModelRepositoryConfig,
    NORMALIZATION_PROFILE,
};

/// Current compatibility-manifest schema version.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Maximum UTF-8 byte size accepted for one serialized compatibility manifest.
pub const MAX_COMPATIBILITY_MANIFEST_BYTES: usize = 256 * 1024 * 1024;

/// Deterministic compatibility description of a repository snapshot.
#[derive(Clone, Debug, Serialize)]
pub struct CompatibilityManifest {
    /// Manifest schema version.
    schema_version: u32,
    /// Versioned rule profile used to produce normalized values and defaults.
    normalization_profile: String,
    /// Lossless source document identities and fingerprints.
    documents: Vec<ManifestDocument>,
    /// Normalized repository configuration, if normalization succeeded.
    normalized: Option<ModelRepositoryConfig>,
    /// Stable diagnostics produced for the snapshot.
    diagnostics: Vec<Diagnostic>,
}

impl CompatibilityManifest {
    pub(crate) fn from_repository(repository: &ModelRepository) -> Result<Self, ConfigError> {
        let mut documents = repository
            .documents()
            .iter()
            .map(|document| {
                if crate::normalize::manifest_sensitive_path(document.relative_path()) {
                    return Err(ConfigError::ManifestSensitivePath);
                }
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
            push_priority_diagnostic(&mut diagnostics, diagnostic);
        }
        for diagnostic in &mut diagnostics {
            let sensitive_document_path = diagnostic
                .document_path
                .as_deref()
                .is_some_and(crate::normalize::manifest_sensitive_path);
            let sensitive_related_path = diagnostic
                .related_path
                .as_deref()
                .is_some_and(crate::normalize::manifest_sensitive_path);
            let sensitive_json_path = diagnostic
                .json_path
                .as_ref()
                .is_some_and(|path| crate::normalize::manifest_sensitive_json_pointer(path));
            if sensitive_document_path {
                diagnostic.document_path = None;
            }
            if sensitive_related_path {
                diagnostic.related_path = None;
            }
            if sensitive_json_path {
                diagnostic.json_path = None;
            }
            if sensitive_document_path
                || sensitive_related_path
                || sensitive_json_path
                || crate::normalize::manifest_sensitive_message(&diagnostic.message)
            {
                diagnostic.message =
                    "diagnostic message redacted because it contained source-sensitive text".into();
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

    /// Returns the compatibility-manifest schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Returns the versioned normalization profile.
    #[must_use]
    pub fn normalization_profile(&self) -> &str {
        &self.normalization_profile
    }

    /// Returns the ordered exact-source fingerprints.
    #[must_use]
    pub fn documents(&self) -> &[ManifestDocument] {
        &self.documents
    }

    /// Returns the credential-safe normalized projection, when available.
    #[must_use]
    pub const fn normalized(&self) -> Option<&ModelRepositoryConfig> {
        self.normalized.as_ref()
    }

    /// Returns the ordered credential-safe diagnostics.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
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
        let wire = parse_manifest_wire(source)?;
        validate_manifest_wire(&wire)?;
        let documents = validate_manifest_documents(&wire.documents)?;
        validate_manifest_relationships(&wire, &documents)?;
        Ok(Self {
            schema_version: wire.schema_version,
            normalization_profile: wire.normalization_profile,
            documents,
            normalized: wire.normalized,
            diagnostics: wire.diagnostics,
        })
    }
}

fn push_priority_diagnostic(diagnostics: &mut Vec<Diagnostic>, diagnostic: Diagnostic) {
    if crate::diagnostic::push_bounded(diagnostics, diagnostic.clone()) {
        return;
    }
    let replacement = diagnostics
        .iter()
        .rposition(|entry| entry.code != crate::DiagnosticCode::DiagnosticLimitReached)
        .unwrap_or_else(|| diagnostics.len().saturating_sub(1));
    if let Some(entry) = diagnostics.get_mut(replacement) {
        *entry = diagnostic;
    }
}

fn parse_manifest_wire(source: &str) -> Result<CompatibilityManifestWire, ManifestReadError> {
    if source.len() > MAX_COMPATIBILITY_MANIFEST_BYTES {
        return Err(ManifestReadError::SourceTooLarge {
            size: source.len(),
            limit: MAX_COMPATIBILITY_MANIFEST_BYTES,
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
    serde_json::from_value(value).map_err(ManifestReadError::from)
}

fn validate_manifest_wire(wire: &CompatibilityManifestWire) -> Result<(), ManifestReadError> {
    if wire.normalization_profile.is_empty()
        || wire.normalization_profile.len() > crate::MAX_DIAGNOSTIC_TEXT_BYTES
        || crate::normalize::manifest_sensitive_message(&wire.normalization_profile)
    {
        return Err(ManifestReadError::InvalidNormalizationProfile);
    }
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
    let sensitive_normalized = wire.normalized.as_ref().is_some_and(|normalized| {
        crate::normalize::manifest_safe(normalized.clone()).as_ref() != Some(normalized)
    });
    let sensitive_diagnostic = wire.diagnostics.iter().any(|diagnostic| {
        crate::normalize::manifest_sensitive_message(&diagnostic.message)
            || diagnostic
                .document_path()
                .is_some_and(crate::normalize::manifest_sensitive_path)
            || diagnostic
                .related_path()
                .is_some_and(crate::normalize::manifest_sensitive_path)
            || diagnostic
                .json_path
                .as_ref()
                .is_some_and(|path| crate::normalize::manifest_sensitive_json_pointer(path))
    });
    if sensitive_normalized || sensitive_diagnostic {
        return Err(ManifestReadError::SensitiveContent);
    }
    Ok(())
}

fn validate_manifest_documents(
    wire_documents: &[ManifestDocumentWire],
) -> Result<Vec<ManifestDocument>, ManifestReadError> {
    let mut documents = Vec::with_capacity(wire_documents.len());
    let mut paths = BTreeSet::new();
    let mut portable_paths = Vec::with_capacity(wire_documents.len());
    let mut total_size = 0_u64;
    for document in wire_documents {
        let path = Path::new(&document.path);
        validate_manifest_document(document, path, &mut paths)?;
        total_size = total_size.saturating_add(document.size);
        if total_size > crate::MAX_REPOSITORY_SOURCE_BYTES {
            return Err(ManifestReadError::AggregateDocumentBytesLimit {
                size: total_size,
                limit: crate::MAX_REPOSITORY_SOURCE_BYTES,
            });
        }
        portable_paths.push(path.to_path_buf());
        documents.push(ManifestDocument {
            path: document.path.clone(),
            kind: document.kind,
            sha256: document.sha256.clone(),
            size: document.size,
        });
    }
    crate::repository::validate_no_portable_collisions(&portable_paths, &[])
        .map_err(|_| ManifestReadError::NonPortableDocumentPaths)?;
    Ok(documents)
}

fn validate_manifest_document(
    document: &ManifestDocumentWire,
    path: &Path,
    paths: &mut BTreeSet<String>,
) -> Result<(), ManifestReadError> {
    if crate::path_serde::validate(path).is_err() {
        return Err(ManifestReadError::UnsafeDocumentPath {
            path: document.path.clone(),
        });
    }
    if crate::normalize::manifest_sensitive_path(path) {
        return Err(ManifestReadError::SensitiveContent);
    }
    if document.size > crate::MAX_SOURCE_DOCUMENT_BYTES as u64 {
        return Err(ManifestReadError::DocumentSourceTooLarge {
            path: document.path.clone(),
            size: document.size,
            limit: crate::MAX_SOURCE_DOCUMENT_BYTES as u64,
        });
    }
    let Some(expected_kind) = DocumentKind::for_path(path) else {
        return Err(ManifestReadError::UnsupportedDocumentPath {
            path: document.path.clone(),
        });
    };
    if document.kind != expected_kind {
        return Err(ManifestReadError::DocumentKindMismatch {
            path: document.path.clone(),
            expected: expected_kind,
            found: document.kind,
        });
    }
    if !paths.insert(document.path.clone()) {
        return Err(ManifestReadError::DuplicateDocumentPath {
            path: document.path.clone(),
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
    Ok(())
}

fn validate_manifest_relationships(
    wire: &CompatibilityManifestWire,
    documents: &[ManifestDocument],
) -> Result<(), ManifestReadError> {
    let Some(normalized) = wire.normalized.as_ref() else {
        return Ok(());
    };
    let source = crate::path_serde::portable(normalized.source_path());
    if documents.iter().all(|document| document.path != source) {
        return Err(ManifestReadError::MissingNormalizedSourceDocument);
    }
    Ok(())
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
    /// The normalization profile is empty, oversized, or source-sensitive.
    #[error("compatibility manifest has an invalid normalization profile")]
    InvalidNormalizationProfile,
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
    /// One document fingerprint claims a source larger than the source parser accepts.
    #[error(
        "compatibility manifest document {path} is {size} bytes, exceeding the {limit}-byte limit"
    )]
    DocumentSourceTooLarge {
        /// Safe repository-relative document path.
        path: String,
        /// Claimed exact source size.
        size: u64,
        /// Maximum accepted source size.
        limit: u64,
    },
    /// Aggregate claimed source bytes exceed the repository retention limit.
    #[error("compatibility manifest claims {size} source bytes, exceeding the {limit}-byte limit")]
    AggregateDocumentBytesLimit {
        /// Claimed aggregate exact source bytes.
        size: u64,
        /// Maximum accepted aggregate source bytes.
        limit: u64,
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
    /// Serialized content violates the credential-safe manifest boundary.
    #[error("compatibility manifest contains source-sensitive content")]
    SensitiveContent,
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
    /// Document paths collide under portable host materialization.
    #[error("compatibility manifest document paths are not jointly portable")]
    NonPortableDocumentPaths,
    /// The normalized authoritative source has no corresponding fingerprint entry.
    #[error("compatibility manifest normalized source has no document fingerprint")]
    MissingNormalizedSourceDocument,
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
