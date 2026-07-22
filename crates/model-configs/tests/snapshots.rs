//! Focused snapshots for structured manifests and diagnostics.

use std::fs;

use insta::assert_snapshot;
use model_configs::ModelRepository;

#[test]
fn missing_component_diagnostic_wire_shape() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(
        temp.path().join("model_index.json"),
        r#"{"_class_name":"Pipeline","unet":["diffusers","UNet2DConditionModel"]}"#,
    )?;
    let diagnostics = ModelRepository::read(temp.path())?.diagnostics();
    let json = serde_json::to_string_pretty(&diagnostics)?;

    assert_snapshot!(json, @r###"
    [
      {
        "level": "error",
        "code": "missing_component_directory",
        "message": "component refers to missing or linked directory unet",
        "document_path": "model_index.json",
        "json_path": "/unet",
        "related_path": "unet"
      }
    ]
    "###);
    Ok(())
}

#[test]
fn normalized_manifest_wire_shape() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    fs::write(
        temp.path().join("config.json"),
        r#"{"architectures":["ExampleModel"],"model_type":"example","future":{"z":1}}"#,
    )?;
    let manifest = ModelRepository::read(temp.path())?.manifest()?;
    let projection = serde_json::json!({
        "schema_version": manifest.schema_version,
        "normalization_profile": manifest.normalization_profile,
        "document_paths": manifest.documents.iter().map(model_configs::ManifestDocument::path).collect::<Vec<_>>(),
        "normalized": manifest.normalized,
        "diagnostics": manifest.diagnostics,
    });
    let json = serde_json::to_string_pretty(&projection)?;

    assert_snapshot!(json, @r###"
    {
      "schema_version": 1,
      "normalization_profile": "dinoml-v1",
      "document_paths": [
        "config.json"
      ],
      "normalized": {
        "source_path": "config.json",
        "architecture": "ExampleModel",
        "architecture_source": "config_architectures",
        "model_type": "example",
        "transformers_version": null,
        "diffusers_version": null,
        "task": null,
        "components": [],
        "extra": {
          "future": {
            "z": 1
          }
        },
        "applied_defaults": []
      },
      "diagnostics": []
    }
    "###);
    Ok(())
}
