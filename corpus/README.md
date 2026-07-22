# Configuration corpus

The corpus itself is external. This directory contains only reproducible inventory metadata and the tooling contract used to build it; no third-party model configuration is committed.

The current audit uses two inputs:

- Diffusers reports under `agents/plans/diffusers/**/report.md`.
- Transformers reports under `agents/plans/transformers/**/report.md`.

`report-candidates.json` is the deterministic, line-attributed extraction result. It intentionally includes low-confidence path-shaped text so extraction mistakes are auditable. `report-repositories.json` contains only candidates for which the Hugging Face API returned a concrete commit revision. `unresolved-report-candidates.json` retains the rest with their fetch status. A `401` cannot prove that a repository exists because the Hub uses that response for both inaccessible and invalid identifiers.

`audit.json` is the machine-readable corpus inventory and `AUDIT.md` is its concise rendering. Byte hashes are used only for duplicate accounting; duplicates are not deleted because identical bytes in different repositories remain distinct source documents.

## Supported documents

The audit recognizes these basenames at the repository root or any safe nested component path:

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

Paths must be relative UTF-8 slash-separated repository paths. Empty, `.`,
`..`, dot-prefixed, backslash-containing, control-character, Windows-reserved,
drive/prefix-like, trailing-dot/space, oversized segment, and oversized total
path spellings are excluded before host-path conversion. This omits Hugging
Face `.cache` sidecars and prevents metadata from being mistaken for source
configuration. Root files outside an `owner/repository` directory are also
excluded.

Corpus owner, repository, component, and file entries must be ordinary
directories or regular files. The audit fails on a symbolic link or Windows
reparse point instead of traversing an alias. Fetches apply the same check to
every existing target component and verify resolved containment before an
atomic replacement, so a pre-existing alias cannot redirect a write outside
the selected corpus root. Audit reads use a metadata precheck and the same
bounded-reader fallback, so manually populated files above 64 MiB fail with
their logical corpus path before they can become source evidence.

JSON validation is strict UTF-8 JSON. In particular, bare `NaN`, `Infinity`, and `-Infinity` are reported as invalid even though Python's default JSON decoder accepts them. Duplicate object keys remain valid source JSON but are reported separately because last-key-wins normalization would otherwise lose information.

## Reproduce

The tool uses only the Python standard library and treats configuration as inert bytes/data. It never imports remote Python or executes code named by a document.

From the repository root, extract report evidence:

```powershell
python tools/corpus_inventory.py extract `
  --reports diffusers=H:\dinoml_v2_agents\agents\plans\diffusers `
  --reports transformers=H:\dinoml_v2_agents\agents\plans\transformers `
  --output corpus\report-candidates.json
```

Fetch supported files into the external corpus and record the resolved revisions. Anonymous Hub API access is limited to 500 repository lookups per five-minute window, so batches of 450 are resumable:

```powershell
python tools/corpus_inventory.py fetch `
  --corpus H:\configs `
  --repositories corpus\report-candidates.json `
  --metadata H:\configs\.model-configs-rs\fetch-manifest.json `
  --workers 8 --resume --limit 450
```

Repeat that command until no `http_429`, transient per-file `partial`, or
network statuses remain. A partial containing only `401`, `too_large`, or
`unsafe_filesystem_path` file results is permanent and is not retried. HTTP
response bodies are read only through the v0.1 64 MiB source-document limit;
one additional byte is inspected to produce the `too_large` outcome. The
manifest is revision-pinned and written atomically after each batch. Set
`HF_TOKEN` to use authenticated Hub access; the token is sent as an
authorization header to the requested origin, stripped from cross-origin
redirects, and never recorded.

Separate resolved repositories from unresolved extraction candidates:

```powershell
python tools/corpus_inventory.py resolve `
  --candidates corpus\report-candidates.json `
  --fetch-manifest H:\configs\.model-configs-rs\fetch-manifest.json `
  --output corpus\report-repositories.json `
  --unresolved-output corpus\unresolved-report-candidates.json
```

Then audit the full external corpus while reporting coverage for revision-backed report repositories:

```powershell
python tools/corpus_inventory.py audit `
  --corpus H:\configs `
  --repositories corpus\report-repositories.json `
  --fetch-manifest H:\configs\.model-configs-rs\fetch-manifest.json `
  --output corpus\audit.json `
  --markdown corpus\AUDIT.md
```

The hermetic behavior tests require no network or external corpus:

```powershell
python -m unittest discover -s tools -p 'test_*.py' -v
```
