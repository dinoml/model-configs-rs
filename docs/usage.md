# Usage guide

This guide uses the crate name `model-configs` in `Cargo.toml` and the Rust
module name `model_configs` in code.

## Add the dependency

The crate is currently developed from the Git repository and is not published
to crates.io:

```toml
[dependencies]
model-configs = { git = "https://github.com/dinoml/model-configs-rs", branch = "main" }
```

When working in a sibling checkout, a path dependency is usually faster:

```toml
[dependencies]
model-configs = { path = "../model-configs-rs/crates/model-configs" }
```

The minimum supported Rust version is 1.85.

## Read a materialized repository

`ModelRepository::read` recursively discovers supported files beneath a local
snapshot. Paths stored by the library remain portable repository-relative
paths; the host root is not written into compatibility manifests.

```rust
use model_configs::{DiagnosticLevel, ModelRepository};

fn inspect_snapshot() -> Result<(), Box<dyn std::error::Error>> {
    let repository = ModelRepository::read("model-snapshot")?;

    println!("recognized documents: {}", repository.documents().len());
    for diagnostic in repository.diagnostics() {
        println!(
            "{:?} {:?} {}: {}",
            diagnostic.level,
            diagnostic.code,
            diagnostic
                .document_path()
                .map_or("<repository>", |path| path.to_str().unwrap_or("<non-UTF-8>")),
            diagnostic.message,
        );

        if diagnostic.level == DiagnosticLevel::Error {
            // Apply caller policy here. Parsing deliberately keeps inspectable
            // source documents even when content diagnostics exist.
        }
    }

    Ok(())
}
```

Discovery recognizes the basenames listed in the root README at any safe
component depth. Unrelated JSON, `.git`, and Hugging Face download metadata are
not treated as configuration.

`read` returns an operation error when it cannot safely represent the supplied
snapshot—for example, an unreadable recognized file, an unsafe path, or a
resource-limit violation. Malformed JSON and wrong field shapes are normally
retained as source documents and reported through diagnostics instead.

## Construct a repository from downloaded bytes

Hub transport and caching belong outside this crate. A client can supply
already-downloaded bytes as `SourceDocument` values and provide a logical
inventory for existence checks. No filesystem or network access occurs during
construction.

```rust
use model_configs::{ModelRepository, RepositoryInventory, SourceDocument};

fn from_cache_entries() -> Result<ModelRepository, Box<dyn std::error::Error>> {
    let config = SourceDocument::parse(
        "config.json",
        br#"{"architectures":["ExampleForCausalLM"],"model_type":"example"}"#,
    )?;
    let index = SourceDocument::parse(
        "weights/model.safetensors.index.json",
        br#"{"weight_map":{"model.embed.weight":"model-00001-of-00002.safetensors"}}"#,
    )?;

    let mut inventory = RepositoryInventory::new();
    inventory.insert_directory("weights")?;
    inventory.insert_file("weights/model-00001-of-00002.safetensors")?;

    Ok(ModelRepository::from_documents_with_inventory(
        vec![config, index],
        inventory,
    )?)
}
```

Use `SourceDocument::parse_owned` when the caller can transfer a `Vec<u8>` and
avoid copying it. `ModelRepository::from_documents` is convenient when only
configuration documents matter, but an explicit inventory is required for
meaningful component-directory and shard-presence validation.

Repository paths use `/`, are relative and case-sensitive, and reject `.` or
`..` segments, backslashes, drive/UNC prefixes, NULs, and unsafe portable path
shapes. Pass logical repository paths—not cache paths or URLs.

## Inspect lossless source documents

The exact bytes are the authoritative representation. Parsed JSON is a
convenient projection and is not a lexical round-trip format.

```rust
use model_configs::SourceDocument;

fn inspect_document(bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let document = SourceDocument::parse("config.json", bytes)?;

    assert_eq!(document.original(), bytes);
    println!("kind: {:?}", document.kind());
    println!("path: {}", document.relative_path().display());
    println!("sha256: {}", document.sha256_hex());

    if let Some(error) = document.json_error() {
        println!("JSON is not usable: {:?}", error.category);
    } else if document.has_duplicate_keys() {
        println!("duplicate members at: {:?}", document.duplicate_keys());
    } else if let Some(json) = document.json() {
        println!("generic JSON: {json}");
    }

    Ok(())
}
```

Malformed JSON, invalid UTF-8, and duplicate keys do not erase bytes already
read. Typed interpretation is unavailable when the document cannot be
interpreted unambiguously.

`chat_template.jinja` is handled as inert text. `original()` always returns its
bytes; `text()` attempts UTF-8 decoding but never parses or renders Jinja.

## Read typed fields without applying defaults

`typed_view` returns a borrowed view selected by document kind. Match the
non-exhaustive enum and retain a fallback arm for future document kinds.

