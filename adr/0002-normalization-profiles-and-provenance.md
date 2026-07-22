# ADR 0002: Normalization profiles and applied-value provenance

- Status: Accepted
- Date: 2026-07-22

## Context

Transformers and Diffusers source files are not complete runtime objects. Their
Python constructors supply version- and class-specific defaults, dedicated
files may replace legacy embedded fields, and a Diffusers component directory
is conventionally derived from a `model_index.json` key rather than stored as a
path. Reproducing these behaviors implicitly would make results depend on the
installed Python version and hide values DinoML supplied.

At the same time, consumers need a compact architecture and component view.
They must be able to distinguish source facts from inference and policy.

## Decision

Source-local typed views never apply defaults. Normalization creates a separate
owned value under an explicit profile. Manifest schema version 1 uses the
immutable profile identifier `dinoml-v1`.

At repository root, the presence of `model_index.json` makes it authoritative;
invalid content does not silently fall back to `config.json`. If no root index
exists, root `config.json` is authoritative. Nested configs require explicit
component-scope inspection.

Architecture precedence is:

- root model index: non-empty `_class_name`;
- root config: first non-empty `architectures` entry, then `_class_name`, then
  `model_type`.

`dinoml-v1` uses only explicit root task metadata. It never assumes that a
Diffusers index is text-to-image and does not infer task from class-name
patterns.

At one scope, an existing `generation_config.json` is the sole source for the
generation view; legacy generation fields in `config.json` are considered only
when the dedicated file is absent. A valid `chat_template.jinja` supersedes the
corresponding inline tokenizer or processor template. Invalid authoritative
documents are diagnosed and do not silently trigger fallback.

No global untyped merge is performed. Special-token map and tokenizer config,
standalone and nested quantization config, and processor and tokenizer inline
templates remain separately attributable where upstream behavior is
version-dependent.

Every normalized value not explicit at its authoritative source records its
field, JSON value, and stable rule identifier. This includes audited
constructor defaults and deterministic derived paths. Rules state their exact
missing/null semantics and audited upstream baseline. Wrong-typed source never
silently becomes a default.

The initial source-audit baselines are Transformers commit
`b75feb2af64c3e29cbbc1bd859958c5432cc7ed4` and Diffusers commit
`b3a515080752a3ba7ca92161e25530c7f280f629`. No Python constructor is invoked to
discover a default at runtime.

Successfully consumed root fields leave normalized `extra`; all unconsumed or
invalid fields remain there in sorted order. The original source remains
complete in every case.

## Consequences

- Normalization is reproducible without Python or ambient package versions.
- Consumers can audit every value DinoML supplied.
- Dedicated-file conflicts fail visibly rather than changing with load order.
- Empty or audio/video Diffusers indexes are not mislabeled text-to-image.
- New upstream default behavior requires a new rule/profile rather than
  silently changing existing manifests.
- Some repositories normalize only partially when their class has no audited
  rules; their source documents remain fully usable.
- Normalized serialization is not a source round-trip format.

## References

- [RFC 0001](../rfcs/0001-model-repository-config-v0.1.md)
- [Transformers configuration source](https://github.com/huggingface/transformers/blob/b75feb2af64c3e29cbbc1bd859958c5432cc7ed4/src/transformers/configuration_utils.py)
- [Transformers generation source](https://github.com/huggingface/transformers/blob/b75feb2af64c3e29cbbc1bd859958c5432cc7ed4/src/transformers/generation/configuration_utils.py)
- [Diffusers configuration source](https://github.com/huggingface/diffusers/blob/b3a515080752a3ba7ca92161e25530c7f280f629/src/diffusers/configuration_utils.py)
- [Tracking issue #1](https://github.com/dinoml/model-configs-rs/issues/1)
