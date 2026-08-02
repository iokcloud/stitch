# 从源码构建 Stitch

## 前置依赖（Windows）

| 依赖 | 用途 | 获取 |
| ---- | ---- | ---- |
| Rust 工具链（1.85+） | Rust 编译 | https://rustup.rs |
| MSVC Build Tools | Windows 原生链接（含 C++ 工具集） | https://visualstudio.microsoft.com/visual-cpp-build-tools/ |
| Node.js 18+ | 前端构建（SvelteKit SSG） | https://nodejs.org |
| WebView2 Runtime | 运行窗口（Win10/11 自带，旧系统需装） | https://developer.microsoft.com/microsoft-edge/webview2/ |

## 国内网络镜像（可选但推荐）

```bash
# crates.io 镜像（清华）
export CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
# 在 ~/.cargo/config.toml 写入：
#   [source.crates-io]
#   replace-with = "tuna"
#   [source.tuna]
#   registry = "sparse+https://mirrors.tuna.tsinghua.edu.cn/crates.io-index/"
```

npm 镜像：`npm config set registry https://registry.npmmirror.com`

## 构建步骤

```bash
git clone https://github.com/PROMPTSTDIO_REPO/stitch.git
cd stitch

# 1. 构建前端（SvelteKit SSG，产物嵌入 exe）
cd crates/stitch-desktop/frontend
npm install
npm run build
cd ../../..

# 2. 编译桌面端
cargo build --release -p stitch-desktop

# 产物
#   target/release/stitch-desktop.exe          （绿色版，双击即用）
#   target/release/bundle/nsis/Stitch_*.exe    （安装包）
```

也可以直接运行 `scripts/install.ps1`（PowerShell 一键：检测环境 → 装缺失依赖 → 配置镜像 → 构建 → 启动）。

## 运行测试

```bash
# Rust 单测（含 stitch lib 200+ 用例）
cargo test --workspace

# 前端类型检查
cd crates/stitch-desktop/frontend && npm run check

# Layer A 冒烟（mock IPC，无需真实模型）
bash scripts/smoke-ui.sh

# Layer B 冒烟（真实 exe + webdriver）
bash scripts/smoke-desktop.sh
```

## 常见问题

- **首次编译慢**：tauri + 依赖树首次需 5-15 分钟，之后增量编译秒级
- **cargo 拉取失败**：检查镜像配置（见上）
- **缺 WebView2**：旧系统运行 exe 报错时安装 WebView2 Runtime
- **SmartScreen 提示**：本地构建的 exe 无签名，Windows 首次运行会提示，点「更多信息」→「仍要运行」；这是所有未签名软件的标准行为，可核对源码自行构建后运行
