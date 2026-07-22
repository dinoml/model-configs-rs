use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Component, Path, PathBuf};

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
///
/// Serialization is intentionally available only through
/// [`Self::to_json_pretty`], which enforces the manifest output bound.
///
/// ```compile_fail
/// fn requires_serialize<T: serde::Serialize>() {}
/// requires_serialize::<model_configs::CompatibilityManifest>();
/// ```
#[derive(Clone)]
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

impl fmt::Debug for CompatibilityManifest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompatibilityManifest")
            .field("schema_version", &self.schema_version)
            .field(
                "normalization_profile_byte_len",
                &self.normalization_profile.len(),
            )
            .field("document_count", &self.documents.len())
            .field("has_normalized", &self.normalized.is_some())
            .field("diagnostic_count", &self.diagnostics.len())
            .finish_non_exhaustive()
    }
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
                    let normalized =
                        crate::normalize::manifest_safe(normalized).map(|mut normalized| {
                            canonicalize_normalized_values(&mut normalized);
                            normalized
                        });
                    (normalized, Some(source_path))
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
    /// Returns an error if a future manifest field cannot be serialized, the
    /// bounded output buffer cannot be allocated, or pretty output would
    /// exceed [`MAX_COMPATIBILITY_MANIFEST_BYTES`].
    pub fn to_json_pretty(&self) -> Result<String, ManifestWriteError> {
        self.to_json_pretty_with_limit(MAX_COMPATIBILITY_MANIFEST_BYTES)
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
        let mut wire = parse_manifest_wire(source)?;
        validate_manifest_wire(&wire)?;
        let mut documents = validate_manifest_documents(&wire.documents)?;
        if let Some(normalized) = &mut wire.normalized {
            canonicalize_normalized(normalized)?;
        }
        validate_manifest_relationships(&wire, &documents)?;
        documents.sort_by(|left, right| left.path.cmp(&right.path));
        let mut diagnostics = wire.diagnostics;
        crate::diagnostic::sort_diagnostics(&mut diagnostics);
        Ok(Self {
            schema_version: wire.schema_version,
            normalization_profile: wire.normalization_profile,
            documents,
            normalized: wire.normalized,
            diagnostics,
        })
    }

    fn to_json_pretty_with_limit(&self, limit: usize) -> Result<String, ManifestWriteError> {
        let mut output = BoundedManifestBuffer::new(limit);
        let wire = CompatibilityManifestRef {
            schema_version: self.schema_version,
            normalization_profile: &self.normalization_profile,
            documents: &self.documents,
            normalized: self.normalized.as_ref(),
            diagnostics: &self.diagnostics,
        };
        if let Err(source) = serde_json::to_writer_pretty(&mut output, &wire) {
            if output.limit_exceeded {
                return Err(ManifestWriteError::OutputTooLarge { limit });
            }
            if output.allocation_failed {
                return Err(ManifestWriteError::AllocationFailed { limit });
            }
            return Err(ManifestWriteError::Json(source));
        }
        String::from_utf8(output.bytes).map_err(|_error| ManifestWriteError::InvalidUtf8)
    }
}

#[derive(Serialize)]
struct CompatibilityManifestRef<'a> {
    schema_version: u32,
    normalization_profile: &'a str,
    documents: &'a [ManifestDocument],
    normalized: Option<&'a ModelRepositoryConfig>,
    diagnostics: &'a [Diagnostic],
}

#[derive(Debug)]
struct BoundedManifestBuffer {
    bytes: Vec<u8>,
    limit: usize,
    limit_exceeded: bool,
    allocation_failed: bool,
}

impl BoundedManifestBuffer {
    const fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            limit_exceeded: false,
            allocation_failed: false,
        }
    }
}

