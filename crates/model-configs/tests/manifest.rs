//! Integration tests for deterministic compatibility manifests.

use std::fs;

use model_configs::{
    CompatibilityManifest, DiagnosticCode, MANIFEST_SCHEMA_VERSION, MAX_REPOSITORY_DIAGNOSTICS,
    MAX_REPOSITORY_DOCUMENTS, ManifestReadError, ModelRepository, NORMALIZATION_PROFILE,
};

#[test]
fn manifest_is_stable_across_file_creation_order() -> Result<(), Box<dyn std::error::Error>> {
    let first = tempfile::tempdir()?;
    let second = tempfile::tempdir()?;
    fs::write(
        first.path().join("tokenizer_config.json"),
        r#"{"tokenizer_class":"BertTokenizer"}"#,
    )?;
    fs::write(first.path().join("config.json"), r#"{"model_type":"bert"}"#)?;
    fs::write(
        second.path().join("config.json"),
        r#"{"model_type":"bert"}"#,
    )?;
    fs::write(
        second.path().join("tokenizer_config.json"),
        r#"{"tokenizer_class":"BertTokenizer"}"#,
    )?;
    let first_json = ModelRepository::read(first.path())?
        .manifest()?
        .to_json_pretty()?;
    let second_json = ModelRepository::read(second.path())?
        .manifest()?
        .to_json_pretty()?;

    assert_eq!(first_json, second_json);
    Ok(())
}

#[test]
fn manifest_reader_rejects_unknown_schema_versions() {
    let result = CompatibilityManifest::from_json(
        r#"{"schema_version":2,"normalization_profile":"dinoml-v1","documents":[],"normalized":null,"diagnostics":[]}"#,
    );

    assert!(matches!(
        result,
        Err(ManifestReadError::UnsupportedSchemaVersion { found: 2 })
    ));
}

#[test]
fn manifest_reader_rejects_duplicate_schema_and_document_members() {
    let duplicate_schema = CompatibilityManifest::from_json(
        r#"{"schema_version":2,"schema_version":1,"normalization_profile":"dinoml-v1","documents":[],"normalized":null,"diagnostics":[]}"#,
    );
    let duplicate_path = CompatibilityManifest::from_json(
        r#"{"schema_version":1,"normalization_profile":"dinoml-v1","documents":[{"path":"config.json","path":"other/config.json","kind":"config","sha256":"0000000000000000000000000000000000000000000000000000000000000000","size":0}],"normalized":null,"diagnostics":[]}"#,
    );

    assert!(matches!(
        duplicate_schema,
        Err(ManifestReadError::DuplicateObjectMember { .. })
    ));
    assert!(matches!(
        duplicate_path,
        Err(ManifestReadError::DuplicateObjectMember { .. })
    ));
}

#[test]
fn manifest_reader_ignores_unknown_fields_and_codes() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"{
        "schema_version": 1,
        "normalization_profile": "dinoml-v1",
        "documents": [],
        "normalized": null,
        "diagnostics": [{
            "level": "warning",
            "code": "future_diagnostic",
            "message": "future",
            "document_path": null,
            "json_path": null,
            "related_path": null,
            "future_location": 7
        }],
        "future_manifest_field": true
    }"#;
    let manifest = CompatibilityManifest::from_json(source)?;

    assert_eq!(manifest.diagnostics[0].code, DiagnosticCode::Unknown);
    assert_eq!(manifest.schema_version, 1);
    Ok(())
}

#[test]
fn manifest_reader_rejects_non_portable_document_paths() {
    let result = CompatibilityManifest::from_json(
        r#"{"schema_version":1,"normalization_profile":"dinoml-v1","documents":[{"path":"../config.json","kind":"config","sha256":"00","size":0}],"normalized":null,"diagnostics":[]}"#,
    );

    assert!(matches!(
        result,
        Err(ManifestReadError::UnsafeDocumentPath { .. })
    ));
}

