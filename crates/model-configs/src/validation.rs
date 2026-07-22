use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::{DocumentKind, ModelRepository, NormalizationError, SourceDocument};

pub(crate) fn validate_repository(repository: &ModelRepository) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    match repository.normalized() {
        Ok(_) => {}
        Err(error) => {
            let malformed_root = matches!(&error, NormalizationError::ExpectedObject(path)
                if repository.document(path).is_some_and(|document| document.json_error().is_some()));
            let duplicate_root = matches!(&error, NormalizationError::DuplicateKeys(_));
            if !malformed_root && !duplicate_root {
                crate::diagnostic::push_bounded(&mut diagnostics, normalization_diagnostic(&error));
            }
        }
    }
    for document in repository.documents() {
        if crate::diagnostic::limit_reached(&diagnostics) {
            break;
        }
        validate_document_shape(document, &mut diagnostics);
        if crate::diagnostic::limit_reached(&diagnostics) {
            break;
        }
        validate_semantic_fields(document, &mut diagnostics);
        if crate::diagnostic::limit_reached(&diagnostics) {
            break;
        }
        validate_executable_references(document, &mut diagnostics);
        if crate::diagnostic::limit_reached(&diagnostics) {
            break;
        }
        match document.kind() {
            DocumentKind::ModelIndex => {
                validate_model_index_references(repository, document, &mut diagnostics);
            }
            DocumentKind::SafetensorsIndex => {
                validate_safetensors_index(repository, document, &mut diagnostics);
            }
            DocumentKind::AdapterConfig => {
                validate_adapter(repository, document, &mut diagnostics);
            }
            DocumentKind::ProcessorConfig => {
                validate_processor(repository, document, &mut diagnostics);
            }
            _ => {}
        }
    }
    diagnostics
}

fn normalization_diagnostic(error: &NormalizationError) -> Diagnostic {
    let (code, document_path) = match &error {
        NormalizationError::MissingRootConfig => (DiagnosticCode::MissingRootConfig, None),
        NormalizationError::ExpectedObject(path) => {
            (DiagnosticCode::RootNotObject, Some(path.clone()))
        }
        NormalizationError::MissingArchitecture(path) => {
            (DiagnosticCode::MissingArchitecture, Some(path.clone()))
        }
        NormalizationError::DuplicateKeys(path) => {
            (DiagnosticCode::DuplicateJsonKey, Some(path.clone()))
        }
    };
    let mut diagnostic = Diagnostic::error(code, error.to_string());
    diagnostic.document_path = document_path;
    diagnostic
}

fn validate_model_index_references(
    repository: &ModelRepository,
    document: &SourceDocument,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if document.has_duplicate_keys() {
        return;
    }
    let Some(object) = document.json().and_then(Value::as_object) else {
        return;
    };
    let base = document.relative_path().parent().unwrap_or(Path::new(""));

    for (name, value) in object {
        if crate::diagnostic::limit_reached(diagnostics) {
            break;
        }
        if crate::views::is_model_index_metadata(name) {
            continue;
        }
        let Value::Array(tuple) = value else {
            continue;
        };
        let requires_code = match tuple.as_slice() {
            [Value::String(_), Value::String(_)] => false,
            [Value::Null, Value::String(_)] => true,
            _ => continue,
        };
        let pointer = bounded_pointer_child(Some(""), name);

        if requires_code {
            let mut diagnostic = Diagnostic::warning(
                DiagnosticCode::CustomComponentRequiresCode,
                "component tuple has no data-only library and requires implementation code",
            );
            diagnostic.document_path = Some(document.relative_path().to_path_buf());
            diagnostic.json_path.clone_from(&pointer);
            crate::diagnostic::push_bounded(diagnostics, diagnostic);
        }

        if !crate::normalize::is_safe_component_name(name) {
            // Semantic validation reports the unsafe name. Do not construct a
            // related path from it here.
            continue;
        }
        let related_path = logical_join(base, Path::new(name));
        if !repository.has_directory(&related_path) {
            let mut diagnostic = Diagnostic::error(
                DiagnosticCode::MissingComponentDirectory,
                format!(
                    "component refers to missing or linked directory {}",
                    related_path.display()
                ),
            );
            diagnostic.document_path = Some(document.relative_path().to_path_buf());
            diagnostic.json_path = pointer;
            diagnostic.related_path = Some(related_path);
            crate::diagnostic::push_bounded(diagnostics, diagnostic);
            continue;
        }
        let has_config = has_component_configuration(repository, &related_path);
        if !has_config {
            let mut diagnostic = Diagnostic::warning(
                DiagnosticCode::MissingComponentConfig,
                format!(
                    "component directory {} contains no recognized configuration document",
                    related_path.display()
                ),
            );
            diagnostic.document_path = Some(document.relative_path().to_path_buf());
            diagnostic.json_path = pointer;
            diagnostic.related_path = Some(related_path);
            crate::diagnostic::push_bounded(diagnostics, diagnostic);
        }
    }
}