impl std::io::Write for BoundedManifestBuffer {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let Some(required) = self.bytes.len().checked_add(buffer.len()) else {
            self.limit_exceeded = true;
            return Err(std::io::Error::other("manifest output limit exceeded"));
        };
        if required > self.limit {
            self.limit_exceeded = true;
            return Err(std::io::Error::other("manifest output limit exceeded"));
        }
        if required > self.bytes.capacity() {
            let doubled = self.bytes.capacity().max(1).saturating_mul(2);
            let target = required.max(doubled).min(self.limit);
            if self
                .bytes
                .try_reserve_exact(target.saturating_sub(self.bytes.len()))
                .is_err()
            {
                self.allocation_failed = true;
                return Err(std::io::Error::other("manifest output allocation failed"));
            }
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
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
    let duplicate_scan = crate::json_scan::duplicate_keys(source.as_bytes())
        .map_err(|error| sanitize_manifest_json_error(&error))?;
    if duplicate_scan.sensitive {
        return Err(ManifestReadError::SensitiveContent);
    }
    if !duplicate_scan.pointers.is_empty() || duplicate_scan.truncated {
        return Err(ManifestReadError::DuplicateObjectMember {
            path: duplicate_scan
                .pointers
                .into_iter()
                .next()
                .unwrap_or_else(|| "<location omitted>".into()),
        });
    }
    let value: serde_json::Value =
        serde_json::from_str(source).map_err(|error| sanitize_manifest_json_error(&error))?;
    let schema_version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or(ManifestReadError::InvalidSchemaVersion)?;
    if schema_version != u64::from(MANIFEST_SCHEMA_VERSION) {
        return Err(ManifestReadError::UnsupportedSchemaVersion {
            found: schema_version,
        });
    }
    let profile = value
        .get("normalization_profile")
        .and_then(serde_json::Value::as_str)
        .ok_or(ManifestReadError::InvalidNormalizationProfile)?;
    validate_normalization_profile(profile)?;
    let input: CompatibilityManifestInput =
        serde_json::from_value(value).map_err(|error| sanitize_manifest_json_error(&error))?;
    input.into_wire()
}

fn sanitize_manifest_json_error(error: &serde_json::Error) -> ManifestReadError {
    ManifestReadError::Json {
        line: error.line(),
        column: error.column(),
    }
}

fn validate_manifest_wire(wire: &CompatibilityManifestWire) -> Result<(), ManifestReadError> {
    validate_normalization_profile(&wire.normalization_profile)?;
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
    for (index, diagnostic) in wire.diagnostics.iter().enumerate() {
        validate_manifest_diagnostic(diagnostic, index)?;
    }
    if let Some(normalized) = &wire.normalized {
        validate_normalized_resource_bounds(normalized)?;
    }
    let sensitive_normalized = wire
        .normalized
        .as_ref()
        .is_some_and(|normalized| !crate::normalize::manifest_is_safe(normalized));
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

fn validate_normalization_profile(profile: &str) -> Result<(), ManifestReadError> {
    if profile.is_empty()
        || profile.len() > crate::MAX_DIAGNOSTIC_TEXT_BYTES
        || crate::normalize::manifest_sensitive_message(profile)
    {
        return Err(ManifestReadError::InvalidNormalizationProfile);
    }
    if profile != NORMALIZATION_PROFILE {
        return Err(ManifestReadError::UnsupportedNormalizationProfile);
    }
    Ok(())
}

fn validate_manifest_diagnostic(
    diagnostic: &Diagnostic,
    index: usize,
) -> Result<(), ManifestReadError> {
    if diagnostic.message.len() > crate::MAX_DIAGNOSTIC_TEXT_BYTES {
        return Err(ManifestReadError::DiagnosticTextTooLarge {
            index,
            field: "message",
        });
    }
    if let Some(path) = diagnostic.json_path.as_deref() {
        if path.len() > crate::MAX_DIAGNOSTIC_TEXT_BYTES {
            return Err(ManifestReadError::DiagnosticTextTooLarge {
                index,
                field: "json_path",
            });
        }
        if !is_json_pointer(path) {
            return Err(ManifestReadError::InvalidDiagnosticJsonPointer { index });
        }
    }
    if expected_diagnostic_level(diagnostic.code).is_some_and(|level| diagnostic.level != level) {
        return Err(ManifestReadError::InvalidDiagnosticLevel { index });
    }
    Ok(())
}

fn expected_diagnostic_level(code: crate::DiagnosticCode) -> Option<crate::DiagnosticLevel> {
    use crate::DiagnosticCode;

    match code {
        DiagnosticCode::MissingRootConfig
        | DiagnosticCode::RootNotObject
        | DiagnosticCode::MissingArchitecture
        | DiagnosticCode::UnsafeReferencePath
        | DiagnosticCode::MissingComponentDirectory
        | DiagnosticCode::EmptyCheckpointWeightMap
        | DiagnosticCode::UnsafeCheckpointShardPath
        | DiagnosticCode::MissingCheckpointShard
        | DiagnosticCode::UnsafeAdapterBasePath
        | DiagnosticCode::MissingAdapterBasePath
        | DiagnosticCode::InvalidDocumentShape
        | DiagnosticCode::InvalidJson
        | DiagnosticCode::DuplicateJsonKey
        | DiagnosticCode::InvalidTextEncoding => Some(crate::DiagnosticLevel::Error),
        DiagnosticCode::MissingComponentConfig
        | DiagnosticCode::MissingTokenizerConfig
        | DiagnosticCode::MissingPreprocessorConfig
        | DiagnosticCode::SymlinkSkipped
        | DiagnosticCode::CustomComponentRequiresCode
        | DiagnosticCode::ExecutableReferenceInert
        | DiagnosticCode::DiagnosticLimitReached
        | DiagnosticCode::ManifestSensitiveDataOmitted => Some(crate::DiagnosticLevel::Warning),
        DiagnosticCode::Unknown => None,
    }
}

fn validate_normalized_resource_bounds(
    normalized: &ModelRepositoryConfig,
) -> Result<(), ManifestReadError> {
    for (index, default) in normalized.applied_defaults.iter().enumerate() {
        for (field, value) in [("field", &default.field), ("rule", &default.rule)] {
            if value.len() > crate::MAX_DIAGNOSTIC_TEXT_BYTES {
                return Err(ManifestReadError::NormalizedTextTooLarge { index, field });
            }
        }
    }
    Ok(())
}

fn is_json_pointer(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    if !value.starts_with('/') {
        return false;
    }
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'~' && !matches!(bytes.next(), Some(b'0' | b'1')) {
            return false;
        }
    }
    true
}

fn validate_manifest_documents(
    wire_documents: &[ManifestDocumentWire],
) -> Result<Vec<ManifestDocument>, ManifestReadError> {
    let mut documents = Vec::with_capacity(wire_documents.len());
    let mut paths = BTreeSet::new();
    let mut portable_paths = Vec::with_capacity(wire_documents.len());
    let mut parent_directories = BTreeSet::new();
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
        insert_parent_directories(
            path,
            &mut parent_directories,
            wire_documents.len(),
            crate::MAX_REPOSITORY_ENTRIES,
        )?;
        documents.push(ManifestDocument {
            path: document.path.clone(),
            kind: document.kind,
            sha256: document.sha256.clone(),
            size: document.size,
        });
    }
    let parent_directories = parent_directories.into_iter().collect::<Vec<_>>();
    crate::repository::validate_no_portable_collisions(&portable_paths, &parent_directories)
        .map_err(|_| ManifestReadError::NonPortableDocumentPaths)?;
    Ok(documents)
}

