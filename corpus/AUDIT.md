# External corpus audit

This is a deterministic summary. The third-party documents remain in the external corpus and are not committed.

## Summary

- Repository directories: 10,780
- Repositories with supported documents: 10,780
- Supported documents: 16,445
- Valid JSON documents: 16,221 / 16,246
- Invalid JSON documents: 25
- JSON documents with duplicate object keys: 5
- Duplicate byte copies after the first occurrence: 11,157
- Report-referenced repositories present: 1,620 / 1,718
- Report-referenced repositories with supported documents: 1,620 / 1,718

## Document kinds

| Kind | Files | Unique byte contents |
|---|---:|---:|
| `*.safetensors.index.json` | 526 | 351 |
| `adapter_config.json` | 18 | 15 |
| `chat_template.jinja` | 199 | 101 |
| `config.json` | 2,327 | 1,855 |
| `generation_config.json` | 610 | 367 |
| `model_index.json` | 9,448 | 1,157 |
| `preprocessor_config.json` | 726 | 323 |
| `processor_config.json` | 136 | 64 |
| `quantization_config.json` | 4 | 4 |
| `scheduler_config.json` | 240 | 143 |
| `special_tokens_map.json` | 971 | 267 |
| `tokenizer_config.json` | 1,240 | 641 |

## Semantic empty objects

| Kind | Empty objects |
|---|---:|
| `config.json` | 2 |
| `model_index.json` | 4,727 |
| `preprocessor_config.json` | 3 |
| `special_tokens_map.json` | 3 |
| `tokenizer_config.json` | 2 |

## Invalid JSON

| Path | Location | Error |
|---|---:|---|
| `alibaba-pai/EasyAnimateV2-XL-2-768x768/scheduler/scheduler_config.json` | n/a | non-standard JSON constant -Infinity |
| `Efficient-Large-Model/SANA-Video_2B_480p_diffusers/scheduler/scheduler_config.json` | n/a | non-standard JSON constant -Infinity |
| `Efficient-Large-Model/SANA-Video_2B_480p_LongLive_diffusers/longsana/scheduler/scheduler_config.json` | n/a | non-standard JSON constant -Infinity |
| `Efficient-Large-Model/SANA-Video_2B_480p_LongLive_diffusers/scheduler/scheduler_config.json` | n/a | non-standard JSON constant -Infinity |
| `Efficient-Large-Model/SANA-Video_2B_720p_diffusers/scheduler/scheduler_config.json` | n/a | non-standard JSON constant -Infinity |
| `Efficient-Large-Model/SANA1.5_1.6B_1024px_diffusers/scheduler/scheduler_config.json` | n/a | non-standard JSON constant -Infinity |
| `Efficient-Large-Model/SANA1.5_4.8B_1024px_diffusers/scheduler/scheduler_config.json` | n/a | non-standard JSON constant -Infinity |
| `Efficient-Large-Model/Sana_1600M_1024px_diffusers/scheduler/scheduler_config.json` | n/a | non-standard JSON constant -Infinity |
| `Efficient-Large-Model/Sana_1600M_4Kpx_BF16_diffusers/scheduler/scheduler_config.json` | n/a | non-standard JSON constant -Infinity |
| `hunyuanvideo-community/HunyuanVideo/config.json` | 5:1 | Expecting property name enclosed in double quotes |
| `katuni4ka/tiny-random-sana/scheduler/scheduler_config.json` | n/a | non-standard JSON constant -Infinity |
| `kurogane/Nemotron-H-micro-test03/config.json` | n/a | non-standard JSON constant Infinity |
| `MVRL/VectorSynth/scheduler/scheduler_config.json` | n/a | non-standard JSON constant -Infinity |
| `nvidia/Nemotron-H-47B-Reasoning-128K-FP8/config.json` | n/a | non-standard JSON constant Infinity |
| `nvidia/Nemotron-H-47B-Reasoning-128K/config.json` | n/a | non-standard JSON constant Infinity |
| `nvidia/Nemotron-H-4B-Base-8K/config.json` | n/a | non-standard JSON constant Infinity |
| `nvidia/Nemotron-H-56B-Base-8K/config.json` | n/a | non-standard JSON constant Infinity |
| `nvidia/Nemotron-H-8B-Base-8K/config.json` | n/a | non-standard JSON constant Infinity |
| `nvidia/Nemotron-H-8B-Reasoning-128K/config.json` | n/a | non-standard JSON constant Infinity |
| `nvidia/NVIDIA-Nemotron-3-Nano-30B-A3B-Base-BF16/config.json` | n/a | non-standard JSON constant Infinity |
| `nvidia/NVIDIA-Nemotron-Nano-12B-v2/config.json` | n/a | non-standard JSON constant Infinity |
| `PixArt-alpha/PixArt-Sigma-XL-2-1024-MS/scheduler/scheduler_config.json` | n/a | non-standard JSON constant -Infinity |
| `PixArt-alpha/PixArt-XL-2-1024-MS/scheduler/scheduler_config.json` | n/a | non-standard JSON constant -Infinity |
| `PixArt-alpha/PixArt-XL-2-512x512/scheduler/scheduler_config.json` | n/a | non-standard JSON constant -Infinity |
| `westlake-repl/Evolla-80B/config.json` | 1:1 | Expecting value |

