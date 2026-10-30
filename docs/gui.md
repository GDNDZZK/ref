**English** · [简体中文](gui.zh-CN.md)

# Desktop GUI

- [Installing](#installing)
- [Starting](#starting)
- [Files and backend](#files-and-backend)
- [Building and developing](#building-and-developing)

`ref-gui` shows ref in a desktop window (Qt Quick). Like `ref` and `ref-web` it only displays sessions: they run in `ref-server`, so a session can be open in the terminal, the GUI and a browser tab at the same time. When no backend is running, `ref-gui` starts the `ref-server` that sits in its own folder; a backend started this way exits when the last client disconnects.

## Installing

Download `ref-gui-<version>-<platform>` (or `ref-all-…`) from the [latest release](https://github.com/GDNDZZK/ref/releases/latest); [Installation](../README.md#installation) in the README lists the platforms and what each system needs. The archive holds one folder:

| System | Contents |
|---|---|
| Linux | `ref-gui`, `ref-server`, `lib/`, `plugins/` and `qml/` (Qt and the other libraries), `qt.conf`, `ref-gui.desktop`, `ref.png` |
| macOS | `ref.app`, which holds `ref-server` and all libraries |
| Windows | `ref-gui.exe`, `ref-server.exe`, the Qt and Visual C++ runtime DLLs, `plugins\` and `qml\` |

Keep the folder together and run the program from where it is; moving the whole folder is fine. No installation or administrator rights are needed. Everything the GUI needs besides the system's own libraries is in the folder, so it does not use a Qt that is installed on the system.

On Linux the program runs with X11 or through XWayland. Qt's Wayland plugin is not included.

## Starting

```bash
ref-gui                       # new session in the current directory
ref-gui ~/src/project         # new session in another directory
ref-gui -c                    # continue the most recent session for this directory
ref-gui -s ses_01a1…          # open a session by id (live or from history)
ref-gui --agent plan --model openai/gpt-5#high
```

| Option | Meaning |
|---|---|
| `DIR` | Directory for the new session (default: the current directory). |
| `-c`, `--continue` | Continue the most recent session for this directory. |
| `-s`, `--session ID` | Open a session by id, live or from history. |
| `-w`, `--watch` | Split view of all live background sessions (not available yet). |
| `--agent NAME` | Agent for the new session. |
| `--model provider/model[#variant]` | Model for the new session. |
| `--server ws://host:port` | Connect to this backend instead of finding or starting one. |
| `--token T` | Token for `--server`. Needs `--server`. |
| `--mock` | Use a private fake backend with canned scripts (for development, see [The mock backend](development.md#the-mock-backend)). Cannot be combined with `--server`. |
| `-V`, `--version`, `-h`, `--help` | Version and help. |

The window title is `ref`. On Windows `ref-gui.exe` is a window-only program: it opens no console, so `--help` and `--version` print nothing there.

## Files and backend

The GUI uses the same instance files, configuration and session storage as `ref` ([Files and directories](cli.md#files-and-directories)), and the same environment variables.

## Building and developing

See [The GUI](development.md#the-gui) in the development guide.