fn has_component_configuration(repository: &ModelRepository, directory: &Path) -> bool {
    [
        "config.json",
        "tokenizer_config.json",
        "preprocessor_config.json",
        "processor_config.json",
        "scheduler_config.json",
        "adapter_config.json",
        "quantization_config.json",
    ]
    .iter()
    .any(|basename| {
        repository
            .document(logical_join(directory, Path::new(basename)))
            .is_some()
    })
}

fn validate_document_shape(document: &SourceDocument, diagnostics: &mut Vec<Diagnostic>) {
    if document.kind() == &DocumentKind::ChatTemplate {
        if let Err(error) = document.text() {
            let mut diagnostic = Diagnostic::error(
                DiagnosticCode::InvalidTextEncoding,
                format!(
                    "{} is not valid UTF-8: {error}",
                    document.relative_path().display()
                ),
            );
            diagnostic.document_path = Some(document.relative_path().to_path_buf());
            crate::diagnostic::push_bounded(diagnostics, diagnostic);
        }
        return;
    }
    if let Some(error) = document.json_error() {
        let mut diagnostic = Diagnostic::error(
            DiagnosticCode::InvalidJson,
            format!(
                "invalid JSON in {} at line {}, column {}: {}",
                document.relative_path().display(),
                error.line,
                error.column,
                error.message
            ),
        );
        diagnostic.document_path = Some(document.relative_path().to_path_buf());
        crate::diagnostic::push_bounded(diagnostics, diagnostic);
    }
    for pointer in document.duplicate_keys() {
        if crate::diagnostic::limit_reached(diagnostics) {
            return;
        }
        let mut diagnostic = Diagnostic::error(
            DiagnosticCode::DuplicateJsonKey,
            "JSON object repeats a key; generic JSON retains the last value",
        );
        diagnostic.document_path = Some(document.relative_path().to_path_buf());
        if pointer.len() <= crate::MAX_DIAGNOSTIC_TEXT_BYTES {
            diagnostic.json_path = Some(pointer.clone());
        }
        crate::diagnostic::push_bounded(diagnostics, diagnostic);
    }
    if document.duplicate_keys_truncated() && !crate::diagnostic::limit_reached(diagnostics) {
        let mut diagnostic = Diagnostic::error(
            DiagnosticCode::DuplicateJsonKey,
            "additional duplicate-key locations were omitted by the retention limit",
        );
        diagnostic.document_path = Some(document.relative_path().to_path_buf());
        crate::diagnostic::push_bounded(diagnostics, diagnostic);
    }
    if document.kind() != &DocumentKind::ChatTemplate
        && document.json().is_some_and(|value| !value.is_object())
    {
        let mut diagnostic = Diagnostic::error(
            DiagnosticCode::InvalidDocumentShape,
            format!(
                "{} must contain a JSON object",
                document.relative_path().display()
            ),
        );
        diagnostic.document_path = Some(document.relative_path().to_path_buf());
        crate::diagnostic::push_bounded(diagnostics, diagnostic);
    }
}

