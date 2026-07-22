# Curated conformance fixtures

These small configuration-only fixtures are retained for deterministic, offline
conformance tests. They do not include weights or executable remote code.

- `qwen3-tiny/` is from `llamafactory/tiny-random-qwen3`, collected by the
  DinoML Qwen3 source audit on 2026-07-22. The generation configuration is from
  `Qwen/Qwen3-0.6B` at the same audit point.
- `flux-schnell/` is from `black-forest-labs/FLUX.1-schnell` revision
  `741f7c3ce8b383c54771c7003378a50191e9efe9`, collected on 2026-07-22.
- `real-formats/` contains one small source document for each remaining format:
  PEFT from `jinaai/jina-embeddings-v3-hf` revision
  `d18862d9a48706220815554fac3ebb4dfa46fc28`; processor metadata from
  `BAAI/Emu3-Chat-hf` revision `414c0a163edad789827ee473a71b75c7de546347`;
  quantization metadata from `Intel/GLM-Image-int4-AutoRound` revision
  `51bed3b07311705885ffe3e732672f914e8e45bf`; a chat template and checkpoint
  index from `hf-internal-testing/olmo-hybrid` revision
  `8db450a26dd667a5fd22727fd46378433a52bb84` and
  `hf-internal-testing/cohere-random` revision
  `1259306580d0b4305319eec03ac5f77875599aa3`; and tokenizer metadata from
  `albert/albert-base-v2` revision `8e2f239c5f8a2c0f253781ca60135db913e5c80c`.
  The preprocessor sample is from `BridgeTower/bridgetower-large-itm-mlm`
  revision `a09c7040bd151773f3bc7ea0eb47a0065720eff1`.

The upstream repositories supply their own model licenses. These JSON files are
included only as interoperability test data and retain their source spelling.
