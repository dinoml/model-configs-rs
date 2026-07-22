//! Regression tests for content-safe public `Debug` implementations.

use std::fmt::Debug;
use std::path::PathBuf;

use model_configs::{
    ChatTemplateError, ChatTemplateValue, ConfigError, Diagnostic, DocumentKind, JsonError,
    JsonErrorCategory, ManifestReadError, ManifestWriteError, ModelRepository, NormalizationError,
    SelectionError, SourceDocument, SourceField, SpecialTokenValue, TypedDocumentView, ViewError,
};

const SENTINEL: &str = "MODEL_CONFIG_DEBUG_SENTINEL_9f8c2d";

fn assert_debug_omits_sentinel(label: &str, value: &impl Debug) {
    let rendered = format!("{value:?}");
    assert!(
        !rendered.contains(SENTINEL),
        "{label} leaked source content: {rendered}"
    );
}

#[test]
fn json_error_debug_omits_parser_message() {
    let error = JsonError {
        message: SENTINEL.into(),
        line: 7,
        column: 11,
        category: JsonErrorCategory::Data,
    };

    assert_debug_omits_sentinel("JSON parser error", &error);
}

#[test]
fn public_error_debug_omits_paths_and_nested_source_messages() {
    let absolute = PathBuf::from(format!(r"C:\Users\{SENTINEL}\repository"));
    let config = ConfigError::Read {
        path: absolute.clone(),
        source: std::io::Error::other(SENTINEL),
    };
    assert_debug_omits_sentinel("configuration error", &config);

    let selection = SelectionError::from(ConfigError::UnsafePath(absolute.clone()));
    assert_debug_omits_sentinel("selection error", &selection);

    let mut invalid_utf8 = SENTINEL.as_bytes().to_vec();
    invalid_utf8[0] = 0xff;
    let utf8_error = std::str::from_utf8(&invalid_utf8).expect_err("bytes remained valid UTF-8");
    let chat_template = ChatTemplateError::InvalidUtf8 {
        path: absolute.clone(),
        source: utf8_error,
    };
    assert_debug_omits_sentinel("chat-template error", &chat_template);

    let normalization = NormalizationError::ExpectedObject(absolute.clone());
    assert_debug_omits_sentinel("normalization error", &normalization);

    let view = ViewError::ExpectedObject {
        kind: DocumentKind::Config,
        path: absolute,
    };
    assert_debug_omits_sentinel("typed-view error", &view);

    let manifest_read = ManifestReadError::DuplicateObjectMember {
        path: format!("/{SENTINEL}"),
    };
    assert_debug_omits_sentinel("manifest read error", &manifest_read);

    let json_error = serde_json::from_str::<serde_json::Value>(SENTINEL)
        .expect_err("sentinel unexpectedly parsed as JSON");
    let manifest_write = ManifestWriteError::Json(json_error);
    assert_debug_omits_sentinel("manifest write error", &manifest_write);
}

