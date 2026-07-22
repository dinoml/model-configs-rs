# model-configs-rs

Rust-native, data-only parsing, normalization, and validation for Hugging Face
model repository configuration.

`model-configs` retains exact source bytes and unknown JSON fields while exposing
borrowed format-specific views and a separate normalized repository view. It is
designed for DinoML architecture identification, package conversion,
diagnostics, and compatibility manifests without importing Python, downloading
dependencies, loading weights, or executing classes named by a repository.

## v0.1 contract

The accepted [configuration and normalization RFC](rfcs/0001-model-repository-config-v0.1.md)
defines path identity, source precedence, applied-default provenance, stable
diagnostics, manifest schema 1, and the `dinoml-v1` normalization profile. The
[ADRs](adr/README.md) record the source/normalized boundary, versioning model,
data-only security boundary, and compatibility policy.

Supported documents are recognized at the repository root and nested component
paths:

- `config.json`
- `generation_config.json`
- `tokenizer_config.json`
- `special_tokens_map.json`
- `preprocessor_config.json`
- `processor_config.json`
- `scheduler_config.json`
- `model_index.json`
- `adapter_config.json`
- `quantization_config.json`
- `chat_template.jinja`
- `*.safetensors.index.json`

Malformed JSON, duplicate keys, invalid UTF-8, wrong field shapes, unsafe paths,
and missing internal references remain inspectable through exact bytes and
structured diagnostics. Unknown fields are never discarded from source views.
Construction is explicitly bounded per document, across retained repository
source bytes, by JSON nesting depth, recognized-document count, inventory size,
diagnostic retention, portable path length, and serialized manifest size.

## Usage

The expanded [documentation](docs/README.md) covers filesystem and Hub/cache
integration, lossless source access, typed views, normalization, diagnostics,
source selection, compatibility manifests, and the crate's security boundary.

Read a materialized repository and produce a deterministic manifest:

```rust
use model_configs::ModelRepository;

let repository = ModelRepository::read("model-snapshot")?;
for diagnostic in repository.diagnostics() {
    eprintln!("{}: {}", diagnostic.code, diagnostic.message);
}

let manifest_json = repository.manifest()?.to_json_pretty()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Hub/cache integrations can parse bytes independently and construct a repository
without filesystem access:

```rust
use model_configs::{ModelRepository, RepositoryInventory, SourceDocument};

let config = SourceDocument::parse(
    "config.json",
    br#"{"architectures":["ExampleModel"],"model_type":"example"}"#,
)?;
let index = SourceDocument::parse(
    "weights/model.safetensors.index.json",
    br#"{"weight_map":{"tensor":"model-1.safetensors"}}"#,
)?;

let mut inventory = RepositoryInventory::new();
inventory.insert_directory("weights")?;
inventory.insert_file("weights/model-1.safetensors")?;
let repository =
    ModelRepository::from_documents_with_inventory(vec![config, index], inventory)?;

assert_eq!(repository.normalized()?.architecture.as_str(), "ExampleModel");
# Ok::<(), Box<dyn std::error::Error>>(())
```

`SourceDocument::typed_view` exposes source-local fields without defaults.
Tokenizer views represent named tokens as `SpecialTokenValue` and expose
structured `AddedTokenView` metadata without flattening the original JSON.
`ModelRepository::normalized` applies only named `dinoml-v1` rules and returns a
separate `NormalizationError` for content that cannot identify a repository.
Root precedence helpers have validated `*_in(scope)` counterparts for nested
components; dedicated generation and standalone template files remain
authoritative within their own scope.

## Corpus

The deterministic [corpus audit](corpus/AUDIT.md) covers 16,445 supported
documents from the external `H:\configs` snapshot, including 7,089 documents
verified against concrete Hub revisions and exact byte hashes. The remaining
9,356 legacy snapshot documents have no matching revision file record and are
not claimed as independently reproducible. Bulk third-party files remain
outside Git; this repository stores revision-backed inventory metadata, audit
results, tooling, and small attributed conformance fixtures. See
[corpus/README.md](corpus/README.md) for reproduction commands and strict-JSON
findings.

## Development

The MSRV is Rust 1.85.

```text
cargo +stable fmt --all --check
cargo +stable clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +stable test --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo +stable doc --workspace --all-features --no-deps --locked
cargo +1.85.0 fmt --all --check
cargo +1.85.0 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.85.0 test --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo +1.85.0 doc --workspace --all-features --no-deps --locked
python -B -m unittest discover -s tools -p "test_*.py" -v
```

Licensed under Apache-2.0.
