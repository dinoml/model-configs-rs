//! Integration tests for internal-reference validation.

use std::fs;

use model_configs::{
    DiagnosticCode, MAX_DIAGNOSTIC_TEXT_BYTES, MAX_REPOSITORY_DIAGNOSTICS, ModelRepository,
    RepositoryInventory, SourceDocument,
};

#[test]
fn safetensors_index_reports_each_missing_unique_shard() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("config.json"), r#"{"model_type":"bert"}"#)?;
    fs::write(
        temp.path().join("model.safetensors.index.json"),
        r#"{"metadata":{"total_size":12},"weight_map":{"a":"model-1.safetensors","b":"model-1.safetensors","c":"model-2.safetensors"}}"#,
    )?;
    let diagnostics = ModelRepository::read(temp.path())?.diagnostics();
    let missing = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == DiagnosticCode::MissingCheckpointShard)
        .count();

    assert_eq!(missing, 2);
    Ok(())
}

#[test]
fn safetensors_index_rejects_parent_traversal() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("config.json"), r#"{"model_type":"bert"}"#)?;
    fs::write(
        temp.path().join("model.safetensors.index.json"),
        r#"{"weight_map":{"a":"../outside.safetensors"}}"#,
    )?;
    let diagnostics = ModelRepository::read(temp.path())?.diagnostics();

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::UnsafeCheckpointShardPath)
    );
    Ok(())
}

#[test]
fn local_adapter_base_path_must_resolve_inside_repository() -> Result<(), Box<dyn std::error::Error>>
{
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("config.json"), r#"{"model_type":"bert"}"#)?;
    fs::write(
        temp.path().join("adapter_config.json"),
        r#"{"base_model_name_or_path":"./missing","peft_type":"LORA"}"#,
    )?;
    let diagnostics = ModelRepository::read(temp.path())?.diagnostics();

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::MissingAdapterBasePath)
    );
    Ok(())
}

#[test]
fn hub_adapter_base_id_is_not_mistaken_for_local_path() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("config.json"), r#"{"model_type":"bert"}"#)?;
    fs::write(
        temp.path().join("adapter_config.json"),
        r#"{"base_model_name_or_path":"org/model","peft_type":"LORA"}"#,
    )?;
    let diagnostics = ModelRepository::read(temp.path())?.diagnostics();

    assert!(!diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.code,
            DiagnosticCode::UnsafeAdapterBasePath | DiagnosticCode::MissingAdapterBasePath
        )
    }));
    Ok(())
}

#[test]
fn external_adapter_url_with_credentials_stays_opaque_and_out_of_manifest_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("config.json"), r#"{"model_type":"bert"}"#)?;
    fs::write(
        temp.path().join("adapter_config.json"),
        r#"{"base_model_name_or_path":"https://user:secret@host/model","peft_type":"LORA"}"#,
    )?;
    let repository = ModelRepository::read(temp.path())?;

    assert!(!repository.diagnostics().iter().any(|diagnostic| {
        matches!(
            diagnostic.code,
            DiagnosticCode::UnsafeAdapterBasePath | DiagnosticCode::MissingAdapterBasePath
        )
    }));
    let manifest = repository.manifest()?.to_json_pretty()?;
    assert!(!manifest.contains("user:secret"));
    assert!(!manifest.contains("https://"));
    Ok(())
}

#[test]
fn diagnostic_count_and_pointer_retention_are_bounded_for_long_shared_prefixes()
-> Result<(), Box<dyn std::error::Error>> {
    let long_key = "k".repeat(1024 * 1024);
    let locators = std::iter::repeat_n(r#"{"auto_map":"x"}"#, MAX_REPOSITORY_DIAGNOSTICS + 8)
        .collect::<Vec<_>>()
        .join(",");
    let source = format!(r#"{{"model_type":"bert","{long_key}":[{locators}]}}"#);
    let document = SourceDocument::parse("config.json", source.as_bytes())?;
    let repository = ModelRepository::from_documents(vec![document])?;
    let diagnostics = repository.diagnostics();

    assert_eq!(diagnostics.len(), MAX_REPOSITORY_DIAGNOSTICS);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::DiagnosticLimitReached)
    );
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic
            .json_path
            .as_ref()
            .is_none_or(|path| path.len() <= MAX_DIAGNOSTIC_TEXT_BYTES)
    }));
    Ok(())
}