```rust
use model_configs::{SourceDocument, SourceField, TypedDocumentView};

fn inspect_config() -> Result<(), Box<dyn std::error::Error>> {
    let document = SourceDocument::parse(
        "config.json",
        br#"{"architectures":["ExampleModel"],"vocab_size":32000,"future_field":true}"#,
    )?;

    let TypedDocumentView::Config(view) = document.typed_view()? else {
        return Err("expected config view".into());
    };

    match view.vocab_size() {
        SourceField::Value(size) => println!("vocabulary size: {size}"),
        SourceField::Missing => println!("vocab_size was not supplied"),
        SourceField::Null => println!("vocab_size was explicitly null"),
        SourceField::Invalid(value) => println!("unexpected value: {value}"),
        _ => println!("a newer crate returned an unknown field state"),
    }

    for (name, value) in view.extra() {
        println!("unrecognized field {name}: {value}");
    }

    // The complete object and exact bytes remain available.
    assert!(view.raw().contains_key("future_field"));
    assert_eq!(view.source().original(), document.original());
    Ok(())
}
```

`SourceField` deliberately distinguishes four source states:

| Variant | Meaning |
| --- | --- |
| `Missing` | The key was absent. |
| `Null` | The key was explicitly JSON `null`. |
| `Value(T)` | The value had the expected shape. |
| `Invalid(&Value)` | The key existed with the wrong shape. |

That distinction matters because a normalization rule may default a missing
value but must not silently replace a wrong-typed value. Use `.value()` only
when collapsing the states is acceptable for the caller.

Every JSON view provides `source()`, `raw()`, `get(name)`, and an `extra()`
iterator (with specialized equivalents for model indexes). Specialized views
cover model/component configs, generation, tokenizers, special tokens,
processors, preprocessors, schedulers, Diffusers indexes, adapters,
quantization, and safetensors indexes.

## Normalize repository identity

Normalization is a separate owned projection under the immutable `dinoml-v1`
profile. It does not mutate source documents.

```rust
use model_configs::ModelRepository;

fn print_identity(repository: &ModelRepository) {
    match repository.normalized() {
        Ok(config) => {
            println!("source: {}", config.source_path().display());
            println!("architecture: {}", config.architecture);
            println!("architecture source: {:?}", config.architecture_source);
            println!("task: {:?}", config.task);

            for component in &config.components {
                println!(
                    "component {} at {:?}, requires external code: {}",
                    component.name,
                    component.path(),
                    component.requires_code,
                );
            }

            for applied in &config.applied_defaults {
                println!(
                    "DinoML supplied {}={} via {}",
                    applied.field, applied.value, applied.rule,
                );
            }
        }
        Err(error) => {
            // Source documents and repository diagnostics remain available.
            println!("repository identity is unavailable: {error}");
        }
    }
}
```

Root precedence is strict:

1. root `model_index.json`, when present;
2. otherwise root `config.json`;
3. otherwise no normalized root identity.

An invalid authoritative file blocks fallback. Nested `config.json` files are
component sources and never become the root identity accidentally.

For `config.json`, architecture selection uses the first non-empty
`architectures` entry, then `_class_name`, then `model_type`. For
`model_index.json`, `_class_name` is authoritative. Tasks are only taken from
explicit source metadata; class names and the presence of a Diffusers index do
not imply a task.

`extra` contains unconsumed top-level fields from the authoritative root.
`applied_defaults` records every derived or defaulted normalized value with a
stable rule identifier. Neither collection replaces the complete generic JSON
held by each `SourceDocument`.

## Select generation configuration

Source selection exposes precedence without merging documents:

```rust
use model_configs::ModelRepository;

fn generation_origin(repository: &ModelRepository) {
    if let Some(selection) = repository.generation_source() {
        println!(
            "generation source: {} {:?}",
            selection.document().relative_path().display(),
            selection.json_pointer(),
        );
    }
}
```

At one scope, `generation_config.json` wins whenever it exists—even if its
content is invalid. Only when it is absent can documented legacy generation
fields in `config.json` be selected. No opportunistic merge occurs.

Use `generation_source_in("component")` for a nested scope. The scope is itself
validated as a safe portable repository path.

## Select inert chat templates

Standalone `chat_template.jinja` takes precedence over an inline
`chat_template` field. Tokenizer and processor inline templates remain separate
sources.

```rust
use model_configs::{ChatTemplateValue, ModelRepository};

fn inspect_template(repository: &ModelRepository) -> Result<(), Box<dyn std::error::Error>> {
    let Some(selection) = repository.tokenizer_chat_template()? else {
        return Ok(());
    };

    println!(
        "template source: {}",
        selection.source.document().relative_path().display(),
    );
    match selection.value {
        ChatTemplateValue::Text(text) => println!("standalone bytes: {}", text.len()),
        ChatTemplateValue::Inline(value) => println!("inline JSON type: {value}"),
        _ => println!("a newer crate returned an unknown template representation"),
    }

    // The crate only selects and returns template data. It never renders Jinja.
    Ok(())
}
```

