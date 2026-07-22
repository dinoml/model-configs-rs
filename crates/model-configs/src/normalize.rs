use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{DocumentKind, NormalizationError, SourceDocument};

/// Versioned normalization rule profile used by v0.1 manifests.
pub const NORMALIZATION_PROFILE: &str = "dinoml-v1";

/// Stable architecture identity derived without loading implementation code.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ArchitectureId(String);

impl ArchitectureId {
    /// Creates an architecture identifier from source configuration text.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the source spelling of the architecture identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ArchitectureId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Exact source field used to identify the normalized architecture.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum ArchitectureSource {
    /// Root `model_index.json#/_class_name`.
    ModelIndexClassName,
    /// First usable root `config.json#/architectures` entry.
    ConfigArchitectures,
    /// Root `config.json#/_class_name`.
    ConfigClassName,
    /// Root `config.json#/model_type` family fallback.
    ConfigModelType,
}

/// A coarse model task explicitly declared by source configuration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "kebab-case", tag = "kind", content = "value")]
pub enum TaskKind {
    /// Text generation or causal language modeling.
    TextGeneration,
    /// Text classification.
    TextClassification,
    /// Token classification.
    TokenClassification,
    /// Extractive question answering.
    QuestionAnswering,
    /// Feature extraction or embedding generation.
    FeatureExtraction,
    /// Image classification.
    ImageClassification,
    /// Object detection.
    ObjectDetection,
    /// Image segmentation.
    ImageSegmentation,
    /// Automatic speech recognition.
    AutomaticSpeechRecognition,
    /// Text-to-speech generation.
    TextToSpeech,
    /// Text-to-image generation.
    TextToImage,
    /// Image-to-image generation.
    ImageToImage,
    /// Image inpainting.
    Inpainting,
    /// Unconditional image generation.
    UnconditionalImageGeneration,
    /// Video generation.
    VideoGeneration,
    /// Audio generation.
    AudioGeneration,
    /// Another source-provided task spelling.
    Other(String),
}

impl TaskKind {
    pub(crate) fn from_source(value: String) -> Self {
        match value.as_str() {
            "text-generation" => Self::TextGeneration,
            "text-classification" => Self::TextClassification,
            "token-classification" => Self::TokenClassification,
            "question-answering" => Self::QuestionAnswering,
            "feature-extraction" => Self::FeatureExtraction,
            "image-classification" => Self::ImageClassification,
            "object-detection" => Self::ObjectDetection,
            "image-segmentation" => Self::ImageSegmentation,
            "automatic-speech-recognition" => Self::AutomaticSpeechRecognition,
            "text-to-speech" => Self::TextToSpeech,
            "text-to-image" => Self::TextToImage,
            "image-to-image" => Self::ImageToImage,
            "image-inpainting" | "inpainting" => Self::Inpainting,
            "unconditional-image-generation" => Self::UnconditionalImageGeneration,
            "text-to-video" | "image-to-video" | "video-generation" => Self::VideoGeneration,
            "text-to-audio" | "audio-generation" => Self::AudioGeneration,
            _ => Self::Other(value),
        }
    }
}

/// A component named by a pipeline or composite configuration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComponentReference {
    /// Source field naming the component.
    pub name: String,
    /// Repository-relative component directory when the component is present.
    #[serde(
        default,
        serialize_with = "crate::path_serde::serialize_option",
        deserialize_with = "crate::path_serde::deserialize_option"
    )]
    pub(crate) path: Option<PathBuf>,
    /// Library named in a Diffusers component tuple.
    pub library: Option<String>,
    /// Class or architecture identifier when the component is present.
    pub architecture: Option<ArchitectureId>,
    /// Whether the source tuple explicitly disables this component with nulls.
    pub optional: bool,
    /// Whether the component lacks a library and would require external code.
    pub requires_code: bool,
}

impl ComponentReference {
    /// Returns the validated repository-relative component directory, if present.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

/// A default materialized during normalization rather than present in source.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AppliedDefault {
    /// JSON-pointer-like normalized field path.
    pub field: String,
    /// Value supplied by `DinoML`.
    pub value: Value,
    /// Stable identifier of the normalization rule.
    pub rule: String,
}

