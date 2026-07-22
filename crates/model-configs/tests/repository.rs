//! Integration tests for repository parsing and normalization.

use std::fs;

use model_configs::{ArchitectureId, DiagnosticLevel, DocumentKind, ModelRepository, TaskKind};

#[test]
fn transformers_config_preserves_source_and_unknown_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let source = b"{\n  \"architectures\": [\"Qwen3ForCausalLM\"],\n  \"model_type\": \"qwen3\",\n  \"transformers_version\": \"4.51.0\",\n  \"future_field\": 42\n}\n";
    fs::write(temp.path().join("config.json"), source)?;
    let repository = ModelRepository::read(temp.path())?;
    let document = &repository.documents()[0];
    assert_eq!(document.kind(), &DocumentKind::Config);
    assert_eq!(document.original(), source);
    let normalized = repository.normalized().ok_or("missing normalized config")?;
    assert_eq!(
        normalized.architecture,
        ArchitectureId("Qwen3ForCausalLM".into())
    );
    assert_eq!(normalized.extra["future_field"], 42);
    Ok(())
}

#[test]
fn diffusers_index_extracts_components_and_reports_applied_task_default()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::create_dir(temp.path().join("unet"))?;
    fs::write(
        temp.path().join("model_index.json"),
        r#"{"_class_name":"StableDiffusionPipeline","_diffusers_version":"0.30.0","unet":["diffusers","UNet2DConditionModel"],"custom":true}"#,
    )?;
    let repository = ModelRepository::read(temp.path())?;
    let normalized = repository.normalized().ok_or("missing normalized config")?;
    assert_eq!(normalized.task, Some(TaskKind::TextToImage));
    assert_eq!(
        normalized.components[0].path.as_deref(),
        Some(std::path::Path::new("unet"))
    );
    assert_eq!(normalized.extra["custom"], true);
    assert_eq!(normalized.applied_defaults.len(), 1);
    assert!(repository.diagnostics().is_empty());
    Ok(())
}

#[test]
fn missing_component_directory_is_diagnosed() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(
        temp.path().join("model_index.json"),
        r#"{"_class_name":"Pipeline","vae":["diffusers","AutoencoderKL"]}"#,
    )?;
    let diagnostics = ModelRepository::read(temp.path())?.diagnostics();
    assert_eq!(diagnostics[0].level, DiagnosticLevel::Error);
    assert_eq!(diagnostics[0].code, "missing_component_directory");
    Ok(())
}

#[test]
fn chat_template_is_retained_as_text() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("config.json"), r#"{"model_type":"bert"}"#)?;
    fs::write(temp.path().join("chat_template.jinja"), "{{ messages }}\n")?;
    let repository = ModelRepository::read(temp.path())?;
    let template = repository
        .documents()
        .iter()
        .find(|doc| doc.kind() == &DocumentKind::ChatTemplate)
        .ok_or("missing template")?;
    assert_eq!(template.json(), None);
    assert_eq!(template.original(), b"{{ messages }}\n");
    Ok(())
}
