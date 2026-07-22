//! Integration tests for repository parsing and normalization.

use std::fs;
use std::path::Path;

use model_configs::{
    ArchitectureId, ArchitectureSource, DiagnosticCode, DiagnosticLevel, DocumentKind,
    ModelRepository, NormalizationError, SourceDocument, ViewError,
};

#[test]
fn source_document_preserves_exact_bytes_and_unknown_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let source = b"{\n  \"architectures\": [\"Qwen3ForCausalLM\"],\n  \"future_field\": 42\n}\n";
    let document = SourceDocument::parse("config.json", source)?;

    assert_eq!(document.original(), source);
    assert_eq!(
        document.json().and_then(|json| json.get("future_field")),
        Some(&42.into())
    );
    Ok(())
}

#[test]
fn transformers_config_normalizes_identity_without_consuming_unknown_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(
        temp.path().join("config.json"),
        b"{\n  \"architectures\": [\"Qwen3ForCausalLM\"],\n  \"model_type\": \"qwen3\",\n  \"transformers_version\": \"4.51.0\",\n  \"future_field\": 42\n}\n",
    )?;
    let normalized = ModelRepository::read(temp.path())?.normalized()?;

    assert_eq!(
        normalized.architecture,
        ArchitectureId::new("Qwen3ForCausalLM")
    );
    assert_eq!(normalized.extra["future_field"], 42);
    Ok(())
}

#[test]
fn model_index_is_strictly_authoritative_over_root_config() -> Result<(), Box<dyn std::error::Error>>
{
    let config = SourceDocument::parse("config.json", br#"{"model_type":"fallback"}"#)?;
    let invalid_index = SourceDocument::parse("model_index.json", br#"{"task":"text-to-image"}"#)?;
    let repository = ModelRepository::from_documents(vec![config.clone(), invalid_index])?;

    assert!(matches!(
        repository.normalized(),
        Err(NormalizationError::MissingArchitecture(path)) if path == Path::new("model_index.json")
    ));

    let malformed_index = SourceDocument::parse("model_index.json", br#"{"_class_name":"broken""#)?;
    let repository = ModelRepository::from_documents(vec![config, malformed_index])?;
    assert!(matches!(
        repository.normalized(),
        Err(NormalizationError::ExpectedObject(path)) if path == Path::new("model_index.json")
    ));
    Ok(())
}

#[test]
fn config_architecture_precedence_is_architectures_then_class_then_model_type()
-> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        (
            br#"{"architectures":["Primary"],"_class_name":"Secondary","model_type":"fallback"}"#
                .as_slice(),
            "Primary",
            ArchitectureSource::ConfigArchitectures,
        ),
        (
            br#"{"_class_name":"Secondary","model_type":"fallback"}"#.as_slice(),
            "Secondary",
            ArchitectureSource::ConfigClassName,
        ),
        (
            br#"{"model_type":"fallback"}"#.as_slice(),
            "fallback",
            ArchitectureSource::ConfigModelType,
        ),
    ];

    for (source, expected, expected_source) in cases {
        let document = SourceDocument::parse("config.json", source)?;
        let normalized = ModelRepository::from_documents(vec![document])?.normalized()?;
        assert_eq!(normalized.architecture.as_str(), expected);
        assert_eq!(normalized.architecture_source, expected_source);
    }
    Ok(())
}

#[test]
fn taskless_model_index_does_not_infer_a_task() -> Result<(), Box<dyn std::error::Error>> {
    let document = SourceDocument::parse(
        "model_index.json",
        br#"{"_class_name":"TextToImageLookingPipeline"}"#,
    )?;
    let normalized = ModelRepository::from_documents(vec![document])?.normalized()?;

    assert_eq!(normalized.source_path(), Path::new("model_index.json"));
    assert!(normalized.task.is_none());
    Ok(())
}

#[test]
fn diffusers_index_extracts_present_and_optional_components()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::create_dir(temp.path().join("unet"))?;
    fs::write(
        temp.path().join("unet/config.json"),
        r#"{"_class_name":"UNet2DConditionModel"}"#,
    )?;
    fs::write(
        temp.path().join("model_index.json"),
        r#"{"_class_name":"StableDiffusionPipeline","_diffusers_version":"0.30.0","safety_checker":[null,null],"unet":["diffusers","UNet2DConditionModel"],"custom":true}"#,
    )?;
    let repository = ModelRepository::read(temp.path())?;
    let normalized = repository.normalized()?;

    assert_eq!(normalized.components.len(), 2);
    assert_eq!(normalized.components[0].name, "safety_checker");
    assert!(normalized.components[0].optional);
    assert_eq!(normalized.components[1].path(), Some(Path::new("unet")));
    assert_eq!(
        normalized.applied_defaults,
        vec![model_configs::AppliedDefault {
            field: "/components/unet/path".into(),
            value: serde_json::json!("unet"),
            rule: "diffusers-component-name-is-path-v1".into(),
        }]
    );
    assert!(repository.diagnostics().is_empty());
    Ok(())
}

