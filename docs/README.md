# model-configs documentation

`model-configs` reads Hugging Face model-repository configuration as inert
data. It preserves exact input bytes and unknown fields, exposes borrowed typed
views, derives a separate normalized repository view, and validates internal
references without importing Python or downloading anything.

## Guides

- [Usage guide](usage.md) — installation, filesystem and in-memory loading,
  typed views, normalization, diagnostics, source selection, and manifests.
- [Integration guide](integration.md) — the boundary between `model-configs`,
  Hub/cache clients such as `hf-store-rs`, and downstream runtimes.

The normative v0.1 behavior is defined by
[RFC 0001](../rfcs/0001-model-repository-config-v0.1.md). The
[accepted ADRs](../adr/README.md) explain the source/normalized split, path
identity, the no-execution boundary, and manifest stability.

## Choose the right layer

| Need | API |
| --- | --- |
| Preserve and fingerprint one known file | `SourceDocument` |
| Read fields exactly as supplied | `SourceDocument::typed_view` |
| Scan a materialized snapshot | `ModelRepository::read` |
| Integrate bytes from a Hub/cache client | `ModelRepository::from_documents_with_inventory` |
| Identify the root architecture and components | `ModelRepository::normalized` |
| Report malformed content and missing references | `ModelRepository::diagnostics` |
| Select generation or chat-template sources | repository selection methods |
| Exchange a portable compatibility summary | `CompatibilityManifest` |

Parsing, normalization, and compatibility are intentionally different
questions. A repository can be parsed and diagnosed even when it cannot be
normalized, and successful normalization does not mean the model can be loaded
or executed.