#[test]
fn manifest_debug_omits_normalized_identity_and_document_digest()
-> Result<(), Box<dyn std::error::Error>> {
    let source = format!(r#"{{"model_type":"{SENTINEL}"}}"#);
    let manifest = ModelRepository::from_documents(vec![SourceDocument::parse(
        "config.json",
        source.as_bytes(),
    )?])?
    .manifest()?;
    let digest = manifest.documents()[0].sha256().to_owned();
    let manifest_debug = format!("{manifest:?}");
    let document_debug = format!("{:?}", manifest.documents()[0]);

    assert!(!manifest_debug.contains(SENTINEL));
    assert!(!manifest_debug.contains(&digest));
    assert!(!document_debug.contains(&digest));
    Ok(())
}

#[test]
fn chat_template_selection_debug_omits_standalone_and_inline_contents()
-> Result<(), Box<dyn std::error::Error>> {
    let standalone = ModelRepository::from_documents(vec![SourceDocument::parse(
        "chat_template.jinja",
        SENTINEL.as_bytes(),
    )?])?;
    let selection = standalone
        .tokenizer_chat_template()?
        .ok_or("missing standalone template selection")?;
    assert!(matches!(selection.value, ChatTemplateValue::Text(SENTINEL)));
    assert_debug_omits_sentinel("standalone template value", &selection.value);
    assert_debug_omits_sentinel("standalone template source", &selection.source);
    assert_debug_omits_sentinel("standalone template selection", &selection);

    let inline_json = format!(r#"{{"chat_template":{{"template":"{SENTINEL}"}}}}"#);
    let inline = ModelRepository::from_documents(vec![SourceDocument::parse(
        "tokenizer_config.json",
        inline_json.as_bytes(),
    )?])?;
    let selection = inline
        .tokenizer_chat_template()?
        .ok_or("missing inline template selection")?;
    assert!(matches!(selection.value, ChatTemplateValue::Inline(_)));
    assert_debug_omits_sentinel("inline template value", &selection.value);
    assert_debug_omits_sentinel("inline template selection", &selection);
    Ok(())
}

#[test]
fn json_view_and_source_field_debug_omit_source_values() -> Result<(), Box<dyn std::error::Error>> {
    let json = format!(
        r#"{{"model_type":"example","architectures":{{"secret":"{SENTINEL}"}},"future":"{SENTINEL}"}}"#
    );
    let document = SourceDocument::parse("config.json", json.as_bytes())?;
    let typed = TypedDocumentView::try_from(&document)?;
    let TypedDocumentView::Config(view) = typed else {
        return Err("expected config view".into());
    };

    assert_debug_omits_sentinel("typed document view", &typed);
    assert_debug_omits_sentinel("JSON document view", &view);
    assert_debug_omits_sentinel("invalid source field", &view.first_architecture());
    assert_debug_omits_sentinel("raw source field", &view.architectures());
    assert_debug_omits_sentinel("extra-field iterator", &view.extra());
    assert_debug_omits_sentinel("string source field", &SourceField::Value(SENTINEL));
    Ok(())
}

#[test]
fn special_token_debug_omits_token_contents_and_decoder_values()
-> Result<(), Box<dyn std::error::Error>> {
    let json = format!(
        r#"{{
            "bos_token":"{SENTINEL}",
            "eos_token":{{"content":"{SENTINEL}","special":true}},
            "additional_special_tokens":["{SENTINEL}",{{"content":"{SENTINEL}"}}],
            "added_tokens_decoder":{{"7":{{"content":"{SENTINEL}"}}}}
        }}"#
    );
    let document = SourceDocument::parse("tokenizer_config.json", json.as_bytes())?;
    let TypedDocumentView::TokenizerConfig(view) = TypedDocumentView::try_from(&document)? else {
        return Err("expected tokenizer view".into());
    };

    let SourceField::Value(bos) = view.bos_token() else {
        return Err("expected beginning-of-sequence token".into());
    };
    assert!(matches!(bos, SpecialTokenValue::String(SENTINEL)));
    assert_debug_omits_sentinel("string special token", &bos);

    let SourceField::Value(SpecialTokenValue::AddedToken(eos)) = view.eos_token() else {
        return Err("expected structured end-of-sequence token".into());
    };
    assert_debug_omits_sentinel("added-token view", &eos);
    assert_debug_omits_sentinel("added-token content field", &eos.content());

    let SourceField::Value(tokens) = view.additional_special_tokens() else {
        return Err("expected additional special tokens".into());
    };
    assert_debug_omits_sentinel("special-token iterator", &tokens);

    let SourceField::Value(decoder) = view.added_tokens_decoder() else {
        return Err("expected added-token decoder".into());
    };
    assert_debug_omits_sentinel("added-token decoder iterator", &decoder);
    Ok(())
}