#[test]
fn processor_declared_tokenizer_relationship_is_checked() -> Result<(), Box<dyn std::error::Error>>
{
    let temp = tempfile::tempdir()?;
    fs::write(
        temp.path().join("config.json"),
        r#"{"model_type":"vision-text"}"#,
    )?;
    fs::write(
        temp.path().join("processor_config.json"),
        r#"{"processor_class":"ExampleProcessor","tokenizer_class":"ExampleTokenizer"}"#,
    )?;
    let diagnostics = ModelRepository::read(temp.path())?.diagnostics();

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::MissingTokenizerConfig)
    );
    Ok(())
}

#[test]
fn null_and_empty_processor_fields_do_not_declare_companion_relationships()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("config.json"), r#"{"model_type":"test"}"#)?;
    fs::write(
        temp.path().join("processor_config.json"),
        r#"{"tokenizer":{},"tokenizer_class":null,"image_processor_type":"","feature_extractor":null}"#,
    )?;
    let diagnostics = ModelRepository::read(temp.path())?.diagnostics();

    assert!(!diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.code,
            DiagnosticCode::MissingTokenizerConfig | DiagnosticCode::MissingPreprocessorConfig
        )
    }));
    Ok(())
}

#[test]
fn logical_inventory_validates_shards_without_filesystem_access()
-> Result<(), Box<dyn std::error::Error>> {
    let config = SourceDocument::parse("config.json", br#"{"model_type":"bert"}"#)?;
    let index = SourceDocument::parse(
        "weights/model.safetensors.index.json",
        br#"{"weight_map":{"tensor":"model-1.safetensors"}}"#,
    )?;
    let mut inventory = RepositoryInventory::new();
    inventory.insert_file("weights/model-1.safetensors")?;
    inventory.insert_directory("weights")?;
    let repository =
        ModelRepository::from_documents_with_inventory(vec![config, index], inventory)?;

    assert!(
        !repository
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::MissingCheckpointShard)
    );
    Ok(())
}

#[test]
fn logical_inventory_comparisons_are_case_sensitive() -> Result<(), Box<dyn std::error::Error>> {
    let config = SourceDocument::parse("config.json", br#"{"model_type":"bert"}"#)?;
    let index = SourceDocument::parse(
        "model.safetensors.index.json",
        br#"{"weight_map":{"tensor":"Shard.safetensors"}}"#,
    )?;
    let mut inventory = RepositoryInventory::new();
    inventory.insert_file("shard.safetensors")?;
    let repository =
        ModelRepository::from_documents_with_inventory(vec![config, index], inventory)?;

    assert!(
        repository
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::MissingCheckpointShard)
    );
    Ok(())
}

#[test]
fn safetensors_index_diagnoses_non_string_and_empty_shard_values()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("config.json"), r#"{"model_type":"bert"}"#)?;
    fs::write(
        temp.path().join("model.safetensors.index.json"),
        r#"{"weight_map":{"a":42,"b":""}}"#,
    )?;
    let diagnostics = ModelRepository::read(temp.path())?.diagnostics();

    for pointer in ["/weight_map/a", "/weight_map/b"] {
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::InvalidDocumentShape
                && diagnostic.json_path.as_deref() == Some(pointer)
        }));
    }
    Ok(())
}

#[test]
fn chat_template_alone_is_not_a_component_configuration() -> Result<(), Box<dyn std::error::Error>>
{
    let temp = tempfile::tempdir()?;
    fs::create_dir(temp.path().join("vae"))?;
    fs::write(temp.path().join("vae/chat_template.jinja"), "{{ value }}")?;
    fs::write(
        temp.path().join("model_index.json"),
        r#"{"_class_name":"Pipeline","vae":["diffusers","AutoencoderKL"]}"#,
    )?;
    let diagnostics = ModelRepository::read(temp.path())?.diagnostics();

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::MissingComponentConfig)
    );
    Ok(())
}