fn validate_semantic_fields(document: &SourceDocument, diagnostics: &mut Vec<Diagnostic>) {
    if document.has_duplicate_keys() {
        return;
    }
    let Some(object) = document.json().and_then(Value::as_object) else {
        return;
    };
    let fields: &[(&str, FieldShape)] = match document.kind() {
        DocumentKind::Config => CONFIG_FIELDS,
        DocumentKind::GenerationConfig => GENERATION_FIELDS,
        DocumentKind::TokenizerConfig => TOKENIZER_FIELDS,
        DocumentKind::PreprocessorConfig => PREPROCESSOR_FIELDS,
        DocumentKind::ProcessorConfig => PROCESSOR_FIELDS,
        DocumentKind::SchedulerConfig => SCHEDULER_FIELDS,
        DocumentKind::AdapterConfig => ADAPTER_FIELDS,
        DocumentKind::QuantizationConfig => QUANTIZATION_FIELDS,
        DocumentKind::ModelIndex => MODEL_INDEX_FIELDS,
        _ => &[],
    };
    for (field, shape) in fields {
        if crate::diagnostic::limit_reached(diagnostics) {
            return;
        }
        validate_typed_field(document, object.get(*field), field, *shape, diagnostics);
    }
    match document.kind() {
        DocumentKind::Config => {
            validate_architectures(document, object.get("architectures"), diagnostics);
        }
        DocumentKind::ModelIndex => {
            validate_model_index_components(document, object, diagnostics);
        }
        DocumentKind::TokenizerConfig | DocumentKind::ProcessorConfig
            if object.get("chat_template").is_some_and(|value| {
                !value.is_null()
                    && !matches!(value, Value::String(_) | Value::Object(_) | Value::Array(_))
            }) =>
        {
            invalid_field(
                document,
                "/chat_template".into(),
                "chat_template must be a string, object, array, or null",
                diagnostics,
            );
        }
        DocumentKind::SafetensorsIndex => {
            if let Some(Value::Object(metadata)) = object.get("metadata") {
                validate_typed_field(
                    document,
                    metadata.get("total_size"),
                    "metadata/total_size",
                    FieldShape::U64,
                    diagnostics,
                );
            } else if object.get("metadata").is_some_and(|value| !value.is_null()) {
                invalid_field(
                    document,
                    "/metadata".into(),
                    "safetensors metadata must be an object or null",
                    diagnostics,
                );
            }
        }
        _ => {}
    }
}

#[derive(Clone, Copy)]
enum FieldShape {
    String,
    Bool,
    U64,
    I64,
    F64,
}

fn validate_typed_field(
    document: &SourceDocument,
    value: Option<&Value>,
    field: &str,
    shape: FieldShape,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(value) = value else {
        return;
    };
    if value.is_null() {
        return;
    }
    let valid = match shape {
        FieldShape::String => value.is_string(),
        FieldShape::Bool => value.is_boolean(),
        FieldShape::U64 => value.as_u64().is_some(),
        FieldShape::I64 => value.as_i64().is_some(),
        FieldShape::F64 => value.as_f64().is_some(),
    };
    if valid {
        return;
    }
    let expected = match shape {
        FieldShape::String => "a string",
        FieldShape::Bool => "a boolean",
        FieldShape::U64 => "a non-negative integer",
        FieldShape::I64 => "an integer",
        FieldShape::F64 => "a number",
    };
    let pointer = field
        .split('/')
        .map(escape_pointer)
        .collect::<Vec<_>>()
        .join("/");
    invalid_field(
        document,
        format!("/{pointer}"),
        format!("{} must be {expected} or null", field.replace('/', ".")),
        diagnostics,
    );
}

const CONFIG_FIELDS: &[(&str, FieldShape)] = &[
    ("_name_or_path", FieldShape::String),
    ("_class_name", FieldShape::String),
    ("model_type", FieldShape::String),
    ("transformers_version", FieldShape::String),
    ("pipeline_tag", FieldShape::String),
    ("task", FieldShape::String),
    ("torch_dtype", FieldShape::String),
    ("tie_word_embeddings", FieldShape::Bool),
    ("vocab_size", FieldShape::U64),
    ("is_encoder_decoder", FieldShape::Bool),
];

