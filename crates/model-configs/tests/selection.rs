//! Integration tests for effective-source precedence.

use std::fs;

use model_configs::{ChatTemplateError, ChatTemplateValue, DiagnosticCode, ModelRepository};

#[test]
fn dedicated_generation_config_blocks_legacy_merge() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(
        temp.path().join("config.json"),
        r#"{"model_type":"bert","temperature":0.5}"#,
    )?;
    fs::write(
        temp.path().join("generation_config.json"),
        r#"{"top_p":0.9}"#,
    )?;
    let repository = ModelRepository::read(temp.path())?;
    let selection = repository.generation_source().ok_or("missing selection")?;

    assert_eq!(
        selection.document().relative_path(),
        std::path::Path::new("generation_config.json")
    );
    Ok(())
}

#[test]
fn standalone_chat_template_wins_over_inline_template() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("config.json"), r#"{"model_type":"bert"}"#)?;
    fs::write(
        temp.path().join("tokenizer_config.json"),
        r#"{"chat_template":"inline"}"#,
    )?;
    fs::write(temp.path().join("chat_template.jinja"), "standalone")?;
    let repository = ModelRepository::read(temp.path())?;
    let selection = repository
        .tokenizer_chat_template()?
        .ok_or("missing template")?;

    assert!(matches!(
        selection.value,
        ChatTemplateValue::Text("standalone")
    ));
    assert_eq!(selection.source.json_pointer(), None);
    Ok(())
}

#[test]
fn inline_chat_template_preserves_structured_value() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("config.json"), r#"{"model_type":"bert"}"#)?;
    fs::write(
        temp.path().join("processor_config.json"),
        r#"{"chat_template":[{"name":"default","template":"{{ messages }}"}]}"#,
    )?;
    let repository = ModelRepository::read(temp.path())?;
    let selection = repository
        .processor_chat_template()?
        .ok_or("missing template")?;

    assert!(matches!(selection.value, ChatTemplateValue::Inline(value) if value.is_array()));
    assert_eq!(selection.source.json_pointer(), Some("/chat_template"));
    Ok(())
}

#[test]
fn invalid_inline_chat_template_is_not_selected() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(
        temp.path().join("tokenizer_config.json"),
        r#"{"chat_template":42}"#,
    )?;
    let repository = ModelRepository::read(temp.path())?;

    assert!(repository.tokenizer_chat_template()?.is_none());
    assert!(repository.diagnostics().iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidDocumentShape
            && diagnostic.json_path.as_deref() == Some("/chat_template")
    }));
    Ok(())
}

#[test]
fn duplicate_inline_chat_template_is_not_selected() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(
        temp.path().join("processor_config.json"),
        r#"{"chat_template":"first","chat_template":"second"}"#,
    )?;
    let repository = ModelRepository::read(temp.path())?;

    assert!(repository.processor_chat_template()?.is_none());
    assert!(
        repository
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::DuplicateJsonKey)
    );
    Ok(())
}

#[test]
fn invalid_standalone_template_blocks_inline_fallback() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(
        temp.path().join("tokenizer_config.json"),
        r#"{"chat_template":"inline"}"#,
    )?;
    fs::write(temp.path().join("chat_template.jinja"), b"invalid\xff")?;
    let repository = ModelRepository::read(temp.path())?;

    assert!(matches!(
        repository.tokenizer_chat_template(),
        Err(ChatTemplateError::InvalidUtf8 { .. })
    ));
    assert!(
        repository
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::InvalidTextEncoding)
    );
    Ok(())
}