fn insert_parent_directories(
    path: &Path,
    directories: &mut BTreeSet<std::path::PathBuf>,
    document_count: usize,
    entry_limit: usize,
) -> Result<(), ManifestReadError> {
    let mut parent = path.parent();
    while let Some(directory) = parent.filter(|directory| !directory.as_os_str().is_empty()) {
        if directories.contains(directory) {
            break;
        }
        directories.insert(directory.to_path_buf());
        let count = document_count.saturating_add(directories.len());
        if count > entry_limit {
            return Err(ManifestReadError::RepositoryEntryLimit {
                count,
                limit: entry_limit,
            });
        }
        parent = directory.parent();
    }
    Ok(())
}

fn validate_manifest_document(
    document: &ManifestDocumentWire,
    path: &Path,
    paths: &mut BTreeSet<String>,
) -> Result<(), ManifestReadError> {
    if crate::normalize::manifest_sensitive_path(path) {
        return Err(ManifestReadError::SensitiveContent);
    }
    if crate::path_serde::validate(path).is_err() {
        return Err(ManifestReadError::UnsafeDocumentPath {
            path: document.path.clone(),
        });
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
    for (index, diagnostic) in wire.diagnostics.iter().enumerate() {
        if let Some(path) = diagnostic.document_path() {
            let source = crate::path_serde::portable(path);
            if !documents.iter().any(|document| document.path == source) {
                return Err(ManifestReadError::MissingDiagnosticSourceDocument { index });
            }
        }
    }
    let Some(normalized) = wire.normalized.as_ref() else {
        return Ok(());
    };
    let source = crate::path_serde::portable(normalized.source_path());
    let source_document = documents
        .iter()
        .find(|document| document.path == source)
        .ok_or(ManifestReadError::MissingNormalizedSourceDocument)?;
    let expected_root_path = match source_document.kind {
        DocumentKind::Config => "config.json",
        DocumentKind::ModelIndex => "model_index.json",
        _ => return Err(ManifestReadError::InvalidNormalizedSourceKind),
    };
    if source != expected_root_path {
        return Err(ManifestReadError::InvalidNormalizedSourcePath);
    }
    if normalized.architecture.as_str().is_empty() {
        return Err(ManifestReadError::InvalidNormalizedArchitecture);
    }
    if source_document.kind == DocumentKind::Config
        && documents
            .iter()
            .any(|document| document.path == "model_index.json")
    {
        return Err(ManifestReadError::NormalizedSourcePrecedenceMismatch);
    }
    let architecture_source_matches = match source_document.kind {
        DocumentKind::Config => matches!(
            normalized.architecture_source,
            crate::ArchitectureSource::ConfigArchitectures
                | crate::ArchitectureSource::ConfigClassName
                | crate::ArchitectureSource::ConfigModelType
        ),
        DocumentKind::ModelIndex => matches!(
            normalized.architecture_source,
            crate::ArchitectureSource::ModelIndexClassName
        ),
        _ => false,
    };
    if !architecture_source_matches {
        return Err(ManifestReadError::ArchitectureSourceMismatch);
    }
    validate_dinoml_v1_normalized(normalized, source_document.kind)?;
    Ok(())
}

fn validate_dinoml_v1_normalized(
    normalized: &ModelRepositoryConfig,
    source_kind: DocumentKind,
) -> Result<(), ManifestReadError> {
    if normalized.model_type.as_deref().is_some_and(str::is_empty)
        || normalized
            .transformers_version
            .as_deref()
            .is_some_and(str::is_empty)
        || normalized
            .diffusers_version
            .as_deref()
            .is_some_and(str::is_empty)
        || matches!(
            &normalized.task,
            Some(crate::TaskKind::Other(value))
                if value.is_empty() || is_known_task_spelling(value)
        )
        || matches!(
            normalized.architecture_source,
            crate::ArchitectureSource::ConfigModelType
        ) && normalized.model_type.as_deref() != Some(normalized.architecture.as_str())
        || normalized.model_type.is_some() && normalized.extra.contains_key("model_type")
        || normalized.transformers_version.is_some()
            && normalized.extra.contains_key("transformers_version")
        || normalized.diffusers_version.is_some()
            && normalized.extra.contains_key("_diffusers_version")
        || matches!(
            normalized.architecture_source,
            crate::ArchitectureSource::ModelIndexClassName
                | crate::ArchitectureSource::ConfigClassName
        ) && normalized.extra.contains_key("_class_name")
    {
        return Err(ManifestReadError::InvalidDinomlV1Normalization);
    }
    if source_kind == DocumentKind::Config {
        if normalized.components.is_empty() && normalized.applied_defaults.is_empty() {
            return Ok(());
        }
        return Err(ManifestReadError::InvalidDinomlV1Normalization);
    }

    if normalized.extra.iter().any(|(name, value)| {
        !crate::views::is_model_index_metadata(name) && is_model_index_component_tuple(value)
    }) {
        return Err(ManifestReadError::InvalidDinomlV1Normalization);
    }

    let mut expected_default_count = 0_usize;
    for component in &normalized.components {
        if crate::views::is_model_index_metadata(&component.name)
            || normalized.extra.contains_key(&component.name)
        {
            return Err(ManifestReadError::InvalidDinomlV1Normalization);
        }
        let state_is_valid = if component.optional {
            component.path().is_none()
                && component.library.is_none()
                && component.architecture.is_none()
                && !component.requires_code
        } else {
            component.architecture.is_some()
                && matches!(
                    (&component.library, component.requires_code),
                    (Some(_), false) | (None, true)
                )
        };
        if !state_is_valid {
            return Err(ManifestReadError::InvalidDinomlV1Normalization);
        }
        if component.optional {
            continue;
        }

        if crate::normalize::is_safe_component_name(&component.name) {
            if component.path() != Some(Path::new(&component.name)) {
                return Err(ManifestReadError::InvalidDinomlV1Normalization);
            }
            expected_default_count = expected_default_count.saturating_add(1);
        } else if component.path().is_some() {
            return Err(ManifestReadError::InvalidDinomlV1Normalization);
        }
    }
    if expected_default_count != normalized.applied_defaults.len() {
        return Err(ManifestReadError::InvalidDinomlV1Normalization);
    }
    let expected_components = normalized.components.iter().filter(|component| {
        !component.optional && crate::normalize::is_safe_component_name(&component.name)
    });
    for (component, default) in expected_components.zip(&normalized.applied_defaults) {
        if !component_default_field_matches(&default.field, &component.name)
            || default.rule != "diffusers-component-name-is-path-v1"
            || default.value.as_str() != Some(component.name.as_str())
        {
            return Err(ManifestReadError::InvalidDinomlV1Normalization);
        }
    }
    Ok(())
}

fn is_known_task_spelling(value: &str) -> bool {
    matches!(
        value,
        "text-generation"
            | "text-classification"
            | "token-classification"
            | "question-answering"
            | "feature-extraction"
            | "image-classification"
            | "object-detection"
            | "image-segmentation"
            | "automatic-speech-recognition"
            | "text-to-speech"
            | "text-to-image"
            | "image-to-image"
            | "image-inpainting"
            | "inpainting"
            | "unconditional-image-generation"
            | "text-to-video"
            | "image-to-video"
            | "video-generation"
            | "text-to-audio"
            | "audio-generation"
    )
}

fn is_model_index_component_tuple(value: &serde_json::Value) -> bool {
    let serde_json::Value::Array(tuple) = value else {
        return false;
    };
    if tuple.len() != 2 {
        return false;
    }
    matches!(
        (
            tuple[0].as_str(),
            tuple[1].as_str(),
            tuple[0].is_null(),
            tuple[1].is_null(),
        ),
        (Some(_), Some(_), _, _) | (None, Some(_), true, _) | (_, _, true, true)
    )
}

fn component_default_field_matches(field: &str, component_name: &str) -> bool {
    let Some(encoded_name) = field
        .strip_prefix("/components/")
        .and_then(|field| field.strip_suffix("/path"))
    else {
        return false;
    };
    let mut expected = component_name.bytes();
    let mut encoded = encoded_name.bytes();
    while let Some(byte) = encoded.next() {
        let decoded = if byte == b'~' {
            match encoded.next() {
                Some(b'0') => b'~',
                Some(b'1') => b'/',
                _ => return false,
            }
        } else {
            byte
        };
        if expected.next() != Some(decoded) {
            return false;
        }
    }
    expected.next().is_none()
}

fn canonicalize_normalized(
    normalized: &mut ModelRepositoryConfig,
) -> Result<(), ManifestReadError> {
    let mut component_names = BTreeSet::new();
    if normalized
        .components
        .iter()
        .any(|component| !component_names.insert(component.name.as_str()))
    {
        return Err(ManifestReadError::DuplicateNormalizedComponentName);
    }
    normalized
        .components
        .sort_by(|left, right| left.name.cmp(&right.name));

    let mut default_fields = BTreeSet::new();
    if normalized
        .applied_defaults
        .iter()
        .any(|default| !default_fields.insert(default.field.as_str()))
    {
        return Err(ManifestReadError::DuplicateAppliedDefaultField);
    }
    normalized
        .applied_defaults
        .sort_by(|left, right| left.field.cmp(&right.field));
    canonicalize_normalized_values(normalized);
    Ok(())
}

fn canonicalize_normalized_values(normalized: &mut ModelRepositoryConfig) {
    for value in normalized.extra.values_mut().chain(
        normalized
            .applied_defaults
            .iter_mut()
            .map(|default| &mut default.value),
    ) {
        canonicalize_json(value);
    }
}

fn canonicalize_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                canonicalize_json(value);
            }
        }
        serde_json::Value::Object(object) => {
            for value in object.values_mut() {
                canonicalize_json(value);
            }
            object.sort_keys();
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
}

