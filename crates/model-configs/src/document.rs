use std::fmt;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::ConfigError;

/// Maximum exact-byte size accepted for one source configuration document.
///
/// This keeps parsing and duplicate-key diagnostics bounded for untrusted Hub
/// content while remaining far above representative configuration sizes.
pub const MAX_SOURCE_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;

/// Maximum retained duplicate-key locations for one source document.
pub const MAX_DUPLICATE_KEY_LOCATIONS: usize = 1_024;

/// Maximum aggregate bytes retained for duplicate-key JSON Pointers.
pub const MAX_DUPLICATE_KEY_LOCATION_BYTES: usize = 1024 * 1024;

/// Portable category of a JSON syntax or data error.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JsonErrorCategory {
    /// Invalid JSON syntax.
    Syntax,
    /// Input ended before a complete JSON value was read.
    Eof,
    /// A syntactically valid value could not be represented.
    Data,
    /// The parser reported an underlying I/O error.
    Io,
}

/// Cloneable structural error retained alongside exact source bytes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct JsonError {
    /// Human-readable parser message.
    pub message: String,
    /// One-based source line reported by the parser.
    pub line: usize,
    /// One-based source column reported by the parser.
    pub column: usize,
    /// Broad stable error category.
    pub category: JsonErrorCategory,
}

impl JsonError {
    fn from_serde(error: &serde_json::Error) -> Self {
        let category = match error.classify() {
            serde_json::error::Category::Syntax => JsonErrorCategory::Syntax,
            serde_json::error::Category::Eof => JsonErrorCategory::Eof,
            serde_json::error::Category::Data => JsonErrorCategory::Data,
            serde_json::error::Category::Io => JsonErrorCategory::Io,
        };
        Self {
            message: error.to_string(),
            line: error.line(),
            column: error.column(),
            category,
        }
    }
}

/// The recognized role of a model repository document.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DocumentKind {
    /// Main Transformers or component configuration.
    Config,
    /// Generation defaults.
    GenerationConfig,
    /// Tokenizer configuration.
    TokenizerConfig,
    /// Tokenizer special-token mapping.
    SpecialTokensMap,
    /// Feature extractor or image/audio preprocessor configuration.
    PreprocessorConfig,
    /// Multimodal processor configuration.
    ProcessorConfig,
    /// Diffusers scheduler configuration.
    SchedulerConfig,
    /// Diffusers pipeline index.
    ModelIndex,
    /// Adapter/PEFT configuration.
    AdapterConfig,
    /// Quantization configuration.
    QuantizationConfig,
    /// Jinja chat template.
    ChatTemplate,
    /// Safetensors sharded checkpoint index.
    SafetensorsIndex,
}

impl DocumentKind {
    /// Recognizes a supported document from its filename.
    #[must_use]
    pub fn for_path(path: impl AsRef<Path>) -> Option<Self> {
        let path = path.as_ref();
        crate::path_serde::validate(path).ok()?;
        Self::from_path(path)
    }

    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?;
        match name {
            "config.json" => Some(Self::Config),
            "generation_config.json" => Some(Self::GenerationConfig),
            "tokenizer_config.json" => Some(Self::TokenizerConfig),
            "special_tokens_map.json" => Some(Self::SpecialTokensMap),
            "preprocessor_config.json" => Some(Self::PreprocessorConfig),
            "processor_config.json" => Some(Self::ProcessorConfig),
            "scheduler_config.json" => Some(Self::SchedulerConfig),
            "model_index.json" => Some(Self::ModelIndex),
            "adapter_config.json" => Some(Self::AdapterConfig),
            "quantization_config.json" => Some(Self::QuantizationConfig),
            "chat_template.jinja" => Some(Self::ChatTemplate),
            _ if name
                .strip_suffix(".safetensors.index.json")
                .is_some_and(|prefix| !prefix.is_empty()) =>
            {
                Some(Self::SafetensorsIndex)
            }
            _ => None,
        }
    }

    fn is_json(self) -> bool {
        !matches!(self, Self::ChatTemplate)
    }
}

