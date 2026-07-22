---
rfc: "0001"
title: Model repository configuration v0.1
status: accepted
owners: [hlky]
created: 2026-07-22
updated: 2026-07-22
tracking_issue: https://github.com/dinoml/model-configs-rs/issues/1
parent_issue: https://github.com/dinoml/model-configs-rs/issues/6
project: https://github.com/orgs/dinoml/projects/1
depends_on: []
related:
  - https://github.com/dinoml/model-configs-rs/issues/2
  - https://github.com/dinoml/model-configs-rs/issues/3
  - https://github.com/dinoml/model-configs-rs/issues/4
  - https://github.com/dinoml/model-configs-rs/issues/5
source_material:
  - Transformers audit commit b75feb2af64c3e29cbbc1bd859958c5432cc7ed4
  - Diffusers audit commit b3a515080752a3ba7ca92161e25530c7f280f629
  - Representative Hugging Face repository corpus tracked by issue 2
---

# RFC 0001: Model repository configuration v0.1

## Summary

`model-configs-rs` reads model-repository configuration as inert data. It keeps
the exact source bytes and unknown JSON fields, offers source-local typed views,
and derives a separate normalized repository view under an explicit,
versioned rule profile. It validates local references without importing Python,
loading weights, contacting a Hub, or executing code named by configuration.

This RFC accepts the semantic contract for v0.1. The exact Rust module layout
may be refined until the v0.1 release, but an implementation claiming the
`dinoml-v1` normalization profile must satisfy the behavior below.

## Outcome

Given a complete local repository snapshot or equivalent logical file
inventory, v0.1 can:

1. identify every supported configuration document by its portable repository
   path;
2. retain the exact bytes and a generic data projection where one is valid;
3. expose typed, source-local access without discarding unknown fields;
4. identify a root model or pipeline conservatively;
5. materialize only audited normalization defaults and record each rule;
6. resolve and validate component and checkpoint-index references; and
7. produce a deterministic, versioned compatibility manifest.

A successful parse means that source data was read and represented. It does not
mean the repository is executable, supported by DinoML, safe to run as Python,
or compatible with any particular tensor runtime.

## Goals

- Preserve exact input bytes for fingerprints and diagnostics.
- Preserve all semantically representable JSON values, including unknown
  fields, independently of typed interpretation.
- Keep source documents and normalized views as different values with different
  lifetimes and guarantees.
- Recognize configuration documents at repository root and component-relative
  paths.
- Make architecture selection, task handling, precedence, inference, and
  defaults deterministic and inspectable.
- Record every normalized value that DinoML supplies when it was not explicit
  in source.
- Treat unknown fields, classes, task strings, quantizers, and future formats as
  data instead of reasons to execute extension code.
- Validate internal references using portable repository paths.
- Offer stable machine-readable diagnostics and a schema-versioned manifest.
- Remain usable independently of DinoML, Transformers, Diffusers, PEFT, Python,
  a Hub client, and a model runtime.

## Non-goals

- Hub transport, revision resolution, authentication, cache reuse, or download.
- Model weights, tensor headers, state-dict loading, graph construction, or
  inference.
- Python imports, dynamic modules, `trust_remote_code`, custom pipeline loading,
  Jinja rendering, or any other code execution.
- Exact behavioral emulation of an arbitrary installed version of Transformers,
  Diffusers, PEFT, or a quantization plug-in.
- Model-card YAML, arbitrary JSON files, tokenizer vocabulary formats, or
  checkpoint formats other than safetensors index metadata in v0.1.
- Proving that an external repository identifier exists or downloading it.
- Rewriting or round-tripping a source file from a parsed JSON value.

## Boundary

This repository owns logical configuration path identity, supported-document
classification, lossless source retention, generic JSON projections,
source-local typed views, normalized configuration, normalization provenance,
architecture identification, component-relative references, internal-reference
diagnostics, and compatibility manifests.