struct CompatibilityManifestWire {
    schema_version: u32,
    normalization_profile: String,
    documents: Vec<ManifestDocumentWire>,
    normalized: Option<ModelRepositoryConfig>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Deserialize)]
struct CompatibilityManifestInput {
    schema_version: u32,
    normalization_profile: String,
    documents: Vec<ManifestDocumentWire>,
    normalized: Option<ModelRepositoryConfigWire>,
    diagnostics: Vec<DiagnosticWire>,
}

impl CompatibilityManifestInput {
    fn into_wire(self) -> Result<CompatibilityManifestWire, ManifestReadError> {
        let diagnostics = self
            .diagnostics
            .into_iter()
            .enumerate()
            .map(|(index, diagnostic)| diagnostic_from_wire(diagnostic, index))
            .collect::<Result<Vec<_>, ManifestReadError>>()?;
        Ok(CompatibilityManifestWire {
            schema_version: self.schema_version,
            normalization_profile: self.normalization_profile,
            documents: self.documents,
            normalized: self.normalized.map(normalized_from_wire).transpose()?,
            diagnostics,
        })
    }
}

#[derive(Deserialize)]
struct DiagnosticWire {
    level: crate::DiagnosticLevel,
    code: crate::DiagnosticCode,
    message: String,
    document_path: Option<String>,
    json_path: Option<String>,
    related_path: Option<String>,
}