#[test]
fn model_index_and_safetensors_debug_omit_nested_source_contents()
-> Result<(), Box<dyn std::error::Error>> {
    let model_index_json = format!(
        r#"{{
            "_class_name":"{SENTINEL}",
            "component":["{SENTINEL}","{SENTINEL}"],
            "future":{{"secret":"{SENTINEL}"}}
        }}"#
    );
    let model_index = SourceDocument::parse("model_index.json", model_index_json.as_bytes())?;
    let TypedDocumentView::ModelIndex(view) = TypedDocumentView::try_from(&model_index)? else {
        return Err("expected model-index view".into());
    };
    assert_debug_omits_sentinel("model-index view", &view);
    assert_debug_omits_sentinel("model-index component iterator", &view.components());
    assert_debug_omits_sentinel("model-index extra iterator", &view.extra());
    let component = view.components().next().ok_or("missing component")?;
    assert_debug_omits_sentinel("model-index component", &component);
    assert_debug_omits_sentinel("model-index component value", &component.value());

    let index_json = format!(
        r#"{{
            "metadata":{{"total_size":1,"secret":"{SENTINEL}"}},
            "weight_map":{{"{SENTINEL}":"{SENTINEL}"}}
        }}"#
    );
    let index = SourceDocument::parse("model.safetensors.index.json", index_json.as_bytes())?;
    let TypedDocumentView::SafetensorsIndex(view) = TypedDocumentView::try_from(&index)? else {
        return Err("expected safetensors-index view".into());
    };
    assert_debug_omits_sentinel("safetensors-index view", &view);
    let SourceField::Value(metadata) = view.metadata() else {
        return Err("expected safetensors metadata".into());
    };
    assert_debug_omits_sentinel("safetensors metadata view", &metadata);
    assert_debug_omits_sentinel("safetensors metadata iterator", &metadata.extra());
    let SourceField::Value(weight_map) = view.weight_map() else {
        return Err("expected safetensors weight map".into());
    };
    assert_debug_omits_sentinel("safetensors weight-map view", &weight_map);
    assert_debug_omits_sentinel("safetensors weight-map iterator", &weight_map.entries());
    Ok(())
}

#[test]
fn chat_template_view_debug_omits_exact_template_contents() -> Result<(), Box<dyn std::error::Error>>
{
    let document = SourceDocument::parse("chat_template.jinja", SENTINEL.as_bytes())?;
    let TypedDocumentView::ChatTemplate(view) = TypedDocumentView::try_from(&document)? else {
        return Err("expected chat-template view".into());
    };

    assert_debug_omits_sentinel("chat-template view", &view);
    Ok(())
}

#[test]
fn diagnostic_debug_omits_message_and_location_contents() -> Result<(), Box<dyn std::error::Error>>
{
    let repository = ModelRepository::from_documents(vec![SourceDocument::parse(
        "config.json",
        br#"{"model_type":"example","vocab_size":"invalid"}"#,
    )?])?;
    let mut diagnostic: Diagnostic = repository
        .diagnostics()
        .into_iter()
        .next()
        .ok_or("missing repository diagnostic")?;
    diagnostic.message = SENTINEL.into();
    diagnostic.json_path = Some(format!("/{SENTINEL}"));

    assert_debug_omits_sentinel("diagnostic", &diagnostic);
    Ok(())
}

#[test]
fn normalized_debug_omits_identity_component_default_and_extra_values()
-> Result<(), Box<dyn std::error::Error>> {
    let source = format!(
        r#"{{
            "_class_name":"{SENTINEL}",
            "task":"{SENTINEL}",
            "{SENTINEL}":["{SENTINEL}","{SENTINEL}"],
            "future":{{"secret":"{SENTINEL}"}}
        }}"#
    );
    let repository = ModelRepository::from_documents(vec![SourceDocument::parse(
        "model_index.json",
        source.as_bytes(),
    )?])?;
    let normalized = repository.normalized()?;

    assert_debug_omits_sentinel("normalized repository config", &normalized);
    assert_debug_omits_sentinel("architecture identifier", &normalized.architecture);
    assert_debug_omits_sentinel("task", normalized.task.as_ref().ok_or("missing task")?);
    assert_debug_omits_sentinel("component", &normalized.components[0]);
    assert_debug_omits_sentinel("applied default", &normalized.applied_defaults[0]);
    Ok(())
}

#[test]
fn repository_debug_omits_source_values_and_filesystem_root()
-> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().join(SENTINEL);
    std::fs::create_dir(&root)?;
    std::fs::write(
        root.join("config.json"),
        format!(r#"{{"model_type":"{SENTINEL}"}}"#),
    )?;
    let repository = ModelRepository::read(root)?;

    assert_debug_omits_sentinel("model repository", &repository);
    Ok(())
}
