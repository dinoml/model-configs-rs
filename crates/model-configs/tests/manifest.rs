//! Integration tests for deterministic compatibility manifests.

use std::fs;

use model_configs::{
    CompatibilityManifest, DiagnosticCode, MANIFEST_SCHEMA_VERSION, ManifestReadError,
    ModelRepository, NORMALIZATION_PROFILE,
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