## Duplicate object keys

| Path | Keys | Repeated occurrences |
|---|---|---:|
| `h94/IP-Adapter/sdxl_models/image_encoder/config.json` | `architectures` | 1 |
| `impira/layoutlm-document-qa/tokenizer_config.json` | `add_prefix_space` | 1 |
| `kandinsky-community/kandinsky-2-2-prior/image_encoder/config.json` | `architectures` | 1 |
| `kandinsky-community/kandinsky-2-2-prior/text_encoder/config.json` | `_name_or_path`, `architectures` | 2 |
| `salesforce/blip-vqa-base/tokenizer_config.json` | `model_input_names` | 1 |

## Missing report coverage

| Repository | Directory present | Fetch status |
|---|---:|---|
| `ai21labs/AI21-Jamba-1.5-Large` | no | partial |
| `ai21labs/AI21-Jamba-1.5-Mini` | no | partial |
| `alandao/open-chameleon` | no | ok |
| `arcee-ai/AFM-4.5B-GGUF` | no | ok |
| `BAAI/SegGPT` | no | ok |
| `black-forest-labs/FLUX.1-Fill-dev` | no | partial |
| `black-forest-labs/FLUX.2-dev` | no | partial |
| `black-forest-labs/FLUX.2-klein-9B` | no | partial |
| `black-forest-labs/FLUX.2-klein-9b-kv` | no | partial |
| `briaai/FIBO` | no | partial |
| `briaai/Fibo-Edit` | no | partial |
| `CohereLabs/c4ai-command-a-03-2025` | no | partial |
| `CohereLabs/c4ai-command-r7b-12-2024` | no | partial |
| `CohereLabs/cohere-transcribe-03-2026` | no | partial |
| `CohereLabs/command-a-vision-07-2025` | no | partial |
| `facebook/blt-1b` | no | partial |
| `facebook/blt-7b` | no | partial |
| `facebook/blt-entropy` | no | partial |
| `facebook/chameleon-30b` | no | partial |
| `facebook/cwm` | no | partial |
| `facebook/dinov3-convnext-base-pretrain-lvd1689m` | no | partial |
| `facebook/dinov3-convnext-large-pretrain-lvd1689m` | no | partial |
| `facebook/dinov3-convnext-small-pretrain-lvd1689m` | no | partial |
| `facebook/dinov3-convnext-tiny-pretrain-lvd1689m` | no | partial |
| `facebook/dinov3-vit7b16-pretrain-lvd1689m` | no | partial |
| `facebook/dinov3-vit7b16-pretrain-sat493m` | no | partial |
| `facebook/dinov3-vitb16-pretrain-lvd1689m` | no | partial |
| `facebook/dinov3-vith16plus-pretrain-lvd1689m` | no | partial |
| `facebook/dinov3-vitl16-chmv2-dpt-head` | no | partial |
| `facebook/dinov3-vitl16-pretrain-lvd1689m` | no | partial |
| `facebook/dinov3-vitl16-pretrain-sat493m` | no | partial |
| `facebook/dinov3-vits16-pretrain-lvd1689m` | no | partial |
| `facebook/dinov3-vits16plus-pretrain-lvd1689m` | no | partial |
| `facebook/EdgeTAM` | no | ok |
| `facebook/Perception-LM-1B` | no | partial |
| `facebook/Perception-LM-3B` | no | partial |
| `facebook/Perception-LM-8B` | no | partial |
| `facebook/sam3` | no | partial |
| `facebook/seamless-m4t-unity-small` | no | ok |
| `google/gemma-3n-E2B-it` | no | partial |
| `google/gemma-3n-E4B-it` | no | partial |
| `google/medasr` | no | partial |
| `google/paligemma-3b-mix-224` | no | partial |
| `google/paligemma-3b-pt-224` | no | partial |
| `google/paligemma-3b-pt-448` | no | partial |
| `google/recurrentgemma-2b` | no | partial |
| `google/recurrentgemma-9b` | no | partial |
| `google/shieldgemma-2-4b-it` | no | partial |
| `google/t5gemma-2-1b-1b` | no | partial |
| `google/t5gemma-2-270m-270m` | no | partial |
| `google/t5gemma-2-4b-4b` | no | partial |
| `google/t5gemma-2b-2b-prefixlm-it` | no | partial |
| `google/t5gemma-2b-2b-ul2` | no | partial |
| `google/t5gemma-9b-2b-ul2` | no | partial |
| `google/t5gemma-9b-9b-ul2` | no | partial |
| `google/vaultgemma-1b` | no | partial |
| `inceptionai/Jais-2-70B-Chat` | no | partial |
| `inceptionai/Jais-2-8B-Chat` | no | partial |
| `jbetker/tts-scores-clvp` | no | ok |
| `juliendenize/COMEDIAN-ViViT-tiny` | no | ok |
| `kakaobrain/coyo-align-b7-base` | no | ok |
| `karpathy/nanochat-d32` | no | ok |
| `kornia/Efficient_LOFTR` | no | ok |
| `lodestones/meta-chameleon-7b` | no | ok |
| `meta-llama/CodeLlama-13b-Instruct-hf` | no | partial |
| `meta-llama/CodeLlama-34b-hf` | no | partial |
| `meta-llama/CodeLlama-70b-hf` | no | partial |
| `meta-llama/CodeLlama-7b-hf` | no | partial |
| `meta-llama/CodeLlama-7b-Python-hf` | no | partial |
| `microsoft/bitnet-b1.58-2B-4T-gguf` | no | ok |
| `microsoft/focalnet-large-lrf-fl3` | no | ok |
| `microsoft/focalnet-large-lrf-fl4` | no | ok |
| `my3bikaht/univnet-RU` | no | ok |
| `namctin/patchtst_etth1_forecast` | no | partial |
| `nvidia/ChronoEdit-14B-Diffusers-Paint-Brush-Lora` | no | ok |
| `nvidia/ChronoEdit-14B-Diffusers-Upscaler-Lora` | no | ok |
| `nvidia/Cosmos-1.0-Diffusion-14B-Text2World` | no | partial |
| `nvidia/Cosmos-1.0-Diffusion-14B-Video2World` | no | partial |
| `nvidia/Cosmos-1.0-Diffusion-7B-Text2World` | no | partial |
| `nvidia/Cosmos-1.0-Diffusion-7B-Video2World` | no | partial |
| `nvidia/Cosmos-Predict2-14B-Text2Image` | no | partial |
| `nvidia/Cosmos-Predict2-14B-Video2World` | no | partial |
| `nvidia/Cosmos-Predict2-2B-Text2Image` | no | partial |
| `nvidia/Cosmos-Predict2-2B-Video2World` | no | partial |
| `nvidia/Cosmos-Predict2.5-2B` | no | ok |
| `nvidia/Cosmos-Transfer2.5-2B` | no | ok |
| `OFA-Sys/chinese-clip-rn50` | no | ok |
| `omlab/OmDet-Turbo_tiny_SWIN_T` | no | ok |
| `sesame/csm-1b` | no | partial |
| `stabilityai/stable-audio-open-1.0` | no | partial |
| `stevenbucaille/efficient_loftr_pth` | no | ok |
| `Tom9000/not-chameleon-30b` | no | ok |
| `Tom9000/not-chameleon-7b` | no | ok |
| `tue-mps/coco_panoptic_eomt_large_640_dinov3` | no | ok |
| `UsefulSensors/moonshine-streaming` | no | ok |
| `xmanifold/efficient_loftr` | no | ok |
| `zahilaty/EfficientLoFTR-ONNX` | no | ok |
| `ZeroWw/chameleon-7b` | no | ok |
