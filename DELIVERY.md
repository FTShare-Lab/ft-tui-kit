# Financial Canvas 交付方案

本文描述当前项目最适合的交付方式，以及从内部 Hackathon 原型演进到公开 OpenCode 插件时
建议采用的发布路径。

## 结论

当前阶段推荐使用：

> **版本化、按平台构建的 release 压缩包 + SHA-256 + HTTPS 下载 + 小型安装脚本**

不建议把“克隆源码并在用户机器上编译 Rust”作为默认路径，也不建议直接把整个仓库路径写进
每个项目的 `opencode.json`。源码安装可以作为开发与 release 缺失时的后备方案。

原因如下：

- 插件本体是 Bun bundle，但五个核心金融 renderer 是 Rust 原生二进制；
- renderer 会按 `process.platform-process.arch` 查找自己的二进制；
- Bun/Ink renderer 仍需要 production dependencies；
- 插件会根据自身安装根目录发现 `canvases/`、skills、启动脚本和 renderer manifests，不能只
  单独复制 `dist/index.js`；
- 当前仓库只附带 `linux-x64` 二进制，直接宣称 macOS/ARM 一键可用会造成交付缺口。

## Release 文件

每个版本发布以下四个平台资产：

```text
ft-financial-canvas-v<VERSION>-linux-x64.tar.gz
ft-financial-canvas-v<VERSION>-linux-arm64.tar.gz
ft-financial-canvas-v<VERSION>-darwin-x64.tar.gz
ft-financial-canvas-v<VERSION>-darwin-arm64.tar.gz
```

每个压缩包必须同时发布对应的：

```text
<archive>.sha256
```

Windows 用户通过 WSL 使用 Linux 资产。当前协议依赖 tmux 和 Unix socket，不应发布原生
Windows 资产，除非这些边界已经被重新设计并验证。

压缩包应包含：

- `dist/index.js`；
- `src/` 中运行时读取的命令资源；
- `canvases/` manifests、launcher、Bun renderer 和当前平台二进制；
- `skills/`；
- `package.json`、`bun.lock`；
- `run-canvas.sh`、`ftopencode`；
- LICENSE 和必要文档。

不得包含：

- `.git/`、`node_modules/`、Cargo `target/`；
- `.memory/`、运行时 socket、行情导出或用户数据；
- token、私有 registry 配置、Git credentials；
- 其他平台的错误或过期二进制。

## 构建 release

在目标平台的干净 runner 上执行：

```bash
cd opencode-canvas-main
./scripts/package-release.sh
```

脚本默认执行：

```text
bun install --frozen-lockfile
bun run check
./ftopencode-build --force
```

随后验证五个原生 renderer，使用显式 allowlist 组装压缩包，并生成 SHA-256 sidecar。产物
默认写入 `release/`。

仅在已经由前序 CI job 完成相同检查与构建时，才使用：

```bash
./scripts/package-release.sh --skip-checks --skip-build
```

不要从 Linux 交叉生成 macOS release。使用真实的 Linux x64、Linux ARM64、macOS Intel 和
macOS Apple Silicon runner，确保 libc、系统 API 和二进制格式均在目标环境中验证。

## 发布流水线

推荐的 release pipeline：

1. 创建 `v0.1.2` 形式的不可变 tag；
2. 四个平台并行运行 `package-release.sh`；
3. 每个平台启动一个临时 tmux/OpenCode 环境做 smoke test；
4. 验证 `canvas_renderers`；
5. 至少启动并关闭 `candlesticks`、`chart`、`dag`；
6. 将四个压缩包及 checksum 上传到同一个 release；
7. 更新 landing page 安装器中的默认版本；
8. 构建并部署 `landing-page/static/`。

发布失败时不要覆盖同一 tag 下的现有资产。修复后发布新的 patch 版本，以保证脚本、checksum
和用户安装结果可复现。

## 一键安装的行为

Landing page 的 `install.sh`：

1. 检测 OS、CPU、Bun、OpenCode 和 tmux；
2. 下载当前平台的 release 与 SHA-256；
3. 校验 checksum 和 tar 路径安全；
4. 安装 production dependencies；
5. 在 XDG data 目录中原子替换旧版本；
6. 在 `~/.config/opencode/plugins/` 创建一个受管 loader；
7. 在 `~/.local/bin/` 创建 `ftopencode` 链接；
8. 不改写用户的 `opencode.json`。

全局 loader 比修改每个项目的配置更合适，因为它符合 OpenCode 对全局本地插件目录的加载
方式，同时不会破坏 JSONC 注释或覆盖用户已有策略。

默认 release 不存在时，安装器会退回 Git 源码安装。这个路径主要服务开发环境，不应成为
公开用户的正常体验。

## 下载安全

当前仓库页面使用：

```text
http://code.non-convex.com:18118/jobby/financial-canvas
```

HTTP 不适合承载 `curl | bash` 或可执行 release。公开交付前至少应满足：

- landing page、`install.sh`、release 和 checksum 全部通过 HTTPS；
- release URL 不会重定向到 HTTP；
- tag 和 release 资产不可被静默覆盖；
- 最好进一步对 checksum manifest 使用 cosign/minisign 签名。

同一 HTTP 连接下载的 SHA-256 只能发现传输损坏，不能抵御中间人同时替换压缩包和 checksum。

## Agent 安装

Landing page 同源提供 `install-guide.md`。提示词只需要把该 URL 交给 Agent；指南要求 Agent：

- 先检查依赖和脚本；
- 不自动使用 sudo；
- 不自动安装缺失工具；
- 不破坏用户配置；
- 安装后调用 `canvas_renderers` 并完成一个 Canvas smoke test。

## npm：长期目标，不是当前捷径

公开 npm 包最终会带来最自然的 OpenCode 使用方式，但当前不应直接把现有目录原样发布：

- `package.json` 仍为 `private: true`；
- 需要明确包名、所有者、repository、engines 和 OpenCode 兼容范围；
- 原生 renderer 应拆成平台包，并由主包通过 `optionalDependencies` 选择，类似常见的原生
  Node 工具链；
- 安装后必须能可靠定位平台包里的二进制；
- 需要重新审查 vendored renderer、字体/数据资源和第三方许可证；
- 需要验证 npm 安装目录中的只读、缓存和升级行为。

完成这些改造后，长期推荐体验可以收敛到 OpenCode 原生的 npm plugin 安装流程；在此之前，
release archive 是风险更低、可验证性更强的交付边界。

## 手工安装入口

面向维护者和需要源码调试的用户，保留仓库页面：

```text
http://code.non-convex.com:18118/jobby/financial-canvas
```

这里应展示源码构建、平台限制、Canvas v2 协议和开发说明，但不应替代普通用户的版本化
release 下载体验。
