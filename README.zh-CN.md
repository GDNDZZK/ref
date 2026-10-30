<div align="center">

# ref

**用 Rust 编写的终端编程智能体。**

会话运行在后台服务器中，没有打开任何窗口时也会继续工作。
多个会话可以并行工作，并互相发送消息。

[![CI](https://github.com/GDNDZZK/ref/actions/workflows/ci.yml/badge.svg)](https://github.com/GDNDZZK/ref/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/GDNDZZK/ref?sort=semver)](https://github.com/GDNDZZK/ref/releases/latest)
[![License: GPL-3.0](https://img.shields.io/badge/license-GPL--3.0-blue.svg)](LICENSE)
![Platforms](https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-lightgrey)

[English](README.md) · **简体中文**

</div>

---

## 特性

- **服务器与 TUI 分离。** `ref-server` 负责运行会话、智能体循环、LLM 调用、工具和权限，`ref` 只负责绘制界面。关掉窗口后会话继续运行，之后可以在任意终端重新打开。
- **同时运行多个会话。** 分屏视图平铺显示所有后台会话。会话之间可以互发消息，`build` 智能体可以启动工作会话并给它们分派任务。
- **兼容 opencode 配置。** 已有的 `opencode.json`（提供商、模型、智能体、权限、MCP 服务器和命令）无需修改即可使用。
- **支持任意模型提供商。** 支持 Anthropic Messages、OpenAI Responses 和兼容 OpenAI 的 Chat Completions，并可通过模型变体设置推理强度或扩展思考。
- **可控的工具调用。** 按工具设置允许、询问或拒绝，可以按命令、路径和 URL 匹配模式，还可以在工具运行前加倒计时。
- **可靠的文件编辑。** 编辑基于带哈希标签的行号，文件视图过期时会被检测出来，而不会把文件改坏。
- **适合长会话。** 自动压缩上下文，遇到限流和服务中断时无限重试，并为所有提供商自动缩小图片。
- **MCP**：支持 stdio 和 Streamable HTTP 两种传输方式的服务器。
- **Linux、macOS 和 Windows**，支持 x86_64 和 arm64。

## 安装

### 下载

从[最新发布版](https://github.com/GDNDZZK/ref/releases/latest)下载适合你系统的压缩包：

| 系统 | 压缩包 |
|---|---|
| Linux x86_64 | `ref-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Linux arm64 | `ref-<version>-aarch64-unknown-linux-gnu.tar.gz` |
| macOS Apple 芯片 | `ref-<version>-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `ref-<version>-x86_64-apple-darwin.tar.gz` |
| Windows x86_64 | `ref-<version>-x86_64-pc-windows-msvc.zip` |
| Windows arm64 | `ref-<version>-aarch64-pc-windows-msvc.zip` |

解压后，把 `ref` 和 `ref-server` 放到 `PATH` 中的**同一个目录**：

```bash
tar -xzf ref-*.tar.gz
install -m 755 ref-*/ref ref-*/ref-server ~/.local/bin/
```

- **Linux**：需要 glibc 2.35 或更新版本（Ubuntu 22.04、Debian 12、Fedora 36、RHEL 10、Arch 及更新版本）。NixOS 需要设置 `programs.nix-ld.enable = true`。
- **macOS**：二进制文件没有签名。如果被 macOS 拦截，运行 `xattr -d com.apple.quarantine ref ref-server`。
- **Windows**：需要安装 [Git for Windows](https://git-scm.com/download/win)，`bash` 工具使用其中的 Git Bash。

每个发布版都附带 `SHA256SUMS` 文件，用于校验下载内容。

### 从源码构建

```bash
git clone https://github.com/GDNDZZK/ref.git
cd ref
cargo build --release      # 需要当前稳定版 Rust 和 C 编译器
./target/release/ref
```

## 快速开始

1. 设置 API 密钥，例如 `export ANTHROPIC_API_KEY=...`。也可以使用 `OPENAI_API_KEY`、`DEEPSEEK_API_KEY` 和 `OPENROUTER_API_KEY`。其他提供商请参阅[配置](docs/configuration.zh-CN.md#提供商与模型)。
2. 在项目目录中运行 `ref`。
3. 输入任务并按 `enter`。

| 按键 | 作用 |
|---|---|
| `ctrl+x m` / `ctrl+x a` | 选择模型 / 智能体 |
| `tab` | 切换到下一个智能体（`build` 可编辑，`plan` 只读） |
| `ctrl+x l` | 会话列表（运行中的和历史的） |
| `ctrl+x w` | 后台会话分屏视图 |
| `esc esc` | 中断 |
| `ctrl+x h` | 全部按键和命令 |

## 文档

| 文档 | 内容 |
|---|---|
| [使用 ref](docs/usage.zh-CN.md) | 会话、界面、提示输入、快捷键、斜杠命令、分屏视图、多会话协作 |
| [配置](docs/configuration.zh-CN.md) | 全部配置项：提供商与模型、智能体、权限、延迟、压缩、MCP、命令、指令和技能 |
| [工具](docs/tools.zh-CN.md) | 模型能做什么：文件工具、bash、webfetch、图片处理、上下文与跨会话工具 |
| [命令行](docs/cli.zh-CN.md) | `ref` 和 `ref-server` 的选项、远程后端、文件和环境变量 |
| [开发](docs/development.zh-CN.md) | 构建、测试、crate 结构、CI 和发布 |

## 许可证

[GPL-3.0](LICENSE)
