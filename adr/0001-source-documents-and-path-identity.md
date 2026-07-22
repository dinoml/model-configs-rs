# ADR 0001: Source documents and repository path identity

- Status: Accepted
- Date: 2026-07-22

## Context

Model repositories mix root configuration with nested component configuration.
The same basename can appear at several paths, source formatting is useful for
fingerprints and diagnostics, and future ecosystem releases routinely add JSON
fields. A typed deserialization alone loses byte spelling, may discard unknown
data, and cannot distinguish two same-named files in different components.

The representative corpus also contains valid-but-empty `model_index.json`
placeholders, large safetensors indexes, Hub download sidecars, and unrelated
JSON. Content sniffing or flattening by basename would therefore produce false
repository facts.

## Decision

Every recognized source document is identified by a validated, portable path
relative to one repository snapshot. Paths are UTF-8, slash-separated,
case-sensitive, and retain their spelling. They are never represented in a
manifest by an absolute host path.

Classification uses the basename only after the complete path is validated.
All supported basenames can occur at nested component scopes. Root selection,
however, uses only exact root paths. Tool metadata trees such as `.git/` and
`.cache/huggingface/download/` are excluded from discovery.

A source document retains its exact bytes, byte length, kind, path, and SHA-256
of those bytes. JSON and UTF-8 text are additional generic projections, not the
lossless representation. Typed views borrow or remain linked to this source
value and cannot overwrite it.

Malformed JSON, invalid UTF-8, duplicate object members, and invalid top-level
shape retain already-read source bytes and produce structural diagnostics.
Duplicate members are not collapsed with first- or last-value semantics for
typed normalization. An unreadable recognized file remains an operation error
because the supplied snapshot cannot be represented completely.

JSON projections use strict JSON syntax. Bare `Infinity`, `-Infinity`, and
`NaN` tokens remain byte-preserved malformed source with diagnostics; they are
not normalized into values accepted by a permissive decoder.

Unknown fields and wrong-typed recognized fields remain in the generic source.
A normalized `extra` map is convenience data from the authoritative root, not a
substitute for the source document.

Repository-reference paths reject absolute or prefixed paths, `.`, `..`, empty
segments, backslashes, NULs, and unsafe host representations before joining.
No Unicode normalization, case folding, percent decoding, or shell expansion is
performed.

## Consequences

- Byte fingerprints cover formatting and all unknown data.
- Generic JSON cannot be mistaken for a byte-for-byte round-trip format.
- Nested component configs remain distinguishable and cannot become root by
  accident.
- Corpus and Hub sidecar metadata cannot be mistaken for model configuration by
  content sniffing.
- Callers can inspect malformed source evidence without treating it as typed
  configuration.
- Path conversion and reference validation have one portable identity model
  across Windows, Linux, and macOS.
- Adding a recognized basename changes discovery and requires compatibility
  review.

## References

- [RFC 0001](../rfcs/0001-model-repository-config-v0.1.md)
- [Tracking issue #1](https://github.com/dinoml/model-configs-rs/issues/1)