#[derive(Deserialize)]
struct ModelRepositoryConfigWire {
    source_path: String,
    architecture: String,
    architecture_source: ArchitectureSourceWire,
    model_type: Option<String>,
    transformers_version: Option<String>,
    diffusers_version: Option<String>,
    task: Option<TaskKindWire>,
    components: Vec<ComponentReferenceWire>,
    extra: BTreeMap<String, serde_json::Value>,
    applied_defaults: Vec<AppliedDefaultWire>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ArchitectureSourceWire {
    ModelIndexClassName,
    ConfigArchitectures,
    ConfigClassName,
    ConfigModelType,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "value")]
enum TaskKindWire {
    TextGeneration,
    TextClassification,
    TokenClassification,
    QuestionAnswering,
    FeatureExtraction,
    ImageClassification,
    ObjectDetection,
    ImageSegmentation,
    AutomaticSpeechRecognition,
    TextToSpeech,
    TextToImage,
    ImageToImage,
    Inpainting,
    UnconditionalImageGeneration,
    VideoGeneration,
    AudioGeneration,
    Other(String),
}

#[derive(Deserialize)]
struct ComponentReferenceWire {
    name: String,
    path: Option<String>,
    library: Option<String>,
    architecture: Option<String>,
    optional: bool,
    requires_code: bool,
}

#[derive(Deserialize)]
struct AppliedDefaultWire {
    field: String,
    value: serde_json::Value,
    rule: String,
}

fn normalized_from_wire(
    wire: ModelRepositoryConfigWire,
) -> Result<ModelRepositoryConfig, ManifestReadError> {
    let source_path = normalized_source_path(wire.source_path)?;
    let components = wire
        .components
        .into_iter()
        .map(component_from_wire)
        .collect::<Result<Vec<_>, ManifestReadError>>()?;
    let applied_defaults = wire
        .applied_defaults
        .into_iter()
        .map(|default| crate::AppliedDefault {
            field: default.field,
            value: default.value,
            rule: default.rule,
        })
        .collect();
    Ok(ModelRepositoryConfig {
        source_path,
        architecture: crate::ArchitectureId::new(wire.architecture),
        architecture_source: architecture_source_from_wire(wire.architecture_source),
        model_type: wire.model_type,
        transformers_version: wire.transformers_version,
        diffusers_version: wire.diffusers_version,
        task: wire.task.map(task_from_wire),
        components,
        extra: wire.extra,
        applied_defaults,
    })
}