const GENERATION_FIELDS: &[(&str, FieldShape)] = &[
    ("_from_model_config", FieldShape::Bool),
    ("transformers_version", FieldShape::String),
    ("max_length", FieldShape::U64),
    ("max_new_tokens", FieldShape::U64),
    ("min_length", FieldShape::U64),
    ("min_new_tokens", FieldShape::U64),
    ("do_sample", FieldShape::Bool),
    ("num_beams", FieldShape::U64),
    ("temperature", FieldShape::F64),
    ("top_k", FieldShape::U64),
    ("top_p", FieldShape::F64),
    ("typical_p", FieldShape::F64),
    ("repetition_penalty", FieldShape::F64),
    ("length_penalty", FieldShape::F64),
    ("no_repeat_ngram_size", FieldShape::U64),
    ("use_cache", FieldShape::Bool),
    ("cache_implementation", FieldShape::String),
];

const TOKENIZER_FIELDS: &[(&str, FieldShape)] = &[
    ("tokenizer_class", FieldShape::String),
    ("padding_side", FieldShape::String),
    ("truncation_side", FieldShape::String),
    ("clean_up_tokenization_spaces", FieldShape::Bool),
    ("add_prefix_space", FieldShape::Bool),
];

const PREPROCESSOR_FIELDS: &[(&str, FieldShape)] = &[
    ("feature_extractor_type", FieldShape::String),
    ("image_processor_type", FieldShape::String),
    ("processor_class", FieldShape::String),
    ("do_resize", FieldShape::Bool),
    ("do_center_crop", FieldShape::Bool),
    ("do_rescale", FieldShape::Bool),
    ("rescale_factor", FieldShape::F64),
    ("do_normalize", FieldShape::Bool),
    ("return_attention_mask", FieldShape::Bool),
    ("sampling_rate", FieldShape::U64),
];

const PROCESSOR_FIELDS: &[(&str, FieldShape)] = &[
    ("processor_class", FieldShape::String),
    ("image_processor_type", FieldShape::String),
    ("feature_extractor_type", FieldShape::String),
    ("tokenizer_class", FieldShape::String),
    ("num_additional_image_tokens", FieldShape::U64),
    ("patch_size", FieldShape::U64),
    ("vision_feature_select_strategy", FieldShape::String),
];

const SCHEDULER_FIELDS: &[(&str, FieldShape)] = &[
    ("_class_name", FieldShape::String),
    ("_diffusers_version", FieldShape::String),
    ("beta_start", FieldShape::F64),
    ("beta_end", FieldShape::F64),
    ("beta_schedule", FieldShape::String),
    ("num_train_timesteps", FieldShape::U64),
    ("prediction_type", FieldShape::String),
    ("timestep_spacing", FieldShape::String),
    ("steps_offset", FieldShape::I64),
    ("clip_sample", FieldShape::Bool),
    ("set_alpha_to_one", FieldShape::Bool),
    ("skip_prk_steps", FieldShape::Bool),
    ("variance_type", FieldShape::String),
];

const ADAPTER_FIELDS: &[(&str, FieldShape)] = &[
    ("peft_type", FieldShape::String),
    ("task_type", FieldShape::String),
    ("base_model_name_or_path", FieldShape::String),
    ("revision", FieldShape::String),
    ("inference_mode", FieldShape::Bool),
    ("r", FieldShape::U64),
    ("lora_alpha", FieldShape::F64),
    ("lora_dropout", FieldShape::F64),
    ("fan_in_fan_out", FieldShape::Bool),
    ("bias", FieldShape::String),
    ("use_dora", FieldShape::Bool),
    ("use_rslora", FieldShape::Bool),
];

const QUANTIZATION_FIELDS: &[(&str, FieldShape)] = &[
    ("quant_method", FieldShape::String),
    ("bits", FieldShape::U64),
    ("load_in_4bit", FieldShape::Bool),
    ("load_in_8bit", FieldShape::Bool),
    ("bnb_4bit_compute_dtype", FieldShape::String),
    ("bnb_4bit_quant_type", FieldShape::String),
    ("bnb_4bit_use_double_quant", FieldShape::Bool),
    ("llm_int8_threshold", FieldShape::F64),
    ("llm_int8_has_fp16_weight", FieldShape::Bool),
    ("llm_int8_enable_fp32_cpu_offload", FieldShape::Bool),
];

