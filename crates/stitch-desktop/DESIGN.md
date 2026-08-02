# Stitch Desktop — 设计薄规范

独立于网站 [`design-system/`](../../../design-system/)。Token 以 `frontend/src/app.css` 为准；本文只钉协作铁律，避免与主站 Leptos/会员页视觉混用。

文案遵守 [ADR-025](../../../docs/DECISIONS.md) / `copy-tone`：无 emoji · 中性陈述 · 通俗 L1。

## 方向

- **产品面**：Agent 工具工作台（非营销落地页、非会员专区复刻）
- **气质**：瑞士风——克制、清晰层级、少装饰；品牌色对齐 PromptStdio（`#1A2B4C` · `#007BFF`）
- **字体**：Figtree（`static/fonts/`）；正文字号约 13px
- **默认壳尺寸**：1120×740（min 960×640），见 `tauri.conf.json`

## Token（摘要）

| 角色 | Light（IDE 冷灰） | Dark |
| ---- | ----------------- | ---- |
| 页面底 / 侧栏轨 / 聊天区 | `#f4f6f8` / `#eef2f6` / `#ffffff` | `#0f172a` / `#111827` / `#0f172a` |
| 表面 | `#ffffff` | `#111827` |
| 主文字 | `#0f172a` | `#f1f5f9` |
| muted / 边框 | `#5c6b84` / `#d8dee8`（强边 `#b8c0cc`） | `#94a3b8` / `#1e293b` |
| 用户气泡 | 浅蓝底 `#edf3fb` + 深字 | slate 底 |
| 助手气泡 | 白底 + 左边强调色 | 透明 + 左边强调色 |
| 圆角 | `--radius-sm/md/lg` = 4/8/10px，基准 `--radius` = 6px | 同左 |
| 侧栏宽 | `256px` | 同左 |
| 终端壳 | `--color-shell-bg/fg/muted/prompt`（两主题均深壳，暗色更深） | `#020617` |
| 语义补充 | `--color-on-accent`（accent 上文字）· `--color-danger`（红点）· `--color-shadow` | 同左 |

全局控件规范：`::selection` = accent 24%；按钮/链接 `:focus-visible` = accent 焦点环；滚动条 hover 加深至 `--color-border-strong`；代码高亮浅主题 `github.css`，暗主题用 `app.css` 末尾 `[data-theme="dark"] .hljs-*` token 组。**禁止**在组件里写死 hex 色值与圆角像素，一律走 token。

日间走 **侧栏灰 / 聊天白**（忌暖米色、忌整窗同色脏灰）；输入区与聊天同白底；顶栏 pill；发送品牌蓝 `#007BFF`。品牌海军 `#1A2B4C` 留给 logo / 强调，不作大面积底色。

主题：默认 **跟随系统**（`stitch-theme=system`）；三种偏好 `浅色 / 深色 / 跟随系统`（`light` / `dark` / `system`；图标分别为太阳 / 月亮 / 显示器）。设置「通用」横向分段；顶栏按钮循环切换。解析结果写在 `[data-theme="light|dark"]`。勿引入第二套紫色渐变或「AI 通用脸」配色。

## 布局铁律

1. **主壳铺满窗口**：`#app-root` → `.app-frame` / `.app-shell` 必须形成完整高度链；聊天区是纯 flex，中间层无高度会塌成 0（设置→聊天已踩坑）。
2. **可点击优先**：揭窗后 `#app-root` 须不透明且 `pointer-events: auto`；禁止长时间用透明层 / `display: contents` 挡 hit-test。
3. **设置页**：桌面偏好壳（左栏分类 + 右栏内容 + 底栏操作），**不是**居中 Web 表单大卡片。
4. **启动**：`#app-loader` 仅过渡；先 reveal 再挂长监听，避免「看得见点不动」。
5. **卡片**：默认少用 card；Plan / 确认 / 工具展开等**交互容器**才用卡片样式。

## 开箱与沉淀（对齐 PRODUCT-NORTH-STAR）

