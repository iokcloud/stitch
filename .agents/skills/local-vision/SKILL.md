---
name: local-vision
description: >-
  Local image recognition (qwen3-vl:8b via Ollama) and image generation (ComfyUI).
  Use when user drags/pastes an image and asks to describe, analyze, OCR, or
  answer questions about it. Also use when user asks to generate images with
  local Stable Diffusion / Flux models. All local, zero API tokens.
---

# Local Vision & Image Gen（本地识图 / 生图）

用本地模型处理图像，不消耗云端 token。

## 触发条件

- 用户拖入/粘贴图片，要求描述、分析、OCR、问答
- 用户说「生成一张图」「帮我画个…」
- 用户说「用本地模型识别这张图」

## 架构

```text
识图  qwen3-vl:8b → Ollama (port 11434) → OpenAI-compatible API
生图  Stable Diffusion / Flux → ComfyUI (port 8188) → REST API
```

## 前置检查

### Ollama（识图）

```bash
# 检查是否在运行
curl -s http://localhost:11434/v1/models -H "Authorization: Bearer ollama"

# 检查模型是否已安装
ollama list | grep qwen3-vl

# 如果没运行，手动启动（非服务，用完可关）
ollama serve
```

### ComfyUI（生图）

```bash
# 检查是否在运行
curl -s http://localhost:8188/system_stats

# 如果没有，启动
# 在你的 ComfyUI 安装目录启动
cd <你的 ComfyUI 目录> && python main.py --port 8188
```

## 识图：调用方式

用 Python 调 Ollama OpenAI 兼容端点，不需要额外依赖：

```python
import base64, json, urllib.request
from pathlib import Path

def describe_image(image_path: str, prompt: str = "请详细描述这张图片的内容") -> str:
    """Send an image to local qwen3-vl:8b and get a description."""
    ext = Path(image_path).suffix.lower()
    mime_map = {".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg",
                ".webp": "image/webp", ".gif": "image/gif"}
    mime = mime_map.get(ext, "image/png")

    b64 = base64.b64encode(Path(image_path).read_bytes()).decode()

    req = urllib.request.Request(
        "http://localhost:11434/v1/chat/completions",
        data=json.dumps({
            "model": "qwen3-vl:8b",
            "max_tokens": 2048,  # reasoning model needs headroom; <512 often gets cut off mid-think
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": prompt},
                    {"type": "image_url", "image_url": {"url": f"data:{mime};base64,{b64}"}}
                ]
            }]
        }).encode(),
        headers={"Content-Type": "application/json", "Authorization": "Bearer ollama"}
    )
    resp = json.loads(urllib.request.urlopen(req, timeout=60).read())
    return resp["choices"][0]["message"]["content"]
```

常用 prompt：
- 描述图片：`"请详细描述这张图片的内容，包括文字、布局、颜色等细节"`
- OCR 提取文字：`"请提取图片中的所有文字，按原格式输出"`
- 回答特定问题：`"根据图片内容回答：{user question}"`

## 生图：调用方式

使用 `gen` CLI（`~/bin/gen`），一行命令出图，无需构造 workflow JSON。

```bash
# 检查状态
gen status

# 基础生图
gen "a cat sleeping on a laptop, warm lighting"

# 高级参数
gen "cyberpunk city at night" \
  --steps 30 --width 768 \
  --checkpoint Juggernaut-XL_v9_RunDiffusionPhoto_v2.safetensors \
  --negative "blurry, distorted" \
  --output tmp/my-image.png

# JSON 输出（机器可读）
gen "sunset" --json
```

### 启动 ComfyUI

ComfyUI 未运行时 `gen` 会报错提示启动命令：

```bash
cd ~/ComfyUI_new && ./venv/Scripts/python main.py --port 8188
```

约 10 秒就绪，CUDA 自动可用（RTX 4060 Laptop ~23s/512px）。

### 可用参数

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `-s, --steps` | 20 | 采样步数 |
| `-c, --cfg` | 7 | CFG scale |
| `-W, --width` | 512 | 宽度 |
| `-H, --height` | 512 | 高度 |
| `-k, --checkpoint` | sd_xl_base_1.0 | 模型名 |
| `-n, --negative` | blurry, low quality | 负向词 |
| `--sampler` | dpmpp_2m | 采样器 |
| `--scheduler` | karras | 调度器 |
| `--seed` | 随机 | 固定种子 |
| `-o, --output` | tmp/gen-时间戳.png | 输出路径 |
| `--json` | - | 结构化输出 |

### 已安装模型

- `sd_xl_base_1.0.safetensors` (SDXL base)
- `Juggernaut-XL_v9_RunDiffusionPhoto_v2.safetensors` (写实风格)
- LoRAs: `cinematic_lighting_sdxl`, `extreme_detail_sdxl`, `face_helper_sdxl`, `photorealistic_sdxl`, `simpstyle_sdxl`

## 已安装模型

| 功能 | 模型 | 后端 | 大小 | 实测速度 |
|------|------|------|------|----------|
| 识图 | qwen3-vl:8b (Q4_K_M) | Ollama | 6.2GB | ~8s/图 |
| 生图 | sd_xl_base_1.0 | ComfyUI | ~6.5GB | ~23s/512px |
| 生图 | Juggernaut-XL v9 | ComfyUI | ~6.5GB | ~23s/512px |
| 生图 LoRA | cinematic / detail / face / photo / simp | ComfyUI | 各~250MB | — |

GPU: NVIDIA GeForce RTX 4060 Laptop (8GB), PyTorch 2.5.1+cu121

## 注意事项

- **识图必须有 GPU**：qwen3-vl:8b 虽能 CPU 跑，但极慢 (~40-60s)；你的 4060 8GB 刚好够
- **显存余量紧张**：生图和识图模型都 ~6GB，同时加载会爆显存，用时二选一
- **用后即关**：两个服务都不需要常驻，用完关掉
- **ComfyUI 用了 venv**：必须 `./venv/Scripts/python main.py` 而不是系统 python
