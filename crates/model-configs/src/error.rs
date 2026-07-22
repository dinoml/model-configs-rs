use std::fmt;
use std::path::PathBuf;

/// An operation-level error constructing model repository configuration.
#[derive(thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// A configuration file could not be read.
    #[error("could not read configuration file {path}: {source}")]
    Read {
        /// Path that could not be read.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// A JSON configuration could not complete structural decoding.
    #[error("could not structurally decode JSON in {path}: {source}")]
    Json {
        /// Path containing malformed JSON.
        path: PathBuf,
        /// JSON parser error.
        source: serde_json::Error,
    },
    /// A path is not a safe, portable repository-relative identity.
    #[error("configuration path is not a portable repository-relative path: {0}")]
    UnsafePath(PathBuf),
    /// A path does not name one of the supported repository documents.
    #[error("unsupported configuration document: {0}")]
    UnsupportedDocument(PathBuf),
    /// A path could not be represented as a portable UTF-8 Hub path.
    #[error("configuration path is not valid UTF-8: {0:?}")]
    NonUtf8Path(PathBuf),
    /// A source document exceeded the crate's bounded parsing limit.
    #[error("configuration document {path} is {size} bytes, exceeding the {limit}-byte limit")]
    SourceTooLarge {
        /// Repository-relative document path.
        path: PathBuf,
        /// Observed source byte length.
        size: u64,
        /// Maximum accepted source byte length.
        limit: u64,
    },
    /// A JSON source exceeded the nesting reserved for manifest round trips.
    #[error(
        "configuration document {path} has JSON nesting depth {depth}, exceeding the {limit}-container limit"
    )]
    SourceJsonNestingLimit {
        /// Repository-relative source path.
        path: PathBuf,
        /// Observed maximum array/object nesting depth.
        depth: usize,
        /// Maximum accepted array/object nesting depth.
        limit: usize,
    },
    /// A repository inventory exceeded the bounded discovery limit.
    #[error("repository {root} contains more than {limit} filesystem entries")]
    RepositoryEntryLimit {
        /// Repository root being scanned.
        root: PathBuf,
        /// Maximum number of files and directories accepted.
        limit: usize,
    },
    /// A repository contained too many recognized source documents.
    #[error(
        "repository source document {path} raised the document count to {count}, exceeding the {limit}-document limit"
    )]
    RepositoryDocumentLimit {
        /// First repository-relative source path beyond the limit.
        path: PathBuf,
        /// Observed source-document count at failure.
        count: usize,
        /// Maximum accepted source-document count.
        limit: usize,
    },
    /// Exact bytes across recognized documents exceeded the repository limit.
    #[error(
        "repository source document {path} raised retained source bytes to {size}, exceeding the {limit}-byte limit"
    )]
    RepositorySourceBytesLimit {
        /// Repository-relative source path that crossed the limit.
        path: PathBuf,
        /// Cumulative source byte length at failure.
        size: u64,
        /// Maximum accepted cumulative source byte length.
        limit: u64,
    },
    /// Two source documents claimed the same logical repository path.
    #[error("multiple source documents use repository path {0}")]
    DuplicateDocumentPath(PathBuf),
    /// Two logical paths collide under case-insensitive host materialization.
    #[error("repository paths {first} and {second} are not jointly portable")]
    NonPortablePathCollision {
        /// First colliding logical path.
        first: PathBuf,
        /// Second colliding logical path.
        second: PathBuf,
    },
    /// A repository path contains credential-like text that cannot cross the
    /// compatibility-manifest boundary without losing identity fidelity.
    #[error("repository contains a path that cannot be represented in a credential-safe manifest")]
    ManifestSensitivePath,
}

impl fmt::Debug for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let variant = match self {
            Self::Read { .. } => "Read",
            Self::Json { .. } => "Json",
            Self::UnsafePath(_) => "UnsafePath",
            Self::UnsupportedDocument(_) => "UnsupportedDocument",
            Self::NonUtf8Path(_) => "NonUtf8Path",
            Self::SourceTooLarge { .. } => "SourceTooLarge",
            Self::SourceJsonNestingLimit { .. } => "SourceJsonNestingLimit",
            Self::RepositoryEntryLimit { .. } => "RepositoryEntryLimit",
            Self::RepositoryDocumentLimit { .. } => "RepositoryDocumentLimit",
            Self::RepositorySourceBytesLimit { .. } => "RepositorySourceBytesLimit",
            Self::DuplicateDocumentPath(_) => "DuplicateDocumentPath",
            Self::NonPortablePathCollision { .. } => "NonPortablePathCollision",
            Self::ManifestSensitivePath => "ManifestSensitivePath",
        };
        redacted_error_debug(formatter, variant)
    }
}

/// A content-level error selecting an inert chat template.
#[derive(thiserror::Error)]
#[non_exhaustive]
pub enum ChatTemplateError {
    /// A standalone chat template exists but is not valid UTF-8.
    #[error("text configuration document {path} is not valid UTF-8: {source}")]
    InvalidUtf8 {
        /// Repository-relative text document path.
        path: PathBuf,
        /// UTF-8 decoder error.
        source: std::str::Utf8Error,
    },
}

impl fmt::Debug for ChatTemplateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let variant = match self {
            Self::InvalidUtf8 { .. } => "InvalidUtf8",
        };
        redacted_error_debug(formatter, variant)
    }
}

/// An operation-level error selecting configuration within a repository scope.
#[derive(thiserror::Error)]
#[non_exhaustive]
pub enum SelectionError {
    /// The requested repository scope is not a safe portable relative path.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The selected standalone chat template could not be decoded.
    #[error(transparent)]
    ChatTemplate(#[from] ChatTemplateError),
}

impl fmt::Debug for SelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let variant = match self {
            Self::Config(_) => "Config",
            Self::ChatTemplate(_) => "ChatTemplate",
        };
        redacted_error_debug(formatter, variant)
    }
}

/// A content-level reason that a normalized repository view is unavailable.
///
/// Source bytes and source-local views remain inspectable when this error is
/// returned. Filesystem, path, and resource failures use [`ConfigError`].
#[derive(Clone, Eq, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum NormalizationError {
    /// A recognized JSON document did not contain an object at its root.
    #[error("configuration document {0} must contain a JSON object")]
    ExpectedObject(PathBuf),
    /// No root model or pipeline document could be normalized.
    #[error("repository has no root config.json or model_index.json")]
    MissingRootConfig,
    /// A root document did not declare an architecture identity.
    #[error("configuration document {0} does not declare an architecture or model type")]
    MissingArchitecture(PathBuf),
    /// A document contains duplicate JSON object keys and cannot be normalized.
    #[error("configuration document {0} contains duplicate JSON object keys")]
    DuplicateKeys(PathBuf),
}

impl fmt::Debug for NormalizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let variant = match self {
            Self::ExpectedObject(_) => "ExpectedObject",
            Self::MissingRootConfig => "MissingRootConfig",
            Self::MissingArchitecture(_) => "MissingArchitecture",
            Self::DuplicateKeys(_) => "DuplicateKeys",
        };
        redacted_error_debug(formatter, variant)
    }
}

fn redacted_error_debug(formatter: &mut fmt::Formatter<'_>, variant: &str) -> fmt::Result {
    formatter.debug_struct(variant).finish_non_exhaustive()
}