[`hf-store-rs`](https://github.com/dinoml/hf-store-rs) owns Hub identity,
transport, cache and immutable snapshot behavior. A downstream package owns
artifact selection, package conversion policy, weight loading, runtime graph
construction, device placement, and execution.

## Terminology

- **Repository path**: a validated, slash-separated logical path inside one
  repository snapshot. It is not a host filesystem path.
- **Scope**: the repository root or a component directory relative to it.
- **Source document**: one recognized path, its kind, exact bytes, and any
  generic parse result. Source facts never include applied defaults.
- **Typed view**: a read-only interpretation of one source document. It does
  not mutate, replace, or fully deserialize the source.
- **Normalized view**: a separately owned, consumer-oriented projection created
  from one or more source documents under a normalization profile.
- **Explicit value**: a valid value present at the authoritative source path and
  JSON Pointer for that normalized field.
- **Derived value**: a value computed deterministically from explicit source,
  such as a component directory derived from its `model_index.json` key.
- **Applied default**: a normalized value not explicit in source and supplied by
  a named normalization rule. In the v0.1 data model, deterministic derived
  values also use this provenance channel because consumers must be able to
  identify every materialized, non-explicit value.
- **Diagnostic**: a content or compatibility finding returned with an otherwise
  inspectable repository.
- **Operation error**: a failure that prevents the requested read or repository
  representation from being constructed safely.

## Required public seams

Exact module paths and accessor names may be refined before release, but the
v0.1 Rust surface must make these semantic layers independently accessible:

```rust,ignore
struct SourceDocument {
    // Validated repository-relative identity, recognized kind, exact bytes,
    // and an optional generic JSON projection.
}

struct ModelRepositoryConfig {
    source_path: PathBuf,
    architecture: ArchitectureId,
    model_type: Option<String>,
    transformers_version: Option<String>,
    diffusers_version: Option<String>,
    task: Option<TaskKind>,
    components: Vec<ComponentReference>,
    extra: BTreeMap<String, Value>,
    applied_defaults: Vec<AppliedDefault>,
}

struct AppliedDefault {
    field: String,
    value: Value,
    rule: String,
}
```

`ModelRepository` owns a stable path-ordered collection of source documents and
can return normalized output, diagnostics, and a compatibility manifest without
consuming or modifying those documents. Format-specific typed views remain
reachable from their source document rather than being copied into one global
configuration object.

Public value and diagnostic types implement `Debug`. Enums whose upstream value
set can grow are non-exhaustive or have an `Other` representation. Logical path
invariants are enforced at construction rather than delegated to every caller.

## Supported document kinds

Classification uses the basename, while identity always retains the complete
repository path. Except where noted, the document may occur at any safe depth.

| Basename or pattern | Kind | v0.1 interpretation |
| --- | --- | --- |
| `config.json` | model/component config | Transformers model configuration or Diffusers component configuration; typed access includes `architectures`, `model_type`, `_class_name`, ecosystem versions, nested configs, and unknown fields. |
| `generation_config.json` | generation config | Explicit generation policy and metadata. It is not populated with the current Transformers Python defaults implicitly. |
| `tokenizer_config.json` | tokenizer config | Tokenizer class, limits, token metadata, optional inline chat template, dynamic-code metadata, and unknown fields. |
| `special_tokens_map.json` | special-token map | Named special tokens as strings or structured added-token objects. It remains distinct from tokenizer configuration. |
| `preprocessor_config.json` | preprocessor config | Image, audio, feature-extractor, or preprocessor class metadata and parameters. |
| `processor_config.json` | processor config | Composite or multimodal processor metadata and parameters. |
| `scheduler_config.json` | scheduler config | Diffusers scheduler class, version, and parameters. |
| `model_index.json` | pipeline index | Diffusers pipeline class, version, scalar pipeline settings, and component declarations. |
| `adapter_config.json` | adapter config | PEFT type, task type, base-model identity, revision, auto mapping, and method-specific fields. |
| `quantization_config.json` | quantization config | Provider-specific quantization method and parameters. No provider is imported or assumed. |
| `chat_template.jinja` | chat template | Exact inert template text. v0.1 may decode UTF-8 but never parses or renders Jinja. |
| `*.safetensors.index.json` | safetensors index | Metadata plus tensor-name-to-shard `weight_map`; shard files are references only. |

The set is deliberately narrower than “all JSON.” Files such as Hub download
sidecars, `.metadata` files, `tokenizer.json`, vocabularies, model cards, and
unrelated root JSON are not reclassified by content. Repository discovery must
exclude implementation metadata trees such as `.git/` and
`.cache/huggingface/download/`. Unsupported files do not enter a v0.1 manifest.

Adding a recognized basename changes discovery and manifest output. That is a
profiled compatibility change, not a silent parser enhancement.

## Repository path identity and discovery

Repository paths are UTF-8, slash-separated, relative, non-empty, and
case-sensitive. Their source spelling is preserved. The library does not
Unicode-normalize, case-fold, percent-decode, expand `~`, or reinterpret a
backslash as a separator.

Before a path from configuration is joined or compared, v0.1 rejects:

- absolute, drive-relative, drive-prefixed, or UNC paths;
- empty, `.` or `..` segments;
- backslashes, NULs, and host-specific prefixes;
- segments that cannot be represented safely on a supported host; and
- any lexical resolution that leaves the declaring repository scope.

Host-path conversion happens only after repository-path validation. Logical
identity remains case-sensitive even on a case-insensitive host. A file
inventory that contains two logical paths which collide under the target host's
materialization rules is diagnosed and cannot be treated as portable.

Discovery is recursive over supported paths and produces ascending portable
path order. A nested `text_encoder/config.json` is a source document at that
path; it is never silently promoted to root `config.json`. Directory readers
must not recursively enter tool metadata, temporary staging, or cyclic links.
The strict handling of symlinks or reparse points is part of the input-adapter
contract and must be reported rather than silently changing logical identity.

References are resolved relative to the directory containing the declaring
document unless a format rule below says otherwise. No configuration string is
treated as a host path merely because it resembles one.

## Lossless source documents

### Required retained facts

For every recognized readable path, a source document retains:

- the validated repository path;
- the recognized document kind;
- the exact byte sequence and byte length; and
- SHA-256 over exactly those bytes.

The byte slice is authoritative for source fingerprinting. Newlines, whitespace,
object-key order, escapes, number spelling, and a trailing newline therefore
affect the source digest.

JSON documents additionally offer a generic JSON projection when the bytes are
valid UTF-8 JSON. The projection preserves unknown names and values but is not
a lexical round-trip format: callers use the retained bytes to reproduce the
source. A text view of `chat_template.jinja` is available only when its bytes
are valid UTF-8; its byte view is always authoritative.

JSON decoding follows the strict JSON grammar. In particular, bare non-finite
number tokens such as `Infinity`, `-Infinity`, and `NaN` are malformed input;
the implementation must diagnose them rather than silently convert them to a
finite number, `null`, or an implementation-specific value. This distinction
matters in the representative corpus, which contains such tokens in otherwise
recognizable configuration files.

### Invalid and ambiguous content

Malformed JSON, invalid UTF-8, a non-object top-level value, and duplicate object
member names must not cause already-read bytes to disappear. The document is
retained with a structural diagnostic and without a usable typed view where the
shape is invalid.

Duplicate member names are structurally invalid for typed normalization because
common JSON object models cannot preserve their semantics. The library must not
silently select the first or last occurrence. Exact bytes remain available for
diagnostics and evidence.

An unreadable recognized file, an unsafe requested path, or failure to enumerate
the repository is an operation error because the claimed snapshot cannot be
represented completely. Resource-limit failures must identify the affected
source path and never return a partially parsed value as valid.

The v0.1 construction limits are 64 MiB per recognized source document, 16,384
recognized source documents, 256 MiB of retained source bytes across one
repository, and 250,000 inventoried files plus directories. The public constants
are part of the resource contract; raising them is compatible, while lowering
them requires a new normalization/compatibility review.

Portable logical paths are limited to 1,024 UTF-8 bytes in total and 255 bytes
per segment. Strict JSON nesting is limited to 120 arrays or objects so an
accepted source remains safely representable inside its manifest wrapper.
Duplicate-key evidence retains at most 1,024 locations and 1 MiB of location
text per source document. Repository validation returns at most 4,096
diagnostics, with at most 4,096 bytes retained for each message or structured
location. A serialized compatibility manifest is limited to 256 MiB, including
pretty-print whitespace. Exceeding any limit produces an explicit operation
error or a stable truncation diagnostic; it never silently returns a partial
typed value.

### Unknown-field preservation

Typed access is additive over the generic document. It never deletes a key from
the source object. A normalized view may expose an `extra` map of unconsumed
top-level fields, but consumed fields and nested details remain available from
the source document.

A recognized field is consumed only after its shape is valid. A recognized name
with the wrong type remains recoverable as generic data and receives a
diagnostic; a default must not overwrite it. Unknown nested fields remain under
their original parent object.

## Source-local typed views

Every JSON typed view is a read-only projection over exactly one source
document. A view:

- checks that the kind and top-level shape are appropriate;
- distinguishes missing, explicit `null`, wrong type, and a valid value where
  that distinction affects normalization;
- returns borrowed or otherwise source-linked values where practical;
- exposes the complete raw object and unknown fields;
- does not instantiate an ecosystem class or materialize class defaults; and
- cannot be serialized over the source as though it were the original file.

Typed views may know field names and value shapes, but an unknown enum string is
retained as its source spelling. `quant_method`, `peft_type`, `_class_name`,
`model_type`, tokenizer or processor class names, and future task values are
open vocabularies.

The v0.1 typed contract includes at least:

- Transformers/Diffusers config identity and ecosystem-version fields;
- generation parameters and their source metadata;
- tokenizer class, limits, named special tokens, added-token structures, and
  inline chat-template metadata;
- processor/preprocessor class metadata;
- scheduler class and version;
- pipeline class, settings, and component tuple shapes;
- adapter type, task, base-model identifier and revision;
- quantization method and provider-specific extras; and
- safetensors index metadata, tensor names, and shard names.

## Normalization profiles

Manifest schema and normalization behavior are versioned independently. The
accepted v0.1 manifest has `schema_version: 1` and records
`normalization_profile: "dinoml-v1"`.

A profile fixes:

- supported document kinds;
- authoritative-source and field precedence;
- identifier and task mappings;
- component interpretation and path derivation;
- every inference or applied-default rule;
- normalized ordering; and
- the minimum diagnostic semantics needed to interpret the result.

Normalization is pure with respect to source documents and an explicit logical
file inventory. It cannot consult Python, the network, environment variables,
an ambient cache, a clock, installed packages, or process-global state.

### Root source selection

The repository-root normalized model view uses this strict order:

1. If root `model_index.json` exists, it is authoritative. It must be a valid
   object with a non-empty string `_class_name`. Invalid or incomplete content
   is diagnosed and blocks silent fallback to root `config.json`.
2. Otherwise, root `config.json` is authoritative. It must be a valid object and
   identify an architecture by the precedence below.
3. Otherwise no `ModelRepositoryConfig` is produced and
   `missing_root_config` is reported.

Nested files do not participate in root selection. Callers can inspect a
component scope explicitly, but a component view does not become repository
identity by accident. Adapter-only, processor-only, scheduler-only, and
quantization-only repositories still receive source-local typed views and a
manifest; they do not fabricate a model architecture when none is explicit.

### Architecture precedence

For an authoritative `model_index.json`, `_class_name` is the pipeline
architecture. No Python symbol lookup is performed.

For an authoritative `config.json`, the first usable value wins:

1. the first non-empty string in `architectures`, preserving array order;
2. a non-empty string `_class_name` for Diffusers-style standalone components;
3. a non-empty string `model_type` as a family identifier fallback.

No whitespace trimming, case conversion, module import, alias registry, or
class-name synthesis occurs. A missing, null, or empty candidate advances to the
next candidate. A wrong-typed candidate is diagnosed and left unconsumed; a
later valid candidate may still identify the repository, but it does not erase
that diagnostic. The result records its source path and profile so consumers
can distinguish a class name from a `model_type` fallback.

When multiple strings appear in `architectures`, the normalized single
architecture uses the first; the complete ordered list remains in the typed
source view.

### Task contract

The root task is source-explicit in `dinoml-v1`. A valid `pipeline_tag` (or the
documented legacy `task` alias) on the authoritative root may map to a known
`TaskKind`; an unknown string maps to `Other` while retaining its exact spelling.

The initial known spellings cover the common Hugging Face tags represented by
the public task enum, including text generation and classification, feature
extraction, question answering, image classification/detection/segmentation,
speech recognition, text to speech, text to image, image to image, inpainting,
unconditional image generation, video generation, and audio generation.

An adapter typed view may map an explicit PEFT `task_type` to its corresponding
task while retaining the original PEFT spelling. That mapping does not make an
adapter's external base model local.

`dinoml-v1` does **not** infer a task from an architecture or from the mere
presence of `model_index.json`. In particular, a Diffusers pipeline is not
defaulted to text-to-image: the corpus also contains unconditional image,
image-to-image, video, audio, and other pipelines. A future exact
architecture-to-task registry is a new normalization profile or an explicit
rule addition with inference provenance.

### Dedicated-document precedence

Documents are not merged into one untyped global map. Each normalized subsystem
has an authoritative source at one logical scope.

For generation:

1. If `<scope>/generation_config.json` is absent, a legacy generation view may
   be projected from documented generation fields in `<scope>/config.json`.
2. If `generation_config.json` exists, it is the sole source document for that
   generation view. Missing keys remain missing or become defaults only through
   named profile rules; they are not filled opportunistically from `config.json`.
3. Invalid dedicated content is diagnosed and does not silently fall back to
   legacy fields.

This reflects the ecosystem separation between the dedicated generation file
and legacy model-config parameters without instantiating `GenerationConfig`.

For chat templates:

- a tokenizer template is a valid `<scope>/chat_template.jinja`, otherwise a
  valid `chat_template` field in `<scope>/tokenizer_config.json`;
- a processor template is the same standalone file, otherwise a valid
  `chat_template` field in `<scope>/processor_config.json`; and
- processor and tokenizer inline templates are not cross-merged.

An invalid standalone template blocks inline fallback and is diagnosed. The
template remains inert in every case.

`special_tokens_map.json` and `tokenizer_config.json` remain separate source
views in v0.1 because upstream merge behavior depends on tokenizer serialization
generation and library version. Likewise, a standalone
`quantization_config.json` and a nested `config.json#/quantization_config` are
not blindly merged. A conflict is visible to consumers rather than resolved by
an unpinned provider convention.

### Diffusers components

For each non-private `model_index.json` field:

- only an array of exactly two elements where each element is a string or
  `null` is a component-tuple candidate;
- `["library", "Class"]` is a present component declaration;
- `[null, null]` is an explicitly absent optional component;
- `[null, "Class"]` is retained as a custom/local-code component candidate
  with no library and receives a code-execution compatibility diagnostic; and
- other values remain pipeline settings or invalid component candidates rather
  than being discarded.

Array-ness alone never identifies a component. Model indexes also use arrays
for values such as image statistics, patch sizes, coefficients, and weight
lists; those settings remain in source and normalized `extra`.

For a present component, the source key is its component name. The v0.1
Diffusers convention derives the target directory as exactly one safe child
segment of the directory containing `model_index.json`. This non-explicit path
is recorded with rule `diffusers-component-name-is-path-v1` (or a stable
equivalent rule identifier). Unsafe names do not produce a path.

Components are sorted by source name in normalized output. Their library and
class strings remain opaque. The library name never authorizes an import.

### Defaults, derivations, and provenance

Source configs frequently omit values that a specific Python class constructor
would supply. v0.1 may materialize such a value only when an audited static rule
in `dinoml-v1` identifies the exact ecosystem, class or model type, field,
absence semantics, value, and upstream source baseline.

Each applied default records at least:

- the normalized field path;
- the materialized JSON value; and
- a stable rule identifier.

Rule documentation or manifest metadata must make the reason and audited
upstream baseline discoverable. The initial audit baselines are Transformers
commit `b75feb2af64c3e29cbbc1bd859958c5432cc7ed4` and Diffusers commit
`b3a515080752a3ba7ca92161e25530c7f280f629`; observing a newer upstream version
does not mutate `dinoml-v1`.

Default precedence is:

1. a valid explicit value in the authoritative dedicated document;
2. an explicitly defined legacy source only when the dedicated document is
   absent;
3. a named deterministic derivation or inference rule; and
4. a named audited default rule.

Missing, explicit `null`, and wrong type are different states. A default applies
only to the states named by its rule. Wrong-type input never becomes a default
silently. An explicit source value always wins over an audited constructor
default.

Upstream markers such as Diffusers `_use_default_values` are source metadata;
they are not themselves proof that DinoML applied a value. Runtime or
caller-supplied overrides are outside source normalization and must not be
misrepresented as repository facts.

`dinoml-v1` applies no universal task default and no default requiring execution
of an upstream constructor. Unknown architectures simply receive fewer
normalized fields and an actionable diagnostic where a required fact is absent.

### Normalized unknown fields

The normalized root `extra` map contains every unconsumed top-level field from
the authoritative root object, sorted by key. A field is removed from `extra`
only after successful typed consumption. Source documents remain complete
regardless of `extra`.

Normalized output is a view, not a replacement source format. Serializing it
does not reproduce and must never overwrite a configuration document.

## Internal-reference validation

Validation is deterministic over the logical file inventory and never triggers
download or class loading.

### Component references

A present Diffusers component path resolves relative to the declaring
`model_index.json`. Validation distinguishes:

- unsafe component name;
- missing component directory;
- present directory with no recognized component configuration; and
- present recognized configuration with shape diagnostics of its own.

The expected config basename may be refined by a static library/class registry,
but unknown classes remain inspectable and do not authorize an import.

### Safetensors shard references

A safetensors index must be an object with a `weight_map` object. Each tensor
name maps to a non-empty shard path string. Shard paths resolve relative to the
index document's directory, not repository root.

Validation rejects unsafe paths, diagnoses an empty map, and reports every
referenced shard absent from the inventory. Repeated shard names are deduplicated
for existence checks; tensor-name mappings retain their complete source data.
Unknown `metadata` fields are preserved. `metadata.total_size` is descriptive
and does not authorize reading or allocating that number of bytes.

The crate does not open safetensors shards, validate tensor headers, or compare
the index with model code.

### External and executable references

PEFT `base_model_name_or_path` and `revision` are external identity data unless
an explicit format field unambiguously declares a local repository path. They
are never joined to the current root merely because they contain `/`.

Transformers `auto_map`, tokenizer custom classes, Diffusers `_module`, custom
component tuples, and similar values are executable-code locators. v0.1
preserves and diagnoses them but never resolves, imports, downloads, or invokes
them. `_name_or_path` is provenance text, not an internal path.

## Errors and diagnostics

Operation errors are reserved for failures such as inaccessible repository
input, unreadable recognized files, unsafe caller-supplied paths, and resource
limits that prevent safe construction. Content problems are diagnostics so a
caller can inspect all available evidence in one pass.

A diagnostic contains:

- a stable, non-exhaustive machine code;
- stable severity (`info`, `warning`, or `error`);
- optional source repository path;
- optional RFC 6901 JSON Pointer;
- optional related repository path; and
- a human message.

Machine code, severity meaning, and location fields are the programmatic
contract. Human message wording is not stable and must not be parsed. New codes
are additive; consumers must handle unknown codes. Changing the meaning or
severity of an existing code requires a compatibility review and normally a new
normalization profile.

The v0.1 code registry covers at least missing or invalid roots, missing
architecture, invalid document shape, unsafe paths, missing component
directories/configs, empty checkpoint maps, unsafe or missing shards, adapter
base-reference findings, missing tokenizer/preprocessor companions, and skipped
filesystem links. Structural parse diagnostics for malformed JSON, duplicate
members, and invalid UTF-8 follow the same location contract.

Diagnostics are sorted deterministically by severity, code, source path, JSON
Pointer, related path, and finally message. Because messages may improve across
compatible releases, callers must not use the complete diagnostic JSON as a
cross-version content fingerprint.

## Compatibility manifest v1

The manifest is a deterministic semantic description of the supplied snapshot.
It contains:

```text
schema_version: 1
normalization_profile: "dinoml-v1"
documents: [...]
normalized: {...} | null
diagnostics: [...]
```

Each source entry contains its portable path, kind, lowercase exact-byte
SHA-256, and byte size. Entries are sorted by portable path.

When normalization succeeds, the normalized record contains the authoritative
`source_path`, architecture, optional model type, Transformers and Diffusers
versions, explicit task, sorted components, sorted `extra`, and sorted applied
defaults. It does not contain a host root, timestamp, cache path, credential, or
ambient library version.

The manifest uses a security-filtered clone of normalized `extra`; it does not
change `ModelRepository::normalized` or any source/typed view. Source-only keys
such as `_name_or_path`, `chat_template`, authorization/token/password/secret
fields, and credential-like nested keys are omitted. Nested strings that are
absolute host/cache paths, bearer or Hugging Face access tokens, or URLs with
userinfo or credential query parameters become the literal `"<redacted>"`.
These rules are recursive and deterministic. If a required normalized identity
field itself contains source-sensitive text, the manifest records
`normalized: null` plus `manifest_sensitive_data_omitted`; it never emits a
partially redacted, internally inconsistent identity record. Exact source bytes
remain represented by the document digest and available from the repository.

Diagnostics use the structured contract above. Manifest JSON object and array
ordering is deterministic for a fixed schema, profile, crate version, and input.
Pretty-print whitespace and human diagnostic messages are not a durable content
identity. Consumers fingerprint exact documents from their recorded digests or
use a separately specified repository fingerprint rather than hashing display
JSON.

Manifest readers must reject unknown `schema_version` values and ignore unknown
fields within a known version. Adding an optional field or diagnostic code is
compatible. Removing or reinterpreting a required field, changing path or hash
semantics, or changing a normalization rule under the same profile is not.

## Security and no-Python boundary

All source bytes are untrusted data.

The library:

- has no Python runtime integration and never imports a module;
- never executes a class, function, custom generation hook, processor, pipeline,
  adapter, quantizer, or scheduler named by source;
- never renders or evaluates a chat template;
- never calls a network transport or resolves an external repository;
- never loads a model weight or pickle;
- validates repository paths before host-path conversion;
- does not use class names as Rust module paths, dynamic-library names, shell
  fragments, or commands; and
- bounds parsing and diagnostics so maliciously deep or large documents fail
  explicitly rather than causing unbounded work.

`trust_remote_code`-style metadata is evidence that another ecosystem may need
code execution. It is never an opt-in switch in this crate. Even a caller that
trusts the repository must hand executable work to a different, explicitly
authorized subsystem.

Debug output for public types is metadata-only. It may report kinds, counts,
presence flags, byte lengths, and portable source locations, but it does not
render exact JSON values, normalized unknown-field values, diagnostic prose,
chat-template text, file contents, or absolute repository roots. Callers use
the explicit lossless/source-value accessors when that sensitive detail is
required. Compatibility manifests likewise do not embed chat-template text or
whole source documents.

## Compatibility policy

### Source layer

Exact-byte access, repository path identity, supported kind classification, and
unknown-field retention are v0.1 behavioral guarantees. New public enums remain
non-exhaustive so future kinds and diagnostics can be represented safely.

### Normalization layer

`dinoml-v1` is immutable after release. A change that can alter normalized
output for previously valid bytes—new precedence, a new architecture-to-task
mapping, a changed default, different component classification, or a newly
recognized document participating in the manifest—uses a new opt-in profile.

The crate may add support for new raw field access without changing a profile.
It may also fix a violation of this RFC, but the release notes and conformance
fixtures must identify the correction; if valid prior output changes, a new
profile is preferred.

### Upstream ecosystems

Compatibility is a conformance claim against pinned source and repository
fixtures, not “whatever Python is installed.” The Transformers and Diffusers
versions recorded inside repository files are provenance and rule-selection
inputs; they do not cause a dependency to be installed or imported.

An upstream release may add fields or enum values without breaking the generic
source layer. Claiming its defaults or merge behavior requires an audited rule
set and fixtures. Provider-specific quantization and adapter extensions remain
open data until separately supported.

### Rust API and manifests

Public types are documented, `Debug`, and forward-compatible where their value
set is open. Errors and diagnostics expose stable classification rather than
requiring message parsing. After the v0.1 release, Rust API changes follow
Semantic Versioning and manifest schema/profile changes follow the stricter
rules in this RFC.

## Acceptance criteria

- All supported basenames are recognized at root and nested safe paths while
  excluded metadata trees are ignored.
- Tests prove exact byte and unknown-field retention.
- Malformed JSON, duplicate keys, invalid UTF-8, wrong root shape, and wrong
  field types remain diagnosable without pretending a typed view exists.
- Root and architecture precedence exactly match this RFC.
- A `model_index.json` without a task does not become text-to-image.
- Dedicated generation and chat-template precedence has conflict fixtures.
- Every non-explicit normalized value has stable rule provenance.
- Component, safetensors shard, adapter, and executable-code references have
  safe-path and missing-target coverage.
- Manifests are deterministic, portable, schema-versioned, profile-versioned,
  and insensitive to host root paths.
- Representative Transformers, Diffusers, PEFT, tokenizer, processor,
  quantization, chat-template, and sharded-index fixtures pass conformance.
- No test or implementation imports Python or executes code named by a fixture.
- Formatting, Clippy with warnings denied, all tests, and rustdoc with warnings
  denied pass on the repository MSRV and current Rust.

## Accepted decisions

The material decisions are recorded separately:

1. [ADR 0001](../adr/0001-source-documents-and-path-identity.md) fixes source
   preservation and path identity.
2. [ADR 0002](../adr/0002-normalization-profiles-and-provenance.md) fixes
   normalization precedence, defaults, and unknown-field handling.
3. [ADR 0003](../adr/0003-data-only-validation-boundary.md) fixes reference
   validation and the no-Python security boundary.
4. [ADR 0004](../adr/0004-manifest-and-diagnostic-stability.md) fixes manifest,
   diagnostic, and compatibility stability.

## References

- [RFC 8259: The JavaScript Object Notation (JSON) Data Interchange Format](https://www.rfc-editor.org/rfc/rfc8259)
- [Transformers configuration documentation](https://huggingface.co/docs/transformers/main/main_classes/configuration)
- [Transformers audited configuration source](https://github.com/huggingface/transformers/blob/b75feb2af64c3e29cbbc1bd859958c5432cc7ed4/src/transformers/configuration_utils.py)
- [Transformers generation configuration source](https://github.com/huggingface/transformers/blob/b75feb2af64c3e29cbbc1bd859958c5432cc7ed4/src/transformers/generation/configuration_utils.py)
- [Transformers tokenizer loading source](https://github.com/huggingface/transformers/blob/b75feb2af64c3e29cbbc1bd859958c5432cc7ed4/src/transformers/tokenization_utils_base.py)
- [Transformers processor loading source](https://github.com/huggingface/transformers/blob/b75feb2af64c3e29cbbc1bd859958c5432cc7ed4/src/transformers/processing_utils.py)
- [Transformers chat-template documentation](https://huggingface.co/docs/transformers/main/chat_templating_writing)
- [Transformers sharded-checkpoint documentation](https://huggingface.co/docs/transformers/main/big_models)
- [Diffusers configuration documentation](https://huggingface.co/docs/diffusers/api/configuration)
- [Diffusers audited configuration source](https://github.com/huggingface/diffusers/blob/b3a515080752a3ba7ca92161e25530c7f280f629/src/diffusers/configuration_utils.py)
- [Diffusers audited pipeline source](https://github.com/huggingface/diffusers/blob/b3a515080752a3ba7ca92161e25530c7f280f629/src/diffusers/pipelines/pipeline_utils.py)
- [Diffusers pipeline component format](https://github.com/huggingface/diffusers/blob/b3a515080752a3ba7ca92161e25530c7f280f629/src/diffusers/pipelines/README.md)
- [PEFT configuration documentation](https://huggingface.co/docs/peft/main/package_reference/config)
- [PEFT checkpoint format](https://huggingface.co/docs/peft/main/developer_guides/checkpoint)
