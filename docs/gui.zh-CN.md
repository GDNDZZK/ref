[English](gui.md) · **简体中文**

# 桌面 GUI

- [安装](#安装)
- [启动](#启动)
- [文件与后端](#文件与后端)
- [构建与开发](#构建与开发)

`ref-gui` 在桌面窗口中显示 ref（Qt Quick）。与 `ref` 和 `ref-web` 一样，它只负责显示会话：会话运行在 `ref-server` 中，所以同一个会话可以同时在终端、GUI 和浏览器标签页中打开。没有运行中的后端时，`ref-gui` 会启动它所在文件夹里的 `ref-server`；这样启动的后端在最后一个客户端断开后退出。

## 安装

从[最新发布版](https://github.com/GDNDZZK/ref/releases/latest)下载 `ref-gui-<version>-<platform>`（或 `ref-all-…`）；README 的[安装](../README.zh-CN.md#安装)一节列出了各平台以及各系统的要求。压缩包里是一个文件夹：

| 系统 | 内容 |
|---|---|
| Linux | `ref-gui`、`ref-server`、`lib/`、`plugins/` 和 `qml/`（Qt 和其他库）、`qt.conf`、`ref-gui.desktop`、`ref.png` |
| macOS | `ref.app`，里面有 `ref-server` 和全部库 |
| Windows | `ref-gui.exe`、`ref-server.exe`、Qt 和 Visual C++ 运行库的 DLL、`plugins\` 和 `qml\` |

请保持文件夹完整，并从它所在的位置运行程序；整个文件夹可以移动。不需要安装，也不需要管理员权限。除系统自带的库之外，GUI 需要的一切都在文件夹里，所以它不会使用系统上安装的 Qt。

Linux 上程序运行在 X11 上，或通过 XWayland 运行。不包含 Qt 的 Wayland 插件。

## 启动

```bash
ref-gui                       # 在当前目录新建会话
ref-gui ~/src/project         # 在另一个目录新建会话
ref-gui -c                    # 继续此目录最近的会话
ref-gui -s ses_01a1…          # 按 id 打开会话（运行中的或历史中的）
ref-gui --agent plan --model openai/gpt-5#high
```

| 选项 | 含义 |
|---|---|
| `DIR` | 新会话的目录（默认：当前目录）。 |
| `-c`, `--continue` | 继续此目录最近的会话。 |
| `-s`, `--session ID` | 按 id 打开会话，运行中的或历史中的。 |
| `-w`, `--watch` | 所有运行中后台会话的分屏视图（暂未提供）。 |
| `--agent NAME` | 新会话使用的智能体。 |
| `--model provider/model[#variant]` | 新会话使用的模型。 |
| `--server ws://host:port` | 连接指定的后端，而不是查找或启动后端。 |
| `--token T` | `--server` 的 token。必须与 `--server` 一起使用。 |
| `--mock` | 使用带预设脚本的私有假后端（开发用，见 [Mock 后端](development.zh-CN.md#mock-后端)）。不能与 `--server` 同时使用。 |
| `-V`, `--version`, `-h`, `--help` | 版本和帮助。 |

窗口标题是 `ref`。Windows 上的 `ref-gui.exe` 是纯窗口程序，不会打开控制台，所以 `--help` 和 `--version` 在那里没有输出。

## 文件与后端

GUI 与 `ref` 使用相同的 instance 文件、配置和会话存储（[文件与目录](cli.zh-CN.md#文件与目录)），也使用相同的环境变量。

## 构建与开发

见开发文档中的 [GUI](development.zh-CN.md#gui) 一节。
