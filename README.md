# ft financial canvas

`ft financial canvas` 是一个同时支持 **OpenCode** 与 **Codex** 的终端金融可视化项目，
源自 **Feitu 7 月 Hackathon**。它把行情图表、结构化数据和分析上下文放进 tmux 侧边
画布，让用户可以在 agent 会话中查看、选择并继续分析数据。

项目目前处于 Hackathon 原型持续整理阶段，以本地源码方式使用，尚未发布为 npm 包。
画布展示的行情和分析结果仅用于研发与演示，不构成投资建议。

当前工作区仍使用历史目录名 `opencode-canvas-main/` 以兼容已有的相对路径配置；正式项目名
和包标识分别为 `ft financial canvas` 与 `ft-financial-canvas`。

## 当前能力

| Renderer            | 场景                               | 定位                                                     |
| ------------------- | ---------------------------------- | -------------------------------------------------------- |
| `candlesticks`      | `kline`                            | A 股 K 线与成交量，可选择区间、附加上下文或请求分析      |
| `market-table`      | `quotes`                           | FTShare 行情快照表格，支持导航、多选、上下文、分析与导出 |
| `news-list`         | `search`                           | FTShare 新闻搜索卡片，支持滚动、外链、高亮和 AI 解读     |
| `security-snapshot` | `overview`                         | FTShare 单标的行情、表现、估值和市值快照                 |
| `chart`             | `bar-graph`                        | 由 JSON 数据驱动的分组柱状图、折线图或组合图             |
| `dag`               | `display`                          | 用于数据血缘、分析链路和依赖关系的有向无环图             |

## 工作方式

项目采用“共享核心 + 两个 adapter”：`CanvasManager` 负责会话归属、生命周期、tmux 和
renderer 协议；OpenCode adapter 注册原生插件工具并使用会话事件桥接上下文；Codex adapter
通过 MCP 注册同一组工具，并用 `canvas_wait` 与 `UserPromptSubmit` Hook 回送交互事件。

每个 renderer 都是独立子进程，在 tmux 右侧 pane 中渲染，并通过两条带鉴权的 Unix socket
与 host 通信：

- `control.sock`：初始化、配置更新、状态查询和关闭；
- `event.sock`：选择、上下文、分析动作、artifact 和内部命令事件。

renderer 可以使用 Bun/Ink、Rust/Ratatui，或任何能够读取 `launch.json` 并实现 Canvas v2
NDJSON 协议的运行时。详细设计见 [Canvas v2 文档](./docs/v2/README.md)。

OpenCode adapter 还包含一项不暴露给 LLM 工具的自动能力：当单轮对话从 working 开始到
idle 结束的用时超过 1 分钟时，插件会读取截至该轮结束时已完成的对话轮次，通过临时 AI 子会话生成
一条技术社区动态草稿，
并在右侧打开审核卡片。点击“发送”目前会把正文保存到工作区
`.memory/social-posts/*.md`，点击“取消”则直接关闭；两项操作都经 `event.sock` 返回插件。
该卡片的 manifest 标记为 `internalOnly`，不会出现在 `canvas_renderers` 中，也不能通过
`canvas_spawn` 或 standalone launcher 启动。`social-post-card` 及其自动草稿协调器仅由
OpenCode adapter 启用，Codex adapter 不注册、不启动这一能力。

## 环境要求

- Bun 1.3 或兼容版本；
- OpenCode 或 Codex；
- tmux；
- Linux、macOS，或使用 WSL 的 Windows；
- Rust 1.88+（仅在重新编译 Rust renderer 时需要）；原生 renderer 使用根 Cargo workspace，
  共享一个 lockfile 和 `target/` 缓存。

仓库当前附带 `linux-x64` 原生二进制。其他平台需要安装 Rust，并在本机执行构建。

## 快速开始

安装依赖并构建三个 host bundle（OpenCode、Codex MCP、Codex Hook）：

```bash
cd opencode-canvas-main
bun install
bun run build:plugin
```

仓库已附带当前平台的 renderer 二进制时，只构建 host bundle 即可。需要重新编译并打包所有
Rust renderer 时再运行 `./ftopencode-build`。

### OpenCode

在需要使用画布的 OpenCode 工作区中配置本地插件。下面示例假设该工作区与
`opencode-canvas-main/` 同级：

```json
{
  "$schema": "https://opencode.ai/config.json",
  "plugin": ["../opencode-canvas-main/src/index.ts"],
  "permission": {
    "canvas_*": "allow"
  }
}
```

从该工作区启动 OpenCode：

```bash
../opencode-canvas-main/ftopencode
```

`ftopencode` 会在已有 tmux 环境中直接启动 OpenCode；否则会询问是否创建或附加到名为
`ftopencode` 的 session。CI 或脚本可传入 `--yes`，自定义 session 可使用
`--session <name>`。

启动后可以让模型“显示 `000001.SZ` 的 K 线”，或基于绝对路径下的 JSON 文件创建统计图。

### Codex

`.codex-plugin/plugin.json` 声明插件，`.mcp.json` 启动 `dist/codex-mcp.js`，而
`hooks/hooks.json` 在下一次 `UserPromptSubmit` 时附加尚未由 `canvas_wait` 消费的 Canvas
交互。`.mcp.json` 还会显式转发 `TMUX` 与 `TMUX_PANE`；Codex 默认会过滤这两个环境变量，
缺少它们时 MCP 无法识别宿主 tmux pane。仓库提供的安装脚本会构建三个 bundle、从本目录
生成个人 marketplace 中
`ft-financial-canvas` 的受控 local source 快照，并调用 Codex CLI 安装：

```bash
bun run codex:install
```