const MODEL_INDEX_FIELDS: &[(&str, FieldShape)] = &[
    ("_class_name", FieldShape::String),
    ("_diffusers_version", FieldShape::String),
    ("_name_or_path", FieldShape::String),
    ("pipeline_tag", FieldShape::String),
    ("task", FieldShape::String),
];

fn validate_architectures(
    document: &SourceDocument,
    value: Option<&Value>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match value {
        None | Some(Value::Null) => {}
        Some(Value::Array(values)) => {
            for (index, value) in values.iter().enumerate() {
                if crate::diagnostic::limit_reached(diagnostics) {
                    break;
                }
                if !value.is_null() && !value.is_string() {
                    invalid_field(
                        document,
                        format!("/architectures/{index}"),
                        "architectures entries must be strings or null",
                        diagnostics,
                    );
                }
            }
        }
        Some(_) => invalid_field(
            document,
            "/architectures".into(),
            "architectures must be an array or null",
            diagnostics,
        ),
    }
}

fn validate_model_index_components(
    document: &SourceDocument,
    object: &serde_json::Map<String, Value>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (name, value) in object {
        if crate::diagnostic::limit_reached(diagnostics) {
            break;
        }
        if crate::views::is_model_index_metadata(name) {
            continue;
        }
        let Value::Array(tuple) = value else {
            continue;
        };
        if tuple.len() != 2 || !tuple.iter().all(|item| item.is_null() || item.is_string()) {
            continue;
        }
        let present = matches!(
            tuple.as_slice(),
            [Value::String(_) | Value::Null, Value::String(_)]
        );
        let optional = matches!(tuple.as_slice(), [Value::Null, Value::Null]);
        let pointer = bounded_pointer_child(Some(""), name);
        if !present && !optional {
            invalid_field_at(
                document,
                pointer,
                "Diffusers component tuple has an unsupported null placement",
                diagnostics,
            );
        } else if present && !crate::normalize::is_safe_component_name(name) {
            let mut diagnostic = Diagnostic::error(
                DiagnosticCode::UnsafeReferencePath,
                "component key is not exactly one safe repository path segment",
            );
            diagnostic.document_path = Some(document.relative_path().to_path_buf());
            diagnostic.json_path = pointer;
            crate::diagnostic::push_bounded(diagnostics, diagnostic);
        }
    }
}

fn invalid_field(
    document: &SourceDocument,
    pointer: String,
    message: impl Into<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    invalid_field_at(document, Some(pointer), message, diagnostics);
}

fn invalid_field_at(
    document: &SourceDocument,
    pointer: Option<String>,
    message: impl Into<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut diagnostic = Diagnostic::error(DiagnosticCode::InvalidDocumentShape, message);
    diagnostic.document_path = Some(document.relative_path().to_path_buf());
    diagnostic.json_path = pointer;
    crate::diagnostic::push_bounded(diagnostics, diagnostic);
}

fn validate_executable_references(document: &SourceDocument, diagnostics: &mut Vec<Diagnostic>) {
    if document.has_duplicate_keys() {
        return;
    }
    let Some(value) = document.json() else {
        return;
    };
    walk_executable_references(document, value, Some(""), diagnostics);
}

fn walk_executable_references(
    document: &SourceDocument,
    value: &Value,
    pointer: Option<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if crate::diagnostic::limit_reached(diagnostics) {
        return;
    }
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if crate::diagnostic::limit_reached(diagnostics) {
                    break;
                }
                let child_pointer = bounded_pointer_child(pointer, key);
                if is_executable_locator(key, child) {
                    let mut diagnostic = Diagnostic::warning(
                        DiagnosticCode::ExecutableReferenceInert,
                        "source field names executable metadata that remains inert",
                    );
                    diagnostic.document_path = Some(document.relative_path().to_path_buf());
                    diagnostic.json_path.clone_from(&child_pointer);
                    crate::diagnostic::push_bounded(diagnostics, diagnostic);
                }
                walk_executable_references(document, child, child_pointer.as_deref(), diagnostics);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                if crate::diagnostic::limit_reached(diagnostics) {
                    break;
                }
                let child_pointer = bounded_pointer_index(pointer, index);
                walk_executable_references(document, child, child_pointer.as_deref(), diagnostics);
            }
        }
        _ => {}
    }
}

