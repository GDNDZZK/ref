**English** · [简体中文](configuration.zh-CN.md)

# Configuration

ref reads [opencode](https://opencode.ai)'s configuration format, so an existing `opencode.json` works unchanged. ref adds a few keys of its own (marked **ref** below).

- [Quick start](#quick-start)
- [Where configuration is read from](#where-configuration-is-read-from)
- [Syntax](#syntax)
- [Top-level keys](#top-level-keys)
- [Providers and models](#providers-and-models)
- [Agents](#agents)
- [Permissions](#permissions)
- [Tool delays](#tool-delays)
- [Context compaction](#context-compaction)
- [Retries, images and TUI defaults](#retries-images-and-tui-defaults)
- [MCP servers](#mcp-servers)
- [Custom commands](#custom-commands)
- [Instructions, context files and skills](#instructions-context-files-and-skills)
- [Full example](#full-example)

## Quick start

Setting an API key is enough to get started. When one of these environment variables is set and you have not configured the provider yourself, ref adds the provider with a small built-in model list:

| Variable | Provider id | Built-in models |
|---|---|---|
| `ANTHROPIC_API_KEY` | `anthropic` | `claude-sonnet-4-5`, `claude-opus-4-1`, `claude-haiku-4-5` |
| `OPENAI_API_KEY` | `openai` | `gpt-5`, `gpt-5-mini` |
| `DEEPSEEK_API_KEY` | `deepseek` | `deepseek-chat`, `deepseek-reasoner` |
| `OPENROUTER_API_KEY` | `openrouter` | `anthropic/claude-sonnet-4.5`, `openai/gpt-5` |

For any other OpenAI-compatible endpoint, create `~/.config/ref/ref.json`:

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

## Where configuration is read from

All files that exist are loaded and merged. Later files win.

1. **Global directories**, in this order:
   - `~/.config/opencode/` (or `$OPENCODE_CONFIG_DIR` when set)
   - `~/.config/ref/`

   In each one: `opencode.json`, `opencode.jsonc`, `config.json`, `ref.json`, `ref.jsonc`. `$XDG_CONFIG_HOME` replaces `~/.config` when set.
2. **Explicit files**: `$OPENCODE_CONFIG`, then `$REF_CONFIG`. A path that does not exist produces a warning.
3. **Project directories**, from the git root (the nearest ancestor containing `.git`) down to the working directory. Outside a git repository, only the working directory is used. In each directory:
   `opencode.json`, `opencode.jsonc`, `.opencode/opencode.json`, `.opencode/opencode.jsonc`, `ref.json`, `ref.jsonc`, `.ref/ref.json`, `.ref/ref.jsonc`.

Markdown agents and commands are loaded from the same global directories and from every `.opencode/` and `.ref/` directory (see [Agents](#markdown-agents) and [Custom commands](#markdown-commands)).

**How files are merged:** objects are merged key by key, recursively. Any other value (string, number, array) replaces the earlier one. Three things are collected from every file instead of replaced: permission rules (appended in order), `instructions` and skill paths.

Configuration is read again for every new session and on `/cd`, so edits take effect without restarting the backend.

Problems such as invalid JSON, unknown permission actions or a missing `template` do not stop ref. The file or entry is skipped and a warning is written to the backend log (`~/.local/state/ref/logs/`, see [CLI](cli.md#files-and-directories)).

## Syntax

- **JSONC**: `//` and `/* */` comments and trailing commas are allowed in every file.
- **`{env:VAR}`** in any string is replaced with the variable's value, or with an empty string when it is unset.
- **`{file:path}`** is replaced with the file's contents, trimmed. `~` and relative paths are resolved against the directory of the config file that contains it. A file that cannot be read produces a warning and an empty string.

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

## Top-level keys

| Key | Type | Default | Meaning |
|---|---|---|---|
| `model` | `"provider/model"` | first configured model | Model for agents that do not set one. |
| `small_model` | `"provider/model"` | session model | Model used to generate session titles. |
| `default_agent` | string | `"build"` | Agent for new sessions. |
| `provider` | object | built-ins from env vars | [Providers and models](#providers-and-models). |
| `enabled_providers` | string[] | all | Only these provider ids are loaded. |
| `disabled_providers` | string[] | none | These provider ids are skipped. |
| `agent` | object | built-in agents | [Agents](#agents). `mode` is accepted as an older name. |
| `permission` | string or object | everything allowed | [Permissions](#permissions). |
| `tools` | object | | Older opencode form: `{"bash": false}` denies a tool. |
| `delay` | object or number | no delay | **ref** [Tool delays](#tool-delays). |
| `compaction` | object | auto at 90% | **ref** [Context compaction](#context-compaction). |
| `retry` | object | `{"max_delay": 600}` | **ref** [Retries](#retries-images-and-tui-defaults). |
| `images` | object | `{"auto_resize": true}` | **ref** [Images](#retries-images-and-tui-defaults). |
| `tui` | object | | **ref** [TUI defaults](#retries-images-and-tui-defaults). |
| `mcp` | object | none | [MCP servers](#mcp-servers). |
| `command` | object | none | [Custom commands](#custom-commands). |
| `instructions` | string[] | none | Extra instruction files or globs. |
| `skills` | string[] or `{"paths": [...]}` | none | Extra skill directories. |

## Providers and models

```jsonc
"provider": {
  "<provider id>": {
    "name": "Display name",                  // default: the id
    "npm": "@ai-sdk/openai-compatible",      // selects the API, see below
    "api": "chat",                           // overrides npm: "chat" | "responses" | "anthropic"
    "env": ["MY_KEY", "MY_OTHER_KEY"],       // env vars to read the API key from
    "options": {
      "baseURL": "https://example.com/v1",
      "apiKey": "{env:MY_KEY}",
      "headers": { "X-Org": "acme" },
      "setCacheKey": true,
      "chunkTimeout": 300000
    },
    "whitelist": ["model-a"],                // only these models
    "blacklist": ["model-b"],                // never these models
    "models": { /* see Model fields below */ }
  }
}
```

### API selection

ref speaks three APIs. It picks one from `npm`, unless `api` is set:

| `npm` | API |
|---|---|
| `@ai-sdk/anthropic`, `@ai-sdk/google-vertex/anthropic` | Anthropic Messages |
| `@ai-sdk/openai`, `@ai-sdk/azure` | OpenAI Responses |
| anything else, or missing | OpenAI Chat Completions |

`api` accepts `chat` (also `openai-chat`, `openai-compatible`, `completions`, `chat-completions`), `responses` (also `openai-responses`, `openai`) and `anthropic` (also `messages`, `anthropic-messages`). If `api` holds an `http(s)://` URL, as models.dev files do, it is used as the base URL instead.

### Provider fields

| Field | Meaning |
|---|---|
| `options.baseURL` | API base URL (`baseUrl` and `base_url` also work). The ids `anthropic`, `openai`, `deepseek` and `openrouter` have a default. |
| `options.apiKey` | API key (`api_key` also works). |
| `env` | When `apiKey` is empty, the first non-empty variable in this list is used. For the four built-in ids the conventional variable is the default, and `@ai-sdk/anthropic` / `@ai-sdk/openai` also fall back to `ANTHROPIC_API_KEY` / `OPENAI_API_KEY`. |
| `options.headers` | Extra HTTP headers for every request. |
| `options.setCacheKey` | Send a per-session prompt cache key. Default: on for the Responses API and for `api.openai.com`. |
| `options.chunkTimeout` | Milliseconds to wait for the next streamed chunk before the request is retried. Default `300000`. |
| `whitelist` / `blacklist` | Filter the `models` map by key. |

### Model fields

```jsonc
"models": {
  "some-model": {
    "id": "vendor/some-model-2026",     // id sent to the API (default: the key)
    "name": "Some Model",               // display name (default: the key)
    "limit": {
      "context": 200000,                // context window in tokens (default 128000)
      "output": 32000,                  // max output tokens
      "input": 180000,                  // accepted for opencode compatibility, not used
      "compact": 120000,                // ref: recommended compaction point in tokens
      "threshold": 0.8                  // ref: automatic compaction point for this model (fraction, or tokens if above 1)
    },
    "attachment": true,                 // accepts images (or "modalities": {"input": ["text", "image"]})
    "reasoning": true,                  // reasoning model ("thinking" also works)
    "temperature": 0.7,                 // only numbers are sent; opencode's `true` is ignored
    "top_p": 0.9,
    "headers": { "X-Model": "1" },
    "options": { "reasoningEffort": "medium" },
    "variants": {
      "high": { "reasoningEffort": "high" },
      "off":  { "disabled": true }      // removed
    },
    "disabled": false                   // true removes the model
  }
}
```

A model is referred to as `provider/model`, and with a variant as `provider/model#variant` (for example `--model openai/gpt-5#high`). `ctrl+t` in the TUI cycles through a model's variants.

### Request options

`options` are added to the request body. Keys are written in camelCase as in opencode and converted to snake_case for the API, so `reasoningEffort` becomes `reasoning_effort`. Objects are merged in this order, later winning:

1. the model's `options`
2. the agent's `options`, plus any unrecognized keys on the agent itself
3. the selected variant

For example, `{"thinking": {"type": "enabled", "budgetTokens": 16000}}` enables extended thinking on the Anthropic API. On the Responses API, `{"reasoningEffort": "high", "reasoningSummary": "auto"}` sets `reasoning.effort` and `reasoning.summary`.

`extraBody` (or `body`) is merged into the request body as it is, without key conversion. Use it for fields that must keep their exact spelling.

### Which model a session uses

A new session uses its agent's `model` (with the agent's `variant`). Without one it uses the top-level `model`, and without that the first model of the first provider. `--model` on the command line and the model picker (`ctrl+x m`) change it for one session only.

## Agents

An agent is a system prompt plus settings: a default model, permissions and delays. Every session runs one agent. `tab` in the TUI cycles agents, and `ctrl+x a` opens the agent list.

### Built-in agents

| Agent | Purpose | Restrictions |
|---|---|---|
| `build` | Default agent with full tool access. | Can create and close other sessions (`can_spawn`). |
| `plan` | Read-only analysis and planning. | `edit`, `write`, `delete` denied; `bash` asks. |
| `general` | Multi-step tasks in a session of its own. | None. |
| `explore` | Fast, read-only codebase exploration. | `edit`, `write`, `delete` denied. |
| `compaction` | Hidden. Writes the summary when a session is compacted. | Cannot run a session. |

You can change any built-in agent by defining an agent with the same name; only the fields you set change. `"disable": true` removes an agent. Setting `model` on `compaction` makes summaries use that model instead of the session's model.

### Agent fields

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

| Field | Default | Meaning |
|---|---|---|
| `description` | `""` | Shown in the agent list and to agents that can create sessions, so they know when to use this one. |
| `prompt` | `""` | Agent prompt, placed at the start of the system prompt. `{file:...}` is the usual way to keep it in a separate file. |
| `model`, `variant` | | Default model for sessions with this agent. |
| `temperature`, `top_p` | | Sampling parameters. |
| `options` | | Request options merged over the model's options (see [Request options](#request-options)). Unrecognized keys on the agent are treated as options too. |
| `permission` | | Rules evaluated after the global rules, so they take priority. |
| `delay` | | Delays evaluated before the global ones. |
| `creatable` | `true` | **ref** Other sessions may create sessions with this agent. |
| `can_spawn` | `false` (`true` for `build`) | **ref** The agent gets the `create_session` and `close_session` tools. `spawn` also works. |
| `color` | | Color of the agent's name in the TUI. |
| `hidden` | `false` | Not listed in the TUI and not offered to other sessions. |
| `mode` | | opencode's `primary` / `subagent` / `all`. Accepted for compatibility; ref does not restrict agents by mode. |
| `disable` | `false` | Remove the agent (`disabled` also works). |

`steps` and `maxSteps` are accepted and ignored.

### Markdown agents

An agent can also be a markdown file. The file name without `.md` is the agent name, the body is the prompt, and the frontmatter holds the other fields:

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

ref looks for these files in `agent/`, `agents/`, `mode/` and `modes/` (including subdirectories) under `~/.config/opencode/`, `~/.config/ref/` and every project `.opencode/` and `.ref/` directory. Files under `mode/` or `modes/` become `primary` agents. The frontmatter supports `key: value` pairs, quoted strings, numbers, booleans, `[a, b]` and `- item` lists, `|` and `>` block text and nested maps by indentation.

## Permissions

Each tool call is checked against the permission rules before it runs. The result is one of:

- `allow`: run it.
- `ask`: the TUI asks. `a` allows this call, `s` allows every call of this tool for the rest of the session, `d` denies it.
- `deny`: refuse it. The model gets an error and continues.

`auto`, `yes` and `true` also mean allow; `confirm` and `prompt` mean ask; `reject`, `no` and `false` mean deny.

```jsonc
"permission": {
  "*": "allow",
  "bash": { "*": "ask", "git status*": "allow", "git diff*": "allow", "rm *": "deny" },
  "edit": { "*": "allow", "*.lock": "deny" },
  "webfetch": { "*": "ask", "https://docs.rs/*": "allow" },
  "mcp_github_*": "ask"
}
```

`"permission": "ask"` applies one action to every tool.

**What a pattern matches.** The key is a tool name and may be a glob (`*` and `?`), for example `mcp_*`. The pattern under a tool is matched against:

| Tool | Subject |
|---|---|
| `bash` | the command |
| `read`, `edit`, `write`, `delete`, `grep`, `find` | the `path` argument as the model wrote it (relative or absolute) |
| `webfetch` | the URL |
| other tools (MCP, cross-session) | nothing: only `"*"` applies |

A pattern ending in ` *` also matches the bare command: `"git *"` matches `git` as well as `git log`.

**Order.** Rules are checked in the order they are written, across all files, and the last matching rule wins. Agent rules are checked after global rules, so an agent's rule beats a global one. When no rule matches, the call is allowed.

**Tool aliases.** opencode's tool names are mapped to ref's: `edit` covers `edit`, `write` and `delete`; `write`, `patch`, `multiedit` and `apply_patch` cover `write` and `edit`; `glob`, `list` and `ls` mean `find`; `shell` means `bash`; `fetch` and `web_fetch` mean `webfetch`.

**Disabled tools.** When a tool's last `"*"` rule is `deny` and no pattern rule allows or asks for it, the tool is not offered to the model at all. The older form `"tools": {"bash": false}` does the same.

## Tool delays

**ref** A delay makes ref wait before running a tool call, so you can read it first. The TUI shows a countdown, and `esc` during the countdown cancels the call.

```jsonc
"delay": { "*": 0, "bash": 2, "delete": 5, "mcp_*": 1 }
```

Keys are tool-name globs and values are seconds. The last matching key wins. `"delay": 3` applies to every tool. An agent's `delay` is checked first: if any of its keys matches, the global setting is not used.

## Context compaction

**ref** When a conversation fills the model's context window, ref compacts it: the `compaction` agent writes a summary, and the next requests send the summary instead of the older messages. The full history stays in the session and in the TUI, with a divider where compaction happened.

```jsonc
"compaction": {
  "auto": true,        // compact automatically at `threshold` (default true)
  "recommend": 0.6,    // point the model is told to compact at (default 0.6)
  "threshold": 0.9     // automatic compaction point (default 0.9)
}
```

Values up to 1 are fractions of the context window. Larger values are token counts. A model's `limit.compact` overrides `recommend` for that model. A model's `limit.threshold` overrides `threshold` for that model, with the same meaning of the value.

The model sees both points through the `context_window` tool and can compact on its own with `compact_context`. You can compact any time with `ctrl+x c` or `/compact`.

## Retries, images and TUI defaults

```jsonc
"retry":  { "max_delay": 600 },     // seconds; cap for the retry backoff
"images": { "auto_resize": true },  // downscale images before the model sees them
"tui":    { "sidebar": true, "thinking": false, "details": false }
```

- **`retry.max_delay`** (**ref**, default `600`): failed LLM requests that can be retried are retried without limit: HTTP 408, 409, 425, 429 and 5xx, overload messages, network errors and timeouts. The wait starts at 2 s and doubles up to this cap; a `retry-after` header from the provider is respected. `esc esc` stops retrying. Other errors, such as an invalid API key, end the turn with the error shown.
- **`images.auto_resize`** (**ref**, default `true`): images from `read`, `webfetch` and pasted images are scaled and recompressed to sizes the models handle well. See [Tools](tools.md#images).
- **`tui`** (**ref**): initial state of three TUI toggles: `sidebar` (default shown), `thinking` (expand reasoning, `ctrl+x t`) and `details` (expand tool calls, `ctrl+x d`).

## MCP servers

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

| Field | Meaning |
|---|---|
| `type` | `local` (also `stdio`) starts a process and talks over stdin/stdout. `remote` (also `http`, `sse`, `streamable-http`) uses Streamable HTTP. When missing, `url` means remote and `command` means local. |
| `command` | Local: the program and its arguments as an array. A string is split at spaces, and `args` is appended to it. On Windows, `npx` also finds `npx.cmd`. |
| `environment` | Local: extra environment variables (`env` also works). |
| `cwd` | Local: working directory of the process. |
| `url`, `headers` | Remote: endpoint and HTTP headers. |
| `timeout` | Milliseconds allowed for starting the server and listing its tools (default `30000`). Tool calls have no time limit; `esc esc` interrupts them. |
| `enabled` | `false` keeps the server configured but does not start it. A file that only sets `enabled` toggles a server defined in another file. |

MCP tools are offered to the model as `<server>_<tool>`, with characters other than letters, digits, `_` and `-` replaced by `_`. Use that name in `permission` and `delay`, for example `"github_*": "ask"`. The sidebar and `/mcp` show each server's status.

## Custom commands

A command is a prompt template that you run as `/name` in the TUI.

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

The template is expanded and the result is sent as your prompt:

- `$ARGUMENTS` becomes everything typed after the command name.
- `$1` to `$9` become single words. The highest-numbered placeholder in the template takes all remaining words.
- `` !`cmd` `` is replaced with the command's output. It runs in bash in the session directory, with a 30-second limit.
- When the template has no placeholder, the arguments are appended after a blank line.

`agent` and `model` are accepted for opencode compatibility but do not change anything yet: the prompt goes to the current session.

### Markdown commands

Commands can be markdown files in `command/` or `commands/` under the same directories as [markdown agents](#markdown-agents). The file name is the command name, the body is the template and the frontmatter can hold `description`:

```markdown
---
description: Review the current diff
---
Review this diff and list problems:

!`git diff`
```

## Instructions, context files and skills

**Context files** are added to the system prompt of every session:

1. `~/.config/opencode/AGENTS.md` and `~/.config/ref/AGENTS.md`
2. In every directory from the git root down to the working directory: `AGENTS.md`, or `CLAUDE.md` when there is no `AGENTS.md`
3. The files listed in `instructions`. Entries can be paths or globs, absolute, starting with `~`, or relative to the session directory. URLs are skipped.

```jsonc
"instructions": ["docs/CONTRIBUTING.md", "~/notes/style-*.md"]
```

**Skills** are directories containing a `SKILL.md` file whose frontmatter has a `name` and a `description`. The system prompt lists each skill's name, description and path, and the model reads the `SKILL.md` with `read` when a task matches. ref looks for skills in:

- `skill/` and `skills/` in the global config directories and in every project `.opencode/` and `.ref/` directory
- `~/.claude/skills`, `~/.agents/skills`, and `.claude/skills` and `.agents/skills` in every project directory
- the directories in `skills` (`"skills": ["path"]` or `"skills": {"paths": ["path"]}`), relative to the config file that lists them

When two skills have the same name, the one found later in this list is used.

## Full example

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