默认 marketplace 是 Codex 自动发现的 `~/.agents/plugins/marketplace.json`，source 快照位于
`~/plugins/ft-financial-canvas`。快照遵循 `package.json#files`，排除 `.git`、renderer
`target/` 和开发依赖；脚本只安装 production 依赖并移除 Codex 不接受的符号链接。先查看将
执行的操作而不写入外部状态：

```bash
bun run codex:install -- --dry-run
```

使用非默认的 repo/team marketplace 时，其文件须采用
`<marketplace-root>/.agents/plugins/marketplace.json`，并为新 marketplace 指定名称；脚本会
自动执行一次幂等的 `codex plugin marketplace add`：

```bash
bun run codex:install -- \
  --marketplace-path /absolute/team/.agents/plugins/marketplace.json \
  --marketplace-name team-local
```

若 marketplace entry 已存在，脚本会按 Codex 本地开发规范把 manifest version 更新为单一
`+codex.<timestamp>` cachebuster 后重新安装。冲突的 entry 或 source 目录不会被静默覆盖；
确认要替换时显式传入 `--force`。手工流程仍等价于：

```bash
codex plugin add ft-financial-canvas@<local-marketplace-name>
```

安装或更新后请新建 Codex thread，使 Skill、MCP 工具和 Hook 一起重新加载。Codex 首次发现
插件 Hook 时会要求用户确认信任；确认前 MCP 工具仍可使用，但跨 turn 的自动上下文附加不会
执行。Codex 同样必须运行在 tmux 内，可使用：

```bash
../opencode-canvas-main/ftcodex
```

`ftcodex` 默认以 `--no-alt-screen` 启动 Codex，使对话内容进入 tmux pane 的 scrollback；
需要恢复全屏 alternate screen 时使用 `../opencode-canvas-main/ftcodex --alt-screen`。

交互式选择需要在当前 turn 内返回时，Codex 会调用 `canvas_wait`；如果事件发生在两个 turn
之间，Hook 会把它作为下一条用户提示的附加上下文。`social-post-card` 在 Codex 中不可用。

## 常用命令

```bash
bun run build          # 增量构建插件和原生 renderer
bun run build:plugin   # 只构建 OpenCode、Codex MCP、Codex Hook bundle
bun run codex:install  # 登记 local marketplace source 并安装 Codex plugin
bun run typecheck      # TypeScript 类型检查
bun run lint           # ESLint
bun run format:check   # 检查 TypeScript/TSX 格式
bun run check          # 依次执行类型、lint 和格式检查
./ftopencode-build --force
cargo build --workspace --release --locked # 仅构建全部 Rust renderer
```

单独预览 renderer：

```bash
bun run canvas -- show chart --scenario bar-graph \
  --config "{\"data_file\":\"$PWD/canvases/chart/example-data.json\"}"
```

## Canvas 工具

| 工具                            | 宿主     | 用途                                                          |
| ------------------------------- | -------- | ------------------------------------------------------------- |
| `canvas_renderers`              | 两者     | 列出 renderer、场景和能力                                     |
| `canvas_spawn`                  | 两者     | 创建画布并等待初始配置校验                                    |
| `canvas_update`                 | 两者     | 更新已有画布配置                                              |
| `canvas_selection`              | 两者     | 查询当前选择                                                  |
| `canvas_content`                | 两者     | 查询支持读取的内容                                            |
| `canvas_state`                  | 两者     | 查询 renderer 状态                                            |
| `canvas_list`                   | 两者     | 列出当前 agent session/thread 的画布                          |
| `canvas_switch` / `canvas_next` | 两者     | 切换右侧可见画布                                              |
| `canvas_layout`                 | 两者     | 将当前会话的 1–4 个画布排列为单屏、并列、上下、主辅或网格布局 |
| `canvas_close`                  | 两者     | 关闭画布并清理运行时文件                                      |
| `canvas_wait`                   | 仅 Codex | 等待并返回当前 thread 的 renderer context/action              |

## 目录结构

```text
src/canvas/                  共享 Canvas manager、IPC、manifest 和协议实现
src/hosts/opencode/          OpenCode 会话事件与 prompt adapter
src/hosts/codex/             Codex MCP、事件 broker/store 与 Hook 入口
src/index.ts                 OpenCode 插件入口及其专属 social-post 组装
src/social-post/             OpenCode-only 计时、草稿生成和本地保存
canvases/_sdk/               TypeScript renderer 侧 IPC/鼠标辅助代码
canvases/_sdk-rust/          独立原生 renderer 共用的 Canvas v2 socket runtime crate
canvases/<renderer>/         renderer manifest、源码和可选原生二进制
skills/                      OpenCode 与 Codex 共用的 Canvas Skills
.codex-plugin/ + .mcp.json   Codex 插件与 MCP 声明
hooks/                       Codex UserPromptSubmit Hook 声明
scripts/install-codex-plugin.ts  Codex local marketplace 安装脚本
docs/v2/                     当前 Canvas v2 架构、协议和开发文档
ftopencode                   tmux-aware OpenCode 启动器
ftcodex                      tmux-aware Codex 启动器
ftopencode-build             增量构建入口
```

运行时文件位于
`/tmp/ft-financial-canvas-v2/<host-scope>/<widget-id>/`，其中包括 `config.json`、
`launch.json`、`control.sock` 和 `event.sock`。Codex 未消费的交互事件暂存在同一临时根目录的
`codex-events/` 中；文件名使用 thread ID 哈希，日志有数量、大小和有效期限制。

## 项目来源与许可证

`ft financial canvas` 从 Feitu 7 月 Hackathon 原型演进而来。项目根目录代码沿用 MIT
许可证，并保留原有版权声明；`canvases/candlesticks/` 中的 vendored renderer 另保留
MIT OR Apache-2.0 许可证。详见 [LICENSE](./LICENSE) 及对应 renderer 目录。
