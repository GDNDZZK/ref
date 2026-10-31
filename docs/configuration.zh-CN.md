[English](configuration.md) · **简体中文**

# 配置

ref 读取 [opencode](https://opencode.ai) 的配置格式：已有 `opencode.json` 中的提供商、模型、智能体、权限、MCP 服务器和命令都会被加载。ref 还增加了几个自己的配置项（下文标注 **ref**）。

有些 opencode 配置项会被接受但不起作用：`command.<name>.agent` 和 `command.<name>.model`、智能体的 `steps` 和 `maxSteps`，以及模型的 `limit.input`。智能体的 `mode` 只有一个用途（见[智能体](#智能体)）。

- [快速开始](#快速开始)
- [配置文件的读取位置](#配置文件的读取位置)
- [语法](#语法)
- [顶层配置项](#顶层配置项)
- [提供商与模型](#提供商与模型)
- [智能体](#智能体)
- [权限](#权限)
- [工具延迟](#工具延迟)
- [上下文压缩](#上下文压缩)
- [重试、图片与 TUI 默认值](#重试图片与-tui-默认值)
- [MCP 服务器](#mcp-服务器)
- [自定义命令](#自定义命令)
- [指令、上下文文件与技能](#指令上下文文件与技能)
- [完整示例](#完整示例)

## 快速开始

设置一个 API 密钥就可以开始使用。如果设置了下列环境变量之一，并且你没有自己配置该提供商，ref 会自动添加该提供商和一份内置的小型模型列表：

| 变量 | 提供商 id | 内置模型 |
|---|---|---|
| `ANTHROPIC_API_KEY` | `anthropic` | `claude-sonnet-4-5`、`claude-opus-4-1`、`claude-haiku-4-5` |
| `OPENAI_API_KEY` | `openai` | `gpt-5`、`gpt-5-mini` |
| `DEEPSEEK_API_KEY` | `deepseek` | `deepseek-chat`、`deepseek-reasoner` |
| `OPENROUTER_API_KEY` | `openrouter` | `anthropic/claude-sonnet-4.5`、`openai/gpt-5` |

对于其他兼容 OpenAI 的接口，创建 `~/.config/ref/ref.json`：

```jsonc
{
  "model": "mymodels/some-model",
  "provider": {
    "mymodels": {
      "options": { "baseURL": "https://example.com/v1", "apiKey": "{env:MY_KEY}" },
      "models": {
        "some-model": { "limit": { "context": 200000, "output": 32000 } }
      }
    }
  }
}
```

## 配置文件的读取位置

所有存在的文件都会被加载并合并，后加载的文件优先。

1. **全局目录**，按以下顺序：
   - `~/.config/opencode/`（设置了 `$OPENCODE_CONFIG_DIR` 时使用该目录）
   - `~/.config/ref/`

   每个目录中读取：`opencode.json`、`opencode.jsonc`、`config.json`、`ref.json`、`ref.jsonc`。设置了 `$XDG_CONFIG_HOME` 时，它会替代 `~/.config`。
2. **显式指定的文件**：先 `$OPENCODE_CONFIG`，后 `$REF_CONFIG`。路径不存在时会产生警告。
3. **项目目录**，从 git 根目录（最近的包含 `.git` 的上级目录）一直到工作目录。不在 git 仓库中时只使用工作目录。每个目录中读取：
   `opencode.json`、`opencode.jsonc`、`.opencode/opencode.json`、`.opencode/opencode.jsonc`、`ref.json`、`ref.jsonc`、`.ref/ref.json`、`.ref/ref.jsonc`。

Markdown 智能体和命令从同样的全局目录以及每个 `.opencode/` 和 `.ref/` 目录中加载（参见[智能体](#markdown-智能体)和[自定义命令](#markdown-命令)）。

**文件如何合并：** 对象按键递归合并。其他值（字符串、数字、数组）会替换之前的值。有三类内容会从每个文件收集而不是替换：权限规则（按顺序追加）、`instructions` 和技能路径。

每次新建会话和执行 `/cd` 时都会重新读取配置，因此修改无需重启后端即可生效。

无效的 JSON、未知的权限动作或缺少 `template` 等问题不会让 ref 停止运行。出问题的文件或条目会被跳过，并在后端日志（`~/.local/state/ref/logs/`，参见[命令行](cli.zh-CN.md#文件与目录)）中写入警告。

## 语法

- **JSONC**：所有文件都允许 `//` 和 `/* */` 注释以及尾随逗号。
- **`{env:VAR}`**：任意字符串中的这个写法会被替换为该变量的值，变量未设置时替换为空字符串。
- **`{file:path}`**：替换为该文件的内容（去掉首尾空白）。`~` 和相对路径基于包含它的配置文件所在目录解析。文件无法读取时产生警告并替换为空字符串。

```jsonc
{
  "provider": {
    "work": {
      "options": {
        "apiKey": "{env:WORK_API_KEY}",
        "headers": { "X-Team": "{file:~/.config/ref/team-id}" },
      },
    },
  },
}
```

## 顶层配置项

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `model` | `"provider/model"` | 第一个已配置的模型 | 未设置模型的智能体使用的模型。 |
| `small_model` | `"provider/model"` | 会话模型 | 用于生成会话标题的模型。 |
| `default_agent` | string | `"build"` | 新会话使用的智能体。 |
| `provider` | object | 根据环境变量添加的内置提供商 | [提供商与模型](#提供商与模型)。 |
| `enabled_providers` | string[] | 全部 | 只加载这些提供商 id。 |
| `disabled_providers` | string[] | 无 | 跳过这些提供商 id。 |
| `agent` | object | 内置智能体 | [智能体](#智能体)。也接受旧名称 `mode`。 |
| `permission` | string 或 object | 全部允许 | [权限](#权限)。 |
| `tools` | object | | opencode 的旧写法：`{"bash": false}` 禁用一个工具。 |
| `delay` | object 或 number | 无延迟 | **ref** [工具延迟](#工具延迟)。 |
| `compaction` | object | 90% 时自动压缩 | **ref** [上下文压缩](#上下文压缩)。 |
| `retry` | object | `{"max_delay": 600}` | **ref** [重试](#重试图片与-tui-默认值)。 |
| `images` | object | `{"auto_resize": true}` | **ref** [图片](#重试图片与-tui-默认值)。 |
| `tui` | object | | **ref** [TUI 默认值](#重试图片与-tui-默认值)。 |
| `mcp` | object | 无 | [MCP 服务器](#mcp-服务器)。 |
| `command` | object | 无 | [自定义命令](#自定义命令)。 |
| `instructions` | string[] | 无 | 额外的指令文件或 glob。 |
| `skills` | string[] 或 `{"paths": [...]}` | 无 | 额外的技能目录。 |

## 提供商与模型

```jsonc
"provider": {
  "<provider id>": {
    "name": "Display name",                  // 默认：id
    "npm": "@ai-sdk/openai-compatible",      // 决定使用哪种 API，见下文
    "api": "chat",                           // 覆盖 npm："chat" | "responses" | "anthropic"
    "env": ["MY_KEY", "MY_OTHER_KEY"],       // 从这些环境变量读取 API 密钥
    "options": {
      "baseURL": "https://example.com/v1",
      "apiKey": "{env:MY_KEY}",
      "headers": { "X-Org": "acme" },
      "setCacheKey": true,
      "chunkTimeout": 300000
    },
    "whitelist": ["model-a"],                // 只使用这些模型
    "blacklist": ["model-b"],                // 不使用这些模型
    "models": { /* 见下文“模型字段” */ }
  }
}
```

### API 选择

ref 支持三种 API。除非设置了 `api`，否则根据 `npm` 选择：

| `npm` | API |
|---|---|
| `@ai-sdk/anthropic`、`@ai-sdk/google-vertex/anthropic` | Anthropic Messages |
| `@ai-sdk/openai`、`@ai-sdk/azure` | OpenAI Responses |
| 其他值或未设置 | OpenAI Chat Completions |

`api` 可取 `chat`（也可写 `openai-chat`、`openai-compatible`、`completions`、`chat-completions`）、`responses`（也可写 `openai-responses`、`openai`）和 `anthropic`（也可写 `messages`、`anthropic-messages`）。如果 `api` 是 `http(s)://` URL（models.dev 的文件就是这样），则改为把它当作 base URL。

### 提供商字段

| 字段 | 含义 |
|---|---|
| `options.baseURL` | API 的 base URL（也可写 `baseUrl` 和 `base_url`）。id 为 `anthropic`、`openai`、`deepseek` 和 `openrouter` 时有默认值。 |
| `options.apiKey` | API 密钥（也可写 `api_key`）。 |
| `env` | `apiKey` 为空时，使用此列表中第一个非空的变量。四个内置 id 默认使用各自的常规变量，`@ai-sdk/anthropic` / `@ai-sdk/openai` 还会回退到 `ANTHROPIC_API_KEY` / `OPENAI_API_KEY`。 |
| `options.headers` | 每个请求附加的 HTTP 头。 |
| `options.setCacheKey` | 发送按会话区分的提示缓存键。默认：Responses API 和 `api.openai.com` 开启。 |
| `options.chunkTimeout` | 等待下一个流式数据块的毫秒数，超时后重试请求。默认 `300000`。 |
| `whitelist` / `blacklist` | 按键过滤 `models`。 |

### 模型字段

```jsonc
"models": {
  "some-model": {
    "id": "vendor/some-model-2026",     // 发送给 API 的 id（默认：键名）
    "name": "Some Model",               // 显示名称（默认：键名）
    "limit": {
      "context": 200000,                // 上下文窗口 token 数（默认 128000）
      "output": 32000,                  // 最大输出 token 数
      "input": 180000,                  // 为兼容 opencode 而接受，不使用
      "compact": 120000,                // ref：建议的压缩点（token 数）
      "threshold": 0.8                  // ref：该模型的自动压缩点（比例；大于 1 时为 token 数）
    },
    "attachment": true,                 // 接受图片（或 "modalities": {"input": ["text", "image"]}）
    "reasoning": true,                  // 推理模型（也可写 "thinking"）
    "temperature": 0.7,                 // 只发送数字；opencode 的 `true` 会被忽略
    "top_p": 0.9,
    "headers": { "X-Model": "1" },
    "options": { "reasoningEffort": "medium" },
    "variants": {
      "high": { "reasoningEffort": "high" },
      "off":  { "disabled": true }      // 移除该变体
    },
    "disabled": false                   // true 表示移除该模型
  }
}
```

模型写作 `provider/model`，带变体时写作 `provider/model#variant`（例如 `--model openai/gpt-5#high`）。在 TUI 中按 `ctrl+t` 可在模型的各个变体之间切换。

### 请求选项

`options` 会加入请求体。键名按 opencode 的习惯使用 camelCase，发送给 API 时转换为 snake_case，例如 `reasoningEffort` 变为 `reasoning_effort`。对象按以下顺序合并，后者优先：

1. 模型的 `options`
2. 智能体的 `options`，以及智能体上所有无法识别的键
3. 选中的变体

例如，`{"thinking": {"type": "enabled", "budgetTokens": 16000}}` 在 Anthropic API 上开启扩展思考。在 Responses API 上，`{"reasoningEffort": "high", "reasoningSummary": "auto"}` 会设置 `reasoning.effort` 和 `reasoning.summary`。

`extraBody`（或 `body`）按原样合并到请求体中，不转换键名。用于必须保持原有拼写的字段。

### 会话使用哪个模型

新会话使用其智能体的 `model`（以及智能体的 `variant`）。智能体没有设置时使用顶层 `model`，再没有则使用第一个提供商的第一个模型。命令行的 `--model` 和模型选择器（`ctrl+x m`）只对单个会话生效。

## 智能体

智能体由一段系统提示词和一组设置组成：默认模型、权限和延迟。每个会话运行一个智能体。在 TUI 中按 `tab` 切换智能体，按 `ctrl+x a` 打开智能体列表。

### 内置智能体

| 智能体 | 用途 | 限制 |
|---|---|---|
| `build` | 默认智能体，可使用全部工具。 | 可以创建和关闭其他会话（`can_spawn`）。 |
| `plan` | 只读的分析与规划。 | 禁止 `edit`、`write`、`delete`；`bash` 需询问。 |
| `general` | 在独立会话中执行多步骤任务。 | 无。 |
| `explore` | 快速、只读地浏览代码库。 | 禁止 `edit`、`write`、`delete`。 |
| `compaction` | 隐藏。在会话压缩时撰写摘要。 | 不能运行会话。 |

定义一个同名智能体即可修改内置智能体，只有你设置的字段会改变。`"disable": true` 会移除一个智能体。给 `compaction` 设置 `model` 后，摘要会使用该模型而不是会话的模型。

### 智能体字段

```jsonc
"agent": {
  "reviewer": {
    "description": "Reviews diffs and reports problems.",
    "prompt": "{file:./prompts/reviewer.md}",
    "model": "anthropic/claude-sonnet-4-5",
    "variant": "high",
    "temperature": 0.2,
    "top_p": 0.9,
    "options": { "maxTokens": 8000 },
    "permission": { "edit": "deny", "bash": { "*": "ask", "git diff*": "allow" } },
    "delay": { "bash": 0 },
    "creatable": true,
    "can_spawn": false,
    "color": "#e5c07b",
    "hidden": false,
    "mode": "subagent"
  }
}
```

| 字段 | 默认值 | 含义 |
|---|---|---|
| `description` | `""` | 显示在智能体列表中，也会提供给能创建会话的智能体，让它们知道何时使用这个智能体。 |
| `prompt` | `""` | 智能体提示词，放在系统提示词的开头。通常用 `{file:...}` 把它放在单独的文件里。 |
| `model`、`variant` | | 使用此智能体的会话的默认模型。 |
| `temperature`、`top_p` | | 采样参数。 |
| `options` | | 合并到模型选项之上的请求选项（参见[请求选项](#请求选项)）。智能体上无法识别的键也会被当作选项。 |
| `permission` | | 在全局规则之后检查的规则，因此优先级更高。 |
| `delay` | | 在全局延迟之前检查的延迟。 |
| `creatable` | `true` | **ref** 允许其他会话创建使用此智能体的会话。 |
| `can_spawn` | `false`（`build` 为 `true`） | **ref** 为智能体提供 `create_session` 和 `close_session` 工具。也可写 `spawn`。 |
| `color` | | 智能体名称在 TUI 中的颜色。 |
| `hidden` | `false` | 不在 TUI 中列出，也不提供给其他会话。 |
| `mode` | | opencode 的 `primary` / `subagent` / `all`。ref 不按 mode 限制智能体。只有在没有 `build` 智能体、也没有有效的 `default_agent` 时才有用：第一个不是 `subagent` 且未隐藏的智能体成为默认智能体。 |
| `disable` | `false` | 移除该智能体（也可写 `disabled`）。 |

`steps` 和 `maxSteps` 会被接受并忽略。

### Markdown 智能体

智能体也可以是一个 markdown 文件。去掉 `.md` 的文件名是智能体名称，正文是提示词，frontmatter 中写其他字段：

```markdown
---
description: Reviews diffs and reports problems.
model: anthropic/claude-sonnet-4-5
permission:
  edit: deny
  bash:
    "*": ask
    "git diff*": allow
---
You review code changes. Report bugs, risky changes and missing tests.
Do not modify files.
```

ref 在 `~/.config/opencode/`、`~/.config/ref/` 以及每个项目的 `.opencode/` 和 `.ref/` 目录下的 `agent/`、`agents/`、`mode/` 和 `modes/`（包括子目录）中查找这些文件。`mode/` 或 `modes/` 下的文件成为 `primary` 智能体。frontmatter 支持 `key: value` 键值对、带引号的字符串、数字、布尔值、`[a, b]` 和 `- item` 列表、`|` 和 `>` 块文本，以及通过缩进表示的嵌套映射。

## 权限

每次工具调用在运行前都会按权限规则检查，结果是以下之一：

- `allow`：运行。
- `ask`：由 TUI 询问。`a` 允许本次调用，`s` 在本会话剩余时间内允许该工具的所有调用，`d` 拒绝。
- `deny`：拒绝。模型会收到错误并继续工作。

`auto`、`yes` 和 `true` 也表示 allow；`confirm` 和 `prompt` 表示 ask；`reject`、`no` 和 `false` 表示 deny。

```jsonc
"permission": {
  "*": "allow",
  "bash": { "*": "ask", "git status*": "allow", "git diff*": "allow", "rm *": "deny" },
  "edit": { "*": "allow", "*.lock": "deny" },
  "webfetch": { "*": "ask", "https://docs.rs/*": "allow" },
  "mcp_github_*": "ask"
}
```

`"permission": "ask"` 对所有工具应用同一个动作。

**模式匹配什么。** 键是工具名，可以是 glob（`*` 和 `?`），例如 `mcp_*`。工具下的模式与以下内容匹配：

| 工具 | 匹配对象 |
|---|---|
| `bash` | 命令 |
| `read`、`edit`、`write`、`delete`、`grep`、`find` | 模型写出的 `path` 参数（相对或绝对路径） |
| `webfetch` | URL |
| 其他工具（MCP、跨会话） | 无：只有 `"*"` 生效 |

以 ` *` 结尾的模式也匹配不带参数的命令：`"git *"` 既匹配 `git log`，也匹配 `git`。

**顺序。** 规则按书写顺序跨所有文件检查，最后一条匹配的规则生效。智能体规则在全局规则之后检查，因此智能体的规则优先于全局规则。没有规则匹配时允许调用。

**工具别名。** opencode 的工具名会映射到 ref 的工具：`edit` 涵盖 `edit`、`write` 和 `delete`；`write`、`patch`、`multiedit` 和 `apply_patch` 涵盖 `write` 和 `edit`；`glob`、`list` 和 `ls` 表示 `find`；`shell` 表示 `bash`；`fetch` 和 `web_fetch` 表示 `webfetch`。

**禁用的工具。** 当一个工具最后一条 `"*"` 规则是 `deny`，并且没有模式规则允许或询问它时，这个工具根本不会提供给模型。旧写法 `"tools": {"bash": false}` 效果相同。

## 工具延迟

**ref** 延迟让 ref 在运行工具调用前等待一段时间，以便你先看清内容。TUI 会显示倒计时，倒计时期间按 `esc` 取消该调用。

```jsonc
"delay": { "*": 0, "bash": 2, "delete": 5, "mcp_*": 1 }
```

键是工具名 glob，值是秒数。最后一个匹配的键生效。`"delay": 3` 对所有工具生效。智能体的 `delay` 先检查：只要其中有键匹配，就不再使用全局设置。

## 上下文压缩

**ref** 当对话占满模型的上下文窗口时，ref 会压缩它：`compaction` 智能体撰写摘要，之后的请求发送摘要而不是较早的消息。完整历史仍保留在会话和 TUI 中，压缩发生的位置会显示一条分隔线。

```jsonc
"compaction": {
  "auto": true,        // 到达 `threshold` 时自动压缩（默认 true）
  "recommend": 0.6,    // 告诉模型应当压缩的点（默认 0.6）
  "threshold": 0.9     // 自动压缩点（默认 0.9）
}
```

不大于 1 的值表示上下文窗口的比例，更大的值表示 token 数。模型的 `limit.compact` 会覆盖该模型的 `recommend`。模型的 `limit.threshold` 会覆盖该模型的 `threshold`，取值含义相同。

模型通过 `context_window` 工具看到这两个点，并可以用 `compact_context` 自行压缩。你可以随时用 `ctrl+x c` 或 `/compact` 压缩。

## 重试、图片与 TUI 默认值

```jsonc
"retry":  { "max_delay": 600 },     // 秒；重试退避的上限
"images": { "auto_resize": true },  // 在模型看到图片之前缩小图片
"tui":    { "sidebar": true, "thinking": false, "details": false }
```

- **`retry.max_delay`**（**ref**，默认 `600`）：可重试的 LLM 请求失败后会无限重试：HTTP 408、409、425、429 和 5xx、过载消息、网络错误和超时。等待时间从 2 秒开始，每次翻倍，直到这个上限；会遵守提供商返回的 `retry-after` 头。按 `esc esc` 停止重试。其他错误（例如 API 密钥无效）一共尝试三次，中间分别等待约 2 秒和 4 秒，然后结束本轮并显示错误。
- **`images.auto_resize`**（**ref**，默认 `true`）：来自 `read`、`webfetch` 的图片和粘贴的图片会被缩放并重新压缩到模型处理得好的尺寸。参见[工具](tools.zh-CN.md#图片)。
- **`tui`**（**ref**）：三个 TUI 开关的初始状态：`sidebar`（默认显示）、`thinking`（展开推理内容，`ctrl+x t`）和 `details`（展开工具调用，`ctrl+x d`）。

## MCP 服务器

```jsonc
"mcp": {
  "github": {
    "type": "local",
    "command": ["npx", "-y", "@modelcontextprotocol/server-github"],
    "environment": { "GITHUB_TOKEN": "{env:GITHUB_TOKEN}" },
    "cwd": "/path/to/dir",
    "timeout": 30000
  },
  "docs": {
    "type": "remote",
    "url": "https://mcp.example.com/mcp",
    "headers": { "Authorization": "Bearer {env:DOCS_TOKEN}" }
  },
  "old-server": { "enabled": false }
}
```

| 字段 | 含义 |
|---|---|
| `type` | `local`（也可写 `stdio`）启动一个进程，通过 stdin/stdout 通信。`remote`（也可写 `http`、`sse`、`streamable-http`）使用 Streamable HTTP。未设置时，有 `url` 表示 remote，有 `command` 表示 local。 |
| `command` | local：程序及其参数组成的数组。如果是字符串，会按空格拆分，并在后面追加 `args`。在 Windows 上，`npx` 也能找到 `npx.cmd`。 |
| `environment` | local：额外的环境变量（也可写 `env`）。 |
| `cwd` | local：进程的工作目录。 |
| `url`、`headers` | remote：接口地址和 HTTP 头。 |
| `timeout` | 启动服务器并列出其工具的允许时间，单位毫秒（默认 `30000`）。工具调用没有时间限制，可用 `esc esc` 中断。 |
| `enabled` | `false` 表示保留配置但不启动服务器。只设置 `enabled` 的文件可以开关另一个文件中定义的服务器。 |

MCP 工具以 `<server>_<tool>` 的名称提供给模型，字母、数字、`_` 和 `-` 以外的字符会被替换为 `_`。在 `permission` 和 `delay` 中使用这个名称，例如 `"github_*": "ask"`。侧边栏和 `/mcp` 显示每个服务器的状态。

## 自定义命令

命令是一个提示词模板，在 TUI 中以 `/name` 运行。

```jsonc
"command": {
  "review": {
    "description": "Review the current diff",
    "template": "Review this diff and list problems:\n\n!`git diff`\n\nFocus on: $ARGUMENTS"
  },
  "fix-issue": {
    "description": "Fix a GitHub issue",
    "template": "Fix issue #$1 in repository $2."
  }
}
```

模板展开后，结果作为你的提示词发送：

- `$ARGUMENTS` 替换为命令名之后输入的全部内容。
- `$1` 到 `$9` 替换为单个词。模板中编号最大的占位符获取剩余的所有词。
- `` !`cmd` `` 替换为命令的输出。命令在会话目录中用 bash 运行，限时 30 秒。
- 模板中没有占位符时，参数追加在一个空行之后。

`agent` 和 `model` 为兼容 opencode 而接受，但目前不起作用：提示词会发送到当前会话。

### Markdown 命令

命令可以是 markdown 文件，放在与 [Markdown 智能体](#markdown-智能体)相同目录下的 `command/` 或 `commands/` 中。文件名是命令名，正文是模板，frontmatter 中可以写 `description`：

```markdown
---
description: Review the current diff
---
Review this diff and list problems:

!`git diff`
```

## 指令、上下文文件与技能

**上下文文件**会加入每个会话的系统提示词：

1. `~/.config/opencode/AGENTS.md` 和 `~/.config/ref/AGENTS.md`
2. 从 git 根目录到工作目录的每个目录中的 `AGENTS.md`；没有 `AGENTS.md` 时使用 `CLAUDE.md`
3. `instructions` 中列出的文件。条目可以是路径或 glob，可以是绝对路径、以 `~` 开头，或相对于会话目录。URL 会被跳过。

```jsonc
"instructions": ["docs/CONTRIBUTING.md", "~/notes/style-*.md"]
```

**技能**是包含 `SKILL.md` 文件的目录，`SKILL.md` 的 frontmatter 中有 `name` 和 `description`。系统提示词会列出每个技能的名称、描述和路径，任务匹配时模型用 `read` 读取对应的 `SKILL.md`。ref 在以下位置查找技能：

- 全局配置目录以及每个项目 `.opencode/` 和 `.ref/` 目录中的 `skill/` 和 `skills/`
- `~/.claude/skills`、`~/.agents/skills`，以及每个项目目录中的 `.claude/skills` 和 `.agents/skills`
- `skills` 中列出的目录（`"skills": ["path"]` 或 `"skills": {"paths": ["path"]}`），相对于列出它们的配置文件

两个技能同名时，使用此列表中后找到的那个。

## 完整示例

```jsonc
// ~/.config/ref/ref.json
{
  "model": "anthropic/claude-sonnet-4-5",
  "small_model": "anthropic/claude-haiku-4-5",

  "permission": {
    "bash": { "*": "ask", "git status*": "allow", "git diff*": "allow", "git log*": "allow", "rm *": "deny" },
    "webfetch": { "*": "allow" },
    "delete": "ask"
  },
  "delay": { "bash": 1 },
  "compaction": { "recommend": 0.5, "threshold": 0.85 },

  "agent": {
    "build": { "variant": "high" },
    "reviewer": {
      "description": "Reviews diffs and reports problems. Does not edit files.",
      "prompt": "{file:./prompts/reviewer.md}",
      "permission": { "edit": "deny" }
    }
  },

  "provider": {
    "anthropic": {
      "models": {
        "claude-sonnet-4-5": {
          "attachment": true,
          "reasoning": true,
          "limit": { "context": 200000, "output": 64000 },
          "variants": { "high": { "thinking": { "type": "enabled", "budgetTokens": 16000 } } }
        },
        "claude-haiku-4-5": { "limit": { "context": 200000, "output": 64000 } }
      }
    },
    "local": {
      "name": "Local vLLM",
      "options": { "baseURL": "http://localhost:8000/v1" },
      "models": { "qwen3-coder": { "limit": { "context": 131072, "output": 16384 } } }
    }
  },

  "mcp": {
    "github": {
      "type": "local",
      "command": ["npx", "-y", "@modelcontextprotocol/server-github"],
      "environment": { "GITHUB_TOKEN": "{env:GITHUB_TOKEN}" }
    }
  },

  "command": {
    "review": { "description": "Review the current diff", "template": "Review this diff:\n\n!`git diff`" }
  }
}
```