/// A normalized, forward-compatible view of repository configuration.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModelRepositoryConfig {
    /// Root source document used for normalization.
    #[serde(
        serialize_with = "crate::path_serde::serialize",
        deserialize_with = "crate::path_serde::deserialize"
    )]
    pub(crate) source_path: PathBuf,
    /// Primary architecture or pipeline class.
    pub architecture: ArchitectureId,
    /// Exact precedence branch that supplied [`Self::architecture`].
    pub architecture_source: ArchitectureSource,
    /// Transformers model type when supplied.
    pub model_type: Option<String>,
    /// Source Transformers version when supplied.
    pub transformers_version: Option<String>,
    /// Source Diffusers version when supplied.
    pub diffusers_version: Option<String>,
    /// Explicit source task. Version 0.1 does not guess a task from class names.
    pub task: Option<TaskKind>,
    /// Referenced model or pipeline components.
    pub components: Vec<ComponentReference>,
    /// Unconsumed top-level source fields.
    pub extra: BTreeMap<String, Value>,
    /// Defaults applied while constructing this view.
    pub applied_defaults: Vec<AppliedDefault>,
}

impl ModelRepositoryConfig {
    /// Returns the validated repository-relative authoritative source path.
    #[must_use]
    pub fn source_path(&self) -> &Path {
        &self.source_path
    }
}

pub(crate) fn normalize(
    documents: &[SourceDocument],
) -> Result<ModelRepositoryConfig, NormalizationError> {
    let source = root_source(documents).ok_or(NormalizationError::MissingRootConfig)?;
    if source.has_duplicate_keys() {
        return Err(NormalizationError::DuplicateKeys(
            source.relative_path().to_path_buf(),
        ));
    }
    let mut object = source
        .json()
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| NormalizationError::ExpectedObject(source.relative_path().to_path_buf()))?;
    let (architecture, architecture_source) = take_architecture(&mut object, *source.kind())
        .ok_or_else(|| {
            NormalizationError::MissingArchitecture(source.relative_path().to_path_buf())
        })?;
    let model_type = take_string(&mut object, "model_type");
    let transformers_version = take_string(&mut object, "transformers_version");
    let diffusers_version = take_string(&mut object, "_diffusers_version");
    let task = take_string(&mut object, "pipeline_tag")
        .or_else(|| take_string(&mut object, "task"))
        .map(TaskKind::from_source);
    let (components, applied_defaults) = if source.kind() == &DocumentKind::ModelIndex {
        take_components(&mut object)
    } else {
        (Vec::new(), Vec::new())
    };
    Ok(ModelRepositoryConfig {
        source_path: source.relative_path().to_path_buf(),
        architecture,
        architecture_source,
        model_type,
        transformers_version,
        diffusers_version,
        task,
        components,
        extra: object.into_iter().collect(),
        applied_defaults,
    })
}

pub(crate) fn manifest_safe(mut config: ModelRepositoryConfig) -> Option<ModelRepositoryConfig> {
    let identity_is_sensitive = manifest_sensitive_path(config.source_path())
        || manifest_sensitive_text(config.architecture.as_str())
        || config
            .model_type
            .as_deref()
            .is_some_and(manifest_sensitive_text)
        || config
            .transformers_version
            .as_deref()
            .is_some_and(manifest_sensitive_text)
        || config
            .diffusers_version
            .as_deref()
            .is_some_and(manifest_sensitive_text)
        || matches!(&config.task, Some(TaskKind::Other(value)) if manifest_sensitive_text(value))
        || config.components.iter().any(|component| {
            manifest_sensitive_text(&component.name)
                || component.path().is_some_and(manifest_sensitive_path)
                || component
                    .library
                    .as_deref()
                    .is_some_and(manifest_sensitive_text)
                || component
                    .architecture
                    .as_ref()
                    .is_some_and(|architecture| manifest_sensitive_text(architecture.as_str()))
        });
    if identity_is_sensitive {
        return None;
    }
    config.extra.retain(|key, value| {
        if manifest_sensitive_key(key) || manifest_sensitive_text(key) {
            return false;
        }
        redact_json(value);
        true
    });
    config.applied_defaults.retain_mut(|default| {
        if manifest_sensitive_json_pointer(&default.field)
            || manifest_sensitive_message(&default.rule)
        {
            return false;
        }
        redact_json(&mut default.value);
        true
    });
    Some(config)
}

