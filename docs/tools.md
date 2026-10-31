**English** · [简体中文](tools.zh-CN.md)

# Tools

These are the tools the model can call. Every tool, MCP tools included, takes a `purpose` argument: one short sentence that the TUI shows on the collapsed call, so you can follow what the model is doing without opening each call.

Tool errors are returned to the model as results. They never end the session's turn.

- [File tools](#file-tools)
- [bash](#bash)
- [webfetch](#webfetch)
- [Images](#images)
- [Context tools](#context-tools)
- [question](#question)
- [Cross-session tools](#cross-session-tools)
- [MCP tools](#mcp-tools)

## File tools

| Tool | What it does |
|---|---|
| `read` | Reads a text file, lists a directory or returns an image. Pages of 500 lines by default (`limit` up to 3000, about 60 KB). `offset` / `limit`, or a suffix such as `file.rs:50-80` or `file.rs:50+10`, select a range. Lines over 1000 characters are cut. Binary files are refused. |
| `grep` | Regular-expression search (Rust syntax) in a file or a directory tree, with optional context lines and a `glob` filter. Respects `.gitignore`, includes hidden files, skips `.git`, binary files and files over 4 MB. Up to 200 matches by default (2000 max). |
| `find` | Finds files and directories by glob, newest first. Respects `.gitignore`. Up to 500 results by default. |
| `edit` | Changes an existing file by line number (see below). |
| `write` | Creates a file, or replaces one completely. Parent directories are created. An existing file keeps its line endings and BOM. |
| `delete` | Deletes a file, a symlink or (with `recursive`) a directory. Refuses the working directory, `/` and the home directory. |

Paths are relative to the session directory. Inside it, ref shows them relative and with `/` on every platform.

### Line anchors

The file tools use hash-tagged line numbers, modeled on [oh-my-pi](https://github.com/can1357/oh-my-pi):

```
[src/main.rs#3F2A]
1:fn main() {
2:    println!("hi");
3:}
```

`read` and `grep` print a `[path#TAG]` header, where `TAG` is a hash of the whole file, followed by `N:content` rows. `edit` takes that tag and line numbers from that version of the file, and applies all its changes (replace, delete, insert before or after) at once, so earlier changes in the same call do not shift later line numbers.

If the file changed since the tag was issued, ref maps the line numbers onto the current file through a diff when it can. Otherwise it rejects the edit and returns the current content around the requested lines, so the model can retry without reading the whole file again.

## bash

Runs a command with `bash -c` in the session directory, with stdout and stderr combined in order.

- Timeout: 120 s by default, up to 3600 s (`0` disables it). On timeout the whole process group is stopped: SIGTERM, then SIGKILL after 2 s on Linux and macOS; `taskkill /T /F` on Windows.
- Output over 50 KB or 2000 lines is cut in the middle; the full output is saved to a file the model can `read`.
- Output streams live into the TUI while the command runs.
- Commands run with `TERM=dumb`, `NO_COLOR=1` and `PAGER=cat`, so tools do not wait for a pager or print colour codes.
- **bash is required.** On Windows, install [Git for Windows](https://git-scm.com/download/win): ref uses its Git Bash, found through `git.exe` on `PATH` or the default install locations. The WSL `bash.exe` in `System32` is skipped, because it runs commands inside the Linux VM, where paths are different. On macOS the system `/bin/bash` (3.2) works; a newer bash earlier on `PATH` is used instead.

## webfetch

Fetches an `http(s)` URL.

- **HTML** becomes markdown by default (or plain text, or stays HTML). For articles, only the main content is kept unless `full_page` is set; the result header says when this happened.
- **Markdown** is used directly when the site offers it, through content negotiation or a `<link rel="alternate" type="text/markdown">`.
- **JSON** is pretty-printed. **RSS and Atom feeds** become a list of entries. **Images** up to 5 MB are returned as images (see [Images](#images)).
- **GitHub**: file pages return the raw file, and issues and pull requests return the conversation through the API. `GITHUB_TOKEN` or `GH_TOKEN` is used when set.
- Other binary content, PDFs included, is not shown. The model is told to download it with `curl`.
- Character sets come from the HTTP header, the page itself or detection.
- Content over 50 KB or 2000 lines is cut, and the full text is saved to a file for `read`.
- `HTTP_PROXY`, `HTTPS_PROXY` and `NO_PROXY` are used, except for local and private addresses, which are always fetched directly.

## Images

When `images.auto_resize` is on (the default), images from `read` (up to 10 MB), from `webfetch` and attached to prompts are scaled the way oh-my-pi does it. MCP images are kept as they are.

- The longest edge is fit into 1568 px, which is the size Claude scales to anyway. Images with a short edge under 200 px are scaled up.
- The smaller of PNG and JPEG (quality 80) is kept. When the result is still over the target size (500 KB, 300 KB for `webfetch`), JPEG quality drops to 40, and then the image is scaled to a quarter. Images with transparency stay PNG.
- Small images within the bounds (under a quarter of the target) are sent unchanged.
- When the size changes, the tool result tells the model the original size and the factor that maps coordinates back.
- `read` refuses images it cannot decode, because the provider would reject the whole request.

Whatever that setting, requests to Claude models (the Anthropic API, or a model id containing `claude` on another API) are checked against Claude's limits: images over 8000 px on a side or 10 MB of base64 are scaled down, and all images are kept within 2000 px once a request holds more than 20 of them. Only the outgoing request changes: the session keeps the originals, and results are cached so repeated requests do not encode again.

When a provider rejects a request with a stricter image limit in the error message (for example Bedrock's `image exceeds 5 MB maximum: … > 5242880 bytes`, or `image dimensions exceed max allowed size: N pixels`), ref remembers that limit for the provider until the backend restarts and resends the request with images fit to it. This works for every provider. An image error without a readable limit fails like any other error that cannot be retried.

## Context tools

- `context_window`: tokens used, the maximum, and the recommended and automatic compaction points, for this session or another one.
- `compact_context`: replaces the history with a summary the model writes itself.

See [Context compaction](configuration.md#context-compaction).

## question

`question` lets the model ask you one or more questions and wait for the answers: preferences, a decision between approaches, a clarification. It is opencode's tool of the same name, with one addition.

- Each question has a short `header`, the question, a list of options (a label and a description each), and optionally `multiple: true` (several options may be chosen) and `custom: false` (no typed answer; a "Type your own answer" row is added otherwise).
- An option can be marked `default: true`. It is preselected when the question is shown: in a single choice the cursor starts on it, in a multiple choice it starts checked. There is no timeout, so an unanswered question waits until someone answers.
- A call holds 1 to 4 questions of at most 8 options each. A call that breaks these limits is returned to the model as an error.
- The model gets your answers as the result. If you dismiss the question it is told so and continues. Interrupting the turn cancels the question.
- A session waiting for an answer is shown as such in every window, in the session list too, and sends the "question" notification like a permission request does.

Only the `build` and `plan` agents may ask by default; see [the `question` permission](configuration.md#permissions).

## Cross-session tools

These run inside the backend, so messages between sessions are delivered reliably. Session names can be any Unicode text.

- `list_sessions`: the live sessions you can message: name, id, agent, status, foreground or background, directory and title.
- `send_message`: sends a message to another session. A busy session reads it after its current step.
- `message_status`: whether a sent message is still queued, delivered, or read.
- `read_session`: another session's messages, newest first, page by page.
- `create_session`, `close_session`: only for agents with [`can_spawn`](configuration.md#agent-fields). Created sessions run in the background. With `must_report`, a created session that stops without sending a message gets a reminder to report. `close_session` refuses foreground sessions.

## MCP tools

Tools from [MCP servers](configuration.md#mcp-servers) are offered as `<server>_<tool>`. Text, images, resources and structured content in their results are passed to the model.