#[test]
fn missing_component_directory_has_stable_diagnostic_context()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(
        temp.path().join("model_index.json"),
        r#"{"_class_name":"Pipeline","vae":["diffusers","AutoencoderKL"]}"#,
    )?;
    let diagnostics = ModelRepository::read(temp.path())?.diagnostics();

    assert_eq!(diagnostics[0].level, DiagnosticLevel::Error);
    assert_eq!(
        diagnostics[0].code,
        DiagnosticCode::MissingComponentDirectory
    );
    assert_eq!(
        diagnostics[0].document_path(),
        Some(Path::new("model_index.json"))
    );
    assert_eq!(diagnostics[0].json_path.as_deref(), Some("/vae"));
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
        .find(|document| document.kind() == &DocumentKind::ChatTemplate)
        .ok_or("missing template")?;

    assert_eq!(template.json(), None);
    assert_eq!(template.original(), b"{{ messages }}\n");
    Ok(())
}

#[test]
fn source_document_rejects_parent_traversal() {
    let result = SourceDocument::parse("../config.json", b"{}");

    assert!(result.is_err(), "parent traversal unexpectedly parsed");
}

#[test]
fn malformed_json_retains_bytes_and_structural_error() -> Result<(), Box<dyn std::error::Error>> {
    let source = b"{\"model_type\":\"bert\"";
    let document = SourceDocument::parse("config.json", source)?;

    assert_eq!(document.original(), source);
    assert!(document.json().is_none());
    assert!(document.json_error().is_some());
    Ok(())
}

#[test]
fn non_finite_json_tokens_are_retained_as_strict_parse_errors()
-> Result<(), Box<dyn std::error::Error>> {
    for token in ["Infinity", "-Infinity", "NaN"] {
        let source = format!(r#"{{"value":{token}}}"#).into_bytes();
        let document = SourceDocument::parse("config.json", &source)?;

        assert_eq!(document.original(), source);
        assert!(
            document.json().is_none(),
            "accepted non-standard token {token}"
        );
        assert!(document.json_error().is_some());
    }
    Ok(())
}

#[test]
fn generic_json_preserves_numbers_outside_machine_numeric_ranges()
-> Result<(), Box<dyn std::error::Error>> {
    const INTEGER: &str = "184467440737095516160000000000000000001";
    const EXPONENT: &str = "1.234567890123456789e400";
    let source = format!(
        r#"{{"model_type":"bert","future_integer":{INTEGER},"future_decimal":{EXPONENT}}}"#
    );
    let document = SourceDocument::parse("config.json", source.as_bytes())?;
    let json = document.json().ok_or("missing generic JSON projection")?;

    assert_eq!(json["future_integer"].to_string(), INTEGER);
    assert_eq!(
        json["future_decimal"].to_string(),
        "1.234567890123456789e+400"
    );
    Ok(())
}

#[test]
fn duplicate_json_keys_are_reported_without_discarding_projection()
-> Result<(), Box<dyn std::error::Error>> {
    let document = SourceDocument::parse(
        "config.json",
        br#"{"model_type":"bert","nested":{"x":1,"x":2}}"#,
    )?;

    assert_eq!(document.duplicate_keys(), &["/nested/x"]);
    assert_eq!(
        document.json().and_then(|value| value.pointer("/nested/x")),
        Some(&2.into())
    );
    Ok(())
}

#[test]
fn oversized_duplicate_key_locations_are_summarized_without_losing_ambiguity()
-> Result<(), Box<dyn std::error::Error>> {
    let long_key = "k".repeat(model_configs::MAX_DIAGNOSTIC_TEXT_BYTES + 1);
    let source = format!(r#"{{"model_type":"bert","{long_key}":{{"x":1,"x":2}}}}"#);
    let document = SourceDocument::parse("config.json", source.as_bytes())?;

    assert!(document.has_duplicate_keys());
    assert!(document.duplicate_keys().is_empty());
    assert!(document.duplicate_keys_truncated());
    let repository = ModelRepository::from_documents(vec![document])?;
    assert!(repository.normalized().is_err());
    assert!(repository.diagnostics().iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::DuplicateJsonKey && diagnostic.json_path.is_none()
    }));
    Ok(())
}

#[test]
fn duplicate_keys_block_typed_views_and_normalization() -> Result<(), Box<dyn std::error::Error>> {
    let document = SourceDocument::parse(
        "config.json",
        br#"{"model_type":"bert","model_type":"future"}"#,
    )?;
    let repository = ModelRepository::from_documents(vec![document.clone()])?;

    assert!(matches!(
        document.typed_view(),
        Err(ViewError::DuplicateKeys { .. })
    ));
    assert!(repository.normalized().is_err());
    assert_eq!(
        repository.diagnostics()[0].code,
        DiagnosticCode::DuplicateJsonKey
    );
    Ok(())
}