1. **首启只卡模型 Key**；MCP / 工作目录不阻断进聊天。
2. **保存并开始**后可 nudge 一次工作目录（取消即跳过）。
3. **欢迎区**只挂本地轻场景（`scenes.ts`，点选即发）。**场景侧栏**默认子页「精选」：官方成熟（`mature-scenes.ts`，填入 composer，不自动发送）+ 推荐场景；**套件 / 智能体 / Skill** 各为独立子页，勿与精选列表混挂。列表副文用 `summary`，勿把整段 prompt 塞进侧栏。
4. **一轮成功后**静默预填沉淀候选（会话投影，不自动挂流）；助手操作条「保存」用预填插入轻量沉淀条（文案勿写「可保存」）；成熟场景沉淀为短流程（场景名 · 交付物 · 下次怎么跑 →「场景」→「精选」），非整段聊天 dump；预览仅 playbook（`# ` 开头）。动作用**文字链**（复制 / 保存）；无账号时「连接账号」为次级静音链。设置「沉淀目标」默认「提交到 Explore」（审核后公开，ADR-033）；可选「仅个人库」。成熟场景另有「再跑」（填入原 prompt）。
5. **G1 软提示**：文案极短（「连接账号」/「会员方案」）；静音轨底；关闭后本会话不再弹出；不得写「开通才能用」类硬墙语义。无 Token 时隐藏套件 ID 运行栏；设置齿轮与「账号」导航显示小红点（连上即灭）。
6. **时间线顺序**：同一轮内为 计划卡 → 工具调用 → 助手结论 →（按需）沉淀；禁止结论气泡排在工具之前（流式先建气泡时，后续 `tool_start` 须插到气泡前；`done` 再确保结论在工具后）。
7. **内容宽度**：工具卡 / 计划卡 / 确认卡 / 沉淀条铺满聊天列（勿 `36rem` 窄卡留下大块右空白）；助手正文左缘与上述卡对齐。

## 设置模块

| 分类 | 内容 |
| ---- | ---- |
| 模型 | 已保存列表 +「添加模型」；提供商预设（DeepSeek / OpenAI / 智谱 / Kimi / MiniMax / Ollama / 自定义）；模型名自由填；粘贴完整 URL 自动归一；可设默认；底栏测连 |
| MCP | 服务列表 + 启用/停用；点选后用一块 JSON 编辑（`command`/`args`/`env`/`cwd` 或 `url`/`headers`，对齐 Cursor）；「导入」折叠粘贴整份 `mcpServers`；空态可「添加 PromptStdio」；与「账号」页 PromptStdio REST 分开 |
| 账号 | 多套 PromptStdio 云端账号（`mcp_profiles`）；未连账号时顶部主按钮「打开网站连接」（本机 `127.0.0.1` 回调收 Token，勿依赖手抄）；芯片区分「已保存 / 可用 / 已失效」（勿用「已连接」表示仅本地有 Token）；绿勾=已保存；探测成功脚注「账号可用」、网站连接成功「账号已连接」；默认账号供侧栏「场景」；「沉淀目标」用纵向选项卡（仅个人库 / 提交到公共库），勿复用侧栏 `side-seg`；自定义服务地址放「高级选项」；禁「生产 / 本地 8090」等开发者话术 |
| 工作区 | 侧栏树：工作区下挂所属会话，可收叠（默认仅当前展开；折叠态 `stitch-workspace-collapse`）；会话必属某一命名工作区（无 path / 对不上的归入当前或按 path 补建）；**列表顺序固定**（不按最近使用重排）；**无独立路径条**——行 ··· 为打开文件夹 / 移除（无换目录、无重命名；名称跟文件夹）；标题旁「文件夹+」= 添加工作区；**目录行右侧「+」= 新建会话**；元数据 `stitch-workspaces`，TOML `work_dir` 仍是当前活跃路径 |
| 会话卫生 | 同工作区复用空「新会话」；启动清掉非当前空壳；每工作区最多 40 条（先空壳、再按 `updatedAt` 收最旧）；当前会话永不被自动删 |
| 通用 | 外观（浅色 / 深色 / 跟随系统）、最大迭代、应用更新；可提示工作目录在左侧工作区 |

工作目录**不在设置页、不在聊天顶栏、不在「工作区」标题下路径条**：目录行 ▾ 贴近文件夹图标，**无独立 hover 方块**（随整行高亮）；点名称：未激活则切换并展开，已激活则折叠/展开；换目录用「添加工作区」另加条目，勿原地改 path。持久化 `config.toml` 的 `work_dir`。

顶栏**两段式**：左=会话语境（模型 chip · 计划模式），右=资源与系统（usage · 终端 · 主题 · 设置）；模型 chip 超长省略；模型菜单 >6 项出搜索框（`.model-menu-search`，列表区内滚）。顶栏模型菜单：选项悬停须可见（勿用与 surface 同色的 `--color-rail`）；选中项用品牌浅底。停止生成时进行中工具卡须收成「已停止」并停转圈。

