# Integration guide

`model-configs` is the configuration layer between repository storage and
runtime/package consumers. It deliberately has no transport, cache, Python, or
model-loading authority.

## Responsibility split

| Layer | Owns | Does not delegate to `model-configs` |
| --- | --- | --- |
| Hub/cache client, such as `hf-store-rs` | Repository identity, authentication, revision resolution, download, immutable snapshots, cache reuse | Network access or credential handling |
| `model-configs` | Portable path identity, recognized configuration documents, exact bytes, typed views, normalization, internal-reference diagnostics, compatibility manifests | Transport, class execution, weight loading |
| DinoML conversion/runtime | Support policy, artifact selection, tensor loading, graph construction, device placement, inference | Runtime authorization or execution |

The storage layer should give `model-configs` exact bytes plus a complete
logical inventory. The configuration layer returns inert facts and findings.
The consumer decides whether those facts satisfy its compatibility policy.

## Recommended handoff from a content-addressed store

1. Resolve and pin the repository revision in the storage layer.
2. Enumerate logical repository paths from the immutable snapshot.
3. Read bytes only for basenames recognized by `DocumentKind::for_path`.
4. Parse each recognized file with `SourceDocument::parse_owned`.
5. Add all regular files and directories—not just config files—to a
   `RepositoryInventory`.
6. Construct `ModelRepository::from_documents_with_inventory`.
7. Collect diagnostics, normalize if possible, and produce a manifest.
8. Keep revision identity alongside the manifest in the downstream package;
   Hub revision is intentionally not invented by this crate.

```rust
use model_configs::{
    DocumentKind, ModelRepository, RepositoryInventory, SourceDocument,
};
use std::path::Path;

struct SnapshotEntry {
    path: String,
    is_directory: bool,
    bytes: Option<Vec<u8>>,
}

fn parse_snapshot(entries: Vec<SnapshotEntry>) -> Result<ModelRepository, Box<dyn std::error::Error>> {
    let mut documents = Vec::new();
    let mut inventory = RepositoryInventory::new();

    for entry in entries {
        if entry.is_directory {
            inventory.insert_directory(&entry.path)?;
            continue;
        }

        inventory.insert_file(&entry.path)?;
        if DocumentKind::for_path(Path::new(&entry.path)).is_some() {
            let bytes = entry.bytes.ok_or("recognized document bytes were not supplied")?;
            documents.push(SourceDocument::parse_owned(&entry.path, bytes)?);
        }
    }

    Ok(ModelRepository::from_documents_with_inventory(
        documents,
        inventory,
    )?)
}
```

In a real integration, do not load bytes for every weight shard merely to build
the inventory. Shard path metadata is sufficient for reference validation.

## Completeness and trust

An inventory is a claim about one complete logical snapshot. If it omits files,
the resulting missing-reference diagnostics describe the supplied inventory,
not necessarily the remote repository. Resolve download completeness before
using those diagnostics as an admission decision.

Logical paths must be derived from the repository snapshot, never from
unvalidated configuration strings. Preserve case and `/` separators. Detect
filesystem links, aliases, and case/Unicode materialization collisions in the
input adapter rather than silently rewriting identity.

Do not pass tokens, signed URLs, cache roots, or transport metadata as
configuration fields. Manifest filtering is defense in depth, not a replacement
for keeping credentials outside repository data.

## Consumer policy

A typical package converter should evaluate results in this order:

1. Treat construction errors as a failed snapshot handoff.
2. Inspect error-level diagnostics and organization-specific denied codes.
3. Request normalized identity; handle repositories that are intentionally
   source-only or cannot identify an architecture.
4. Compare architecture, explicit task, component requirements, and diagnostic
   codes with the converter's support matrix.
5. Store the compatibility manifest and exact source hashes with the package.
6. Hand approved weight paths to a separate weight-loading layer.

Do not interpret successful normalization as permission to import custom code.
`requires_code` and executable-reference diagnostics are evidence for policy;
they are never execution switches.

## Versioning expectations

Rust API changes follow Semantic Versioning after v0.1. The wire schema and
normalization profile have independent identities:

- manifest schema 1 defines the interchange shape and validation rules;
- `dinoml-v1` fixes source precedence, architecture/task behavior, component
  interpretation, and applied-default rules.

A future additive manifest field can remain schema 1. A change that alters
normalization for previously valid source requires a new opt-in profile rather
than silently changing `dinoml-v1`.
