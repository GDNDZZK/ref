[English](tools.md) · **简体中文**

# 工具

以下是模型可以调用的工具。所有工具（包括 MCP 工具）都带有一个 `purpose` 参数：一句简短的说明，TUI 会把它显示在折叠的调用上，这样不用逐个展开就能看懂模型在做什么。

工具错误会作为结果返回给模型，不会结束会话的当前轮次。

- [文件工具](#文件工具)
- [bash](#bash)
- [webfetch](#webfetch)
- [图片](#图片)
- [上下文工具](#上下文工具)
- [跨会话工具](#跨会话工具)
- [MCP 工具](#mcp-工具)

## 文件工具

| 工具 | 作用 |
|---|---|
| `read` | 读取文本文件、列出目录或返回图片。默认每页 500 行（`limit` 最多 3000，约 60 KB）。用 `offset` / `limit`，或 `file.rs:50-80`、`file.rs:50+10` 这样的后缀选择范围。超过 1000 字符的行会被截断。拒绝读取二进制文件。 |
| `grep` | 在文件或目录树中做正则搜索（Rust 语法），可选上下文行数和 `glob` 过滤。遵守 `.gitignore`，包含隐藏文件，跳过 `.git`、二进制文件和超过 4 MB 的文件。默认最多 200 条匹配（上限 2000）。 |
| `find` | 按 glob 查找文件和目录，最新的排在前面。遵守 `.gitignore`。默认最多 500 条结果。 |
| `edit` | 按行号修改已有文件（见下文）。 |
| `write` | 创建文件，或完全替换文件内容。会创建父目录。已有文件保留原来的换行符和 BOM。 |
| `delete` | 删除文件、符号链接，或（使用 `recursive` 时）目录。拒绝删除工作目录、`/` 和主目录。 |

路径相对于会话目录。会话目录内的路径在所有平台上都以相对路径和 `/` 分隔符显示。

### 行锚点

文件工具使用带哈希标签的行号，参考了 [oh-my-pi](https://github.com/can1357/oh-my-pi) 的做法：

```
[src/main.rs#3F2A]
1:fn main() {
2:    println!("hi");
3:}
```

`read` 和 `grep` 先输出 `[path#TAG]` 头（`TAG` 是整个文件的哈希），然后是 `N:content` 行。`edit` 接收该标签和该版本文件中的行号，一次性应用所有修改（替换、删除、在前或在后插入），因此同一次调用中靠前的修改不会让后面的行号偏移。

如果文件在标签发出后发生了变化，ref 会尽量通过 diff 把行号映射到当前文件上。映射不了时拒绝编辑，并返回所请求行附近的当前内容，让模型无需重新读取整个文件就能重试。

## bash

在会话目录中用 `bash -c` 运行命令，stdout 和 stderr 按顺序合并输出。

- 超时：默认 120 秒，最长 3600 秒（`0` 表示不限时）。超时后终止整个进程组：Linux 和 macOS 上先发 SIGTERM，2 秒后发 SIGKILL；Windows 上使用 `taskkill /T /F`。
- 超过 50 KB 或 2000 行的输出会从中间截断，完整输出保存到一个模型可以 `read` 的文件中。
- 命令运行时，输出会实时显示在 TUI 中。
- 命令在 `TERM=dumb`、`NO_COLOR=1` 和 `PAGER=cat` 环境下运行，因此工具不会停下来等分页器，也不会输出颜色代码。
- **必须有 bash。** Windows 上请安装 [Git for Windows](https://git-scm.com/download/win)：ref 通过 `PATH` 中的 `git.exe` 或默认安装位置找到其中的 Git Bash。`System32` 中的 WSL `bash.exe` 会被跳过，因为它在 Linux 虚拟机中运行命令，路径不一样。macOS 上系统自带的 `/bin/bash`（3.2）可以使用；如果 `PATH` 中更靠前的位置有更新的 bash，则使用后者。

## webfetch

获取 `http(s)` URL 的内容。

- **HTML** 默认转换为 markdown（也可以转换为纯文本或保留 HTML）。对于文章页面，除非设置 `full_page`，只保留正文；发生这种情况时结果头会注明。
- **Markdown**：如果网站通过内容协商或 `<link rel="alternate" type="text/markdown">` 提供 markdown，直接使用它。
- **JSON** 会格式化输出。**RSS 和 Atom 订阅源**转换为条目列表。5 MB 以内的**图片**作为图片返回（参见[图片](#图片)）。
- **GitHub**：文件页面返回原始文件，issue 和 pull request 通过 API 返回讨论内容。设置了 `GITHUB_TOKEN` 或 `GH_TOKEN` 时会使用它。
- 其他二进制内容（包括 PDF）不显示，会提示模型用 `curl` 下载。
- 字符集来自 HTTP 头、页面本身或自动检测。
- 超过 50 KB 或 2000 行的内容会被截断，完整文本保存到文件中供 `read` 读取。
- 会使用 `HTTP_PROXY`、`HTTPS_PROXY` 和 `NO_PROXY`，但本地和私有地址始终直接访问。

## 图片

`images.auto_resize` 开启时（默认开启），来自 `read`（最大 10 MB）、`webfetch` 以及附加到提示中的图片会按 oh-my-pi 的方式缩放。MCP 返回的图片保持原样。

- 最长边缩放到 1568 px 以内，这也是 Claude 自己会缩放到的尺寸。短边小于 200 px 的图片会被放大。
- 在 PNG 和 JPEG（质量 80）中保留较小的那个。如果结果仍超过目标大小（500 KB，`webfetch` 为 300 KB），JPEG 质量降到 40，再不行就把图片缩小到四分之一。带透明度的图片保持 PNG。
- 尺寸在范围内的小图片（小于目标大小的四分之一）原样发送。
- 尺寸发生变化时，工具结果会告诉模型原始尺寸，以及把坐标换算回原图的比例。
- `read` 拒绝无法解码的图片，因为提供商会拒绝整个请求。

无论这个设置如何，发往 Claude 模型的请求（Anthropic API，或其他 API 上 id 中包含 `claude` 的模型）都会按 Claude 的限制检查：单边超过 8000 px 或 base64 超过 10 MB 的图片会被缩小；一个请求中的图片超过 20 张时，所有图片都限制在 2000 px 以内。只有发出的请求会改变：会话保留原图，结果会被缓存，重复请求不需要重新编码。

当提供商拒绝请求，并在错误信息中给出更严格的图片限制时（例如 Bedrock 的 `image exceeds 5 MB maximum: … > 5242880 bytes`，或 `image dimensions exceed max allowed size: N pixels`），ref 会为该提供商记住这个限制直到后端重启，并按此限制调整图片后重新发送请求。这对所有提供商都有效。如果图片错误中没有可识别的限制，则按其他不可重试的错误处理。

## 上下文工具

- `context_window`：当前会话或其他会话已用的 token 数、上限，以及建议压缩点和自动压缩点。
- `compact_context`：用模型自己写的摘要替换历史。

参见[上下文压缩](configuration.zh-CN.md#上下文压缩)。

## 跨会话工具

这些工具在后端内部运行，因此会话之间的消息能可靠送达。会话名可以是任意 Unicode 文本。

- `list_sessions`：可以发送消息的运行中会话：名称、id、智能体、状态、前台或后台、目录和标题。
- `send_message`：向另一个会话发送消息。忙碌的会话会在当前步骤结束后读取。
- `message_status`：已发送的消息是仍在排队、已送达还是已读。
- `read_session`：分页读取另一个会话的消息，最新的在前。
- `create_session`、`close_session`：仅供设置了 [`can_spawn`](configuration.zh-CN.md#智能体字段) 的智能体使用。创建的会话在后台运行。使用 `must_report` 时，如果创建的会话停止时没有发送过消息，它会收到提醒去汇报。`close_session` 拒绝关闭前台会话。

## MCP 工具

来自 [MCP 服务器](configuration.zh-CN.md#mcp-服务器)的工具以 `<server>_<tool>` 的名称提供。其结果中的文本、图片、资源和结构化内容都会传给模型。
