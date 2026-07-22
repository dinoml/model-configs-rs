use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Maximum diagnostics returned for one repository validation pass.
pub const MAX_REPOSITORY_DIAGNOSTICS: usize = 4_096;

/// Maximum retained bytes for one diagnostic message or structured location.
pub const MAX_DIAGNOSTIC_TEXT_BYTES: usize = 4_096;

/// Severity of a repository diagnostic.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLevel {
    /// Informational compatibility note.
    Info,
    /// A likely repository configuration problem.
    Warning,
    /// A reference or source problem that prevents reliable use.
    Error,
}

/// Stable machine-readable code for a repository diagnostic.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    /// No root `config.json` or `model_index.json` exists.
    MissingRootConfig,
    /// The root document is not a JSON object.
    RootNotObject,
    /// The root document does not identify an architecture.
    MissingArchitecture,
    /// A reference is absolute, traverses a parent, or is not portable.
    UnsafeReferencePath,
    /// A present Diffusers component names a directory that does not exist.
    MissingComponentDirectory,
    /// A component directory contains no recognized configuration document.
    MissingComponentConfig,
    /// A safetensors index contains no weights.
    EmptyCheckpointWeightMap,
    /// A checkpoint shard path is unsafe.
    UnsafeCheckpointShardPath,
    /// A checkpoint shard named by an index does not exist.
    MissingCheckpointShard,
    /// A local adapter base-model reference is unsafe.
    UnsafeAdapterBasePath,
    /// A local adapter base-model reference does not exist.
    MissingAdapterBasePath,
    /// A processor references tokenizer behavior without tokenizer metadata.
    MissingTokenizerConfig,
    /// A processor references image/audio behavior without preprocessor metadata.
    MissingPreprocessorConfig,
    /// A symbolic link was not followed while scanning a repository.
    SymlinkSkipped,
    /// A recognized typed view could not be constructed from its JSON shape.
    InvalidDocumentShape,
    /// A JSON document is malformed; exact bytes remain available.
    InvalidJson,
    /// A JSON object repeats a key and its generic projection is ambiguous.
    DuplicateJsonKey,
    /// A component requires a local/custom implementation and remains inert.
    CustomComponentRequiresCode,
    /// Source metadata names executable code that this crate keeps inert.
    ExecutableReferenceInert,
    /// A text document is not valid UTF-8.
    InvalidTextEncoding,
    /// Additional findings were omitted after the bounded diagnostic limit.
    DiagnosticLimitReached,
    /// A newer manifest used a diagnostic code unknown to this crate version.
    #[serde(other)]
    Unknown,
}

impl DiagnosticCode {
    /// Returns the stable snake-case wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingRootConfig => "missing_root_config",
            Self::RootNotObject => "root_not_object",
            Self::MissingArchitecture => "missing_architecture",
            Self::UnsafeReferencePath => "unsafe_reference_path",
            Self::MissingComponentDirectory => "missing_component_directory",
            Self::MissingComponentConfig => "missing_component_config",
            Self::EmptyCheckpointWeightMap => "empty_checkpoint_weight_map",
            Self::UnsafeCheckpointShardPath => "unsafe_checkpoint_shard_path",
            Self::MissingCheckpointShard => "missing_checkpoint_shard",
            Self::UnsafeAdapterBasePath => "unsafe_adapter_base_path",
            Self::MissingAdapterBasePath => "missing_adapter_base_path",
            Self::MissingTokenizerConfig => "missing_tokenizer_config",
            Self::MissingPreprocessorConfig => "missing_preprocessor_config",
            Self::SymlinkSkipped => "symlink_skipped",
            Self::InvalidDocumentShape => "invalid_document_shape",
            Self::InvalidJson => "invalid_json",
            Self::DuplicateJsonKey => "duplicate_json_key",
            Self::CustomComponentRequiresCode => "custom_component_requires_code",
            Self::ExecutableReferenceInert => "executable_reference_inert",
            Self::InvalidTextEncoding => "invalid_text_encoding",
            Self::DiagnosticLimitReached => "diagnostic_limit_reached",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A validation message tied to source and related repository paths.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Diagnostic {
    /// Severity of the finding.
    pub level: DiagnosticLevel,
    /// Stable machine-readable diagnostic code.
    pub code: DiagnosticCode,
    /// Human-readable explanation.
    pub message: String,
    /// Source document associated with the finding.
    #[serde(
        default,
        serialize_with = "crate::path_serde::serialize_option",
        deserialize_with = "crate::path_serde::deserialize_option"
    )]
    pub(crate) document_path: Option<PathBuf>,
    /// JSON Pointer locating the source field when applicable.
    pub json_path: Option<String>,
    /// Repository-relative path named by the source field when applicable.
    #[serde(
        default,
        serialize_with = "crate::path_serde::serialize_option",
        deserialize_with = "crate::path_serde::deserialize_option"
    )]
    pub(crate) related_path: Option<PathBuf>,
}

