use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::{DocumentKind, SourceDocument};

/// Stable architecture identity derived without loading implementation code.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ArchitectureId(pub String);

/// A coarse model task when it is explicitly available.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TaskKind {
    /// Text generation or causal language modeling.
    TextGeneration,
    /// Image generation.
    ImageGeneration,
    /// Text-to-image generation.
    TextToImage,
    /// Another source-provided pipeline tag.
    Other(String),
}

/// A component named by a pipeline or composite configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentReference {
    /// Source field naming the component.
    pub name: String,
    /// Optional repository-relative component directory.
    pub path: Option<PathBuf>,
    /// Optional library named in a Diffusers component tuple.
    pub library: Option<String>,
    /// Class or architecture identifier.
    pub architecture: ArchitectureId,
}

/// A default materialized during normalization rather than present in source.
#[derive(Clone, Debug, PartialEq)]
pub struct AppliedDefault {
    /// JSON-pointer-like normalized field path.
    pub field: String,
    /// Value supplied by `DinoML`.
    pub value: Value,
    /// Short stable explanation of the rule.
    pub reason: String,
}

/// A normalized, forward-compatible view of repository configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelRepositoryConfig {
    /// Primary architecture or pipeline class.
    pub architecture: ArchitectureId,
    /// Transformers model type when supplied.
    pub model_type: Option<String>,
    /// Source Transformers version when supplied.
    pub transformers_version: Option<String>,
    /// Explicit or conservatively inferred task.
    pub task: Option<TaskKind>,
    /// Referenced model or pipeline components.
    pub components: Vec<ComponentReference>,
    /// Unconsumed top-level source fields.
    pub extra: BTreeMap<String, Value>,
    /// Defaults applied while constructing this view.
    pub applied_defaults: Vec<AppliedDefault>,
}

pub(crate) fn normalize(documents: &[SourceDocument]) -> Option<ModelRepositoryConfig> {
    let source = documents
        .iter()
        .find(|doc| doc.relative_path() == std::path::Path::new("model_index.json"))
        .or_else(|| {
            documents
                .iter()
                .find(|doc| doc.relative_path() == std::path::Path::new("config.json"))
        })?;
    let mut object = source.json()?.as_object()?.clone();
    let architecture = take_architecture(&mut object, source.kind())?;
    let model_type = take_string(&mut object, "model_type");
    let transformers_version = take_string(&mut object, "transformers_version");
    let task = take_string(&mut object, "pipeline_tag").map(|value| match value.as_str() {
        "text-generation" => TaskKind::TextGeneration,
        "image-to-image" | "unconditional-image-generation" => TaskKind::ImageGeneration,
        "text-to-image" => TaskKind::TextToImage,
        _ => TaskKind::Other(value),
    });
    let components = take_components(&mut object);
    let mut applied_defaults = Vec::new();
    if task.is_none() && matches!(source.kind(), DocumentKind::ModelIndex) {
        applied_defaults.push(AppliedDefault {
            field: "/task".into(),
            value: Value::String("text-to-image".into()),
            reason: "diffusers_pipeline_default".into(),
        });
    }
    Some(ModelRepositoryConfig {
        architecture,
        model_type,
        transformers_version,
        task: task.or_else(|| {
            matches!(source.kind(), DocumentKind::ModelIndex).then_some(TaskKind::TextToImage)
        }),
        components,
        extra: object.into_iter().collect(),
        applied_defaults,
    })
}

fn take_architecture(
    object: &mut Map<String, Value>,
    kind: &DocumentKind,
) -> Option<ArchitectureId> {
    if let Some(Value::Array(values)) = object.remove("architectures") {
        if let Some(value) = values.first().and_then(Value::as_str) {
            return Some(ArchitectureId(value.to_owned()));
        }
    }
    take_string(object, "_class_name")
        .map(ArchitectureId)
        .or_else(|| {
            matches!(kind, DocumentKind::Config)
                .then(|| object.get("model_type").and_then(Value::as_str))
                .flatten()
                .map(|value| ArchitectureId(value.to_owned()))
        })
}

fn take_string(object: &mut Map<String, Value>, key: &str) -> Option<String> {
    object
        .remove(key)
        .and_then(|value| value.as_str().map(str::to_owned))
}

fn take_components(object: &mut Map<String, Value>) -> Vec<ComponentReference> {
    let keys: Vec<String> = object.keys().cloned().collect();
    let mut components = Vec::new();
    for key in keys {
        let Some(Value::Array(tuple)) = object.get(&key) else {
            continue;
        };
        if tuple.len() != 2 {
            continue;
        }
        let (Some(library), Some(class)) = (tuple[0].as_str(), tuple[1].as_str()) else {
            continue;
        };
        components.push(ComponentReference {
            name: key.clone(),
            path: Some(PathBuf::from(&key)),
            library: Some(library.to_owned()),
            architecture: ArchitectureId(class.to_owned()),
        });
        object.remove(&key);
    }
    components
}