pub(crate) fn manifest_sensitive_text(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let bytes = value.as_bytes();
    let windows_absolute = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    let authority_has_userinfo = value.find("://").is_some_and(|scheme_end| {
        value[scheme_end + 3..]
            .split(['/', '?', '#'])
            .next()
            .is_some_and(|authority| authority.contains('@'))
    });
    let url_query_has_secret = value.contains("://") && manifest_sensitive_url_query(value);
    value.starts_with(['/', '\\', '~'])
        || lower.starts_with("file://")
        || windows_absolute
        || lower.contains("/.cache/huggingface")
        || lower.contains("\\.cache\\huggingface")
        || lower.contains("bearer ")
        || contains_hf_token(value)
        || authority_has_userinfo
        || url_query_has_secret
}

pub(crate) fn manifest_sensitive_message(value: &str) -> bool {
    manifest_sensitive_text(value)
        || value.split_whitespace().any(|word| {
            let word = word.trim_matches(|character: char| {
                matches!(
                    character,
                    '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ','
                )
            });
            manifest_sensitive_text(word)
        })
}

pub(crate) fn manifest_sensitive_path(value: &Path) -> bool {
    value.iter().any(|segment| {
        segment.to_str().is_none_or(|segment| {
            manifest_sensitive_key(segment) || manifest_sensitive_text(segment)
        })
    })
}

fn contains_hf_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    (0..bytes.len().saturating_sub(2)).any(|start| {
        bytes[start..start + 3].eq_ignore_ascii_case(b"hf_")
            && bytes[start + 3..]
                .iter()
                .take_while(|byte| byte.is_ascii_alphanumeric())
                .count()
                >= 17
    })
}

fn manifest_sensitive_url_query(value: &str) -> bool {
    let Some((_base, query)) = value.split_once('?') else {
        return false;
    };
    query
        .split('#')
        .next()
        .unwrap_or_default()
        .split(['&', ';'])
        .filter_map(|parameter| parameter.split_once('=').map(|(name, _value)| name))
        .any(|name| {
            let name = name
                .to_ascii_lowercase()
                .replace("%2d", "-")
                .replace("%5f", "_");
            let compact = name
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .collect::<String>();
            matches!(compact.as_str(), "key" | "sig" | "sas")
                || compact.contains("token")
                || compact.contains("secret")
                || compact.contains("password")
                || compact.contains("credential")
                || compact.contains("signature")
        })
}

pub(crate) fn manifest_sensitive_json_pointer(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    if !value.starts_with('/') {
        return manifest_sensitive_text(value);
    }
    value.split('/').skip(1).any(|token| {
        let token = token.replace("~1", "/").replace("~0", "~");
        manifest_sensitive_key(&token) || manifest_sensitive_text(&token)
    })
}

fn manifest_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    let compact = key
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>();
    let safe_special_token = matches!(
        compact.as_str(),
        "bostoken"
            | "eostoken"
            | "unktoken"
            | "septoken"
            | "padtoken"
            | "clstoken"
            | "masktoken"
            | "additionalspecialtokens"
    );
    matches!(compact.as_str(), "nameorpath" | "chattemplate")
        || matches!(
            compact.as_str(),
            "authorization"
                | "password"
                | "passwd"
                | "secret"
                | "clientsecret"
                | "apikey"
                | "credential"
                | "credentials"
                | "token"
                | "useauthtoken"
                | "authtoken"
                | "accesstoken"
                | "apitoken"
                | "hftoken"
                | "secretkey"
                | "privatekey"
        )
        || compact.contains("password")
        || compact.contains("credential")
        || compact.ends_with("secret")
        || compact.contains("clientsecret")
        || compact.contains("secretkey")
        || compact.contains("privatekey")
        || compact.contains("apikey")
        || compact.contains("accesskey")
        || compact.contains("authtoken")
        || compact.contains("accesstoken")
        || compact.contains("apitoken")
        || compact.contains("hftoken")
        || (!safe_special_token && compact.ends_with("token"))
}

fn redact_string(value: &mut String) {
    if manifest_sensitive_text(value) {
        value.clear();
        value.push_str("<redacted>");
    }
}