Agent 读写/生成文件**必须落在当前工作目录**（`stitch` 工具侧 `resolve_under_work_dir`；拒绝对路径与 `..`）。提示词写明「生成内容默认在工作目录」。

输入框：增高后删除须能收回高度（先置 `height:0` 再量 `scrollHeight`）；发送清空后同步重置；IME 组合中不触发 Enter 发送。

## 聊天生产级铁律

1. **取消必达终态**：计划等待 / generate_plan / agent 任一阶段点停止，必须 emit `cancelled`（或等价），前端 `isStreaming` 不得永久卡住；进行中工具卡与计划步骤一并收口（「已停止」/ failed·skipped），禁止停生成后仍转圈。
2. **流式绑定会话**：`streamSessionId` 固定写入目标会话；切换会话若在流式中先取消。
3. **计划步骤收尾**：批准执行结束后批量 `PlanStepDone`，勿只发 Start。
4. **历史干净**：`stopped` / `error` 消息不进 `historyForSend`；停止文案勿污染模型上下文。
5. **确认框随终态关闭**：`done` / `cancelled` / `error` 时关 `confirmOpen`。
6. **Markdown 消毒**：`{@html}` 前 sanitize，禁 script / on* / `javascript:`。
7. **生成互斥**：Rust `CancelState.busy` 拒绝并行 `send_message`。
8. **落盘节流**：流式中 debounce localStorage；终态 `flushSessionPersist`。
9. **权威历史（ADR-036）**：Agent `tool_calls` 落盘于工作区 `.stitch/sessions/<id>/`；UI localStorage 仅为投影（工具 detail 超长截断）。重启恢复优先磁盘权威，勿依赖纯文本 `historyForSend`。
10. **Session Autopilot**：soft(~70%) 并行预压候选、hard(~85%) 同步压缩 bump `epoch` 并写 checkpoint；tool 大输出落 `outputs/`；Done 后静默预填沉淀候选；删除会话清内存与磁盘权威。静默 GC 保留近 3 代 checkpoint 与引用中的 `outputs/`；侧栏 ···「回退检查点」打开选择器（列表 + 与当前 diff + 回退到所选世代）。跨会话：启动/删会话后静默清孤儿 `.stitch/sessions`；空欢迎区可选「载入上一检查点」。

## 聊天壳（观感）

