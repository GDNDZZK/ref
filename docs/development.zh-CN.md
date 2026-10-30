[English](development.md) · **简体中文**

# 开发

## 构建

需要当前稳定版 Rust 工具链（代码使用 edition 2024 和较新的语言特性）、C 编译器（内置的 SQLite 和 TLS 库需要），以及用于 Web 前端的 [Node.js](https://nodejs.org/) 24 和 [pnpm](https://pnpm.io/)。

**先构建前端。** `ref-web` 嵌入 `web/dist/` 中的文件，`web/dist/index.html` 不存在时它的 build 脚本会报错并停止编译。所有会编译整个工作区的 cargo 命令都是如此：`cargo build`、`cargo test`、`cargo clippy --workspace`。

```bash
(cd web && pnpm install && pnpm build)
cargo build --release
./target/release/ref
```

程序生成在 `target/release/` 中：`ref`、`ref-web` 和 `ref-server`。请让每个界面程序与 `ref-server` 放在同一个目录，因为它们会优先在自身所在目录查找 `ref-server`。修改前端后重新运行 `pnpm build`，再重新编译 `ref-web`；cargo 会发现 `web/dist/` 的变化。

## 测试

```bash
cargo test --workspace                                         # 所有 crate（需要 web/dist，见上文）
cargo fmt --all --check                                        # 格式检查
cargo clippy --workspace --all-targets -- -D warnings          # lint
ref --mock                                                     # 用假后端运行 TUI
(cd web && pnpm check)                                         # 前端：类型检查、lint、单元测试、生成的 Theme/Motion 文件是否最新
python3 scripts/e2e.py provider/model "prompt" [cwd]           # 用真实模型做端到端测试
REF_DUMP_REQUESTS=/tmp/dump ref-server ...                     # 导出 LLM 请求体
```

部分测试会启动 `python3`（假 MCP 服务器）和 `bash`。端到端脚本需要 Python 的 `websockets` 包和一个已配置的模型。

## Crate

| Crate | 内容 |
|---|---|
| `proto` | TUI 与后端之间的通信协议、共享数据类型和目录路径。 |
| `config` | 加载与合并配置（兼容 opencode）。 |
| `llm` | OpenAI Chat Completions、OpenAI Responses 和 Anthropic Messages 的流式客户端；错误分类与重试。 |
| `tools` | 内置工具：`read`、`edit`、`write`、`delete`、`grep`、`find`、`bash`、`webfetch`，以及图片缩放。 |
| `mcp` | 基于 stdio 和 Streamable HTTP 的 MCP 客户端。 |
| `sys` | 操作系统层：进程、信号、Windows 相关处理。 |
| `client` | 通过 instance 文件找到运行中的后端，或启动程序同目录的 `ref-server`，或启动私有的 `ref-server --mock`。`ref` 和 `ref-web` 共用。 |
| `server` | `ref-server`：会话、智能体循环、权限、压缩、存储（SQLite）、跨会话消息、WebSocket API。 |
| `tui` | `ref`：终端界面（ratatui）。 |
| `web` | `ref-web`：提供 `web/` 中的前端（嵌入并预压缩）、为浏览器登录，并把它们的 WebSocket 帧转发给 `ref-server`。见 [Web 界面](web.zh-CN.md)。 |

## 协议说明

TUI 与后端通过 WebSocket 交换 JSON 帧。前端分页获取历史：`Attach` 返回最新的消息（100 条，超过约 1 MiB 时更少），`Messages` 返回指定消息之前的一页，因此 TUI 会在向上滚动时加载更早的页。图片以占位信息发给前端（MIME 类型、尺寸和 `id`，不含字节数据）；`GetImage` 按 id 返回图片字节。

## Mock 后端

`ref-server --mock` 是假后端，用于在没有 LLM 的情况下开发界面。`ref-gui --mock` 和 `ref-web --mock` 通过 `ref-client` 启动私有的 mock 后端。（`ref --mock` 是另一个进程内的假后端，与此无关。）

手动运行时要保持 stdin 打开，因为 stdin 读到文件结束时它会退出：

```bash
sleep 3600 | ref-server --mock      # stdout 只打印一行 {"port":N,"token":"..."}
```

- **退出**：stdin 文件结束、最后一个客户端断开，或 ctrl-c。退出时会删除它的临时目录（被 `SIGKILL` 杀死时会留下 `/tmp/ref-mock-*`）。
- **工作区**：无论客户端请求哪个目录，所有会话都在带示例文件的临时工作区中运行（`README.md`、`NOTES.txt`、`src/parser.rs`、`src/lib.rs`、`docs/design.md`、`data/big.log`、`logo.png`、`scratch/`），不会改动你的文件。预置了 3 条历史会话。
- **模型**：`mock/mock-1`（默认；200k 上下文、图片、推理、变体 `low`、`medium`、`high`）和 `mock/mock-fast`（64k）。多一个智能体 `reviewer`。
- **权限**：`edit` 和 `write` 询问（`scratch/` 下允许）。`delete` 允许，但延迟 5 秒。`bash` 询问，以 `echo`、`ls`、`cat`、`seq`、`sleep`、`printf` 开头的命令允许，`curl` 拒绝。`webfetch` 关闭。
- **脚本**：提示词的第一个词选择脚本，不区分大小写。其他文字运行 `default`。提示词 `help` 会列出全部脚本。

| 提示词 | 内容 |
|---|---|
| `help` | 列出全部脚本 |
| `hello` | 一句话回复 |
| `default` | 其他文字：回显提示词 |
| `markdown` | 标题、列表、表格、引用、链接、多种语言的代码块 |
| `reasoning` | 长推理，然后简短回答 |
| `article` | 带很多代码块的长文章（约 18 秒） |
| `slow` | 极慢的流式输出，约一分钟，用于逐帧检查 |
| `tools` | `read`、读不存在的文件（出错）、`grep`、`find`、需要批准的 `edit`（带 diff）、成功的 `bash` 和以 3 退出的 `bash` |
| `stream-args` | 参数慢速流式生成的 `write` |
| `permission` | 等待批准的 `edit`：`a` 允许、`s` 本会话都允许、`d` 拒绝 |
| `delay` | 带 5 秒倒计时的 `delete`（`esc` 取消） |
| `running` | 持续输出 8 秒的 `bash` 命令 |
| `bash-fail` | 以 1 退出的 `bash` 命令 |
| `denied` | 被配置拒绝的调用 |
| `cancel` | 永不结束的命令，用 `esc` 取消 |
| `todo` | 分几步更新的待办 |
| `image` | 读取 `logo.png`，工具结果带图片 |
| `long` | 输出 400 行的工具，然后是长回答 |
| `retry` | 两次 529 错误（重试中），然后成功（约 6 秒） |
| `fail` | 401 错误，重试后以错误结束（约 6 秒） |
| `fill` | 上下文接近 95%，下一条提示会先自动压缩 |
| `compact-tool` | 模型调用 `compact_context` |
| `peer` | 创建 `reviewer` 会话 `helper`，它用 `send_message` 回报 |
| `everything` | `tools`、`todo` 和 `image` 连成一个长回合 |

## 持续集成与发布

`.github/workflows/` 中有两个 GitHub Actions 工作流。两者都先用 `.github/actions/` 中的 action 构建前端（Node 24、带缓存的 pnpm、`pnpm install --frozen-lockfile`、`pnpm build`）。

- **`ci.yml`** 在每次推送和拉取请求时运行：在 Linux 上执行 `cargo fmt --check` 和 `cargo clippy -D warnings`，在 Linux、macOS 和 Windows 上执行 `cargo test`，并运行前端的 `pnpm check`。测试或前端检查失败时，失败输出会在运行摘要中再显示一次。
- **`release.yml`** 在推送 `v*` 标签时运行。它先构建一次前端，再在原生 runner 上为六个目标构建 `ref`、`ref-server` 和 `ref-web`（每个目标一次 `cargo build --release --workspace`），并用 `scripts/package.sh` 打包。最后一个任务生成 `SHA256SUMS` 文件，标签触发时发布 GitHub Release。每次构建都会在运行摘要中列出各压缩包的体积。

| 目标 | Runner | 压缩包 |
|---|---|---|
| `x86_64-unknown-linux-gnu` | Ubuntu 22.04 | `.tar.gz` |
| `aarch64-unknown-linux-gnu` | Ubuntu 22.04 arm64 | `.tar.gz` |
| `x86_64-apple-darwin` | macOS 15 Intel | `.tar.gz` |
| `aarch64-apple-darwin` | macOS 15 | `.tar.gz` |
| `x86_64-pc-windows-msvc` | Windows Server 2025 | `.zip` |
| `aarch64-pc-windows-msvc` | Windows 11 arm64 | `.zip` |

每个目标生成三个压缩包。每个压缩包里是一个同名文件夹，包含该界面、`ref-server`、两个 README、许可证和 `docs/`。

| 压缩包 | 程序 |
|---|---|
| `ref-tui-<version>-<target>` | `ref`、`ref-server` |
| `ref-web-<version>-<target>` | `ref-web`、`ref-server` |
| `ref-all-<version>-<target>` | `ref`、`ref-web`、`ref-server` |

在 `cargo build --release` 之后手动打包：

```bash
scripts/package.sh --version v0.2.0 --target x86_64-unknown-linux-gnu \
  --bin-dir target/release --out dist
```

发布新版本：

```bash
# 1. 修改 Cargo.toml 中的版本（[workspace.package] version），提交并推送。
# 2. 打标签并推送标签：
git tag v0.2.0
git push origin v0.2.0
```

手动运行 Release 工作流（Actions → Release → Run workflow）会构建压缩包和 `SHA256SUMS` 并附加到该次运行，不会发布。
