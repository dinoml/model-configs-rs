//! Integration tests for deterministic compatibility manifests.

use std::fs;

use model_configs::{
    CompatibilityManifest, ConfigError, DiagnosticCode, MANIFEST_SCHEMA_VERSION,
    MAX_DIAGNOSTIC_TEXT_BYTES, MAX_REPOSITORY_DIAGNOSTICS, MAX_REPOSITORY_DOCUMENTS,
    MAX_REPOSITORY_SOURCE_BYTES, MAX_SOURCE_DOCUMENT_BYTES, MAX_SOURCE_JSON_DEPTH,
    ManifestReadError, ModelRepository, NORMALIZATION_PROFILE,
};
use serde_json::json;

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

    assert_eq!(manifest.diagnostics()[0].code, DiagnosticCode::Unknown);
    assert_eq!(manifest.schema_version(), 1);
    Ok(())
}

#[test]
fn manifest_reader_accepts_safe_unknown_fields_at_every_wire_level()
-> Result<(), Box<dyn std::error::Error>> {
    let document =
        model_configs::SourceDocument::parse("config.json", br#"{"model_type":"example"}"#)?;
    let manifest = ModelRepository::from_documents(vec![document])?.manifest()?;
    let mut value: serde_json::Value = serde_json::from_str(&manifest.to_json_pretty()?)?;
    value["future_manifest"] = json!({"nested": "public"});
    value["documents"][0]["future_document"] = json!({"nested": "public"});
    value["normalized"]["future_normalized"] = json!({"nested": "public"});
    value["diagnostics"] = json!([{
        "level":"warning",
        "code":"future_diagnostic",
        "message":"future",
        "document_path":"config.json",
        "json_path":null,
        "related_path":null,
        "future_diagnostic":{"nested":"public"}
    }]);

    let decoded = CompatibilityManifest::from_json(&serde_json::to_string(&value)?)?;
    assert_eq!(decoded.diagnostics()[0].code, DiagnosticCode::Unknown);
    Ok(())
}

#[test]
fn manifest_reader_rejects_sensitive_unknown_fields_at_every_wire_level()
-> Result<(), Box<dyn std::error::Error>> {
    const TOKEN: &str = "hf_abcdefghijklmnopqrstuvwxyz123456";
    let document =
        model_configs::SourceDocument::parse("config.json", br#"{"model_type":"example"}"#)?;
    let manifest = ModelRepository::from_documents(vec![document])?.manifest()?;
    let base: serde_json::Value = serde_json::from_str(&manifest.to_json_pretty()?)?;

    let mut variants = Vec::new();
    let mut top_level = base.clone();
    top_level["future_manifest"] = json!({"shared_secret_data": TOKEN});
    variants.push(top_level);

    let mut document_level = base.clone();
    document_level["documents"][0]["future_document"] = json!({"shared_secret_data": TOKEN});
    variants.push(document_level);

    let mut normalized_level = base.clone();
    normalized_level["normalized"]["future_normalized"] = json!({"shared_secret_data": TOKEN});
    variants.push(normalized_level);

    let mut diagnostic_level = base;
    diagnostic_level["diagnostics"] = json!([{
        "level":"warning",
        "code":"future_diagnostic",
        "message":"future",
        "document_path":"config.json",
        "json_path":null,
        "related_path":null,
        "future_diagnostic":{"shared_secret_data":TOKEN}
    }]);
    variants.push(diagnostic_level);

    for value in variants {
        let error = CompatibilityManifest::from_json(&serde_json::to_string(&value)?)
            .expect_err("sensitive unknown field entered a manifest");
        assert!(matches!(error, ManifestReadError::SensitiveContent));
        assert!(!format!("{error:?}").contains(TOKEN));
    }
    Ok(())
}

#[test]
fn manifest_reader_checks_sensitive_input_before_duplicate_path_and_enum_errors() {
    const TOKEN: &str = "hf_abcdefghijklmnopqrstuvwxyz123456";
    let digest = "0000000000000000000000000000000000000000000000000000000000000000";
    let cases = [
        format!(
            r#"{{"schema_version":1,"normalization_profile":"dinoml-v1","documents":[],"normalized":null,"diagnostics":[],"hf_token":"{TOKEN}","hf_token":"second"}}"#,
        ),
        format!(
            r#"{{"schema_version":1,"normalization_profile":"dinoml-v1","documents":[{{"path":"../{TOKEN}/config.json","kind":"config","sha256":"{digest}","size":0}}],"normalized":null,"diagnostics":[]}}"#,
        ),
        format!(
            r#"{{"schema_version":1,"normalization_profile":"dinoml-v1","documents":[{{"path":"config.json","kind":"config","sha256":"{digest}","size":0}}],"normalized":{{"source_path":"config.json","architecture":"Example","architecture_source":"{TOKEN}","model_type":null,"transformers_version":null,"diffusers_version":null,"task":null,"components":[],"extra":{{}},"applied_defaults":[]}},"diagnostics":[]}}"#,
        ),
    ];

    for source in cases {
        let error = CompatibilityManifest::from_json(&source)
            .expect_err("sensitive input reached a more specific reader error");
        assert!(matches!(error, ManifestReadError::SensitiveContent));
        assert!(!format!("{error:?}").contains(TOKEN));
    }
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
fn manifest_reader_rejects_unsafe_diagnostic_paths() {
    let result = CompatibilityManifest::from_json(
        r#"{"schema_version":1,"normalization_profile":"dinoml-v1","documents":[],"normalized":null,"diagnostics":[{"level":"warning","code":"unknown","message":"x","document_path":"../config.json","json_path":null,"related_path":null}]}"#,
    );

    assert!(matches!(
        result,
        Err(ManifestReadError::InvalidDiagnosticPath { .. })
    ));
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

    assert_eq!(manifest.schema_version(), MANIFEST_SCHEMA_VERSION);
    assert_eq!(manifest.normalization_profile(), NORMALIZATION_PROFILE);
    assert_eq!(
        manifest.documents()[0].sha256(),
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
    let normalized = manifest.normalized().ok_or("missing normalized view")?;
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
fn manifest_projection_filters_compact_credentials_and_file_uris()
-> Result<(), Box<dyn std::error::Error>> {
    let document = model_configs::SourceDocument::parse(
        "config.json",
        br#"{
            "model_type":"example",
            "future":{
                "apiKey":"api-key-value",
                "accessToken":"access-token-value",
                "authToken":"auth-token-value",
                "clientSecret":"client-secret-value",
                "useAuthToken":"use-auth-token-value",
                "hfToken":"hf-token-value",
                "secret_key":"secret-key-value",
                "private_key":"private-key-value",
                "api-key":"hyphenated-api-key-value",
                "private.key":"dotted-private-key-value",
                "aws_secret_access_key":"aws-secret-value",
                "aws_access_key_id":"aws-access-value",
                "private_key_data":"private-key-data-value",
                "api_key_value":"api-key-data-value",
                "client_secret_value":"client-secret-data-value",
                "shared_secret_data":"shared-secret-data-value",
                "webhook_secret_text":"webhook-secret-text-value",
                "bos_token_data":"<s>",
                "authorizationHeader":"Authorization: Bearer hf_abcdefghijklmnopqrstuvwxyz123456",
                "presignedAws":"https://example.com/model?X-Amz-Credential=value&X-Amz-Signature=signature-value",
                "presignedAzure":"https://example.com/model?sv=1&sig=signature-value",
                "encodedQuery":"https://example.com/model?%74oken=credential-value",
                "encodedFragment":"https://example.com/model#%61ccess%5Ftoken=credential-value",
                "posixFile":"file:///home/alice/private/model",
                "windowsFile":"file:///C:/Users/Alice/private/model",
                "safe":"https://example.com/models/public"
            }
        }"#,
    )?;
    let manifest = ModelRepository::from_documents(vec![document])?.manifest()?;
    let future = manifest
        .normalized()
        .and_then(|normalized| normalized.extra.get("future"))
        .and_then(serde_json::Value::as_object)
        .ok_or("missing future manifest data")?;

    for key in [
        "apiKey",
        "accessToken",
        "authToken",
        "clientSecret",
        "useAuthToken",
        "hfToken",
        "secret_key",
        "private_key",
        "api-key",
        "private.key",
        "aws_secret_access_key",
        "aws_access_key_id",
        "private_key_data",
        "api_key_value",
        "client_secret_value",
        "shared_secret_data",
        "webhook_secret_text",
    ] {
        assert!(!future.contains_key(key), "manifest retained {key}");
    }
    assert_eq!(future.get("posixFile"), Some(&json!("<redacted>")));
    assert_eq!(future.get("windowsFile"), Some(&json!("<redacted>")));
    assert_eq!(
        future.get("authorizationHeader"),
        Some(&json!("<redacted>"))
    );
    assert_eq!(future.get("presignedAws"), Some(&json!("<redacted>")));
    assert_eq!(future.get("presignedAzure"), Some(&json!("<redacted>")));
    assert_eq!(future.get("encodedQuery"), Some(&json!("<redacted>")));
    assert_eq!(future.get("encodedFragment"), Some(&json!("<redacted>")));
    assert_eq!(
        future.get("safe"),
        Some(&json!("https://example.com/models/public"))
    );
    assert_eq!(future.get("bos_token_data"), Some(&json!("<s>")));
    Ok(())
}

#[test]
fn manifest_rejects_sensitive_document_identity_and_redacts_related_paths()
-> Result<(), Box<dyn std::error::Error>> {
    const TOKEN: &str = "hf_abcdefghijklmnopqrstuvwxyz123456";
    let sensitive_document = model_configs::SourceDocument::parse(
        format!("{TOKEN}/config.json"),
        br#"{"model_type":"example"}"#,
    )?;
    let error = ModelRepository::from_documents(vec![sensitive_document])?
        .manifest()
        .expect_err("credential-bearing path entered a manifest");
    assert!(matches!(error, ConfigError::ManifestSensitivePath));
    assert!(!format!("{error:?}").contains(TOKEN));

    let documents = vec![
        model_configs::SourceDocument::parse("config.json", br#"{"model_type":"example"}"#)?,
        model_configs::SourceDocument::parse(
            "model.safetensors.index.json",
            format!(r#"{{"weight_map":{{"tensor":"{TOKEN}/model.safetensors"}}}}"#),
        )?,
    ];
    let manifest = ModelRepository::from_documents(documents)?.manifest()?;
    let diagnostic = manifest
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.code == DiagnosticCode::MissingCheckpointShard)
        .ok_or("missing checkpoint diagnostic")?;
    assert!(diagnostic.related_path().is_none());
    assert!(!manifest.to_json_pretty()?.contains(TOKEN));
    Ok(())
}

#[test]
fn manifest_reader_rejects_sensitive_wire_content() -> Result<(), Box<dyn std::error::Error>> {
    const TOKEN: &str = "hf_abcdefghijklmnopqrstuvwxyz123456";
    let document =
        model_configs::SourceDocument::parse("config.json", br#"{"model_type":"example"}"#)?;
    let manifest = ModelRepository::from_documents(vec![document])?.manifest()?;
    let mut value: serde_json::Value = serde_json::from_str(&manifest.to_json_pretty()?)?;
    value["normalized"]["extra"]["authorization"] = json!(TOKEN);
    assert!(matches!(
        CompatibilityManifest::from_json(&serde_json::to_string(&value)?),
        Err(ManifestReadError::SensitiveContent)
    ));

    value["normalized"]["extra"] = json!({
        "safe_name": "https://example.com/model#%61ccess%5Ftoken=credential-value"
    });
    value["diagnostics"] = json!([]);
    assert!(matches!(
        CompatibilityManifest::from_json(&serde_json::to_string(&value)?),
        Err(ManifestReadError::SensitiveContent)
    ));

    value["normalized"]["extra"] = json!({"shared_secret_data": "credential-value"});
    assert!(matches!(
        CompatibilityManifest::from_json(&serde_json::to_string(&value)?),
        Err(ManifestReadError::SensitiveContent)
    ));

    value["normalized"]["extra"] = json!({});
    value["diagnostics"] = json!([{
        "level":"warning",
        "code":"unknown",
        "message":format!("Authorization: Bearer {TOKEN}"),
        "document_path":null,
        "json_path":null,
        "related_path":null
    }]);
    assert!(matches!(
        CompatibilityManifest::from_json(&serde_json::to_string(&value)?),
        Err(ManifestReadError::SensitiveContent)
    ));

    value["diagnostics"] = json!([]);
    value["normalization_profile"] = json!("future-profile-v2");
    assert!(matches!(
        CompatibilityManifest::from_json(&serde_json::to_string(&value)?),
        Err(ManifestReadError::UnsupportedNormalizationProfile)
    ));
    value["normalized"]["architecture_source"] = json!("future_source_kind");
    assert!(matches!(
        CompatibilityManifest::from_json(&serde_json::to_string(&value)?),
        Err(ManifestReadError::UnsupportedNormalizationProfile)
    ));
    value["normalization_profile"] = json!(format!("Bearer {TOKEN}"));
    assert!(matches!(
        CompatibilityManifest::from_json(&serde_json::to_string(&value)?),
        Err(ManifestReadError::SensitiveContent)
    ));
    Ok(())
}

#[test]
fn manifest_reader_enforces_document_size_and_portability_limits() {
    let digest = "0000000000000000000000000000000000000000000000000000000000000000";
    let manifest = |documents: String| {
        format!(
            r#"{{"schema_version":1,"normalization_profile":"dinoml-v1","documents":[{documents}],"normalized":null,"diagnostics":[]}}"#,
        )
    };
    let entry = |path: &str, size: u64| {
        format!(r#"{{"path":"{path}","kind":"config","sha256":"{digest}","size":{size}}}"#)
    };

    let oversized = CompatibilityManifest::from_json(&manifest(entry(
        "config.json",
        MAX_SOURCE_DOCUMENT_BYTES as u64 + 1,
    )));
    assert!(matches!(
        oversized,
        Err(ManifestReadError::DocumentSourceTooLarge { .. })
    ));

    let aggregate = (0..5)
        .map(|index| entry(&format!("component-{index}/config.json"), 64 * 1024 * 1024))
        .collect::<Vec<_>>()
        .join(",");
    assert!(matches!(
        CompatibilityManifest::from_json(&manifest(aggregate)),
        Err(ManifestReadError::AggregateDocumentBytesLimit {
            size,
            limit: MAX_REPOSITORY_SOURCE_BYTES
        }) if size > MAX_REPOSITORY_SOURCE_BYTES
    ));

    for paths in [
        ["Encoder/config.json", "encoder/config.json"],
        ["caf\u{e9}/config.json", "cafe\u{301}/config.json"],
    ] {
        let documents = paths
            .into_iter()
            .map(|path| entry(path, 0))
            .collect::<Vec<_>>()
            .join(",");
        assert!(matches!(
            CompatibilityManifest::from_json(&manifest(documents)),
            Err(ManifestReadError::NonPortableDocumentPaths)
        ));
    }
}

#[test]
fn sensitive_identity_reason_survives_diagnostic_saturation()
-> Result<(), Box<dyn std::error::Error>> {
    let mut index = serde_json::Map::new();
    index.insert(
        "_class_name".into(),
        json!("https://user:secret@example.com/model"),
    );
    for number in 0..(MAX_REPOSITORY_DIAGNOSTICS + 500) {
        index.insert(format!("invalid_{number:04}"), json!(42));
    }
    let document =
        model_configs::SourceDocument::parse("model_index.json", serde_json::to_vec(&index)?)?;
    let manifest = ModelRepository::from_documents(vec![document])?.manifest()?;

    assert!(manifest.normalized().is_none());
    assert!(
        manifest
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code == DiagnosticCode::ManifestSensitiveDataOmitted })
    );
    assert!(manifest.diagnostics().len() <= MAX_REPOSITORY_DIAGNOSTICS);
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
    assert!(manifest.normalized().is_none());
    assert!(
        manifest
            .diagnostics()
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
        .diagnostics()
        .iter()
        .filter_map(|diagnostic| diagnostic.json_path.as_deref())
        .collect::<Vec<_>>();

    assert!(pointers.contains(&"/vocab_size"));
    assert!(pointers.contains(&"/added_tokens_decoder/0"));
    assert!(!manifest.to_json_pretty()?.contains("auth_token"));
    Ok(())
}

#[test]
fn manifest_reader_detects_parent_directory_materialization_collisions() {
    let digest = "0000000000000000000000000000000000000000000000000000000000000000";
    let source = |first: &str, second: &str| {
        format!(
            r#"{{"schema_version":1,"normalization_profile":"dinoml-v1","documents":[{{"path":"{first}","kind":"config","sha256":"{digest}","size":0}},{{"path":"{second}","kind":"tokenizer_config","sha256":"{digest}","size":0}}],"normalized":null,"diagnostics":[]}}"#,
        )
    };

    for manifest in [
        source("Encoder/config.json", "encoder/tokenizer_config.json"),
        source("config.json", "config.json/tokenizer_config.json"),
    ] {
        assert!(matches!(
            CompatibilityManifest::from_json(&manifest),
            Err(ManifestReadError::NonPortableDocumentPaths)
        ));
    }
}

#[test]
fn manifest_reader_canonicalizes_all_ordered_wire_collections()
-> Result<(), Box<dyn std::error::Error>> {
    let digest = "0000000000000000000000000000000000000000000000000000000000000000";
    let source = format!(
        r#"{{
            "schema_version":1,
            "normalization_profile":"dinoml-v1",
            "documents":[
                {{"path":"z/tokenizer_config.json","kind":"tokenizer_config","sha256":"{digest}","size":0}},
                {{"path":"model_index.json","kind":"model_index","sha256":"{digest}","size":0}},
                {{"path":"a/tokenizer_config.json","kind":"tokenizer_config","sha256":"{digest}","size":0}}
            ],
            "normalized":{{
                "source_path":"model_index.json",
                "architecture":"Pipeline",
                "architecture_source":"model_index_class_name",
                "model_type":null,
                "transformers_version":null,
                "diffusers_version":null,
                "task":null,
                "components":[
                    {{"name":"z","path":"z","library":"diffusers","architecture":"Z","optional":false,"requires_code":false}},
                    {{"name":"a","path":"a","library":"diffusers","architecture":"A","optional":false,"requires_code":false}}
                ],
                "extra":{{"nested_z":{{"second":2,"first":1}},"nested_a":0}},
                "applied_defaults":[
                    {{"field":"/components/z/path","value":"z","rule":"diffusers-component-name-is-path-v1"}},
                    {{"field":"/components/a/path","value":"a","rule":"diffusers-component-name-is-path-v1"}}
                ]
            }},
            "diagnostics":[
                {{"level":"warning","code":"unknown","message":"z","document_path":"z/tokenizer_config.json","json_path":"/z","related_path":null}},
                {{"level":"info","code":"unknown","message":"a","document_path":"a/tokenizer_config.json","json_path":"/a","related_path":null}}
            ]
        }}"#,
    );
    let first = CompatibilityManifest::from_json(&source)?.to_json_pretty()?;
    let second = CompatibilityManifest::from_json(&first)?.to_json_pretty()?;

    assert_eq!(first, second);
    assert!(first.find("a/tokenizer_config.json") < first.find("model_index.json"));
    assert!(first.find("\"name\": \"a\"") < first.find("\"name\": \"z\""));
    assert!(first.find("/components/a/path") < first.find("/components/z/path"));
    assert!(first.find("\"message\": \"a\"") < first.find("\"message\": \"z\""));
    assert!(first.find("\"first\": 1") < first.find("\"second\": 2"));
    Ok(())
}

#[test]
fn generated_manifest_is_byte_stable_after_reading() -> Result<(), Box<dyn std::error::Error>> {
    let document = model_configs::SourceDocument::parse(
        "config.json",
        br#"{"model_type":"example","nested":{"z":2,"a":{"second":2,"first":1}}}"#,
    )?;
    let generated = ModelRepository::from_documents(vec![document])?
        .manifest()?
        .to_json_pretty()?;
    let decoded = CompatibilityManifest::from_json(&generated)?.to_json_pretty()?;

    assert_eq!(generated, decoded);
    assert!(generated.find("\"first\": 1") < generated.find("\"second\": 2"));
    assert!(generated.find("\"a\": {") < generated.find("\"z\": 2"));
    Ok(())
}

#[test]
fn manifest_reader_rejects_impossible_dinoml_v1_component_states()
-> Result<(), Box<dyn std::error::Error>> {
    let document = model_configs::SourceDocument::parse(
        "model_index.json",
        br#"{
            "_class_name":"Pipeline",
            "unet":["diffusers","UNet"],
            "optional":[null,null],
            "custom":[null,"Custom"],
            "bad/name":["diffusers","Bad"]
        }"#,
    )?;
    let manifest = ModelRepository::from_documents(vec![document])?.manifest()?;
    let valid: serde_json::Value = serde_json::from_str(&manifest.to_json_pretty()?)?;

    let mut wrong_path = valid.clone();
    wrong_path["normalized"]["components"][3]["path"] = json!("elsewhere");
    assert!(matches!(
        CompatibilityManifest::from_json(&serde_json::to_string(&wrong_path)?),
        Err(ManifestReadError::InvalidDinomlV1Normalization)
    ));

    let mut wrong_optional = valid.clone();
    wrong_optional["normalized"]["components"][2]["requires_code"] = json!(true);
    assert!(matches!(
        CompatibilityManifest::from_json(&serde_json::to_string(&wrong_optional)?),
        Err(ManifestReadError::InvalidDinomlV1Normalization)
    ));

    let mut wrong_default = valid.clone();
    wrong_default["normalized"]["applied_defaults"][0]["value"] = json!({"path":"custom"});
    assert!(matches!(
        CompatibilityManifest::from_json(&serde_json::to_string(&wrong_default)?),
        Err(ManifestReadError::InvalidDinomlV1Normalization)
    ));

    let mut missing_default = valid.clone();
    missing_default["normalized"]["applied_defaults"] = json!([]);
    assert!(matches!(
        CompatibilityManifest::from_json(&serde_json::to_string(&missing_default)?),
        Err(ManifestReadError::InvalidDinomlV1Normalization)
    ));

    let mut duplicate_extra = valid.clone();
    duplicate_extra["normalized"]["extra"]["unet"] = json!(["diffusers", "UNet"]);
    assert!(matches!(
        CompatibilityManifest::from_json(&serde_json::to_string(&duplicate_extra)?),
        Err(ManifestReadError::InvalidDinomlV1Normalization)
    ));

    let mut unlisted_component = valid.clone();
    unlisted_component["normalized"]["extra"]["new_component"] =
        json!(["diffusers", "NewComponent"]);
    assert!(matches!(
        CompatibilityManifest::from_json(&serde_json::to_string(&unlisted_component)?),
        Err(ManifestReadError::InvalidDinomlV1Normalization)
    ));

    let mut metadata_component = valid.clone();
    metadata_component["normalized"]["components"][3]["name"] = json!("_hidden");
    metadata_component["normalized"]["components"][3]["path"] = json!("_hidden");
    metadata_component["normalized"]["applied_defaults"][1]["field"] =
        json!("/components/_hidden/path");
    metadata_component["normalized"]["applied_defaults"][1]["value"] = json!("_hidden");
    assert!(matches!(
        CompatibilityManifest::from_json(&serde_json::to_string(&metadata_component)?),
        Err(ManifestReadError::InvalidDinomlV1Normalization)
    ));

    let mut wrong_rule = valid.clone();
    wrong_rule["normalized"]["applied_defaults"][0]["rule"] = json!("future-rule");
    assert!(matches!(
        CompatibilityManifest::from_json(&serde_json::to_string(&wrong_rule)?),
        Err(ManifestReadError::InvalidDinomlV1Normalization)
    ));

    let mut duplicate_component = valid;
    let repeated = duplicate_component["normalized"]["components"][0].clone();
    duplicate_component["normalized"]["components"]
        .as_array_mut()
        .ok_or("components is not an array")?
        .push(repeated);
    assert!(matches!(
        CompatibilityManifest::from_json(&serde_json::to_string(&duplicate_component)?),
        Err(ManifestReadError::DuplicateNormalizedComponentName)
    ));
    Ok(())
}

#[test]
fn manifest_reader_rejects_impossible_dinoml_v1_scalar_states()
-> Result<(), Box<dyn std::error::Error>> {
    let document = model_configs::SourceDocument::parse(
        "config.json",
        br#"{"model_type":"example","transformers_version":"1.0"}"#,
    )?;
    let manifest = ModelRepository::from_documents(vec![document])?.manifest()?;
    let valid: serde_json::Value = serde_json::from_str(&manifest.to_json_pretty()?)?;

    let mutations = [
        ("model_type", json!("different")),
        ("model_type", json!("")),
        ("transformers_version", json!("")),
        ("diffusers_version", json!("")),
        ("task", json!({"kind":"other","value":""})),
        ("task", json!({"kind":"other","value":"text-generation"})),
    ];
    for (field, value) in mutations {
        let mut invalid = valid.clone();
        invalid["normalized"][field] = value;
        assert!(matches!(
            CompatibilityManifest::from_json(&serde_json::to_string(&invalid)?),
            Err(ManifestReadError::InvalidDinomlV1Normalization)
        ));
    }

    for (normalized_field, normalized_value, extra_field) in [
        ("model_type", json!("example"), "model_type"),
        ("transformers_version", json!("1.0"), "transformers_version"),
        ("diffusers_version", json!("1.0"), "_diffusers_version"),
    ] {
        let mut invalid = valid.clone();
        invalid["normalized"][normalized_field] = normalized_value.clone();
        invalid["normalized"]["extra"][extra_field] = normalized_value;
        assert!(matches!(
            CompatibilityManifest::from_json(&serde_json::to_string(&invalid)?),
            Err(ManifestReadError::InvalidDinomlV1Normalization)
        ));
    }

    let mut duplicate_class_name = valid.clone();
    duplicate_class_name["normalized"]["architecture_source"] = json!("config_class_name");
    duplicate_class_name["normalized"]["extra"]["_class_name"] = json!("Example");
    assert!(matches!(
        CompatibilityManifest::from_json(&serde_json::to_string(&duplicate_class_name)?),
        Err(ManifestReadError::InvalidDinomlV1Normalization)
    ));

    let mut component_on_config = valid;
    component_on_config["normalized"]["components"] = json!([{
        "name":"unet",
        "path":"unet",
        "library":"diffusers",
        "architecture":"UNet",
        "optional":false,
        "requires_code":false
    }]);
    assert!(matches!(
        CompatibilityManifest::from_json(&serde_json::to_string(&component_on_config)?),
        Err(ManifestReadError::InvalidDinomlV1Normalization)
    ));
    Ok(())
}

#[test]
fn manifest_reader_bounds_applied_default_metadata_before_safety_scans()
-> Result<(), Box<dyn std::error::Error>> {
    let document = model_configs::SourceDocument::parse(
        "model_index.json",
        br#"{"_class_name":"Pipeline","unet":["diffusers","UNet"]}"#,
    )?;
    let manifest = ModelRepository::from_documents(vec![document])?.manifest()?;
    let valid: serde_json::Value = serde_json::from_str(&manifest.to_json_pretty()?)?;

    for field in ["field", "rule"] {
        let mut oversized = valid.clone();
        oversized["normalized"]["applied_defaults"][0][field] =
            json!("x".repeat(MAX_DIAGNOSTIC_TEXT_BYTES + 1));
        assert!(matches!(
            CompatibilityManifest::from_json(&serde_json::to_string(&oversized)?),
            Err(ManifestReadError::NormalizedTextTooLarge { .. })
        ));
    }
    Ok(())
}

#[test]
fn manifest_reader_rejects_oversized_or_malformed_diagnostic_locations() {
    let diagnostic = |message: &str, pointer: &str| {
        serde_json::json!({
            "schema_version": 1,
            "normalization_profile": "dinoml-v1",
            "documents": [],
            "normalized": null,
            "diagnostics": [{
                "level": "warning",
                "code": "unknown",
                "message": message,
                "document_path": null,
                "json_path": pointer,
                "related_path": null
            }]
        })
        .to_string()
    };

    assert!(matches!(
        CompatibilityManifest::from_json(&diagnostic(
            &"x".repeat(MAX_DIAGNOSTIC_TEXT_BYTES + 1),
            "/field"
        )),
        Err(ManifestReadError::DiagnosticTextTooLarge { .. })
    ));
    assert!(matches!(
        CompatibilityManifest::from_json(&diagnostic(
            "message",
            &format!("/{}", "x".repeat(MAX_DIAGNOSTIC_TEXT_BYTES))
        )),
        Err(ManifestReadError::DiagnosticTextTooLarge { .. })
    ));
    for pointer in ["field", "/bad~escape", "/bad~2escape"] {
        assert!(matches!(
            CompatibilityManifest::from_json(&diagnostic("message", pointer)),
            Err(ManifestReadError::InvalidDiagnosticJsonPointer { .. })
        ));
    }
}

#[test]
fn manifest_reader_enforces_diagnostic_level_and_source_coherence() {
    let digest = "0000000000000000000000000000000000000000000000000000000000000000";
    let source = |level: &str, document_path: &str| {
        format!(
            r#"{{"schema_version":1,"normalization_profile":"dinoml-v1","documents":[{{"path":"config.json","kind":"config","sha256":"{digest}","size":0}}],"normalized":null,"diagnostics":[{{"level":"{level}","code":"missing_architecture","message":"missing architecture","document_path":"{document_path}","json_path":null,"related_path":null}}]}}"#,
        )
    };

    assert!(matches!(
        CompatibilityManifest::from_json(&source("info", "config.json")),
        Err(ManifestReadError::InvalidDiagnosticLevel { .. })
    ));
    assert!(matches!(
        CompatibilityManifest::from_json(&source("error", "nested/config.json")),
        Err(ManifestReadError::MissingDiagnosticSourceDocument { .. })
    ));
}

#[test]
fn manifest_reader_enforces_normalized_source_architecture_consistency() {
    let digest = "0000000000000000000000000000000000000000000000000000000000000000";
    let source = |path: &str, kind: &str, architecture_source: &str| {
        format!(
            r#"{{"schema_version":1,"normalization_profile":"dinoml-v1","documents":[{{"path":"{path}","kind":"{kind}","sha256":"{digest}","size":0}}],"normalized":{{"source_path":"{path}","architecture":"Example","architecture_source":"{architecture_source}","model_type":null,"transformers_version":null,"diffusers_version":null,"task":null,"components":[],"extra":{{}},"applied_defaults":[]}},"diagnostics":[]}}"#,
        )
    };

    assert!(matches!(
        CompatibilityManifest::from_json(&source(
            "tokenizer_config.json",
            "tokenizer_config",
            "config_model_type"
        )),
        Err(ManifestReadError::InvalidNormalizedSourceKind)
    ));
    for (path, kind, architecture_source) in [
        ("config.json", "config", "model_index_class_name"),
        ("model_index.json", "model_index", "config_architectures"),
    ] {
        assert!(matches!(
            CompatibilityManifest::from_json(&source(path, kind, architecture_source)),
            Err(ManifestReadError::ArchitectureSourceMismatch)
        ));
    }

    assert!(matches!(
        CompatibilityManifest::from_json(&source(
            "nested/config.json",
            "config",
            "config_model_type"
        )),
        Err(ManifestReadError::InvalidNormalizedSourcePath)
    ));

    let empty_architecture = source("config.json", "config", "config_model_type")
        .replace("\"architecture\":\"Example\"", "\"architecture\":\"\"");
    assert!(matches!(
        CompatibilityManifest::from_json(&empty_architecture),
        Err(ManifestReadError::InvalidNormalizedArchitecture)
    ));

    let with_model_index = source("config.json", "config", "config_model_type").replace(
        "\"documents\":[",
        &format!(
            "\"documents\":[{{\"path\":\"model_index.json\",\"kind\":\"model_index\",\"sha256\":\"{digest}\",\"size\":0}},"
        ),
    );
    assert!(matches!(
        CompatibilityManifest::from_json(&with_model_index),
        Err(ManifestReadError::NormalizedSourcePrecedenceMismatch)
    ));
}

#[test]
fn maximum_supported_source_nesting_round_trips_through_manifest()
-> Result<(), Box<dyn std::error::Error>> {
    let array_depth = MAX_SOURCE_JSON_DEPTH - 1;
    let source = format!(
        r#"{{"model_type":"example","nested":{}null{}}}"#,
        "[".repeat(array_depth),
        "]".repeat(array_depth)
    );
    let document = model_configs::SourceDocument::parse("config.json", source)?;
    assert!(document.json().is_some());
    let manifest = ModelRepository::from_documents(vec![document])?.manifest()?;
    let encoded = manifest.to_json_pretty()?;
    let decoded = CompatibilityManifest::from_json(&encoded)?;

    assert_eq!(decoded.normalized(), manifest.normalized());
    Ok(())
}

#[test]
fn source_nesting_beyond_manifest_headroom_is_rejected() {
    let array_depth = MAX_SOURCE_JSON_DEPTH;
    let source = format!(
        r#"{{"model_type":"example","nested":{}null{}}}"#,
        "[".repeat(array_depth),
        "]".repeat(array_depth)
    );
    assert!(matches!(
        model_configs::SourceDocument::parse("config.json", source),
        Err(ConfigError::SourceJsonNestingLimit {
            depth,
            limit: MAX_SOURCE_JSON_DEPTH,
            ..
        }) if depth > MAX_SOURCE_JSON_DEPTH
    ));
}

#[test]
fn manifest_filters_suffixed_credentials_but_keeps_special_token_values()
-> Result<(), Box<dyn std::error::Error>> {
    let document = model_configs::SourceDocument::parse(
        "config.json",
        br#"{
            "model_type":"example",
            "future":{
                "webhook_secret_value":"secret",
                "shared_secret_value":"secret",
                "refresh_token_value":"credential",
                "token_value":"credential",
                "bos_token_value":"<s>",
                "additional_special_tokens_value":["<image>"]
            }
        }"#,
    )?;
    let manifest = ModelRepository::from_documents(vec![document])?.manifest()?;
    let future = manifest
        .normalized()
        .and_then(|normalized| normalized.extra.get("future"))
        .and_then(serde_json::Value::as_object)
        .ok_or("missing future manifest data")?;

    for key in [
        "webhook_secret_value",
        "shared_secret_value",
        "refresh_token_value",
        "token_value",
    ] {
        assert!(!future.contains_key(key), "manifest retained {key}");
    }
    assert_eq!(future.get("bos_token_value"), Some(&json!("<s>")));
    assert_eq!(
        future.get("additional_special_tokens_value"),
        Some(&json!(["<image>"]))
    );
    Ok(())
}
