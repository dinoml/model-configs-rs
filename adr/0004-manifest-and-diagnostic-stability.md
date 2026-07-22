# ADR 0004: Manifest and diagnostic stability

- Status: Accepted
- Date: 2026-07-22

## Context

DinoML package conversion, architecture admission, diagnostics, and regression
testing need a portable summary of configuration. Raw source bytes are the
fingerprinting authority, while normalized output and diagnostics evolve as
support grows. Conflating the manifest wire format with normalization rules or
human error prose would make compatibility impossible to reason about.

## Decision

Manifest schema and normalization rules have independent identities. The first
accepted manifest records `schema_version: 1` and
`normalization_profile: "dinoml-v1"`.

Each source entry contains portable path, document kind, lowercase SHA-256 over
exact bytes, and byte size. Entries sort by path. The optional normalized record
contains authoritative source path, architecture, optional model type,
Transformers and Diffusers versions, explicit task, and sorted components,
unknown extras, and applied defaults.

A diagnostic exposes a stable non-exhaustive code, severity, optional source
path, optional JSON Pointer, optional related path, and a non-stable human
message. Diagnostics have a deterministic sort order. Programs match code and
structured locations, never message text.

Manifest output contains no absolute host root, cache implementation path,
timestamp, credential, network URL with secrets, whole source document, or chat
template body. For a fixed crate version, schema, profile, and source inventory,
serialization is deterministic.

To enforce that boundary, manifest generation filters a clone of normalized
`extra` while leaving source documents and `ModelRepository::normalized`
unchanged. Provenance paths, chat templates, credential-bearing keys, and
sensitive nested strings are omitted or replaced with `"<redacted>"` under the
rules in RFC 0001. A sensitive required identity suppresses the whole normalized
manifest projection and emits a stable diagnostic instead of producing an
internally inconsistent partial identity.

Pretty manifest JSON is for interchange and inspection, not cross-version byte
fingerprinting: diagnostic prose and compatible optional fields may change.
Exact document hashes are the stable source evidence. A separately specified
repository fingerprint must use domain-separated structured inputs rather than
human display JSON.

Readers reject an unknown schema version and ignore unknown fields in a known
version. New optional fields and new diagnostic codes are additive. Removing or
reinterpreting required fields requires a schema version. Any rule change that
alters normalization of previously valid source requires a new opt-in profile;
`dinoml-v1` is immutable after release.

## Consequences

- Consumers can cache and compare exact source evidence independently of
  evolving presentation text.
- Schema changes and semantic rule changes can evolve at different rates.
- Diagnostics remain actionable for UIs and policy without freezing English
  wording.
- Manifest fixtures are portable across repository locations and operating
  systems.
- Adding a new normalization mapping cannot silently rewrite existing
  `dinoml-v1` compatibility results.

## References

- [RFC 0001](../rfcs/0001-model-repository-config-v0.1.md)
- [ADR 0001](0001-source-documents-and-path-identity.md)
- [ADR 0002](0002-normalization-profiles-and-provenance.md)
- [ADR 0003](0003-data-only-validation-boundary.md)
- [Tracking issue #1](https://github.com/dinoml/model-configs-rs/issues/1)