impl Diagnostic {
    /// Returns the validated source-document path associated with this finding.
    #[must_use]
    pub fn document_path(&self) -> Option<&std::path::Path> {
        self.document_path.as_deref()
    }

    /// Returns the validated repository path named by this finding.
    #[must_use]
    pub fn related_path(&self) -> Option<&std::path::Path> {
        self.related_path.as_deref()
    }

    pub(crate) fn error(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            level: DiagnosticLevel::Error,
            code,
            message: message.into(),
            document_path: None,
            json_path: None,
            related_path: None,
        }
    }

    pub(crate) fn warning(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            level: DiagnosticLevel::Warning,
            code,
            message: message.into(),
            document_path: None,
            json_path: None,
            related_path: None,
        }
    }
}

pub(crate) fn sort_diagnostics(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by(|left, right| {
        left.level
            .cmp(&right.level)
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| {
                portable_path(left.document_path.as_ref())
                    .cmp(&portable_path(right.document_path.as_ref()))
            })
            .then_with(|| left.json_path.cmp(&right.json_path))
            .then_with(|| {
                portable_path(left.related_path.as_ref())
                    .cmp(&portable_path(right.related_path.as_ref()))
            })
            .then_with(|| left.message.cmp(&right.message))
    });
}

pub(crate) fn push_bounded(diagnostics: &mut Vec<Diagnostic>, mut diagnostic: Diagnostic) -> bool {
    if diagnostic.message.len() > MAX_DIAGNOSTIC_TEXT_BYTES {
        diagnostic.message =
            "diagnostic message omitted because source-derived text exceeded the retention limit"
                .into();
    }
    if diagnostic
        .json_path
        .as_ref()
        .is_some_and(|path| path.len() > MAX_DIAGNOSTIC_TEXT_BYTES)
    {
        diagnostic.json_path = None;
    }
    if diagnostic
        .document_path
        .as_ref()
        .is_some_and(|path| crate::path_serde::portable(path).len() > MAX_DIAGNOSTIC_TEXT_BYTES)
    {
        diagnostic.document_path = None;
    }
    if diagnostic
        .related_path
        .as_ref()
        .is_some_and(|path| crate::path_serde::portable(path).len() > MAX_DIAGNOSTIC_TEXT_BYTES)
    {
        diagnostic.related_path = None;
    }
    match diagnostics.len().cmp(&(MAX_REPOSITORY_DIAGNOSTICS - 1)) {
        std::cmp::Ordering::Less => {
            diagnostics.push(diagnostic);
            true
        }
        std::cmp::Ordering::Equal => {
            diagnostics.push(Diagnostic::warning(
                DiagnosticCode::DiagnosticLimitReached,
                format!(
                    "diagnostic output reached the {MAX_REPOSITORY_DIAGNOSTICS}-entry limit; additional findings were omitted"
                ),
            ));
            false
        }
        std::cmp::Ordering::Greater => false,
    }
}

pub(crate) fn limit_reached(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.len() >= MAX_REPOSITORY_DIAGNOSTICS
}

fn portable_path(path: Option<&PathBuf>) -> Option<String> {
    path.map(PathBuf::as_path).map(crate::path_serde::portable)
}