fn is_executable_locator(key: &str, value: &Value) -> bool {
    let recognized = matches!(
        key,
        "auto_map" | "auto_mapping" | "_module" | "custom_pipeline" | "trust_remote_code"
    );
    if !recognized || value.is_null() || value == &Value::Bool(false) {
        return false;
    }
    match value {
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
        _ => true,
    }
}

fn validate_safetensors_index(
    repository: &ModelRepository,
    document: &SourceDocument,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if document.has_duplicate_keys() {
        return;
    }
    let Some(object) = document.json().and_then(Value::as_object) else {
        return;
    };
    let weight_map = match object.get("weight_map") {
        Some(Value::Object(weight_map)) => weight_map,
        Some(_) => {
            invalid_field(
                document,
                "/weight_map".into(),
                "safetensors index weight_map must be an object",
                diagnostics,
            );
            return;
        }
        None => {
            invalid_field(
                document,
                "/weight_map".into(),
                "safetensors index requires a weight_map object",
                diagnostics,
            );
            return;
        }
    };
    if weight_map.is_empty() {
        let mut diagnostic = Diagnostic::error(
            DiagnosticCode::EmptyCheckpointWeightMap,
            "safetensors index weight_map is empty",
        );
        diagnostic.document_path = Some(document.relative_path().to_path_buf());
        diagnostic.json_path = Some("/weight_map".into());
        crate::diagnostic::push_bounded(diagnostics, diagnostic);
        return;
    }
    let base = document
        .relative_path()
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let mut shards = BTreeSet::new();
    for (tensor, value) in weight_map {
        if crate::diagnostic::limit_reached(diagnostics) {
            return;
        }
        let Some(shard) = value.as_str().filter(|value| !value.is_empty()) else {
            invalid_field_at(
                document,
                bounded_pointer_child(Some("/weight_map"), tensor),
                "safetensors weight_map values must be non-empty shard path strings",
                diagnostics,
            );
            continue;
        };
        shards.insert(shard);
    }
    for shard in shards {
        if crate::diagnostic::limit_reached(diagnostics) {
            break;
        }
        let shard_path = Path::new(shard);
        if !is_safe_reference(shard_path) {
            let mut diagnostic = Diagnostic::error(
                DiagnosticCode::UnsafeCheckpointShardPath,
                "checkpoint index contains an unsafe shard path",
            );
            diagnostic.document_path = Some(document.relative_path().to_path_buf());
            diagnostic.json_path = Some("/weight_map".into());
            crate::diagnostic::push_bounded(diagnostics, diagnostic);
            continue;
        }
        let related = logical_join(base, shard_path);
        if !repository.has_file(&related) {
            let mut diagnostic = Diagnostic::error(
                DiagnosticCode::MissingCheckpointShard,
                format!("checkpoint shard does not exist: {}", related.display()),
            );
            diagnostic.document_path = Some(document.relative_path().to_path_buf());
            diagnostic.json_path = Some("/weight_map".into());
            diagnostic.related_path = Some(related);
            crate::diagnostic::push_bounded(diagnostics, diagnostic);
        }
    }
}

fn validate_adapter(
    repository: &ModelRepository,
    document: &SourceDocument,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if document.has_duplicate_keys() {
        return;
    }
    let Some(base) = document
        .json()
        .and_then(Value::as_object)
        .and_then(|object| object.get("base_model_name_or_path"))
        .and_then(Value::as_str)
    else {
        return;
    };
    let Some(local_path) = explicit_local_adapter_path(base) else {
        return;
    };
    let path = Path::new(local_path);
    let parent = document
        .relative_path()
        .parent()
        .unwrap_or_else(|| Path::new(""));
    if !is_safe_adapter_path(path) {
        let mut diagnostic = Diagnostic::error(
            DiagnosticCode::UnsafeAdapterBasePath,
            "adapter declares an unsafe explicit local base-model path",
        );
        diagnostic.document_path = Some(document.relative_path().to_path_buf());
        diagnostic.json_path = Some("/base_model_name_or_path".into());
        crate::diagnostic::push_bounded(diagnostics, diagnostic);
        return;
    }
    let related = logical_join(parent, path);
    if !repository.has_entry(&related) {
        let mut diagnostic = Diagnostic::error(
            DiagnosticCode::MissingAdapterBasePath,
            "adapter explicit local base-model path does not exist",
        );
        diagnostic.document_path = Some(document.relative_path().to_path_buf());
        diagnostic.json_path = Some("/base_model_name_or_path".into());
        diagnostic.related_path = Some(related);
        crate::diagnostic::push_bounded(diagnostics, diagnostic);
    }
}

