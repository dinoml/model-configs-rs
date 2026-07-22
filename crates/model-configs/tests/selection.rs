//! Integration tests for effective-source precedence.

use std::fs;

use model_configs::{
    ChatTemplateError, ChatTemplateValue, ConfigError, DiagnosticCode, ModelRepository,
    SelectionError,
};

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

#[test]
fn component_scopes_apply_generation_precedence_independently()
-> Result<(), Box<dyn std::error::Error>> {
    let documents = vec![
        model_configs::SourceDocument::parse(
            "text_encoder/config.json",
            br#"{"model_type":"clip","temperature":0.4}"#,
        )?,
        model_configs::SourceDocument::parse(
            "text_encoder/generation_config.json",
            br#"{"temperature":0.8}"#,
        )?,
        model_configs::SourceDocument::parse(
            "decoder/config.json",
            br#"{"model_type":"example","top_p":0.9}"#,
        )?,
    ];
    let repository = ModelRepository::from_documents(documents)?;

    let dedicated = repository
        .generation_source_in("text_encoder")?
        .ok_or("missing dedicated selection")?;
    let legacy = repository
        .generation_source_in("decoder")?
        .ok_or("missing legacy selection")?;
    assert_eq!(
        dedicated.document().relative_path(),
        std::path::Path::new("text_encoder/generation_config.json")
    );
    assert_eq!(
        legacy.document().relative_path(),
        std::path::Path::new("decoder/config.json")
    );
    Ok(())
}

#[test]
fn malformed_scoped_dedicated_generation_config_blocks_legacy_fallback()
-> Result<(), Box<dyn std::error::Error>> {
    let documents = vec![
        model_configs::SourceDocument::parse(
            "decoder/config.json",
            br#"{"model_type":"example","top_p":0.9}"#,
        )?,
        model_configs::SourceDocument::parse("decoder/generation_config.json", br#"{"top_p": }"#)?,
    ];
    let repository = ModelRepository::from_documents(documents)?;
    let selection = repository
        .generation_source_in("decoder")?
        .ok_or("missing dedicated selection")?;

    assert_eq!(
        selection.document().relative_path(),
        std::path::Path::new("decoder/generation_config.json")
    );
    assert!(selection.document().json().is_none());
    Ok(())
}

#[test]
fn component_scopes_apply_chat_template_precedence_independently()
-> Result<(), Box<dyn std::error::Error>> {
    let documents = vec![
        model_configs::SourceDocument::parse("processor/chat_template.jinja", b"standalone")?,
        model_configs::SourceDocument::parse(
            "processor/processor_config.json",
            br#"{"chat_template":"inline processor"}"#,
        )?,
        model_configs::SourceDocument::parse(
            "tokenizer/tokenizer_config.json",
            br#"{"chat_template":"inline tokenizer"}"#,
        )?,
    ];
    let repository = ModelRepository::from_documents(documents)?;

    let processor = repository
        .processor_chat_template_in("processor")?
        .ok_or("missing processor template")?;
    let tokenizer = repository
        .tokenizer_chat_template_in("tokenizer")?
        .ok_or("missing tokenizer template")?;
    assert!(matches!(
        processor.value,
        ChatTemplateValue::Text("standalone")
    ));
    assert!(matches!(
        tokenizer.value,
        ChatTemplateValue::Inline(value) if value == "inline tokenizer"
    ));
    assert_eq!(
        processor.source.document().relative_path(),
        std::path::Path::new("processor/chat_template.jinja")
    );
    assert_eq!(
        tokenizer.source.document().relative_path(),
        std::path::Path::new("tokenizer/tokenizer_config.json")
    );
    Ok(())
}

#[test]
fn invalid_scopes_and_ambiguous_legacy_generation_are_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let document = model_configs::SourceDocument::parse(
        "component/config.json",
        br#"{"top_p":0.1,"top_p":0.9}"#,
    )?;
    let repository = ModelRepository::from_documents(vec![document])?;

    assert!(repository.generation_source_in("component")?.is_none());
    assert!(matches!(
        repository.generation_source_in("../escape"),
        Err(ConfigError::UnsafePath(_))
    ));
    assert!(matches!(
        repository.tokenizer_chat_template_in("../escape"),
        Err(SelectionError::Config(ConfigError::UnsafePath(_)))
    ));
    Ok(())
}
