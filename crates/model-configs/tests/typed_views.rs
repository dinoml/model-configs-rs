//! Integration tests for format-specific source document views.

use std::fs;

use model_configs::{
    DocumentKind, ModelRepository, SourceField, SpecialTokenValue, TypedDocumentView, ViewError,
};
use serde_json::json;

#[test]
fn config_view_exposes_typed_fields_and_only_unknown_extras()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(
        temp.path().join("config.json"),
        r#"{
            "architectures": ["ExampleForCausalLM"],
            "model_type": "example",
            "transformers_version": null,
            "is_encoder_decoder": "not-a-boolean",
            "eos_token_id": [2, 3],
            "future_field": {"kept": true}
        }"#,
    )?;
    let repository = ModelRepository::read(temp.path())?;
    let document = repository
        .documents()
        .first()
        .ok_or("missing config document")?;
    let TypedDocumentView::Config(view) = TypedDocumentView::try_from(document)? else {
        return Err("expected config view".into());
    };

    assert_eq!(
        (
            view.model_type(),
            view.transformers_version(),
            view.is_encoder_decoder(),
            view.eos_token_id(),
            view.extra().collect::<Vec<_>>(),
        ),
        (
            SourceField::Value("example"),
            SourceField::Null,
            SourceField::Invalid(&json!("not-a-boolean")),
            SourceField::Value(&json!([2, 3])),
            vec![("future_field", &json!({"kept": true}))],
        )
    );
    Ok(())
}

#[test]
fn every_supported_document_kind_has_a_typed_view() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let files = [
        ("config.json", "{}"),
        ("generation_config.json", "{}"),
        ("tokenizer_config.json", "{}"),
        ("special_tokens_map.json", "{}"),
        ("preprocessor_config.json", "{}"),
        ("processor_config.json", "{}"),
        ("scheduler_config.json", "{}"),
        ("model_index.json", "{}"),
        ("adapter_config.json", "{}"),
        ("quantization_config.json", "{}"),
        ("chat_template.jinja", "{{ messages }}"),
        ("model.safetensors.index.json", "{}"),
    ];
    for (name, contents) in files {
        fs::write(temp.path().join(name), contents)?;
    }
    let repository = ModelRepository::read(temp.path())?;
    let kinds = repository
        .documents()
        .iter()
        .map(TypedDocumentView::try_from)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|view| *view.kind())
        .collect::<Vec<_>>();

    assert_eq!(
        kinds,
        vec![
            DocumentKind::AdapterConfig,
            DocumentKind::ChatTemplate,
            DocumentKind::Config,
            DocumentKind::GenerationConfig,
            DocumentKind::SafetensorsIndex,
            DocumentKind::ModelIndex,
            DocumentKind::PreprocessorConfig,
            DocumentKind::ProcessorConfig,
            DocumentKind::QuantizationConfig,
            DocumentKind::SchedulerConfig,
            DocumentKind::SpecialTokensMap,
            DocumentKind::TokenizerConfig,
        ]
    );
    Ok(())
}

#[test]
fn special_token_values_have_typed_polymorphic_views() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(
        temp.path().join("special_tokens_map.json"),
        r#"{
            "bos_token": "<s>",
            "eos_token": {"content":"</s>","lstrip":false},
            "pad_token": null,
            "additional_special_tokens": [
                "<extra>",
                {"content":"<structured>","special":true},
                42
            ]
        }"#,
    )?;
    let repository = ModelRepository::read(temp.path())?;
    let TypedDocumentView::SpecialTokensMap(view) =
        TypedDocumentView::try_from(&repository.documents()[0])?
    else {
        return Err("expected special tokens view".into());
    };

    assert!(matches!(
        view.bos_token(),
        SourceField::Value(SpecialTokenValue::String("<s>"))
    ));
    let SourceField::Value(SpecialTokenValue::AddedToken(eos)) = view.eos_token() else {
        return Err("expected structured eos token".into());
    };
    assert_eq!(
        (eos.content(), eos.lstrip(), eos.special()),
        (
            SourceField::Value("</s>"),
            SourceField::Value(false),
            SourceField::Missing,
        )
    );
    assert_eq!(view.pad_token(), SourceField::Null);
    let SourceField::Value(tokens) = view.additional_special_tokens() else {
        return Err("expected additional special tokens".into());
    };
    let tokens = tokens.collect::<Vec<_>>();
    assert!(matches!(tokens[0], SpecialTokenValue::String("<extra>")));
    let SpecialTokenValue::AddedToken(token) = tokens[1] else {
        return Err("expected structured additional token".into());
    };
    assert_eq!(
        (token.content(), token.special()),
        (SourceField::Value("<structured>"), SourceField::Value(true))
    );
    assert!(matches!(tokens[2], SpecialTokenValue::Invalid(value) if value == &json!(42)));
    Ok(())
}

