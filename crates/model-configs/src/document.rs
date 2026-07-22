use std::path::{Component, Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::ConfigError;

/// The recognized role of a model repository document.
#[derive(Clone, Debug, Eq, PartialEq)]
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
            _ if name.ends_with(".safetensors.index.json") => Some(Self::SafetensorsIndex),
            _ => None,
        }
    }

    fn is_json(&self) -> bool {
        !matches!(self, Self::ChatTemplate)
    }
}

/// A source document retaining its exact bytes alongside parsed JSON.
#[derive(Clone, Debug)]
pub struct SourceDocument {
    relative_path: PathBuf,
    kind: DocumentKind,
    original: Vec<u8>,
    json: Option<Value>,
}

impl SourceDocument {
    pub(crate) fn read(root: &Path, relative_path: PathBuf) -> Result<Self, ConfigError> {
        if relative_path.is_absolute()
            || relative_path.components().any(|part| {
                matches!(
                    part,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(ConfigError::UnsafePath(relative_path));
        }
        let kind = DocumentKind::from_path(&relative_path)
            .ok_or_else(|| ConfigError::UnsafePath(relative_path.clone()))?;
        let path = root.join(&relative_path);
        let original = std::fs::read(&path).map_err(|source| ConfigError::Read {
            path: path.clone(),
            source,
        })?;
        let json = if kind.is_json() {
            Some(
                serde_json::from_slice(&original)
                    .map_err(|source| ConfigError::Json { path, source })?,
            )
        } else {
            None
        };
        Ok(Self {
            relative_path,
            kind,
            original,
            json,
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

    /// Computes a SHA-256 fingerprint of the exact source bytes.
    #[must_use]
    pub fn sha256(&self) -> [u8; 32] {
        Sha256::digest(&self.original).into()
    }
}