/// A source document retaining its exact bytes alongside parsed JSON.
#[derive(Clone)]
pub struct SourceDocument {
    relative_path: PathBuf,
    kind: DocumentKind,
    original: Vec<u8>,
    json: Option<Value>,
    json_error: Option<JsonError>,
    duplicate_keys: Vec<String>,
    duplicate_keys_truncated: bool,
}

impl fmt::Debug for SourceDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SourceDocument")
            .field("relative_path", &self.relative_path)
            .field("kind", &self.kind)
            .field("size", &self.original.len())
            .field("json_error", &self.json_error)
            .field("duplicate_key_count", &self.duplicate_keys.len())
            .field("duplicate_keys_truncated", &self.duplicate_keys_truncated)
            .finish_non_exhaustive()
    }
}

impl SourceDocument {
    /// Parses one recognized repository-relative document from exact source bytes.
    ///
    /// This constructor performs no filesystem access and is useful when bytes come
    /// from a Hub snapshot or another content-addressed store.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::UnsafePath`] when `relative_path` is absolute or
    /// traverses a parent, and [`ConfigError::UnsupportedDocument`] when its
    /// filename is not supported. Malformed JSON is retained and exposed through
    /// [`SourceDocument::json_error`].
    pub fn parse(
        relative_path: impl AsRef<Path>,
        original: impl AsRef<[u8]>,
    ) -> Result<Self, ConfigError> {
        let relative_path = relative_path.as_ref();
        validate_document_path(relative_path)?;
        let original = original.as_ref();
        if original.len() > MAX_SOURCE_DOCUMENT_BYTES {
            return Err(ConfigError::SourceTooLarge {
                path: relative_path.to_path_buf(),
                size: original.len() as u64,
                limit: MAX_SOURCE_DOCUMENT_BYTES as u64,
            });
        }
        Self::parse_owned(relative_path, original.to_vec())
    }

    /// Parses one recognized document while taking ownership of source bytes.
    ///
    /// This avoids a copy when a Hub store or filesystem reader already owns a
    /// byte vector.
    ///
    /// # Errors
    ///
    /// Returns an error for unsafe paths, unsupported filenames, or an internal
    /// inconsistency while scanning syntactically valid JSON.
    pub fn parse_owned(
        relative_path: impl AsRef<Path>,
        original: Vec<u8>,
    ) -> Result<Self, ConfigError> {
        let relative_path = relative_path.as_ref();
        validate_document_path(relative_path)?;
        if original.len() > MAX_SOURCE_DOCUMENT_BYTES {
            return Err(ConfigError::SourceTooLarge {
                path: relative_path.to_path_buf(),
                size: original.len() as u64,
                limit: MAX_SOURCE_DOCUMENT_BYTES as u64,
            });
        }
        let relative_path = relative_path.to_path_buf();
        let kind = DocumentKind::from_path(&relative_path)
            .ok_or_else(|| ConfigError::UnsupportedDocument(relative_path.clone()))?;
        let (json, json_error, duplicate_keys, duplicate_keys_truncated) = if kind.is_json() {
            match serde_json::from_slice(&original) {
                Ok(json) => {
                    let duplicate_scan =
                        crate::json_scan::duplicate_keys(&original).map_err(|source| {
                            ConfigError::Json {
                                path: relative_path.clone(),
                                source,
                            }
                        })?;
                    (
                        Some(json),
                        None,
                        duplicate_scan.pointers,
                        duplicate_scan.truncated,
                    )
                }
                Err(error) => (None, Some(JsonError::from_serde(&error)), Vec::new(), false),
            }
        } else {
            (None, None, Vec::new(), false)
        };
        Ok(Self {
            relative_path,
            kind,
            original,
            json,
            json_error,
            duplicate_keys,
            duplicate_keys_truncated,
        })
    }