fn normalized_source_path(value: String) -> Result<PathBuf, ManifestReadError> {
    let path = PathBuf::from(value);
    if crate::normalize::manifest_sensitive_path(&path) {
        return Err(ManifestReadError::SensitiveContent);
    }
    crate::path_serde::validate(&path)
        .map(|()| path)
        .map_err(|_error| ManifestReadError::InvalidNormalizedSourcePath)
}

fn component_from_wire(
    wire: ComponentReferenceWire,
) -> Result<crate::ComponentReference, ManifestReadError> {
    let path = wire
        .path
        .map(|value| {
            let path = PathBuf::from(value);
            if crate::normalize::manifest_sensitive_path(&path) {
                return Err(ManifestReadError::SensitiveContent);
            }
            crate::path_serde::validate(&path)
                .map(|()| path)
                .map_err(|_error| ManifestReadError::InvalidDinomlV1Normalization)
        })
        .transpose()?;
    Ok(crate::ComponentReference {
        name: wire.name,
        path,
        library: wire.library,
        architecture: wire.architecture.map(crate::ArchitectureId::new),
        optional: wire.optional,
        requires_code: wire.requires_code,
    })
}

fn diagnostic_from_wire(
    wire: DiagnosticWire,
    index: usize,
) -> Result<Diagnostic, ManifestReadError> {
    Ok(Diagnostic {
        level: wire.level,
        code: wire.code,
        message: wire.message,
        document_path: diagnostic_path_from_wire(wire.document_path, index, "document_path")?,
        json_path: wire.json_path,
        related_path: diagnostic_path_from_wire(wire.related_path, index, "related_path")?,
    })
}

fn diagnostic_path_from_wire(
    value: Option<String>,
    index: usize,
    field: &'static str,
) -> Result<Option<PathBuf>, ManifestReadError> {
    value
        .map(|value| {
            let path = PathBuf::from(value);
            if crate::normalize::manifest_sensitive_path(&path) {
                return Err(ManifestReadError::SensitiveContent);
            }
            crate::path_serde::validate(&path)
                .map(|()| path)
                .map_err(|_error| ManifestReadError::InvalidDiagnosticPath { index, field })
        })
        .transpose()
}

const fn architecture_source_from_wire(wire: ArchitectureSourceWire) -> crate::ArchitectureSource {
    match wire {
        ArchitectureSourceWire::ModelIndexClassName => {
            crate::ArchitectureSource::ModelIndexClassName
        }
        ArchitectureSourceWire::ConfigArchitectures => {
            crate::ArchitectureSource::ConfigArchitectures
        }
        ArchitectureSourceWire::ConfigClassName => crate::ArchitectureSource::ConfigClassName,
        ArchitectureSourceWire::ConfigModelType => crate::ArchitectureSource::ConfigModelType,
    }
}

fn task_from_wire(wire: TaskKindWire) -> crate::TaskKind {
    match wire {
        TaskKindWire::TextGeneration => crate::TaskKind::TextGeneration,
        TaskKindWire::TextClassification => crate::TaskKind::TextClassification,
        TaskKindWire::TokenClassification => crate::TaskKind::TokenClassification,
        TaskKindWire::QuestionAnswering => crate::TaskKind::QuestionAnswering,
        TaskKindWire::FeatureExtraction => crate::TaskKind::FeatureExtraction,
        TaskKindWire::ImageClassification => crate::TaskKind::ImageClassification,
        TaskKindWire::ObjectDetection => crate::TaskKind::ObjectDetection,
        TaskKindWire::ImageSegmentation => crate::TaskKind::ImageSegmentation,
        TaskKindWire::AutomaticSpeechRecognition => crate::TaskKind::AutomaticSpeechRecognition,
        TaskKindWire::TextToSpeech => crate::TaskKind::TextToSpeech,
        TaskKindWire::TextToImage => crate::TaskKind::TextToImage,
        TaskKindWire::ImageToImage => crate::TaskKind::ImageToImage,
        TaskKindWire::Inpainting => crate::TaskKind::Inpainting,
        TaskKindWire::UnconditionalImageGeneration => crate::TaskKind::UnconditionalImageGeneration,
        TaskKindWire::VideoGeneration => crate::TaskKind::VideoGeneration,
        TaskKindWire::AudioGeneration => crate::TaskKind::AudioGeneration,
        TaskKindWire::Other(value) => crate::TaskKind::Other(value),
    }
}

#[derive(Deserialize)]
struct ManifestDocumentWire {
    path: String,
    kind: DocumentKind,
    sha256: String,
    size: u64,
}

