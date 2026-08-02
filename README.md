# Stitch

本机运行的开源桌面 AI 智能体。连你自己的模型密钥，在电脑上干活——读文件、改代码、跑命令、操作桌面，干完活把流程沉淀成可复用的 Skill。

![demo](docs/demo.gif)

## 为什么用 Stitch

- **本机运行**：模型密钥、会话记录、Skill 资产都存在本机，不经过第三方服务器
- **模型自由**：自带密钥，支持 DeepSeek、智谱、Kimi、MiniMax 等 OpenAI 兼容端点，以及 Ollama 本地模型
- **能干活的 Agent**：计划先行 → 工具执行 → 关键步确认 → 交付结果；内置桌面自动化（窗口列表与操作、点击、键入、截图、OCR 读屏）
- **沉淀资产**：把做过的流程录制保存为 Skill，下次一句话复用；支持标准 MCP 服务器
- **开源可审计**：MIT 许可证，代码全公开，本地运行无遥测

## 快速开始

### 方式一：下载安装包（推荐）

- [Stitch 0.1.7 安装包](https://www.promptstdio.com/downloads/Stitch_0.1.7_x64-setup.exe)（Windows x64 · NSIS）
- 绿色版：[Stitch 0.1.7 portable](https://www.promptstdio.com/downloads/Stitch_0.1.7_portable.exe)（单 exe，免安装）
- SHA256：安装包 `e42569265e800ede6cf657faf3e1ad94ea7d8181cc00b6445ad6c6baddaf0fce` · 绿色版 `d841ed9966bdd2b6e43432650fa59329f3a2adc703717c7420ae50b304c67c33`

首次安装若 Windows 提示「无法识别」，点「更多信息」→「仍要运行」继续即可。可在下载后核对上方校验和。

### 方式二：一键构建（无安装包体验）

PowerShell 执行：

```powershell
irm https://raw.githubusercontent.com/PROMPTSTDIO_REPO/stitch/main/scripts/install.ps1 | iex
```

脚本自动检测环境、安装缺失依赖、配置国内镜像、构建并启动。约 10 分钟出产物。

### 方式三：从源码构建

```bash
git clone https://github.com/PROMPTSTDIO_REPO/stitch.git
cd stitch
cargo build --release -p stitch-desktop
```

前置依赖与详细步骤见 [BUILDING.md](docs/BUILDING.md)。

## 能力

| 能力 | 说明 |
| ---- | ---- |
| 多模型接入 | DeepSeek / 智谱 / Kimi / MiniMax / Ollama 等 OpenAI 兼容端点，自带密钥 |
| 桌面自动化 | Win32 原生：窗口管理、点击、键入、滚动、截图 + OCR 读屏、浏览器 DOM 控制 |
| 计划与确认 | 计划先行给人审，危险步确认；允许规则可记住「此目录内自动放行」 |
| 沉淀闭环 | 录制回放保存 Skill · 会话候选一键保存 · 支持提交到 PromptStdio Explore |
| 本地视觉 | 图片消息自动交给本地视觉模型描述（Ollama + qwen3-vl） |
| 标准 MCP | 连接任意 stdio / Streamable HTTP MCP 服务器，工具自动注入 Agent |
| 会话自愈 | 上下文分层压缩、检查点回滚、中断续跑 |

## 开源承诺

- **开源可审计**：全部源码公开，可自行审查与构建
- **本地优先**：会话与资产默认在本机，无强制上传
- **资产可导出**：Skill 是 Markdown + TOML 文本，随时可带走、可迁移到其他工具
- **模型中立**：不绑定任何模型厂商，密钥归你

## 反馈与贡献

- 使用问题与建议：GitHub Issues
- 贡献指南：[CONTRIBUTING.md](CONTRIBUTING.md)
- 项目结构：[crates/README.md](crates/README.md)

## License

[MIT](LICENSE)