#[test]
fn source_document_debug_omits_exact_contents() -> Result<(), Box<dyn std::error::Error>> {
    let document = SourceDocument::parse(
        "config.json",
        br#"{"model_type":"secret-model-marker","secret-token-marker":1,"secret-token-marker":2}"#,
    )?;
    let output = format!("{document:?}");

    assert!(output.contains("config.json"));
    assert!(!output.contains("secret-model-marker"));
    assert!(!output.contains("secret-token-marker"));
    assert!(!output.contains("original"));
    Ok(())
}

#[test]
fn metadata_trees_are_excluded_from_recursive_discovery() -> Result<(), Box<dyn std::error::Error>>
{
    let temp = tempfile::tempdir()?;
    fs::create_dir_all(temp.path().join(".git/nested"))?;
    fs::create_dir_all(temp.path().join(".cache/huggingface/download"))?;
    fs::create_dir_all(temp.path().join("component"))?;
    fs::write(temp.path().join(".git/nested/config.json"), "{}")?;
    fs::write(
        temp.path().join(".cache/huggingface/download/config.json"),
        "{}",
    )?;
    fs::write(
        temp.path().join("component/config.json"),
        r#"{"model_type":"component"}"#,
    )?;
    fs::write(temp.path().join("config.json"), r#"{"model_type":"root"}"#)?;

    let repository = ModelRepository::read(temp.path())?;
    let paths = repository
        .documents()
        .iter()
        .map(SourceDocument::relative_path)
        .collect::<Vec<_>>();

    assert_eq!(
        paths,
        vec![Path::new("component/config.json"), Path::new("config.json")]
    );
    Ok(())
}

#[test]
fn logical_paths_reject_backslashes_and_windows_reserved_segments() {
    for path in [
        "component\\config.json",
        "CON/config.json",
        "bad./config.json",
    ] {
        assert!(
            SourceDocument::parse(path, b"{}").is_err(),
            "unsafe logical path unexpectedly parsed: {path}"
        );
    }
}

#[test]
fn logical_paths_reject_overlong_segments_and_total_lengths() {
    let overlong_segment = format!(
        "{}/config.json",
        "x".repeat(model_configs::MAX_REPOSITORY_PATH_SEGMENT_BYTES + 1)
    );
    assert!(SourceDocument::parse(overlong_segment, b"{}").is_err());

    let overlong_path = format!(
        "{}config.json",
        "a/".repeat(model_configs::MAX_REPOSITORY_PATH_BYTES / 2 + 1)
    );
    let mut inventory = model_configs::RepositoryInventory::new();
    assert!(inventory.insert_file(overlong_path).is_err());
}

#[test]
fn safetensors_index_classification_requires_a_nonempty_filename_prefix() {
    assert_eq!(
        DocumentKind::for_path("model.safetensors.index.json"),
        Some(DocumentKind::SafetensorsIndex)
    );
    assert_eq!(DocumentKind::for_path(".safetensors.index.json"), None);
    assert_eq!(DocumentKind::for_path("../config.json"), None);
    assert_eq!(DocumentKind::for_path("nested\\config.json"), None);
}

#[test]
fn parsed_documents_normalize_without_filesystem_access() -> Result<(), Box<dyn std::error::Error>>
{
    let document = SourceDocument::parse(
        "config.json",
        br#"{"architectures":["MemoryModel"],"model_type":"memory"}"#,
    )?;
    let repository = ModelRepository::from_documents(vec![document])?;

    assert_eq!(
        repository.normalized()?.architecture,
        ArchitectureId::new("MemoryModel")
    );
    assert_eq!(repository.root(), Path::new(""));
    Ok(())
}

#[test]
fn partially_invalid_architectures_remain_in_normalized_extra()
-> Result<(), Box<dyn std::error::Error>> {
    let document = SourceDocument::parse(
        "config.json",
        br#"{"architectures":[7,"ValidModel"],"model_type":"family"}"#,
    )?;
    let repository = ModelRepository::from_documents(vec![document])?;
    let normalized = repository.normalized()?;

    assert_eq!(normalized.architecture, ArchitectureId::new("ValidModel"));
    assert!(normalized.extra.contains_key("architectures"));
    assert!(repository.diagnostics().iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidDocumentShape
            && diagnostic.json_path.as_deref() == Some("/architectures/0")
    }));
    Ok(())
}