/// Error decoding a schema-versioned compatibility manifest.
#[derive(thiserror::Error)]
#[non_exhaustive]
pub enum ManifestReadError {
    /// The input is not valid manifest JSON.
    #[error("invalid compatibility manifest JSON at line {line}, column {column}")]
    Json {
        /// One-based parser line, or zero when structural decoding has no source location.
        line: usize,
        /// One-based parser column, or zero when structural decoding has no source location.
        column: usize,
    },
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
    /// The reader does not implement the manifest's normalization semantics.
    #[error("compatibility manifest uses an unsupported normalization profile")]
    UnsupportedNormalizationProfile,
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
    /// Materializing manifest documents and their parent directories exceeds the repository limit.
    #[error(
        "compatibility manifest materializes {count} entries, exceeding the {limit}-entry limit"
    )]
    RepositoryEntryLimit {
        /// Observed document plus unique parent-directory count.
        count: usize,
        /// Maximum permitted materialized entry count.
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
    /// A diagnostic message or JSON Pointer exceeds its bounded wire size.
    #[error("compatibility manifest diagnostic {index} has an oversized {field}")]
    DiagnosticTextTooLarge {
        /// Zero-based diagnostic array index.
        index: usize,
        /// Oversized diagnostic field.
        field: &'static str,
    },
    /// An applied-default field or rule exceeds its bounded wire size.
    #[error("compatibility manifest applied default {index} has an oversized {field}")]
    NormalizedTextTooLarge {
        /// Zero-based applied-default array index.
        index: usize,
        /// Oversized applied-default field.
        field: &'static str,
    },
    /// A diagnostic location is not an RFC 6901 JSON Pointer.
    #[error("compatibility manifest diagnostic {index} has an invalid JSON Pointer")]
    InvalidDiagnosticJsonPointer {
        /// Zero-based diagnostic array index.
        index: usize,
    },
    /// A known diagnostic code is paired with the wrong stable severity.
    #[error("compatibility manifest diagnostic {index} has an invalid level for its code")]
    InvalidDiagnosticLevel {
        /// Zero-based diagnostic array index.
        index: usize,
    },
    /// A diagnostic source or related path is not portable.
    #[error("compatibility manifest diagnostic {index} has an invalid {field}")]
    InvalidDiagnosticPath {
        /// Zero-based diagnostic array index.
        index: usize,
        /// Invalid diagnostic path field.
        field: &'static str,
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
    /// A diagnostic source path has no corresponding document fingerprint.
    #[error("compatibility manifest diagnostic {index} names an absent source document")]
    MissingDiagnosticSourceDocument {
        /// Zero-based diagnostic array index.
        index: usize,
    },
    /// The normalized authoritative source has no corresponding fingerprint entry.
    #[error("compatibility manifest normalized source has no document fingerprint")]
    MissingNormalizedSourceDocument,
    /// The normalized source fingerprint is not a config or model index.
    #[error("compatibility manifest normalized source has an unsupported document kind")]
    InvalidNormalizedSourceKind,
    /// The normalized source is nested instead of the authoritative repository root.
    #[error("compatibility manifest normalized source is not a root configuration document")]
    InvalidNormalizedSourcePath,
    /// The normalized architecture identity is empty.
    #[error("compatibility manifest normalized architecture is empty")]
    InvalidNormalizedArchitecture,
    /// A root model index exists but the normalized source claims root config precedence.
    #[error("compatibility manifest normalized source contradicts model-index precedence")]
    NormalizedSourcePrecedenceMismatch,
    /// The normalized architecture source is inconsistent with its source document kind.
    #[error("compatibility manifest architecture source does not match its source document kind")]
    ArchitectureSourceMismatch,
    /// The normalized record cannot be produced by the immutable `dinoml-v1` rules.
    #[error("compatibility manifest normalized state is invalid for dinoml-v1")]
    InvalidDinomlV1Normalization,
    /// Two normalized components share one source field name.
    #[error("compatibility manifest repeats a normalized component name")]
    DuplicateNormalizedComponentName,
    /// Two applied defaults target the same normalized field.
    #[error("compatibility manifest repeats an applied-default field")]
    DuplicateAppliedDefaultField,
    /// A document entry does not contain a lowercase SHA-256 digest.
    #[error("compatibility manifest document has an invalid SHA-256 digest: {path}")]
    InvalidDocumentDigest {
        /// Document whose digest is invalid.
        path: String,
    },
}

impl fmt::Debug for ManifestReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let variant = match self {
            Self::Json { .. } => "Json",
            Self::DuplicateObjectMember { .. } => "DuplicateObjectMember",
            Self::InvalidSchemaVersion => "InvalidSchemaVersion",
            Self::UnsupportedSchemaVersion { .. } => "UnsupportedSchemaVersion",
            Self::InvalidNormalizationProfile => "InvalidNormalizationProfile",
            Self::UnsupportedNormalizationProfile => "UnsupportedNormalizationProfile",
            Self::SourceTooLarge { .. } => "SourceTooLarge",
            Self::DocumentLimit { .. } => "DocumentLimit",
            Self::RepositoryEntryLimit { .. } => "RepositoryEntryLimit",
            Self::DocumentSourceTooLarge { .. } => "DocumentSourceTooLarge",
            Self::AggregateDocumentBytesLimit { .. } => "AggregateDocumentBytesLimit",
            Self::DiagnosticLimit { .. } => "DiagnosticLimit",
            Self::DiagnosticTextTooLarge { .. } => "DiagnosticTextTooLarge",
            Self::NormalizedTextTooLarge { .. } => "NormalizedTextTooLarge",
            Self::InvalidDiagnosticJsonPointer { .. } => "InvalidDiagnosticJsonPointer",
            Self::InvalidDiagnosticLevel { .. } => "InvalidDiagnosticLevel",
            Self::InvalidDiagnosticPath { .. } => "InvalidDiagnosticPath",
            Self::SensitiveContent => "SensitiveContent",
            Self::UnsafeDocumentPath { .. } => "UnsafeDocumentPath",
            Self::UnsupportedDocumentPath { .. } => "UnsupportedDocumentPath",
            Self::DocumentKindMismatch { .. } => "DocumentKindMismatch",
            Self::DuplicateDocumentPath { .. } => "DuplicateDocumentPath",
            Self::NonPortableDocumentPaths => "NonPortableDocumentPaths",
            Self::MissingDiagnosticSourceDocument { .. } => "MissingDiagnosticSourceDocument",
            Self::MissingNormalizedSourceDocument => "MissingNormalizedSourceDocument",
            Self::InvalidNormalizedSourceKind => "InvalidNormalizedSourceKind",
            Self::InvalidNormalizedSourcePath => "InvalidNormalizedSourcePath",
            Self::InvalidNormalizedArchitecture => "InvalidNormalizedArchitecture",
            Self::NormalizedSourcePrecedenceMismatch => "NormalizedSourcePrecedenceMismatch",
            Self::ArchitectureSourceMismatch => "ArchitectureSourceMismatch",
            Self::InvalidDinomlV1Normalization => "InvalidDinomlV1Normalization",
            Self::DuplicateNormalizedComponentName => "DuplicateNormalizedComponentName",
            Self::DuplicateAppliedDefaultField => "DuplicateAppliedDefaultField",
            Self::InvalidDocumentDigest { .. } => "InvalidDocumentDigest",
        };
        formatter.debug_struct(variant).finish_non_exhaustive()
    }
}

