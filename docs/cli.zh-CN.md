[English](cli.md) · **简体中文**

# 命令行

- [`ref`](#ref)
- [`ref-server`](#ref-server)
- [在另一台机器上运行后端](#在另一台机器上运行后端)
- [文件与目录](#文件与目录)
- [环境变量](#环境变量)

## `ref`

终端界面。

```
ref [PATH] [options]
```

| 选项 | 含义 |
|---|---|
| `PATH` | 新会话的目录（默认：当前目录）。 |
| `-c`, `--continue` | 继续此目录最近的会话。 |
| `-s`, `--session ID` | 按 id 打开会话，可以是运行中的，也可以是历史中的。 |
| `-w`, `--watch` | 所有运行中后台会话的分屏视图。不创建会话。 |
| `--agent NAME` | 新会话使用的智能体。 |
| `--model provider/model[#variant]` | 新会话使用的模型。不会记住给之后的会话。 |
| `-p`, `--prompt TEXT` | 立即发送这条提示。 |
| `--server ws://host:port` | 连接到这个后端，而不是查找或启动一个。 |
| `--token T` | `--server` 使用的令牌。 |
| `--mock` | 使用内置的假后端运行（用于开发）。 |
| `-V`, `--version`, `-h`, `--help` | 版本和帮助。 |

不带 `--server` 时，`ref` 会在实例文件（见下文）中查找运行中的后端并连接。如果没有，它会从 `ref` 所在目录（或 `PATH`）启动 `ref-server`，使用随机空闲端口和 `--idle-timeout 0`，这样最后一个 `ref` 断开后后端就会退出。

## `ref-server`

后端。通常不需要手动启动，但也可以手动启动，例如在没有窗口的情况下保持会话运行，或从另一台机器连接。

```
ref-server [options]
```

| 选项 | 含义 |
|---|---|
| `--port N` | 端口。不设置或为 `0` 时在 10000–65535 中随机选择空闲端口。 |
| `--host ADDR` | 绑定地址。默认 `127.0.0.1`；允许非回环主机时为 `0.0.0.0`。 |
| `--allow-host H` | 允许连接的主机或 IP。可重复，可用逗号分隔；`*` 允许任意主机。默认：`127.0.0.1`、`::1`、`localhost`。 |
| `--idle-timeout S` | 最后一个客户端断开后继续运行的秒数。`0` 立即退出，`-1`（默认）永不退出。从第一次连接之后才开始计算。 |
| `--token T` | 客户端必须发送的密钥。默认随机生成，保存在实例文件中。 |
| `--log FILE` | 日志文件。默认：`~/.local/state/ref/logs/server-<pid>.log`。 |
| `--status [--json]` | 列出运行中的后端及其端口、地址、目录和设置。没有运行中的后端时退出码为 1。 |
| `--stop [PID]` | 停止所有运行中的后端，或停止指定 PID 的后端。 |
| `-V`, `--version`, `-h`, `--help` | 版本和帮助。 |

## 在另一台机器上运行后端

```bash
# 在服务器上
ref-server --port 7777 --allow-host 192.168.1.20 --token "$(openssl rand -hex 16)" --idle-timeout -1

# 在你的机器上
ref --server ws://server:7777 --token <同一个令牌>
```

连接使用不带 TLS 的普通 WebSocket，任何拿到令牌的人都能控制后端，而后端可以在那台机器上运行命令。只允许你信任的主机，或者通过 SSH 隧道连接并保留默认的仅回环设置：

```bash
ssh -L 7777:127.0.0.1:7777 server
ref --server ws://127.0.0.1:7777 --token <token>
```

## 文件与目录

| 路径 | 内容 |
|---|---|
| `~/.config/ref/`, `~/.config/opencode/` | 配置。参见[配置](configuration.zh-CN.md#配置文件的读取位置)。 |
| `~/.local/share/ref/sessions/` | 保存的会话。 |
| `~/.local/state/ref/instances/<pid>.json` | 每个运行中的后端一个文件：端口、令牌和设置。只有你自己可读（权限 600）。 |
| `~/.local/state/ref/logs/` | 后端日志，包括配置警告。 |

在 Windows 上，`~` 指 `%USERPROFILE%`。

## 环境变量

| 变量 | 含义 |
|---|---|
| `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `DEEPSEEK_API_KEY`, `OPENROUTER_API_KEY` | 添加对应的内置提供商（参见[快速开始](configuration.zh-CN.md#快速开始)）。 |
| `OPENCODE_CONFIG`, `REF_CONFIG` | 额外加载的配置文件。 |
| `OPENCODE_CONFIG_DIR` | 用这个目录代替 `~/.config/opencode`。 |
| `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME` | 代替 `~/.config`、`~/.local/share` 和 `~/.local/state`。 |
| `REF_DATA_DIR`, `REF_STATE_DIR` | 直接代替数据目录和状态目录（`…/share/ref` 和 `…/state/ref`）。 |
| `REF_LOG` | 后端日志过滤器，使用 `tracing` 语法（默认 `info`；例如 `debug` 或 `ref_llm=trace`）。 |
| `REF_DUMP_REQUESTS=DIR` | 把每个 LLM 请求体写入 `DIR`（包含 API 密钥的请求头永远不会写入）。 |
| `REF_TUI_LOG=FILE` | 把 TUI 的输入调试日志写入 `FILE`。 |
| `GITHUB_TOKEN`, `GH_TOKEN` | `webfetch` 请求 GitHub API 时使用。 |
| `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` | 对外 HTTP 请求的代理：LLM API、`webfetch` 和远程 MCP 服务器。 |
| `VISUAL`, `EDITOR` | `ctrl+x e` 使用的编辑器（默认 `vi`，Windows 上为 `notepad`）。 |