| 区域 | 约定 |
| ---- | ---- |
| 侧栏分区 | `.side-seg`：图标 + 短标签；**扁平**（无外轨、无选中白盒、无底边线）；选中=字重加深 + 颜色。设置「外观」`.theme-pref-seg` 仍用分段轨 |
| 侧栏收起 | 固定在侧栏顶栏右侧图标钮；勿塞进「会话」工具条里难发现 |
| 列表操作 | 删除用描边垃圾桶图标，禁用「×」字符 |
| 输入区 | **单一** `.composer` 表面：textarea 与发送钮同盒；左下 **+** 弹出菜单（Skills / MCP Servers 子页，对齐主流 Agent）；Skills 合并工作区与本机（`~/.agents` · `~/.cursor`），轻标来源；禁输入与按钮各套一层边框。**图片入口**：发送钮左侧**常驻**图片钮（`image-attach`，描边 SVG）；当前模型支持视觉（`model_supports_vision` 门控）→ 点击打开文件选择 + 粘贴 image/* 进预览条（40px 缩略图 + 移除钮，`pending-images`）；不支持 → 点击弹引导 dialog（推荐本地视觉模型如 Ollama qwen3-vl，价值：图片只在本机处理、不占云端 token，可跳转模型设置）。图随消息以 data URL 发送，气泡内嵌预览；图片**不落盘**——localStorage 与 Rust `messages.jsonl` 均剥离（磁盘留「[图片]」stub），零资源文件、无需回收 |
| 右键 | **系统原生菜单**（Tauri `Menu.popup`）；禁 WebView 浏览器菜单（检查/刷新等）；输入框=剪切复制粘贴全选；会话/工作区行=与「⋯」同款动作；侧栏「⋯」仍为 HTML 浮层（冒烟可点）；打开时抬高层叠 + 不透明底，避免下排会话透出/点击穿透 |
| 发送 | 图标钮（上行箭头）；流式中改为停止方块；`aria-label` 仍为「发送 / 停止生成」；空输入禁用发送 |
| 过程条 | `.stream-rail`：任务摘要 · 阶段 · **输出格式** · **耗时** · **轮次/tokens/Ctx** · 进度条（计划步数 / 流式已接收字数 / 工具不确定条） |
| Tokens / Context | 顶栏 `.usage-meter`：Ctx 占用条 + `Ctx a/b` · 本轮 tok（↑入 ↓出）· 调用次数；数值来自 `usage` / `done` 事件（CJK 估算，非计费账单）。**发生过分层压缩后**（`usage.layers` 含 warm/cold 条目）进度条变三段：热/温/冷按 token 占比（hot=accent · warm=accent 40%+border mix · cold=muted，全走 token），明细在原生 title（热 a · 温 b · 冷 c · 归档 N 条）；未分层保持单条 accent。**自动回灌**：用户新消息命中归档关键词（≥2）时，命中的 warm/cold 条目提升回热层，摘要块以 `[归档恢复]` 前缀并入该用户消息（可见、随回合持久化/回滚） |
| 工具调用 | `.tool-call`：中文标题；成功默认折叠为薄行；**连续 ≥2 条成功过程工具**（读/列/搜/Git/终端等）合成 `.tool-group`「已执行 N 步」，点开再看明细（chevron，无展开/收起文案）；写入/删除/失败仍单独露出；铺满聊天列；**终端 / Git 过程输出不提供复制**；禁 `✓`/emoji |
| 回合结束 | 流式结束后把最后一条助手结论滚入视口（`scrollAnswerIntoView`），避免只停在工具堆底部 |
| 计划卡 | 进度条 + SVG 步骤态（含 failed / skipped）；超过 5 步可折叠；批准中 spinner；铺满聊天列 |
| 套件失败 | `run_suite` 中途失败须给出可读汇总：已完成步骤 · 失败步与原因 · 未执行步骤；计划卡同步标 failed/skipped |
| 助手消息 | 轻量角色名「Stitch」；全文复制 +「保存」用悬停操作条；**代码块**带头栏语言 + 复制图标；**链接**用强调色下划线；**须排在本轮工具之后**；正文左缘与工具/计划/确认对齐（无额外 inset）；**流式中纯文本追加**（结束后再 Markdown），避免每 token 重渲闪烁 |
| 长内容 | 助手长文硬切 + 不透明底栏**整行可点**展开（chevron 仅指示；无展开文案；`aria-label` / `title` 保留）；`pre` / 表格 / 引用 / 工具输出各自限高、区块内滚动 |
| 格式约定 | 代码 → 代码块+复制；URL → 链接样式；路径/JSON/目录 → 工具卡 `format-badge`+复制；终端/Git → 徽章、无复制；可复制项一律图标钮（禁 emoji） |
| 确认 | 聊天流内联 `.confirm-card`（非全屏弹窗）；结构化标题/图标/终端或路径预览；「本次生成内允许同类操作」；连续确认原地更新（120ms 软关） |
| 工作目录 | 无独立路径条；左 ▾ 展开/收起；点名称激活（列表不重排）；右 ··· 打开文件夹 / 移除；标题旁文件夹+添加；聊天区不再重复 |
| 图标 | 描边 SVG（`stroke-width≈1.75`），**禁 emoji / 禁字符当图标**（含 `✓` `📁`） |
| 沉淀 | 不自动挂流；Done 后静默预填候选；助手「保存」插入轻量条（卡片标题「存成提示词」）；文字链复制/入库；成熟场景可「再跑」 |
| 套件/智能体 | 未连账号先挡；失败用人话落在侧栏横幅（禁原始 `API error`/JSON）；鉴权失败不进聊天气泡 |
| 侧栏会话 | 目录行 = **左 ▾** 折叠 + 文件夹 + 名称；右悬停 ··· / 「+」新建；会话标题优先用场景短名（勿用长 prompt 截断）；上标题、下相对时间；选中=浅底 + 上下横线 + 字重/颜色（**无左竖线**）；切换工作区**不得**改写 `workDirPath` |
| 主题钮 | 图标表示**当前偏好**：跟随系统=显示器 · 浅色=太阳 · 深色=月亮（勿用解析后的明暗反推） |
| 轮次分隔 | 新用户消息起 `.is-turn-start`：顶部 1px 分隔线 + 加大间距（首轮无）；轮内工具/计划/结论平铺 |
| 回到底部 | 上滚 >80px 浮出 `.scroll-bottom-pill`（居中、流式中带脉冲点）；点击强制吸底恢复跟随 |
| 全局提示 | 后台会话完成/出错走 `.toast-stack`（右下、最多叠 3 条、自动消失）；不替代聊天内错误气泡 |
| Composer 提示 | 右侧输入 ≥200 字显示计数（tabular-nums）；生成中补「结束后可发送」 |

改聊天壳时跑 Layer A + Layer V（浅色/深色各一张整窗截图）。

## 组件边界

| 区域 | 组件方向 | 备注 |
| ---- | -------- | ---- |
| 主壳 / 导航 | `+page.svelte` · `nav.svelte.ts` | 单例导航；诊断条可保留；挂 `CommandPalette` / `ShortcutsDialog` / `ToastStack` |
| 聊天 | `ChatView` · `CapabilityRail`（composer + 菜单）· `MessageBubble` · `ToolStatus` · `PlanCard` · `ChatFindBar` | 流式 + Plan；+ 菜单挂本机 Skill / MCP 开关；气泡操作条：助手末条「重新生成」· 用户「编辑」（截断重发，Rust `rewind_to_user`/`rewind_drop` 同步缓存会话） |
| 设置 | `SettingsView`（壳：顶栏 + 图标导航 + 底栏）· `settings/ModelPane` · `AccountPane` · `McpPane` · `GeneralPane` · `settings-state.svelte.ts`（runes 单例，全部表单状态与动作）· `settings-catalog.ts`（搜索索引） | 左栏「模型 / 账号 / MCP / 通用」带描边图标 + 顶部搜索框：输入时导航变命中列表，点击/Enter 切 tab 定位字段并 `.settings-flash` 柔和高亮（`prefers-reduced-motion` 关闭动画）；密钥空输入 + 勾图标（已保存）；成功态脚注勾图标；测试/进入聊天前先探针，失败不写坏密钥；错误 L1 短句 |
| 效率 | `shortcuts.ts`（快捷键注册表）· `stores/palette.ts` · `CommandPalette` · `ShortcutsDialog` · `ChatFindBar` · `lib/fuzzy.ts` · `lib/composer-history.ts` | `Ctrl+N` 新会话 · `Ctrl+K` 面板 · `Ctrl+F` 会话查找 · `Ctrl+,` 设置 · `Ctrl+B` 侧栏 · `Ctrl+/` 帮助 · `Esc` 逐层关浮层（不误停生成）再停生成；面板含动作/会话/模型三组，子序列模糊匹配（中英文关键词别名）+ 最近使用置顶（localStorage 12 条），键盘全可达；composer 空时 `↑/↓` 召回已发送（localStorage 50 条）；浮层关闭后焦点归还 composer；查找用 CSS Highlight API 画标记、不改 DOM |
| 场景 | `LibraryPanel` | 子页：**精选**（成熟 + 推荐）· **套件** · **智能体** · **Skill**（填安装话术）；无 Token 时精选与 Skill 仍可用，套件/智能体引导连账号 |
| 终端 | `TerminalPanel` · `lib/terminal/` | 顶栏开关；汇总当前会话 `run_command` 输出（独立模块，非聊天气泡替代） |
| 浮条 | compact 模式（桌面工具运行时自动缩窗） | `data-compact` 写在 `:root`（非 app-frame）；compact-bar = 当前任务 + 停止 + 展开；done/error 自动还原 |
| 沉淀 | `SedimentCard` | Done 预填；助手「保存」插入；文字链复制/入库或「保存并提交公开」；无账号次级「连接账号」；成熟场景可「再跑」 |
| 确认 | `ConfirmCard`（聊天内联）· `WorkDirDialog` | 危险写文件 / 命令在对话流内确认；首启可 nudge 选工作目录（可取消） |
| 诊断 | `DiagBanner` · `frontend_log` | 真实 exe 排障用 |

新可见控件加稳定 `data-testid`（冒烟依赖）。

## 与网站的关系

| 可共用 | 不可共用 |
| ------ | -------- |
| 品牌名、主色意向、文案语气（ADR-025） | Leptos 组件、会员页布局、网站 `design-system/pages/*` |
| Figtree / 克制瑞士风思路 | 主站 Tailwind 语义类整页搬迁 |

改视觉时优先改 `app.css` token，再动组件；大改方向先更新本文件一两句，避免 Skill 与页面各写各的。
