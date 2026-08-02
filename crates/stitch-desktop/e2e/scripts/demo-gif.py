"""Stitch demo frames -> README GIF.

用法（在 e2e 目录）:
    python scripts/demo-gif.py artifacts/demo-launch/frames artifacts/demo-launch/demo.gif

规则:
    - 每帧缩放到宽 960，裁剪 4:3
    - 丢弃「工作帧」之间完全相同的帧（去重，控制体积）
    - 每帧停留 400ms，循环 0 次（播完停）
"""
import sys
from pathlib import Path

from PIL import Image, ImageOps

WIDTH = 960
HEIGHT = 720
DURATION_MS = 400


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    frames_dir = Path(sys.argv[1])
    out_path = Path(sys.argv[2])

    files = sorted(frames_dir.glob("frame-*.png"))
    if not files:
        print(f"no frames in {frames_dir}")
        return 1

    frames: list[Image.Image] = []
    last_key = None
    for f in files:
        img = Image.open(f).convert("RGB")
        # 去重：相同内容的连续帧跳过（工作态停留帧）
        key = img.resize((240, 180)).tobytes()
        if key == last_key:
            continue
        last_key = key
        # 居中裁剪到 4:3 再缩放
        img = ImageOps.fit(img, (WIDTH, HEIGHT), method=Image.LANCZOS)
        frames.append(img)

    if len(frames) < 2:
        print(f"only {len(frames)} unique frames; keep at least 2")
        return 1

    frames[0].save(
        out_path,
        save_all=True,
        append_images=frames[1:],
        duration=DURATION_MS,
        loop=0,
        optimize=True,
    )
    print(f"demo.gif: {len(frames)} unique frames -> {out_path} ({out_path.stat().st_size // 1024} KB)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