#[test]
fn nested_model_index_components_resolve_from_the_declaring_directory()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(temp.path().join("config.json"), r#"{"model_type":"bert"}"#)?;
    fs::create_dir_all(temp.path().join("unet"))?;
    fs::write(
        temp.path().join("unet/config.json"),
        r#"{"model_type":"unet"}"#,
    )?;
    fs::create_dir_all(temp.path().join("pipelines/sub"))?;
    fs::write(
        temp.path().join("pipelines/sub/model_index.json"),
        r#"{"_class_name":"Pipeline","unet":["diffusers","UNet2DConditionModel"]}"#,
    )?;

    let diagnostics = ModelRepository::read(temp.path())?.diagnostics();
    let missing = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == DiagnosticCode::MissingComponentDirectory)
        .expect("the root-level unet directory must not satisfy the nested reference");

    assert_eq!(
        missing.document_path(),
        Some(std::path::Path::new("pipelines/sub/model_index.json"))
    );
    assert_eq!(
        missing.related_path(),
        Some(std::path::Path::new("pipelines/sub/unet"))
    );
    Ok(())
}

#[test]
fn unsafe_component_name_has_no_derived_path_or_default() -> Result<(), Box<dyn std::error::Error>>
{
    let document = SourceDocument::parse(
        "model_index.json",
        br#"{"_class_name":"Pipeline","../vae":["diffusers","AutoencoderKL"]}"#,
    )?;
    let repository = ModelRepository::from_documents(vec![document])?;
    let normalized = repository.normalized()?;

    assert_eq!(normalized.components.len(), 1);
    assert!(normalized.components[0].path().is_none());
    assert!(normalized.applied_defaults.is_empty());
    assert!(
        repository
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::UnsafeReferencePath)
    );
    Ok(())
}

#[test]
fn executable_metadata_is_preserved_but_reported_inert() -> Result<(), Box<dyn std::error::Error>> {
    let document = SourceDocument::parse(
        "config.json",
        br#"{"model_type":"custom","auto_map":{"AutoConfig":"repo.Config"},"trust_remote_code":true}"#,
    )?;
    let repository = ModelRepository::from_documents(vec![document])?;
    let diagnostics = repository.diagnostics();

    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == DiagnosticCode::ExecutableReferenceInert)
            .count(),
        2
    );
    Ok(())
}

#[test]
fn every_typed_format_reports_wrong_field_shapes() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let files = [
        ("config.json", r#"{"model_type":"ok","vocab_size":"bad"}"#),
        ("generation_config.json", r#"{"max_length":"bad"}"#),
        ("tokenizer_config.json", r#"{"tokenizer_class":42}"#),
        ("preprocessor_config.json", r#"{"do_resize":"bad"}"#),
        ("processor_config.json", r#"{"patch_size":false}"#),
        ("scheduler_config.json", r#"{"steps_offset":1.5}"#),
        ("adapter_config.json", r#"{"inference_mode":"bad"}"#),
        ("quantization_config.json", r#"{"bits":-4}"#),
    ];
    for (path, contents) in files {
        fs::write(temp.path().join(path), contents)?;
    }
    let diagnostics = ModelRepository::read(temp.path())?.diagnostics();
    let pointers = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == DiagnosticCode::InvalidDocumentShape)
        .filter_map(|diagnostic| diagnostic.json_path.as_deref())
        .collect::<Vec<_>>();

    for pointer in [
        "/vocab_size",
        "/max_length",
        "/tokenizer_class",
        "/do_resize",
        "/patch_size",
        "/steps_offset",
        "/inference_mode",
        "/bits",
    ] {
        assert!(
            pointers.contains(&pointer),
            "missing diagnostic for {pointer}"
        );
    }
    Ok(())
}

#[test]
fn wrong_typed_model_index_metadata_is_not_a_component() -> Result<(), Box<dyn std::error::Error>> {
    let document = SourceDocument::parse(
        "model_index.json",
        br#"{"_class_name":"Pipeline","pipeline_tag":["x","y"]}"#,
    )?;
    let repository = ModelRepository::from_documents(vec![document])?;
    let normalized = repository.normalized()?;

    assert!(normalized.components.is_empty());
    assert!(normalized.extra.contains_key("pipeline_tag"));
    assert!(repository.diagnostics().iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidDocumentShape
            && diagnostic.json_path.as_deref() == Some("/pipeline_tag")
    }));
    Ok(())
}
