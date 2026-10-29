# ref

ref 是一个用 Rust 编写的终端编码 agent。它由两个程序组成：

- `refd`：后端守护进程，负责会话、模型调用、工具执行、MCP 和持久化；
- `ref`：TUI 前端，通过 WebSocket 连接 `refd`。

配置格式兼容 opencode（`opencode.json[c]`、markdown agent、skills、`AGENTS.md`、`auth.json`、models.dev 缓存），并在此基础上增加了少量 ref 专用的配置键。

## 简介

主要特点：

- **前后端分离**：会话运行在 `refd` 中。关闭 `ref` 后会话继续在后台运行，之后可以重新 attach；多个 `ref` 可以同时连接同一个 `refd`。
- **hashline 编辑格式**：`read`/`grep` 输出带有 `[path#TAG]` 快照标签和行号，`edit` 用 `PUT`/`CUT`/`REM` 按原始行号打补丁。如果文件在打标签之后被改过，`edit` 会尝试把旧行号映射到当前文件中未改动的行；映射不了就报错，并返回当前内容。
- **跨会话协作**：模型可以列出其他会话、互发消息、读取对方的记录，也可以创建和关闭后台子会话（`create_session` / `close_session`），并可要求子会话完成后汇报（`must_report`）。
- **watch 模式**：在 TUI 中以分屏方式同时查看和操作所有后台会话。
- **三种 API 格式**：Anthropic Messages、OpenAI Chat Completions、OpenAI Responses。
- **每次工具调用都带 `purpose`**：refd 给所有工具 schema 注入必填参数 `purpose`，模型用它简要说明调用原因，TUI 会显示出来。
- **权限与延迟**：opencode 风格的 `allow`/`ask`/`deny` 通配规则，另外支持按工具配置执行前延迟（`delay`）。

## 构建与安装

```sh
cargo build --release
# 产物：target/release/ref 和 target/release/refd
```

`ref` 启动时会先在自身可执行文件所在目录查找 `refd`，找不到再从 `PATH` 中查找。所以安装时两个文件要放在同一目录，或都放进 `PATH`：

```sh
install -m755 target/release/ref target/release/refd ~/.local/bin/
```

## 快速开始