fn redact_json(value: &mut Value) {
    match value {
        Value::String(value) => redact_string(value),
        Value::Array(values) => {
            for value in values {
                redact_json(value);
            }
        }
        Value::Object(object) => object.retain(|key, value| {
            if manifest_sensitive_key(key) || manifest_sensitive_text(key) {
                return false;
            }
            redact_json(value);
            true
        }),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn root_source(documents: &[SourceDocument]) -> Option<&SourceDocument> {
    documents
        .iter()
        .find(|document| document.relative_path() == Path::new("model_index.json"))
        .or_else(|| {
            documents
                .iter()
                .find(|document| document.relative_path() == Path::new("config.json"))
        })
}

fn take_architecture(
    object: &mut Map<String, Value>,
    kind: DocumentKind,
) -> Option<(ArchitectureId, ArchitectureSource)> {
    if kind == DocumentKind::ModelIndex {
        return take_string(object, "_class_name").map(|value| {
            (
                ArchitectureId::new(value),
                ArchitectureSource::ModelIndexClassName,
            )
        });
    }
    let architectures = object.get("architectures").and_then(Value::as_array);
    let architecture = architectures
        .and_then(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .find(|value| !value.is_empty())
        })
        .map(str::to_owned);
    if architecture.is_some() {
        if architectures.is_some_and(|values| {
            values
                .iter()
                .all(|value| value.is_null() || value.is_string())
        }) {
            object.remove("architectures");
        }
        return architecture.map(|value| {
            (
                ArchitectureId::new(value),
                ArchitectureSource::ConfigArchitectures,
            )
        });
    }
    take_string(object, "_class_name")
        .map(|value| {
            (
                ArchitectureId::new(value),
                ArchitectureSource::ConfigClassName,
            )
        })
        .or_else(|| {
            object
                .get("model_type")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(|value| {
                    (
                        ArchitectureId::new(value),
                        ArchitectureSource::ConfigModelType,
                    )
                })
        })
}

fn take_string(object: &mut Map<String, Value>, key: &str) -> Option<String> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())?
        .to_owned();
    object.remove(key);
    Some(value)
}

fn take_components(
    object: &mut Map<String, Value>,
) -> (Vec<ComponentReference>, Vec<AppliedDefault>) {
    let keys: Vec<String> = object.keys().cloned().collect();
    let mut components = Vec::new();
    let mut defaults = Vec::new();
    for key in keys {
        if crate::views::is_model_index_metadata(&key) {
            continue;
        }
        let Some(tuple) = object.get(&key).and_then(Value::as_array) else {
            continue;
        };
        if tuple.len() != 2 {
            continue;
        }
        let component = match (
            tuple[0].as_str(),
            tuple[1].as_str(),
            tuple[0].is_null(),
            tuple[1].is_null(),
        ) {
            (Some(library), Some(class), _, _) => {
                let path = derive_component_path(&key, &mut defaults);
                ComponentReference {
                    name: key.clone(),
                    path,
                    library: Some(library.to_owned()),
                    architecture: Some(ArchitectureId::new(class)),
                    optional: false,
                    requires_code: false,
                }
            }
            (None, Some(class), true, _) => {
                let path = derive_component_path(&key, &mut defaults);
                ComponentReference {
                    name: key.clone(),
                    path,
                    library: None,
                    architecture: Some(ArchitectureId::new(class)),
                    optional: false,
                    requires_code: true,
                }
            }
            (_, _, true, true) => ComponentReference {
                name: key.clone(),
                path: None,
                library: None,
                architecture: None,
                optional: true,
                requires_code: false,
            },
            _ => continue,
        };
        components.push(component);
        object.remove(&key);
    }
    components.sort_by(|left, right| left.name.cmp(&right.name));
    defaults.sort_by(|left, right| left.field.cmp(&right.field));
    (components, defaults)
}

fn derive_component_path(key: &str, defaults: &mut Vec<AppliedDefault>) -> Option<PathBuf> {
    if !is_safe_component_name(key) {
        return None;
    }
    defaults.push(AppliedDefault {
        field: format!("/components/{}/path", escape_pointer(key)),
        value: Value::String(key.to_owned()),
        rule: "diffusers-component-name-is-path-v1".into(),
    });
    Some(PathBuf::from(key))
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

pub(crate) fn is_safe_component_name(value: &str) -> bool {
    !value.contains('/') && crate::path_serde::validate(Path::new(value)).is_ok()
}