    pub(crate) fn read(root: &Path, relative_path: PathBuf) -> Result<Self, ConfigError> {
        validate_document_path(&relative_path)?;
        let path = root.join(&relative_path);
        let metadata = std::fs::metadata(&path).map_err(|source| ConfigError::Read {
            path: path.clone(),
            source,
        })?;
        if metadata.len() > MAX_SOURCE_DOCUMENT_BYTES as u64 {
            return Err(ConfigError::SourceTooLarge {
                path: relative_path,
                size: metadata.len(),
                limit: MAX_SOURCE_DOCUMENT_BYTES as u64,
            });
        }
        let file = std::fs::File::open(&path).map_err(|source| ConfigError::Read {
            path: path.clone(),
            source,
        })?;
        let mut original = Vec::new();
        file.take(MAX_SOURCE_DOCUMENT_BYTES as u64 + 1)
            .read_to_end(&mut original)
            .map_err(|source| ConfigError::Read {
                path: path.clone(),
                source,
            })?;
        if original.len() > MAX_SOURCE_DOCUMENT_BYTES {
            return Err(ConfigError::SourceTooLarge {
                path: relative_path,
                size: original.len() as u64,
                limit: MAX_SOURCE_DOCUMENT_BYTES as u64,
            });
        }
        Self::parse_owned(relative_path, original).map_err(|error| match error {
            ConfigError::Json { source, .. } => ConfigError::Json { path, source },
            other => other,
        })
    }

    /// Returns the path relative to the repository root.
    #[must_use]
    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    /// Returns the recognized document kind.
    #[must_use]
    pub fn kind(&self) -> &DocumentKind {
        &self.kind
    }

    /// Returns the exact bytes read from disk.
    #[must_use]
    pub fn original(&self) -> &[u8] {
        &self.original
    }

    /// Returns parsed JSON, or `None` for text documents.
    #[must_use]
    pub fn json(&self) -> Option<&Value> {
        self.json.as_ref()
    }

    /// Returns the retained JSON parser error for a malformed JSON document.
    #[must_use]
    pub fn json_error(&self) -> Option<&JsonError> {
        self.json_error.as_ref()
    }

    /// Returns JSON Pointers for keys repeated within the same object.
    ///
    /// The generic projection follows `serde_json`'s last-value behavior, while
    /// these pointers ensure that the lossy ambiguity is visible to callers.
    #[must_use]
    pub fn duplicate_keys(&self) -> &[String] {
        &self.duplicate_keys
    }

    /// Returns whether any JSON object key was repeated, including locations
    /// omitted by the bounded duplicate-location retention policy.
    #[must_use]
    pub fn has_duplicate_keys(&self) -> bool {
        !self.duplicate_keys.is_empty() || self.duplicate_keys_truncated
    }

    /// Returns whether additional duplicate-key locations were omitted.
    #[must_use]
    pub const fn duplicate_keys_truncated(&self) -> bool {
        self.duplicate_keys_truncated
    }

    /// Returns UTF-8 text for a text document.
    ///
    /// JSON documents return `Ok(None)` because callers should use [`Self::json`].
    ///
    /// # Errors
    ///
    /// Returns an error when a text document contains invalid UTF-8.
    pub fn text(&self) -> Result<Option<&str>, std::str::Utf8Error> {
        if self.kind == DocumentKind::ChatTemplate {
            std::str::from_utf8(&self.original).map(Some)
        } else {
            Ok(None)
        }
    }

    /// Constructs the format-specific borrowed view for this document.
    ///
    /// # Errors
    ///
    /// Returns [`crate::ViewError`] when a JSON document does not contain the
    /// object shape required by its format. Malformed JSON has no projection and
    /// therefore produces the same typed-view error while exact bytes remain
    /// available from [`Self::original`].
    pub fn typed_view(&self) -> Result<crate::TypedDocumentView<'_>, crate::ViewError> {
        crate::TypedDocumentView::try_from(self)
    }

    /// Computes a SHA-256 fingerprint of the exact source bytes.
    #[must_use]
    pub fn sha256(&self) -> [u8; 32] {
        Sha256::digest(&self.original).into()
    }

    /// Returns the SHA-256 source fingerprint as lowercase hexadecimal.
    #[must_use]
    pub fn sha256_hex(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let digest = self.sha256();
        let mut output = String::with_capacity(digest.len() * 2);
        for byte in digest {
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        output
    }
}

fn validate_document_path(path: &Path) -> Result<(), ConfigError> {
    crate::path_serde::validate(path)
}
