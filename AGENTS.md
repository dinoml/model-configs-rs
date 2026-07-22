# Agent instructions

These instructions apply to the entire repository.

## Boundary

This repository owns lossless parsing, source preservation, normalized typed views, applied-default provenance, architecture identification, component-relative references, and internal-reference diagnostics for model repository configuration.

It does not own Hub transport/cache behavior, model weight loading, Python class execution, graph construction, or inference execution. Never import Python or execute code named by configuration files.

## Implementation rules

- Add observable-behavior tests before implementation.
- Preserve exact input bytes and unknown fields.
- Keep source documents separate from normalized views.
- Record every normalization default that was not explicit in source.
- Reject unsafe relative paths and validate internal references.
- Avoid `unwrap` and `expect` in production library code.
- Keep public types documented and `Debug`.

## Verification

Run formatting, Clippy with warnings denied, all tests, and rustdoc with warnings denied before handoff.