fn validate_processor(
    repository: &ModelRepository,
    document: &SourceDocument,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if document.has_duplicate_keys() {
        return;
    }
    let Some(object) = document.json().and_then(Value::as_object) else {
        return;
    };
    let parent = document
        .relative_path()
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let needs_tokenizer = ["tokenizer", "tokenizer_class"]
        .iter()
        .any(|key| object.get(*key).is_some_and(is_meaningful_declaration));
    let needs_preprocessor = [
        "feature_extractor",
        "feature_extractor_type",
        "image_processor",
        "image_processor_type",
    ]
    .iter()
    .any(|key| object.get(*key).is_some_and(is_meaningful_declaration));
    if needs_tokenizer && !has_document(repository, parent, DocumentKind::TokenizerConfig) {
        let mut diagnostic = Diagnostic::warning(
            DiagnosticCode::MissingTokenizerConfig,
            "processor declares tokenizer behavior without tokenizer_config.json",
        );
        diagnostic.document_path = Some(document.relative_path().to_path_buf());
        crate::diagnostic::push_bounded(diagnostics, diagnostic);
    }
    if needs_preprocessor && !has_document(repository, parent, DocumentKind::PreprocessorConfig) {
        let mut diagnostic = Diagnostic::warning(
            DiagnosticCode::MissingPreprocessorConfig,
            "processor declares image/audio behavior without preprocessor_config.json",
        );
        diagnostic.document_path = Some(document.relative_path().to_path_buf());
        crate::diagnostic::push_bounded(diagnostics, diagnostic);
    }
}

fn is_meaningful_declaration(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
        Value::Bool(_) | Value::Number(_) => true,
    }
}

fn has_document(repository: &ModelRepository, parent: &Path, kind: DocumentKind) -> bool {
    let basename = match kind {
        DocumentKind::TokenizerConfig => "tokenizer_config.json",
        DocumentKind::PreprocessorConfig => "preprocessor_config.json",
        _ => return false,
    };
    repository
        .document(logical_join(parent, Path::new(basename)))
        .is_some()
}

fn is_safe_reference(path: &Path) -> bool {
    crate::path_serde::validate(path).is_ok()
}

fn explicit_local_adapter_path(value: &str) -> Option<&str> {
    if let Some(path) = value.strip_prefix("./") {
        return Some(path);
    }
    let bytes = value.as_bytes();
    let windows_absolute = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    if value.starts_with(['/', '\\'])
        || value.starts_with("../")
        || value.starts_with(".\\")
        || value.starts_with("..\\")
        || windows_absolute
    {
        Some(value)
    } else {
        None
    }
}

fn is_safe_adapter_path(path: &Path) -> bool {
    is_safe_reference(path)
}

fn logical_join(base: &Path, child: &Path) -> PathBuf {
    let base = crate::path_serde::portable(base);
    let child = crate::path_serde::portable(child);
    if base.is_empty() {
        PathBuf::from(child)
    } else {
        PathBuf::from(format!("{base}/{child}"))
    }
}

fn bounded_pointer_child(parent: Option<&str>, key: &str) -> Option<String> {
    let parent = parent?;
    let minimum_length = parent.len().saturating_add(key.len()).saturating_add(1);
    if minimum_length > crate::MAX_DIAGNOSTIC_TEXT_BYTES {
        return None;
    }
    let child = format!("{parent}/{}", escape_pointer(key));
    (child.len() <= crate::MAX_DIAGNOSTIC_TEXT_BYTES).then_some(child)
}

fn bounded_pointer_index(parent: Option<&str>, index: usize) -> Option<String> {
    let parent = parent?;
    let child = format!("{parent}/{index}");
    (child.len() <= crate::MAX_DIAGNOSTIC_TEXT_BYTES).then_some(child)
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
