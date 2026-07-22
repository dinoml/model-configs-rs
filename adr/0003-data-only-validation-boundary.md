# ADR 0003: Data-only reference validation boundary

- Status: Accepted
- Date: 2026-07-22

## Context

Configuration can name Diffusers components, safetensors shards, PEFT base
models, custom Transformers classes, custom Diffusers modules, quantization
providers, and Jinja templates. Some upstream loaders interpret those values as
paths or executable Python. Model repositories are untrusted, and DinoML needs
configuration diagnostics without granting execution or network authority.

## Decision

`model-configs-rs` is a data-only library. It never imports Python, loads a
dynamic module, renders Jinja, invokes a plug-in, contacts a Hub, resolves an
external repository, loads weights, or executes a class named by source.

Diffusers component tuples are interpreted structurally as exactly two elements
that are each a string or null. Two strings declare a present opaque
library/class component; two nulls declare an absent optional component; null
plus a class is retained as a custom-code candidate and diagnosed. Other arrays
remain pipeline data rather than being classified as components. A present
component directory is derived as one safe child segment of the index directory
and records the derivation rule.

Safetensors index values in `weight_map` are repository paths relative to the
index directory. Validation checks shape, path safety, an empty map, and target
presence in the supplied logical inventory. It does not open a shard or trust
`metadata.total_size` for allocation.

PEFT `base_model_name_or_path`, revisions, `_name_or_path`, `auto_map`, `_module`,
custom component libraries, tokenizer classes, and quantizer names remain
opaque identity or code-locator data. They are not converted to local paths
without an explicit format rule. Code-locator metadata produces compatibility
diagnostics but cannot enable execution.

All internal path validation is lexical and precedes host conversion. Existence
checks use an explicit repository inventory and never download a missing target.
The input adapter owns filesystem alias handling and reports skipped links or
reparse points; validation never treats an alias as permission to escape the
logical snapshot.

## Consequences

- Parsing the most hostile remote-code repository does not execute its code.
- A trusted repository still cannot turn this crate into a Python execution
  boundary; callers must use a separate explicitly authorized subsystem.
- Internal-reference diagnostics are deterministic and offline.
- External Hub references remain available for downstream resolution without
  being confused with local component paths.
- Checkpoint topology can be diagnosed without allocating model-size buffers or
  reading tensor data.
- Unknown libraries, classes, adapters, and quantizers remain forward-compatible
  source data.

## References

- [RFC 0001](../rfcs/0001-model-repository-config-v0.1.md)
- [Transformers custom model documentation](https://github.com/huggingface/transformers/blob/main/docs/source/en/models.md)
- [Diffusers custom pipeline documentation](https://github.com/huggingface/diffusers/blob/main/docs/source/en/using-diffusers/custom_pipeline_overview.md)
- [PEFT checkpoint format](https://huggingface.co/docs/peft/main/developer_guides/checkpoint)
- [Tracking issue #1](https://github.com/dinoml/model-configs-rs/issues/1)
