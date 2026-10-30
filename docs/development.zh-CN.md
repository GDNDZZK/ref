[English](development.md) · **简体中文**

# 开发

## 构建

需要当前稳定版 Rust 工具链（代码使用 edition 2024 和较新的语言特性），以及 C 编译器（内置的 SQLite 和 TLS 库需要）。

```bash
cargo build --release
./target/release/ref
```

两个程序都会生成在 `target/release/` 中：`ref` 和 `ref-server`。请把它们放在同一个目录，因为 `ref` 会优先在自身所在目录查找 `ref-server`。

## 测试

```bash
cargo test --workspace                                         # 所有 crate
cargo fmt --all --check                                        # 格式检查
cargo clippy --workspace --all-targets -- -D warnings          # lint
ref --mock                                                     # 用假后端运行 TUI
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
| `server` | `ref-server`：会话、智能体循环、权限、压缩、存储（SQLite）、跨会话消息、WebSocket API。 |
| `tui` | `ref`：终端界面（ratatui）。 |

## 协议说明

TUI 与后端通过 WebSocket 交换 JSON 帧。前端分页获取历史：`Attach` 返回最新的消息（100 条，超过约 1 MiB 时更少），`Messages` 返回指定消息之前的一页，因此 TUI 会在向上滚动时加载更早的页。图片以占位信息发给前端（MIME 类型、尺寸和 `id`，不含字节数据）；`GetImage` 按 id 返回图片字节。

## 持续集成与发布

`.github/workflows/` 中有两个 GitHub Actions 工作流：

- **`ci.yml`** 在每次推送和拉取请求时运行：在 Linux 上执行 `cargo fmt --check` 和 `cargo clippy -D warnings`，在 Linux、macOS 和 Windows 上执行 `cargo test`。测试失败时，失败输出会在运行摘要中再显示一次。
- **`release.yml`** 在推送 `v*` 标签时运行。它在原生 runner 上为六个目标构建 `ref` 和 `ref-server`，与 README、`docs/` 目录和许可证一起打包，然后发布带有 `SHA256SUMS` 文件的 GitHub Release。

| 目标 | Runner | 压缩包 |
|---|---|---|
| `x86_64-unknown-linux-gnu` | Ubuntu 22.04 | `.tar.gz` |
| `aarch64-unknown-linux-gnu` | Ubuntu 22.04 arm64 | `.tar.gz` |
| `x86_64-apple-darwin` | macOS 15 Intel | `.tar.gz` |
| `aarch64-apple-darwin` | macOS 15 | `.tar.gz` |
| `x86_64-pc-windows-msvc` | Windows Server 2025 | `.zip` |
| `aarch64-pc-windows-msvc` | Windows 11 arm64 | `.zip` |

发布新版本：

```bash
# 1. 修改 Cargo.toml 中的版本（[workspace.package] version），提交并推送。
# 2. 打标签并推送标签：
git tag v0.2.0
git push origin v0.2.0
```

手动运行 Release 工作流（Actions → Release → Run workflow）只会构建压缩包并附加到该次运行，不会发布。
