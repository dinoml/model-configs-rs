# Curated conformance fixtures

These small configuration-only fixtures are retained for deterministic, offline
conformance tests. They do not include weights or executable remote code.

- `qwen3-tiny/config.json` is from `llamafactory/tiny-random-qwen3` revision
  `81d6f5f5e05ed53ea8a1d19431266a486e46bbd8`, collected by the DinoML Qwen3
  source audit on 2026-07-22. The fixture adds a final LF; its SHA-256 is
  `1a4902678c080a6747be22cee1b42a5f59c43ee2050a3440b7b42c18f4733e40`.
  `qwen3-tiny/generation_config.json` is from `Qwen/Qwen3-0.6B` revision
  `c1899de289a04d12100db370d81485cdf75e47ca`; it was JSON-reserialized with
  two-space indentation and a final LF and has SHA-256
  `81e8e13e77962857509cc06a9960bb68f8b7893096a60357627b2dfaa72d78fe`.
- `flux-schnell/` is from `black-forest-labs/FLUX.1-schnell` revision
  `741f7c3ce8b383c54771c7003378a50191e9efe9`, collected on 2026-07-22.
- `real-formats/` contains one small source document for each remaining format:
  PEFT from `jinaai/jina-embeddings-v3-hf` path
  `retrieval_query/adapter_config.json` at revision
  `d18862d9a48706220815554fac3ebb4dfa46fc28`; processor metadata from
  `BAAI/Emu3-Chat-hf` revision `414c0a163edad789827ee473a71b75c7de546347`;
  quantization metadata from `Intel/GLM-Image-int4-AutoRound` path
  `transformer/quantization_config.json` at revision
  `51bed3b07311705885ffe3e732672f914e8e45bf`; a chat template and checkpoint
  index from `hf-internal-testing/olmo-hybrid` revision
  `8db450a26dd667a5fd22727fd46378433a52bb84` and
  `hf-internal-testing/cohere-random` revision
  `1259306580d0b4305319eec03ac5f77875599aa3`; and tokenizer metadata from
  `albert/albert-base-v2` revision `8e2f239c5f8a2c0f253781ca60135db913e5c80c`.
  The preprocessor sample is from `BridgeTower/bridgetower-large-itm-mlm`
  revision `a09c7040bd151773f3bc7ea0eb47a0065720eff1`.
  The special-token map is the exact 149-byte `special_tokens_map.json` from
  `abhinand/GOT-OCR-2.0-unofficial` revision
  `4e7a42d71d1a84b51039908cd5ba5fd619b2968e`, SHA-256
  `337f1a03344485ec7ea2acd6f7021567feab077fd81f96b72423fcd3f8f5fc09`.

  Three source files were retained with one final LF added: the OLMo Hybrid
  chat template has SHA-256
  `4d773c6800c326634a217c28d11c096dced7551c3af32ef35f59f0cb20881a72`,
  the ALBERT tokenizer config has SHA-256
  `9adb6f52e3c0d3cc28b7fd21615f46a2b14f771cf435c85c893ecd6bf17683f2`,
  and the GLM-Image quantization config has SHA-256
  `8ba1c893f5532ae3aa3faf82f3ceda80c2bda3b27e768607493005c1fd9b8915`.
  All other `real-formats/` files match the recorded upstream bytes exactly.
  `.gitattributes` marks all fixture blobs as non-text so checkout does not
  perform further line-ending conversion.

The upstream repositories supply their own model licenses. These JSON files are
included only as interoperability test data and retain their source spelling
apart from the explicitly recorded deterministic transformations above.