#[test]
fn manifest_reader_rejects_invalid_document_digests() {
    let result = CompatibilityManifest::from_json(
        r#"{"schema_version":1,"normalization_profile":"dinoml-v1","documents":[{"path":"config.json","kind":"config","sha256":"ABC","size":0}],"normalized":null,"diagnostics":[]}"#,
    );

    assert!(matches!(
        result,
        Err(ManifestReadError::InvalidDocumentDigest { .. })
    ));
}

#[test]
fn manifest_reader_rejects_duplicate_paths_and_kind_mismatches() {
    let digest = "0000000000000000000000000000000000000000000000000000000000000000";
    let duplicate = CompatibilityManifest::from_json(&format!(
        r#"{{"schema_version":1,"normalization_profile":"dinoml-v1","documents":[{{"path":"config.json","kind":"config","sha256":"{digest}","size":0}},{{"path":"config.json","kind":"config","sha256":"{digest}","size":0}}],"normalized":null,"diagnostics":[]}}"#,
    ));
    let mismatch = CompatibilityManifest::from_json(&format!(
        r#"{{"schema_version":1,"normalization_profile":"dinoml-v1","documents":[{{"path":"config.json","kind":"adapter_config","sha256":"{digest}","size":0}}],"normalized":null,"diagnostics":[]}}"#,
    ));
    let unsupported = CompatibilityManifest::from_json(&format!(
        r#"{{"schema_version":1,"normalization_profile":"dinoml-v1","documents":[{{"path":"README.md","kind":"config","sha256":"{digest}","size":0}}],"normalized":null,"diagnostics":[]}}"#,
    ));

    assert!(matches!(
        duplicate,
        Err(ManifestReadError::DuplicateDocumentPath { .. })
    ));
    assert!(matches!(
        mismatch,
        Err(ManifestReadError::DocumentKindMismatch { .. })
    ));
    assert!(matches!(
        unsupported,
        Err(ManifestReadError::UnsupportedDocumentPath { .. })
    ));
}

#[test]
fn manifest_reader_bounds_document_and_diagnostic_arrays() {
    let document = r#"{"path":"config.json","kind":"config","sha256":"0000000000000000000000000000000000000000000000000000000000000000","size":0}"#;
    let documents = std::iter::repeat_n(document, MAX_REPOSITORY_DOCUMENTS + 1)
        .collect::<Vec<_>>()
        .join(",");
    let document_result = CompatibilityManifest::from_json(&format!(
        r#"{{"schema_version":1,"normalization_profile":"dinoml-v1","documents":[{documents}],"normalized":null,"diagnostics":[]}}"#,
    ));

    let diagnostic = r#"{"level":"warning","code":"unknown","message":"x","document_path":null,"json_path":null,"related_path":null}"#;
    let diagnostics = std::iter::repeat_n(diagnostic, MAX_REPOSITORY_DIAGNOSTICS + 1)
        .collect::<Vec<_>>()
        .join(",");
    let diagnostic_result = CompatibilityManifest::from_json(&format!(
        r#"{{"schema_version":1,"normalization_profile":"dinoml-v1","documents":[],"normalized":null,"diagnostics":[{diagnostics}]}}"#,
    ));

    assert!(matches!(
        document_result,
        Err(ManifestReadError::DocumentLimit { .. })
    ));
    assert!(matches!(
        diagnostic_result,
        Err(ManifestReadError::DiagnosticLimit { .. })
    ));
}

#[test]
fn path_bearing_public_values_reject_unsafe_deserialization() {
    let diagnostic = serde_json::from_str::<model_configs::Diagnostic>(
        r#"{"level":"warning","code":"unknown","message":"x","document_path":"../config.json","json_path":null,"related_path":null}"#,
    );
    let component = serde_json::from_str::<model_configs::ComponentReference>(
        r#"{"name":"x","path":"../x","library":null,"architecture":null,"optional":false,"requires_code":false}"#,
    );
    let normalized = serde_json::from_str::<model_configs::ModelRepositoryConfig>(
        r#"{"source_path":"../config.json","architecture":"Example","architecture_source":"config_model_type","model_type":null,"transformers_version":null,"diffusers_version":null,"task":null,"components":[],"extra":{},"applied_defaults":[]}"#,
    );

    assert!(diagnostic.is_err());
    assert!(component.is_err());
    assert!(normalized.is_err());
}