Use `processor_chat_template` for processor policy and the corresponding
`*_in(scope)` methods for nested components. An invalid standalone template
blocks inline fallback and returns an error.

## Validate references and consume diagnostics

Call `diagnostics()` after constructing the complete repository and inventory.
Validation covers, among other findings:

- malformed source shape and wrong-typed known fields;
- missing or invalid root identity;
- unsafe component and shard paths;
- missing Diffusers component directories or component configs;
- empty or invalid safetensors `weight_map` values and missing shards;
- adapter and processor companion findings;
- custom-code metadata that another ecosystem might execute; and
- skipped filesystem links or non-portable materialization collisions.

Diagnostics are deterministically ordered and bounded. Match the stable
`DiagnosticCode`, `DiagnosticLevel`, and structured locations. Human messages
are explanatory text and are not a stable machine interface.

```rust
use model_configs::{DiagnosticCode, ModelRepository};

fn enforce_policy(repository: &ModelRepository) -> Result<(), String> {
    let diagnostics = repository.diagnostics();
    if diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.code,
            DiagnosticCode::UnsafeReferencePath | DiagnosticCode::ExecutableReferenceInert
        )
    }) {
        return Err("repository requires unsupported trust or path policy".into());
    }
    Ok(())
}
```

The exact code names are public Rust API; use the generated rustdoc for the
complete registry. Because `DiagnosticCode` is non-exhaustive, downstream
matches should include a fallback arm where exhaustive matching is required.

## Write and read compatibility manifests

A compatibility manifest is a deterministic, portable summary. It records
schema/profile identity, exact-byte document hashes and sizes, a credential-safe
normalized projection when available, and credential-safe diagnostics.

```rust
use model_configs::{CompatibilityManifest, ModelRepository};

fn manifest_round_trip(repository: &ModelRepository) -> Result<(), Box<dyn std::error::Error>> {
    let manifest = repository.manifest()?;
    let json = manifest.to_json_pretty()?;

    std::fs::write("compatibility-manifest.json", &json)?;

    let decoded = CompatibilityManifest::from_json(&json)?;
    assert_eq!(decoded.schema_version(), 1);
    assert_eq!(decoded.normalization_profile(), "dinoml-v1");
    assert_eq!(decoded.documents().len(), repository.documents().len());
    Ok(())
}
```

Use `CompatibilityManifest::from_json` rather than deserializing the type with
Serde. The reader validates schema, bounds, portable paths, hashes, normalized
invariants, diagnostic coherence, and credential safety before constructing a
manifest. Unknown fields are accepted within schema 1; unknown schema versions
are rejected.

Manifest JSON is appropriate for compatibility exchange and diagnostics, but
pretty JSON is not the source fingerprint. Use each `ManifestDocument::sha256`
or `SourceDocument::sha256` as exact source evidence.

Manifests never contain complete source documents or chat-template bodies.
Credential-like keys and sensitive strings in normalized extras are omitted or
redacted. If required normalized identity is sensitive, the manifest suppresses
the entire normalized projection and reports a diagnostic rather than emitting
an inconsistent partial identity.

## Error-handling model

Keep these outcomes separate in callers:

| Outcome | Meaning | Typical response |
| --- | --- | --- |
| `ConfigError` | The requested repository representation could not be built safely. | Reject or repair the input adapter. |
| Retained `json_error` or duplicate keys | Source bytes exist, but JSON is malformed or ambiguous. | Preserve evidence and report diagnostics. |
| `ViewError` | One source cannot provide the requested typed object view. | Inspect raw bytes/JSON and diagnostics. |
| `NormalizationError` | No valid root architecture identity can be produced. | Continue source inspection; do not infer identity externally without explicit policy. |
| `Diagnostic` values | Inspectable content or compatibility findings. | Apply caller policy by stable code/severity. |
| `ManifestReadError` / `ManifestWriteError` | Manifest interchange failed validation or bounds. | Reject the manifest; do not partially trust it. |

Successful parsing is not a claim that a repository is executable, supported
by DinoML, or compatible with a particular tensor runtime.

## Security boundary

Treat all returned names and templates as data. This crate never:

- imports Python or honors `trust_remote_code`;
- imports a class named by `_class_name`, `auto_map`, `_module`, or a component
  tuple;
- renders Jinja;
- contacts the Hub or resolves an external adapter base model;
- opens model shards or allocates from `metadata.total_size`; or
- constructs a runtime graph or performs inference.

If a caller chooses to execute remote code, render a template, download a
reference, or load weights, that must happen in a separate subsystem with its
own explicit authorization and sandboxing policy.