/// Error encoding a bounded compatibility manifest.
#[derive(thiserror::Error)]
#[non_exhaustive]
pub enum ManifestWriteError {
    /// A manifest field could not be represented as JSON.
    #[error("could not encode compatibility manifest JSON: {0}")]
    Json(serde_json::Error),
    /// Pretty JSON would exceed the compatibility-manifest byte limit.
    #[error("compatibility manifest output exceeds the {limit}-byte limit")]
    OutputTooLarge {
        /// Maximum permitted UTF-8 output bytes.
        limit: usize,
    },
    /// The bounded output buffer could not be allocated.
    #[error("could not allocate the bounded {limit}-byte compatibility manifest buffer")]
    AllocationFailed {
        /// Maximum permitted UTF-8 output bytes.
        limit: usize,
    },
    /// The JSON serializer unexpectedly produced bytes that were not UTF-8.
    #[error("compatibility manifest serializer produced invalid UTF-8")]
    InvalidUtf8,
}

impl fmt::Debug for ManifestWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let variant = match self {
            Self::Json(_) => "Json",
            Self::OutputTooLarge { .. } => "OutputTooLarge",
            Self::AllocationFailed { .. } => "AllocationFailed",
            Self::InvalidUtf8 => "InvalidUtf8",
        };
        formatter.debug_struct(variant).finish_non_exhaustive()
    }
}

/// Source document entry in a [`CompatibilityManifest`].
#[derive(Clone, Eq, PartialEq, Serialize)]
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

impl fmt::Debug for ManifestDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ManifestDocument")
            .field("path", &self.path)
            .field("kind", &self.kind)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn bounded_pretty_writer_stops_deep_indentation_without_large_allocation()
    -> Result<(), Box<dyn std::error::Error>> {
        let array_depth = crate::MAX_SOURCE_JSON_DEPTH - 1;
        let source = format!(
            r#"{{"model_type":"example","nested":{}null{}}}"#,
            "[".repeat(array_depth),
            "]".repeat(array_depth)
        );
        let document = crate::SourceDocument::parse("config.json", source)?;
        let manifest = crate::ModelRepository::from_documents(vec![document])?.manifest()?;

        assert!(matches!(
            manifest.to_json_pretty_with_limit(512),
            Err(ManifestWriteError::OutputTooLarge { limit: 512 })
        ));
        Ok(())
    }

    #[test]
    fn bounded_writer_rejects_one_large_chunk_before_allocating() {
        let mut output = BoundedManifestBuffer::new(512);
        let result = output.write_all(&[b'x'; 513]);

        assert!(result.is_err());
        assert!(output.limit_exceeded);
        assert_eq!(output.bytes.capacity(), 0);
    }

    #[test]
    fn parent_directory_collection_is_deduplicated_and_bounded() {
        let mut directories = BTreeSet::new();
        insert_parent_directories(Path::new("a/b/config.json"), &mut directories, 2, 4)
            .expect("two documents plus two parents fit the test limit");
        insert_parent_directories(Path::new("a/c/config.json"), &mut directories, 2, 4)
            .expect_err("a third unique parent exceeded the test limit");

        assert_eq!(directories.len(), 3);
    }
}
