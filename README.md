# model-configs-rs

Rust-native, data-only parsing and normalization for Hugging Face model repository configuration files.

The crate retains exact source bytes and generic JSON for fingerprinting and diagnostics, while exposing a separate normalized view for architecture identification, conversion, compatibility manifests, and validation. Unknown fields remain available and normalization records defaults applied by DinoML. It never imports Python or instantiates remote classes.

Initial document coverage:

- Transformers, generation, tokenizer, processor, preprocessor, adapter, and quantization JSON
- Diffusers `model_index.json` and scheduler JSON
- `chat_template.jinja`
- `*.safetensors.index.json`

This repository is pre-1.0 and its API is expected to evolve as the compatibility corpus expands.

## Development

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
```

Licensed under Apache-2.0.