#[test]
fn manifest_carries_schema_version_and_exact_source_fingerprint()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(
        temp.path().join("config.json"),
        b"{\"model_type\":\"bert\"}\n",
    )?;
    let manifest = ModelRepository::read(temp.path())?.manifest()?;

    assert_eq!(manifest.schema_version, MANIFEST_SCHEMA_VERSION);
    assert_eq!(manifest.normalization_profile, NORMALIZATION_PROFILE);
    assert_eq!(
        manifest.documents[0].sha256(),
        "643e010a3d5314680744b54eb3389b403dc85a434126e0f9bd7ea0f73e6aabcd"
    );
    Ok(())
}

#[test]
fn manifest_projection_omits_source_only_secrets_paths_and_templates()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(
        temp.path().join("config.json"),
        r#"{
            "model_type":"hf_transformers",
            "_name_or_path":"C:\\Users\\alice\\.cache\\huggingface\\model",
            "token":"hf_abcdefghijklmnopqrstuvwxyz123456",
            "chat_template":"{{ secret_template_marker }}",
            "future":{
                "posix":"/home/alice/.cache/huggingface/model",
                "endpoint":"https://user:secret@host/model",
                "safe":"hf_transformers"
            }
        }"#,
    )?;
    let repository = ModelRepository::read(temp.path())?;
    let source_normalized = repository.normalized()?;
    assert!(source_normalized.extra.contains_key("_name_or_path"));
    assert!(source_normalized.extra.contains_key("chat_template"));
    assert!(source_normalized.extra.contains_key("token"));

    let manifest = repository.manifest()?;
    let normalized = manifest
        .normalized
        .as_ref()
        .ok_or("missing normalized view")?;
    assert!(!normalized.extra.contains_key("_name_or_path"));
    assert!(!normalized.extra.contains_key("chat_template"));
    assert!(!normalized.extra.contains_key("token"));
    assert_eq!(normalized.extra["future"]["posix"], "<redacted>");
    assert_eq!(normalized.extra["future"]["endpoint"], "<redacted>");
    assert_eq!(normalized.extra["future"]["safe"], "hf_transformers");
    let json = manifest.to_json_pretty()?;
    for secret in [
        "secret_template_marker",
        "user:secret",
        "Users\\\\alice",
        "/home/alice",
        "hf_abcdefghijklmnopqrstuvwxyz123456",
    ] {
        assert!(!json.contains(secret), "manifest leaked {secret}");
    }
    Ok(())
}

#[test]
fn sensitive_identity_omission_is_explicitly_diagnosed() -> Result<(), Box<dyn std::error::Error>> {
    let document = model_configs::SourceDocument::parse(
        "config.json",
        br#"{"model_type":"https://user:secret@host/model"}"#,
    )?;
    let repository = ModelRepository::from_documents(vec![document])?;
    assert!(repository.normalized().is_ok());

    let manifest = repository.manifest()?;
    assert!(manifest.normalized.is_none());
    assert!(
        manifest
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == DiagnosticCode::ManifestSensitiveDataOmitted })
    );
    assert!(!manifest.to_json_pretty()?.contains("user:secret"));
    Ok(())
}

#[test]
fn manifest_preserves_ordinary_json_pointers_and_omits_sensitive_tokens()
-> Result<(), Box<dyn std::error::Error>> {
    let documents = vec![
        model_configs::SourceDocument::parse(
            "config.json",
            br#"{"model_type":"example","vocab_size":"bad"}"#,
        )?,
        model_configs::SourceDocument::parse(
            "tokenizer_config.json",
            br#"{"added_tokens_decoder":{"0":false,"auth_token":42}}"#,
        )?,
    ];
    let manifest = ModelRepository::from_documents(documents)?.manifest()?;
    let pointers = manifest
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.json_path.as_deref())
        .collect::<Vec<_>>();

    assert!(pointers.contains(&"/vocab_size"));
    assert!(pointers.contains(&"/added_tokens_decoder/0"));
    assert!(!manifest.to_json_pretty()?.contains("auth_token"));
    Ok(())
}