#[test]
fn case_colliding_logical_paths_are_not_portable() -> Result<(), Box<dyn std::error::Error>> {
    let upper = SourceDocument::parse("Encoder/config.json", br#"{"model_type":"a"}"#)?;
    let lower = SourceDocument::parse("encoder/config.json", br#"{"model_type":"b"}"#)?;

    assert!(ModelRepository::from_documents(vec![upper, lower]).is_err());
    Ok(())
}

#[test]
fn canonically_equivalent_logical_paths_are_not_jointly_portable()
-> Result<(), Box<dyn std::error::Error>> {
    let composed = SourceDocument::parse("caf\u{e9}/config.json", br#"{"model_type":"a"}"#)?;
    let decomposed = SourceDocument::parse("cafe\u{301}/config.json", br#"{"model_type":"b"}"#)?;

    assert!(ModelRepository::from_documents(vec![composed, decomposed]).is_err());
    Ok(())
}

#[test]
fn repository_paths_use_portable_utf8_order_on_every_host() -> Result<(), Box<dyn std::error::Error>>
{
    let private_use_path = "\u{e000}/config.json";
    let supplementary_path = "\u{10000}/config.json";
    let private_use = SourceDocument::parse(private_use_path, br#"{"model_type":"private"}"#)?;
    let supplementary =
        SourceDocument::parse(supplementary_path, br#"{"model_type":"supplementary"}"#)?;
    let repository = ModelRepository::from_documents(vec![supplementary, private_use])?;
    let paths = repository
        .documents()
        .iter()
        .map(SourceDocument::relative_path)
        .collect::<Vec<_>>();

    assert_eq!(
        paths,
        vec![Path::new(private_use_path), Path::new(supplementary_path)]
    );
    assert_eq!(
        repository
            .document(private_use_path)
            .and_then(SourceDocument::json)
            .and_then(|value| value.get("model_type")),
        Some(&serde_json::json!("private"))
    );
    assert!(repository.document(supplementary_path).is_some());

    let mut inventory = model_configs::RepositoryInventory::new();
    inventory.insert_file(supplementary_path)?;
    inventory.insert_file(private_use_path)?;
    let inventory_paths = inventory
        .files()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect::<Vec<_>>();
    assert_eq!(
        inventory_paths,
        vec![private_use_path.to_owned(), supplementary_path.to_owned()]
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn filesystem_scan_skips_file_and_directory_symlinks_with_relative_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    fs::write(root.path().join("config.json"), r#"{"model_type":"root"}"#)?;
    fs::write(
        outside.path().join("generation_config.json"),
        r#"{"top_p":0.9}"#,
    )?;
    fs::write(
        outside.path().join("config.json"),
        r#"{"model_type":"outside"}"#,
    )?;
    symlink(outside.path(), root.path().join("linked-component"))?;
    symlink(
        outside.path().join("generation_config.json"),
        root.path().join("generation_config.json"),
    )?;

    let repository = ModelRepository::read(root.path())?;
    assert_eq!(repository.documents().len(), 1);
    assert_eq!(
        repository.documents()[0].relative_path(),
        Path::new("config.json")
    );
    let mut skipped = repository
        .diagnostics()
        .into_iter()
        .filter(|diagnostic| diagnostic.code == DiagnosticCode::SymlinkSkipped)
        .filter_map(|diagnostic| {
            diagnostic
                .related_path()
                .map(|path| path.to_string_lossy().into_owned())
        })
        .collect::<Vec<_>>();
    skipped.sort();
    assert_eq!(
        skipped,
        vec![
            "generation_config.json".to_owned(),
            "linked-component".to_owned(),
        ]
    );
    Ok(())
}

#[test]
fn large_component_repository_keeps_reference_validation_indexed()
-> Result<(), Box<dyn std::error::Error>> {
    const COMPONENTS: usize = 2_000;
    let mut index = serde_json::Map::new();
    index.insert(
        "_class_name".into(),
        serde_json::Value::String("LargePipeline".into()),
    );
    let mut documents = Vec::with_capacity(COMPONENTS * 2 + 1);
    for number in 0..COMPONENTS {
        let name = format!("component_{number:04}");
        index.insert(name.clone(), serde_json::json!(["diffusers", "Component"]));
        documents.push(SourceDocument::parse(
            format!("{name}/config.json"),
            br#"{"_class_name":"Component"}"#,
        )?);
        documents.push(SourceDocument::parse(
            format!("unrelated_{number:04}/config.json"),
            br#"{"_class_name":"Unrelated"}"#,
        )?);
    }
    documents.push(SourceDocument::parse(
        "model_index.json",
        serde_json::to_vec(&index)?,
    )?);

    let repository = ModelRepository::from_documents(documents)?;
    assert_eq!(repository.normalized()?.components.len(), COMPONENTS);
    assert!(repository.diagnostics().is_empty());
    Ok(())
}