1. 准备至少一个可用的 provider：已有的 opencode 配置会被直接读取；也可以只设置 API key 环境变量（例如 `ANTHROPIC_API_KEY`），前提是 models.dev 缓存 `~/.cache/opencode/models.json` 存在（opencode 会生成它）。也可以按[配置](#配置)一节手写 provider。
2. 在项目目录中运行：

```sh
ref                  # 在当前目录新建会话
ref ~/src/proj       # 在指定目录新建会话
ref -c               # 继续该目录最近更新的会话
ref --model anthropic/claude-sonnet-4-5 --agent plan
```

如果没有正在运行的 `refd`，`ref` 会自动以 `--idle-timeout 0` 启动一个（日志写入 `<data_dir>/refd.log`），最后一个前端断开后它就会退出。需要常驻后端时可以手动启动 `refd`（默认 `--idle-timeout -1`，永不退出）。

## 命令行

### `ref`

| 参数 | 说明 |
|---|---|
| `[PATH]` | 新会话的工作目录（默认当前目录） |
| `-c, --continue` | attach 该目录最近更新的会话 |
| `-s, --session <ID>` | 按 id attach 会话 |
| `-w, --watch` | 直接进入 watch 模式（分屏显示所有打开的后台会话，不新建会话） |
| `--model <provider/model>` | 第一个会话使用的模型 |
| `--agent <NAME>` | 新会话使用的 agent |
| `--url <ws://host:port>` | 连接远程 refd（不再自动发现或启动本地实例）；`/ws` 路径可省略，`http(s)://` 会自动转换 |
| `--token <TOKEN>` | 远程 refd 的 token |

连接断开后 `ref` 会自动重连。多个前端同时发现“没有后端”时，会通过 `<data_dir>/spawn.lock` 加锁，确保只启动一个 `refd`。

### `refd`

| 参数 | 说明 |
|---|---|
| `--port <N>` | 监听端口；`0`（默认）表示在 10000..60000 中随机选一个空闲端口 |
| `--bind <ADDR>` | 绑定地址；默认 `127.0.0.1`，如果 `--allow-host` 里有非 loopback 地址则默认 `0.0.0.0` |
| `--allow-host <HOST>` | 允许连接的远程主机（IP、主机名，`*` 表示任意），可重复。loopback 始终允许，token 始终必需 |
| `--idle-timeout <SECS>` | 没有前端连接多久后退出，见下文 |
| `--status [--json]` | 列出正在运行的实例并退出；有存活实例时退出码为 0，否则为 1 |
| `--stop [PID]` | 向指定实例（不带 PID 时为全部实例）发送 SIGTERM，最多等待 10 秒 |

`--idle-timeout` 的语义：

- `-1`（默认）：永不因空闲退出。
- `0`：最后一个前端断开后立即退出。如果启动后 30 秒内一直没有前端连接，也会退出，避免前端启动失败时留下孤儿进程。`ref` 自动启动 refd 时用的就是这个值。
- `N > 0`：最后一个前端断开 N 秒后退出。计时从第一个前端连接之后才开始。

空闲判断只看前端连接数，打开的会话不会让 refd 保持运行（会话已持久化，之后可以恢复）。

**token 与远程访问**：每次启动 refd 都会生成随机 token，并与端口、pid 一起写入 `<data_dir>/instances/<pid>.json`（权限 0600）。本机的 `ref` 通过 `refd --status --json` 读取这些信息。远程使用的步骤：

```sh
# 服务器
refd --port 7777 --allow-host 192.168.1.20
refd --status            # 查看 url 和 token
# 客户端
ref --url ws://server:7777 --token <TOKEN>
```

HTTP 端点只有两个：`/ws`（WebSocket）和 `/status`，都需要 `?token=`。refd 收到 SIGINT、SIGTERM 或 SIGHUP 时会优雅退出：中止并保存会话，停止 MCP 服务器，删除注册文件。

## 配置

### 读取的文件与合并顺序

以下来源从低优先级到高优先级依次 deep merge，后者覆盖前者：

1. 全局 opencode 配置：`~/.config/opencode/opencode.json[c]`。目录可用 `$OPENCODE_CONFIG_DIR` 或 `$XDG_CONFIG_HOME/opencode` 改变。
2. `$OPENCODE_CONFIG` 指定的文件。
3. 项目配置：从 cwd 向上直到 git 根目录的每一级，读取 `opencode.json[c]`，然后读取 `.opencode/opencode.json[c]`，越远的目录优先级越低。设置 `$OPENCODE_DISABLE_PROJECT_CONFIG` 可跳过这一层。
4. `$OPENCODE_CONFIG_CONTENT`（内联 JSON）。
5. ref 专用配置：`~/.config/ref/ref.json[c]`（目录可用 `$REF_CONFIG_DIR` 改变），然后是项目各级的 `.ref/ref.json[c]`。schema 与 opencode 相同，另外支持下文的扩展键。

所有文件都支持 JSONC（注释、尾逗号）以及 `{env:VAR}`、`{file:path}` 替换。每个配置目录（全局 opencode 目录、`.opencode/`、ref 全局目录、`.ref/`）下的 `agent[s]/*.md`、`mode[s]/*.md`、`command[s]/*.md` 也会被加载。

其他来源：API key 依次取 `provider.<id>.options.apiKey`、`~/.local/share/opencode/auth.json`、provider 声明的环境变量。模型元数据（上下文长度、输出上限、价格、模态）取自 models.dev 缓存 `~/.cache/opencode/models.json`（可用 `$OPENCODE_MODELS_PATH` 改变），ref 只读取这个缓存，不会联网下载。在 TUI 中执行 `/reload` 可重新加载配置。

### Provider、模型与 variant

```jsonc
{
  "model": "anthropic/claude-sonnet-4-5",      // 默认模型；可写成 provider/model#variant
  "small_model": "zai/glm-4.5-air",            // 用于生成会话标题
  "provider": {
    "anthropic": { "options": { "apiKey": "{env:ANTHROPIC_API_KEY}" } },
    "mygw": {
      "npm": "@ai-sdk/openai-compatible",       // 决定默认 API 格式
      "options": {
        "baseURL": "https://gw.example.com/v1",
        "apiKey": "{env:GW_KEY}",
        "headers": { "X-Team": "dev" },
        "timeout": 600000,                      // ms；false 表示不限
        "chunkTimeout": 60000
      },
      "api_format": "chat",                     // ref 扩展：anthropic | chat | responses
      "models": {
        "glm-5.1": {
          "limit": { "context": 200000, "output": 32000, "compact": 150000 },
          "variants": {
            "fast": { "thinking": { "type": "disabled" } },
            "deep": { "thinking": { "type": "enabled" } }
          }
        },
        "claude-x": { "api_format": "anthropic" }
      },
      "whitelist": ["glm-5.1", "claude-x"]
    }
  },
  "disabled_providers": ["openrouter"]
}
```

- API 格式：模型的 `api_format` 优先，其次是 provider 的 `api_format`，再按 `npm` 推断：`@ai-sdk/anthropic` 对应 Anthropic，`@ai-sdk/openai` 对应 Responses，其他一律为 Chat Completions。
- 已配置的 provider 排在前面。models.dev 中有凭据（auth.json 或环境变量）的 provider 也会自动列出。`enabled_providers` 和 `disabled_providers` 用于筛选 provider，`whitelist` 和 `blacklist` 用于筛选模型。
- variant：配置了 `variants` 就使用配置的（`"disabled": true` 可隐藏某个 variant）。没有配置时，推理模型会得到默认的 effort 档位：Anthropic 为 `low/medium/high/max`，Responses 为 `low/medium/high/xhigh`，Chat 为 `low/medium/high`；models.dev 列出 effort 取值时以它为准。
- agent 的 `model` 优先于全局 `model`；两者都没有时，使用列表中的第一个模型。由 `create_session` 创建的子会话在 agent 未指定模型时继承父会话的模型。

### Agent

内置两个 agent：`build`（默认，拥有全部工具）和 `plan`（只读分析；`edit`、`write`、`delete`、`bash` 需要确认）。`default_agent` 可以改变默认 agent。

```jsonc
{
  "agent": {
    "review": {
      "description": "Code review, no edits",
      "model": "anthropic/claude-sonnet-4-5#high",
      "prompt": "{file:./prompts/review.md}",
      "temperature": 0.2,
      "permission": { "edit": "deny", "bash": { "*": "ask", "git *": "allow" } },
      "delay": { "bash": 3 },     // ref 扩展
      "spawnable": true,          // ref 扩展
      "can_spawn": false          // ref 扩展
    },
    "plan": { "disable": true }
  }
}
```

markdown agent 的写法：`.opencode/agent/review.md` 或 `.ref/agents/review.md`。frontmatter 写配置，正文作为 prompt；文件名（相对路径去掉 `.md`）就是 agent 名。放在 `mode/` 目录下的 agent 视为 `mode: primary`。

```markdown
---
description: Code review, no edits
permission:
  edit: deny
---
You review diffs and point out bugs.
```

agent 中未识别的键会作为模型选项传给 provider，与 opencode 行为相同。`hidden: true` 的 agent 不会出现在 `create_session` 的可选列表中。

### 权限与延迟

```jsonc
{
  "permission": {
    "*": "allow",
    "bash": { "*": "ask", "git status*": "allow", "rm *": "deny" },
    "edit": { "*": "allow", "~/.ssh/*": "deny" },
    "webfetch": "deny"
  },
  "delay": { "bash": 2, "*": 0 }
}
```

- 取值为 `allow`、`ask` 或 `deny`，默认全部 `allow`。规则按顺序求值，**最后一条匹配的规则生效**；agent 的规则排在全局规则之后。
- 键可以是工具名、通配符或别名：`edit` 同时覆盖 `write`/`delete`，`glob`/`list` 对应 `find`，`shell` 对应 `bash`。MCP 工具按暴露后的名字 `server_tool` 匹配。旧式写法 `tools: { name: false }` 等价于 `deny`。
- 模式匹配的对象：`bash` 匹配命令文本，`read`/`write`/`delete`/`find`/`grep` 匹配 `path`，`edit` 匹配补丁中第一个 `[path#TAG]` 的路径。`*` 匹配任意字符，`?` 匹配单个字符；以 ` *` 结尾的模式也匹配不带参数的命令本身（`git *` 能匹配 `git`）。路径模式中的 `~/` 和 `$HOME/` 会被展开。
- 遇到 `ask` 时，TUI 弹出确认面板，可以选择允许一次、本会话内始终允许该工具，或拒绝。
- `delay`（ref 扩展）：工具执行前等待的秒数，期间状态显示为 delayed，按 `esc esc` 可以取消。值可以是 map，也可以是一个数字（等价于 `"*"`）。agent 的 `delay` 优先于全局 `delay`。

### MCP

```jsonc
{
  "mcp": {
    "fs":   { "type": "local",  "command": ["npx", "-y", "@modelcontextprotocol/server-filesystem", "."],
              "environment": { "DEBUG": "0" }, "timeout": 30000 },
    "docs": { "type": "remote", "url": "https://mcp.example.com/mcp", "headers": { "Authorization": "Bearer {env:TOKEN}" } },
    "old":  { "enabled": false }
  }
}
```

MCP 工具对外暴露的名字是 `server_tool`，非 `[a-zA-Z0-9_-]` 的字符会被替换，长度不超过 64。每个服务器的连接状态显示在侧边栏中。

### Skills、AGENTS.md 与自定义命令

- **Skills**：扫描包含 `name`/`description` frontmatter 的 `SKILL.md`。扫描位置包括 `~/.config/opencode/skill[s]`、`~/.claude/skills`、`~/.agents/skills`、`~/.config/ref/skills`，项目各级的 `.claude/skills`、`.agents/skills`、`.opencode/skill[s]`、`.ref/skills`，以及配置项 `skills` 列出的路径。同名 skill 由后扫描到的覆盖。系统提示中会列出 skill 清单，模型通过 `skill` 工具加载全文。
- **指令文件**：读取全局 `~/.config/opencode/AGENTS.md`（不存在时读取 `~/.claude/CLAUDE.md`），以及从 git 根目录到 cwd 各级的 `AGENTS.md`（任何一级都没有 `AGENTS.md` 时才读取 `CLAUDE.md`），最后是 `instructions` 配置中的路径或 glob（URL 会被忽略）。超过 200KB 的文件会被跳过。
- **自定义命令**：在 `command` 中配置，或放在 `command[s]/*.md`。模板支持 `$ARGUMENTS` 和 `$1`..`$9`，可以指定 `agent` 和 `model`。在 TUI 中用 `/名字 参数` 调用。

### ref 扩展键

| 键 | 位置 | 默认值 | 说明 |
|---|---|---|---|
| `spawnable` | agent | `true`（`mode: primary` 时为 `false`） | 能否被 `create_session` 用来创建子会话 |
| `can_spawn` | agent | `true` | 能否使用 `create_session`/`close_session` |
| `delay` | 顶层 / agent | 无 | 工具执行前延迟秒数，例如 `{ "bash": 3 }` |
| `retry_max_delay` | 顶层 | `600` | 请求失败重试时退避的最大秒数 |
| `tool_output_limit` | 顶层 | `50000` | 返回给模型的工具输出最大字符数 |
| `compaction.threshold` | 顶层 | `0.8` | 建议压缩点：不超过 1 时表示占上下文窗口的比例，大于 1 时表示绝对 token 数 |
| `compaction.auto` | 顶层 | `true` | 上下文将要溢出时自动压缩（`OPENCODE_DISABLE_AUTOCOMPACT=1` 可关闭） |
| `compaction.reserved` | 顶层 | `min(max_output, 32000)` | 判断是否自动压缩时为输出预留的 token 数 |
| `limit.compact` | 模型 | 无 | 该模型的建议压缩点（token 数），优先于 `compaction.threshold` |
| `api_format` | provider / 模型 | 按 `npm` 推断 | `anthropic`/`messages`、`chat`/`openai-compatible`、`responses`/`openai` |

建议压缩点会显示在上下文进度条上，模型也可以通过 `context_info` 查询。是否触发自动压缩由“已用 token + reserved ≥ 上下文窗口”决定。

## 内置工具

| 工具 | 说明 |
|---|---|
| `read` | 读取文件或列出目录。文本以 hashline 格式输出：先是 `[path#TAG]` 头（TAG 为整个文件的 4 位十六进制快照标签），然后是 `N:text` 行。默认 2000 行或 50KB，可用 `offset`/`limit` 分段读取。模型支持图片时，png/jpg/gif/webp 会以图片形式返回 |
| `edit` | 用 hashline 补丁修改已有文件，一次调用可以改多个文件。也支持 `{path, old_string, new_string, replace_all?}` 精确替换 |
| `write` | 创建或覆盖文件（自动创建父目录），返回新的 `[path#TAG]` |
| `delete` | 删除文件；删除目录需要 `recursive: true` |
| `grep` | 正则搜索（Rust 语法，`literal` 表示纯文本），支持 `glob`、`context`、`ignore_case`。遵守 .gitignore，输出的 `[path#TAG]` 和行号可以直接用于 `edit`。默认最多 100 条匹配 |
| `find` | 按 glob 查找文件，结果按修改时间从新到旧排列，遵守 .gitignore，默认最多 200 条 |
| `bash` | 用 `bash -c` 执行命令（无 stdin）。`timeout` 默认 300 秒，最大 7200，0 表示不限；`cwd` 可选。输出过长时保留头尾，完整输出写入 `$TMPDIR/ref-tool-output/` |

会话级工具由 refd 实现：`context_info`、`compact`、`skill`（存在 skill 时才提供），以及下一节的协作工具。同一批工具调用中，相邻的只读工具（read/grep/find 等）并行执行，其余工具按顺序执行。

hashline `edit` 示例（行号始终是该 TAG 快照中的**原始**行号，不会因为同一次调用中前面的 hunk 而移动）：

```text
[src/app.py#1A2B]
PUT 3:
+    return total
PUT >10:
+
+def helper():
+    pass
CUT 14.=15
```

- `PUT N:` 替换第 N 行；`PUT N.=M:` 替换第 N 到第 M 行（含两端）；`PUT <N:` / `PUT >N:` 在第 N 行之前 / 之后插入；`PUT >$:` 追加到文件末尾。
- `CUT N` / `CUT N.=M` 删除行；`REM` 删除整个文件。
- 正文行以 `+` 开头，内容原样写入；单独一个 `+` 表示空行。保留原文件的 CRLF 和 BOM。
- 只能修改 `read`/`grep`/`edit` 在该 TAG 下显示过的行。编辑之后 TAG 和行号都会变，要使用响应中新的 `[path#TAG]`。

## 跨会话协作

同一个 refd 中所有打开的会话都可以互相通信：

| 工具 | 说明 |
|---|---|
| `list_sessions` | 列出其他打开的会话：名字、id、agent、状态（idle/busy/retrying/waiting-permission）、前台/后台、cwd、父会话 |
| `send_message` | 按名字或 id 给打开的会话发送纯文本消息（最大 64KB）。对方空闲时立即开始处理，忙碌时排队到当前步骤结束后 |
| `read_messages` | 读取另一个会话的记录（包括已关闭的会话），只含文本和工具调用，不含推理过程。默认最近 20 条，`offset`/`limit` 用于翻页 |
| `session_context` | 查看另一个会话的上下文用量 |
| `create_session` | 创建后台子会话并立即开始执行 `prompt`。参数：`name`、`agent`（必须是 `spawnable` 且非 hidden 的 agent）、`cwd`（默认与父会话相同）、`must_report` |
| `close_session` | 中止并卸载一个后台会话，会话仍保留在历史中。不能关闭自己，也不能关闭前台会话 |

`create_session` 和 `close_session` 只提供给 `can_spawn: true` 的 agent。

- **前台会话**：至少有一个前端 attach 的会话。**后台会话**：打开着（已加载到 refd）但没有任何前端 attach 的会话，包括由 `create_session` 创建的会话，以及前端离开后继续运行的会话。watch 模式只是观察会话，不算 attach。
- **`must_report: true`**：子会话的任务末尾会附带“完成后用 send_message 向父会话汇报”的要求。如果子会话一轮结束时还没有向父会话发过消息，refd 会注入提醒，最多 3 次。
- 名字解析顺序：精确 id，然后精确名字，然后不区分大小写的名字。有歧义时报错并列出候选。重名的会话会自动改名为 `name (2)` 这样的形式。

## TUI 使用

**布局**：顶部是标题栏，中间是对话记录，下方是输入框（随内容增高，最多 10 行）和提示栏。终端宽度不小于 140 列时，右侧默认显示侧边栏，内容包括会话名、上下文进度条（带建议压缩点标记）、花费、模型/variant、MCP 状态和其他会话。权限确认面板和重试状态显示在输入框上方。粘贴超过 5 行或 1000 字符的文本时，会折叠成一个整体块；`ctrl+v` 粘贴剪贴板图片。

### 快捷键

| 按键 | 作用 |
|---|---|
| `enter` | 发送（会话忙时排队） |
| `shift+enter` / `ctrl+j` / `alt+enter` | 换行 |
| `esc esc` | 中断正在运行的会话（1 秒内按两次） |
| `ctrl+p` | 命令面板 |
| `/` | 斜杠命令补全 |
| `tab` / `shift+tab` | 切换 agent |
| `ctrl+t` | 切换模型 variant |
| `ctrl+c` | 有选区时复制，输入框非空时清空，否则退出 |
| `ctrl+d` | 输入框为空时退出 |
| `ctrl+v` | 从剪贴板粘贴图片或文本 |
| `↑` / `↓` | 光标在首行或末行时浏览提示历史 |
| `pageup` / `pagedown` / 滚轮 | 滚动记录 |
| `ctrl+g` / `ctrl+alt+g` | 跳到记录开头 / 末尾 |
| `alt+↑` 或 `ctrl+x ↑` | 消息选择模式：`↑↓`/`jk` 移动，`y` 复制原文，`enter` 展开或折叠 |
| 权限面板 `a`/`y`、`A`、`d`/`n` | 允许一次、本会话始终允许、拒绝；`←→` 加 `enter` 选择按钮 |

leader 键为 `ctrl+x`，之后再按：`n` 新会话，`l` 会话列表，`m` 模型，`a` agent，`c` 压缩，`u` 撤销，`e` 用 `$EDITOR` 编辑输入，`b` 侧边栏，`y` 复制最后一条助手消息，`d` 展开或折叠全部工具详情，`w` watch 模式，`s` 鼠标捕获开关，`h` 帮助，`q` 退出。

会话列表中：`ctrl+x` 关闭选中的会话，`ctrl+d` 删除已关闭的会话。

### 斜杠命令

| 命令 | 说明 |
|---|---|
| `/new`（`/clear`） | 新会话 |
| `/sessions`（`/resume`、`/continue`） | 切换会话 |
| `/models`、`/variant`、`/agents` | 切换模型、variant、agent（只对当前会话生效） |
| `/compact`（`/summarize`） | 压缩上下文 |
| `/undo` | 撤销上一条提示，并把它放回输入框 |
| `/rename <name>` | 重命名会话 |
| `/close` | 关闭（卸载）当前会话 |
| `/export` | 把记录导出为 cwd 下的 `ref-<name>-<时间>.md` |
| `/copy` | 以 markdown 复制整个记录 |
| `/details`、`/thinking` | 切换工具详情、推理过程的默认展开状态 |
| `/watch` | watch 模式 |
| `/peers` | 打开的会话列表 |
| `/editor`、`/sidebar`、`/mouse` | `$EDITOR` 编辑、侧边栏开关、鼠标捕获开关 |
| `/reload` | 重新加载配置 |
| `/help`、`/exit`（`/quit`、`/q`） | 帮助、退出 |

配置中的自定义命令也会出现在补全和命令面板中。

### 复制与选择

- 在记录区域拖动鼠标可以选择文本，然后按 `ctrl+c`、`ctrl+shift+c` 或 `y`（输入框为空时）复制，`esc` 取消选择。拖动超过边缘时会自动滚动。
- 点击消息上的 `⧉ copy` 复制该消息原文；点击工具行或推理行可以展开或折叠。
- 复制时同时写 OSC 52（在 tmux 中也有效），并在后台调用 `wl-copy`、`xclip` 或 `xsel`。
- 需要终端原生选择时，按住 `shift` 拖动，或用 `ctrl+x s` 关闭鼠标捕获。

### Watch 模式

用 `ref -w`、`/watch` 或 `ctrl+x w` 进入。界面以网格分屏显示所有打开的**后台**会话（当前会话除外），每个面板最小 40×8。放不下时分页显示。

| 按键 | 作用 |
|---|---|
| `ctrl`/`alt` + 方向键、`F6`、`ctrl+x o` | 在面板之间移动焦点（到边缘时翻页） |
| `enter` | 把面板输入框中的提示发送给聚焦的会话 |
| `↑` / `↓` | 滚动聚焦的面板 |
| `esc esc` | 中断聚焦的会话 |
| `ctrl+x enter` | attach 聚焦的会话（使其成为前台会话） |
| `ctrl+x z` | 放大或还原面板 |
| `ctrl+x [` / `]` | 上一页 / 下一页 |
| `ctrl+x w` | 退出 watch 模式 |

后台会话请求权限时，对应面板底部会显示提示；把焦点移到该面板后，确认面板才会出现。

## 数据目录

`$REF_DATA_DIR`，默认 `~/.local/share/ref`：

```text
instances/<pid>.json        运行中的 refd：端口、bind、token（0600）
sessions/<id>.meta.json     会话元数据（名字、cwd、agent、模型、父会话、must_report…）
sessions/<id>.jsonl         消息记录（追加写，关闭时压缩）
refd.log                    ref 自动启动的 refd 的日志
spawn.lock                  防止重复启动 refd 的锁
prompt-history.jsonl        TUI 提示历史
```

## 架构

```text
┌──────────┐  WebSocket /ws?token=  ┌───────────────────────────────────────┐
│ ref (TUI)│ ─────── JSON ───────── │ refd                                  │
└──────────┘   ClientMsg/ServerMsg  │  server (axum) → hub → session engine │
┌──────────┐                        │    ├ ref-llm   (Anthropic/Chat/Resp.) │
│ ref ...  │ ───────────────────────│    ├ ref-tools (内置工具)              │
└──────────┘                        │    ├ ref-mcp   (stdio / HTTP MCP)     │
                                    │    └ ref-config(opencode 兼容配置)     │
                                    └───────────────────────────────────────┘
```

| crate | 职责 |
|---|---|
| `ref-proto` | 共享数据模型与协议：`Message`/`Part`/`SessionInfo`，`ClientMsg`（Hello、CreateSession、Attach/Detach、Watch/Unwatch、Prompt、Abort、Permission、SetModel…）和 `ServerMsg`（Welcome、SessionSnapshot、MessageUpsert、PartDelta、PermissionAsk…）。所有消息都是带 `"type"` 标签的 JSON，`PROTOCOL_VERSION = 1` |
| `ref-llm` | 三种 API 的流式客户端、SSE 解析、错误分类与重试信息 |
| `ref-tools` | 内置工具与 hashline 快照存储 |
| `ref-mcp` | MCP 客户端（local 子进程 / remote HTTP+SSE） |
| `ref-config` | 配置加载、合并、provider/模型解析、权限、skills、指令文件 |
| `refd` | 守护进程：注册表、hub（客户端与会话管理）、会话引擎（步骤循环、工具执行、压缩、标题生成、持久化）、跨会话通信 |
| `ref` | ratatui TUI：连接与自动重连、后端发现与自动启动、渲染、输入编辑器、剪贴板 |

系统提示由以下部分按顺序拼接：基础提示加环境信息（cwd、平台、日期、是否 git 仓库）、agent prompt、指令文件、skills 列表、会话身份与可创建的 agent 列表。

## 开发

```sh
cargo test                 # 全部单元测试与集成测试（不需要网络）
cargo clippy --all-targets
```

`examples/` 中的程序需要真实模型和用户自己的配置，不在 `cargo test` 中运行，也不会打印 API key：

```sh
# 直接调用某个模型，测试流式输出、工具往返、图片、缓存等
cargo run -p ref-llm --example probe -- anthropic/claude-sonnet-4-5 [--variant high] [--api chat] [--image a.png]

# 让真实模型用内置工具完成一个多文件编辑任务，统计 edit 失败率
cargo run -p ref-tools --example model_trial -- --model zai/glm-5.3 --runs 3 [--steps 25] [--log DIR] [--keep]

# 对运行中的 refd 做端到端冒烟测试（多步工具调用 + must_report 子会话）
REF_DATA_DIR=/tmp/x target/debug/refd --port 0 --idle-timeout -1 &
REF_DATA_DIR=/tmp/x cargo run -p refd --example smoke -- [provider/model] [cwd]
```

## 许可证

GPL-3.0