#[test]
fn tokenizer_added_token_decoder_exposes_ids_and_metadata() -> Result<(), Box<dyn std::error::Error>>
{
    let document = model_configs::SourceDocument::parse(
        "tokenizer_config.json",
        br#"{
            "added_tokens_decoder": {
                "0": "<plain>",
                "1": {
                    "content": "<typed>",
                    "single_word": true,
                    "rstrip": false,
                    "normalized": null
                },
                "2": false
            }
        }"#,
    )?;
    let TypedDocumentView::TokenizerConfig(view) = TypedDocumentView::try_from(&document)? else {
        return Err("expected tokenizer view".into());
    };
    let SourceField::Value(entries) = view.added_tokens_decoder() else {
        return Err("expected added-token decoder".into());
    };
    let entries = entries.collect::<Vec<_>>();

    assert!(matches!(
        entries[0],
        ("0", SpecialTokenValue::String("<plain>"))
    ));
    let ("1", SpecialTokenValue::AddedToken(token)) = entries[1] else {
        return Err("expected structured decoder token".into());
    };
    assert_eq!(
        (
            token.content(),
            token.single_word(),
            token.rstrip(),
            token.normalized(),
        ),
        (
            SourceField::Value("<typed>"),
            SourceField::Value(true),
            SourceField::Value(false),
            SourceField::Null,
        )
    );
    assert!(matches!(
        entries[2],
        ("2", SpecialTokenValue::Invalid(value)) if value == &json!(false)
    ));
    Ok(())
}

#[test]
fn model_index_distinguishes_references_optional_components_and_invalid_tuples()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(
        temp.path().join("model_index.json"),
        r#"{
            "_class_name": "ExamplePipeline",
            "unet": ["diffusers", "UNet2DConditionModel"],
            "safety_checker": [null, null],
            "custom_component": [null, "LocalComponent"],
            "broken": ["custom", null],
            "future_field": 7
        }"#,
    )?;
    let repository = ModelRepository::read(temp.path())?;
    let TypedDocumentView::ModelIndex(view) =
        TypedDocumentView::try_from(&repository.documents()[0])?
    else {
        return Err("expected model index view".into());
    };
    let components = view
        .components()
        .map(|component| (component.name(), component.value()))
        .collect::<Vec<_>>();

    assert_eq!(
        (components, view.extra().collect::<Vec<_>>()),
        (
            vec![
                (
                    "unet",
                    model_configs::DiffusersComponentValue::Reference {
                        library: Some("diffusers"),
                        class_name: "UNet2DConditionModel",
                    },
                ),
                (
                    "safety_checker",
                    model_configs::DiffusersComponentValue::Optional,
                ),
                (
                    "custom_component",
                    model_configs::DiffusersComponentValue::Reference {
                        library: None,
                        class_name: "LocalComponent",
                    },
                ),
                (
                    "broken",
                    model_configs::DiffusersComponentValue::Invalid(&json!(["custom", null])),
                ),
            ],
            vec![("future_field", &json!(7))],
        )
    );
    Ok(())
}

#[test]
fn safetensors_index_exposes_metadata_and_weight_map_without_copying()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(
        temp.path().join("model.safetensors.index.json"),
        r#"{
            "metadata": {"total_size": 1024, "format": "pt"},
            "weight_map": {
                "model.embed.weight": "model-00001-of-00002.safetensors",
                "bad": 42
            },
            "future_field": true
        }"#,
    )?;
    let repository = ModelRepository::read(temp.path())?;
    let TypedDocumentView::SafetensorsIndex(view) =
        TypedDocumentView::try_from(&repository.documents()[0])?
    else {
        return Err("expected safetensors index view".into());
    };
    let SourceField::Value(metadata) = view.metadata() else {
        return Err("expected metadata object".into());
    };
    let SourceField::Value(weight_map) = view.weight_map() else {
        return Err("expected weight map object".into());
    };

    assert_eq!(
        (
            metadata.total_size(),
            metadata.extra().collect::<Vec<_>>(),
            weight_map.entries().collect::<Vec<_>>(),
            view.extra().collect::<Vec<_>>(),
        ),
        (
            SourceField::Value(1024),
            vec![("format", &json!("pt"))],
            vec![
                (
                    "model.embed.weight",
                    SourceField::Value("model-00001-of-00002.safetensors"),
                ),
                ("bad", SourceField::Invalid(&json!(42))),
            ],
            vec![("future_field", &json!(true))],
        )
    );
    Ok(())
}

#[test]
fn chat_template_view_preserves_non_utf8_source_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let bytes = b"{{ messages }}\xff";
    fs::write(temp.path().join("chat_template.jinja"), bytes)?;
    let repository = ModelRepository::read(temp.path())?;
    let TypedDocumentView::ChatTemplate(view) =
        TypedDocumentView::try_from(&repository.documents()[0])?
    else {
        return Err("expected chat template view".into());
    };

    assert_eq!((view.raw(), view.text().is_err()), (bytes.as_slice(), true));
    Ok(())
}

#[test]
fn json_view_reports_a_typed_error_when_root_is_not_an_object()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("generation_config.json"), "[]")?;
    let repository = ModelRepository::read(temp.path())?;
    let result = TypedDocumentView::try_from(&repository.documents()[0]);

    assert!(matches!(
        result,
        Err(ViewError::ExpectedObject {
            kind: DocumentKind::GenerationConfig,
            ..
        })
    ));
    Ok(())
}
