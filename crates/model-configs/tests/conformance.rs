//! Offline conformance tests over curated real-world configuration fixtures.

use std::path::PathBuf;

use model_configs::{
    ArchitectureId, DiagnosticCode, ModelRepository, SourceField, SpecialTokenValue,
    TypedDocumentView,
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn qwen3_fixture_normalizes_transformers_identity() -> Result<(), Box<dyn std::error::Error>> {
    let normalized = ModelRepository::read(fixture("qwen3-tiny"))?.normalized()?;

    assert_eq!(
        normalized.architecture,
        ArchitectureId::new("Qwen3ForCausalLM")
    );
    assert_eq!(normalized.model_type.as_deref(), Some("qwen3"));
    Ok(())
}

#[test]
fn flux_fixture_normalizes_all_pipeline_components() -> Result<(), Box<dyn std::error::Error>> {
    let normalized = ModelRepository::read(fixture("flux-schnell"))?.normalized()?;

    assert_eq!(normalized.architecture, ArchitectureId::new("FluxPipeline"));
    assert_eq!(normalized.components.len(), 7);
    assert_eq!(normalized.applied_defaults.len(), 7);
    Ok(())
}

#[test]
fn incomplete_flux_fixture_reports_missing_component_directories()
-> Result<(), Box<dyn std::error::Error>> {
    let diagnostics = ModelRepository::read(fixture("flux-schnell"))?.diagnostics();
    let missing = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == DiagnosticCode::MissingComponentDirectory)
        .count();

    assert_eq!(missing, 6);
    Ok(())
}

#[test]
fn representative_remaining_formats_have_typed_views() -> Result<(), Box<dyn std::error::Error>> {
    let repository = ModelRepository::read(fixture("real-formats"))?;
    let mut seen = Vec::new();
    for document in repository.documents() {
        match document.typed_view()? {
            TypedDocumentView::AdapterConfig(view) => {
                assert_eq!(view.peft_type(), SourceField::Value("LORA"));
                seen.push("adapter");
            }
            TypedDocumentView::ProcessorConfig(view) => {
                assert_eq!(view.processor_class(), SourceField::Value("Emu3Processor"));
                seen.push("processor");
            }
            TypedDocumentView::PreprocessorConfig(view) => {
                assert!(matches!(view.size(), SourceField::Value(_)));
                seen.push("preprocessor");
            }
            TypedDocumentView::QuantizationConfig(view) => {
                assert_eq!(view.bits(), SourceField::Value(4));
                seen.push("quantization");
            }
            TypedDocumentView::ChatTemplate(view) => {
                assert!(view.text()?.contains("add_generation_prompt"));
                seen.push("chat_template");
            }
            TypedDocumentView::SafetensorsIndex(view) => {
                let SourceField::Value(weight_map) = view.weight_map() else {
                    return Err("missing fixture weight map".into());
                };
                assert_eq!(weight_map.entries().count(), 18);
                seen.push("safetensors_index");
            }
            TypedDocumentView::TokenizerConfig(view) => {
                assert!(matches!(view.model_max_length(), SourceField::Value(_)));
                seen.push("tokenizer");
            }
            TypedDocumentView::SpecialTokensMap(view) => {
                assert!(matches!(
                    view.pad_token(),
                    SourceField::Value(SpecialTokenValue::AddedToken(token))
                        if token.content() == SourceField::Value("<|endoftext|>")
                ));
                seen.push("special_tokens");
            }
            _ => return Err("unexpected fixture document kind".into()),
        }
    }
    seen.sort_unstable();

    assert_eq!(
        seen,
        [
            "adapter",
            "chat_template",
            "preprocessor",
            "processor",
            "quantization",
            "safetensors_index",
            "special_tokens",
            "tokenizer",
        ]
    );
    Ok(())
}
