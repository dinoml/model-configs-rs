use std::path::PathBuf;

/// An error encountered while reading model repository configuration.
#[derive(Debug, thiserror::Error)]
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
    /// A JSON configuration file was malformed.
    #[error("invalid JSON in {path}: {source}")]
    Json {
        /// Path containing malformed JSON.
        path: PathBuf,
        /// JSON parser error.
        source: serde_json::Error,
    },
    /// A repository-relative path escaped the repository root.
    #[error("configuration path must be relative and cannot contain parent traversal: {0}")]
    UnsafePath(PathBuf),
}
