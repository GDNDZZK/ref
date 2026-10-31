[English](development.md) · **简体中文**

# 开发

## 构建

需要当前稳定版 Rust 工具链（代码使用 edition 2024 和较新的语言特性）、C 编译器（内置的 SQLite 和 TLS 库需要），以及用于 Web 前端的 [Node.js](https://nodejs.org/) 24 和 [pnpm](https://pnpm.io/)。

**先构建前端。** `ref-web` 嵌入 `web/dist/` 中的文件，`web/dist/index.html` 不存在时它的 build 脚本会报错并停止编译。所有会编译整个工作区的 cargo 命令都是如此：`cargo build`、`cargo test`、`cargo clippy --workspace`。

```bash
(cd web && pnpm install && pnpm build)
cargo build --release
./target/release/ref
```

程序生成在 `target/release/` 中：`ref`、`ref-web` 和 `ref-server`。请让每个界面程序与 `ref-server` 放在同一个目录，因为它们会优先在自身所在目录查找 `ref-server`。修改前端后重新运行 `pnpm build`，再重新编译 `ref-web`；cargo 会发现 `web/dist/` 的变化。

## 测试

```bash
cargo test --workspace                                         # 所有 crate（需要 web/dist，见上文）
cargo fmt --all --check                                        # 格式检查
cargo clippy --workspace --all-targets -- -D warnings          # lint
ref --mock                                                     # 用假后端运行 TUI
(cd web && pnpm check)                                         # 前端：类型检查、lint、单元测试、生成的 Theme/Motion 文件是否最新
python3 scripts/e2e.py provider/model "prompt" [cwd]           # 用真实模型做端到端测试
REF_DUMP_REQUESTS=/tmp/dump ref-server ...                     # 导出 LLM 请求体
```

`ref-web` 中需要启动后端的测试使用与 `ref-web` 同目录的 `ref-server`，找不到时会失败并给出提示。`cargo test --workspace` 会构建它；单独运行 `cargo test -p ref-web` 时，请先运行 `cargo build -p ref-server`。

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
| `client` | 通过 instance 文件找到运行中的后端，或启动程序同目录的 `ref-server`，或启动私有的 `ref-server --mock`。`ref` 和 `ref-web` 共用。 |
| `server` | `ref-server`：会话、智能体循环、权限、压缩、存储（SQLite）、跨会话消息、WebSocket API。 |
| `tui` | `ref`：终端界面（ratatui）。 |
| `web` | `ref-web`：提供 `web/` 中的前端（嵌入并预压缩）、为浏览器登录，并把它们的 WebSocket 帧转发给 `ref-server`。见 [Web 界面](web.zh-CN.md)。 |

## 协议说明

协议类型在 `crates/proto` 中。由它们生成并提交到仓库的文件有两组：`crates/proto/fixtures/` 中每种帧的 JSON 样例（GUI 和 Web 前端的测试会逐个解析），以及 `web/src/proto/generated/` 中的 TypeScript 类型。文件过期时 `cargo test -p ref-proto` 会失败；`REF_UPDATE_FIXTURES=1 cargo test -p ref-proto` 会重写它们。`.gitattributes` 让这两个目录保持 LF 换行。

TUI 与后端通过 WebSocket 交换 JSON 帧。前端分页获取历史：`Attach` 返回最新的消息（100 条，超过约 1 MiB 时更少），`Messages` 返回指定消息之前的一页，因此 TUI 会在向上滚动时加载更早的页。图片以占位信息发给前端（MIME 类型、尺寸和 `id`，不含字节数据）；`GetImage` 按 id 返回图片字节。

## Mock 后端

`ref-server --mock` 是假后端，用于在没有 LLM 的情况下开发界面。`ref-gui --mock` 和 `ref-web --mock` 通过 `ref-client` 启动私有的 mock 后端。（`ref --mock` 是另一个进程内的假后端，与此无关。）

手动运行时要保持 stdin 打开，因为 stdin 读到文件结束时它会退出：

```bash
sleep 3600 | ref-server --mock      # stdout 只打印一行 {"port":N,"token":"..."}
```

- **退出**：stdin 文件结束、最后一个客户端断开，或 ctrl-c。退出时会删除它的临时目录（被 `SIGKILL` 杀死时会留下 `/tmp/ref-mock-*`）。`--idle-timeout S` 改变第二个条件：`-1` 让 mock 在最后一个客户端离开后继续运行（用来演示后端比窗口活得更久；stdin 必须保持打开），`S` 表示再等 S 秒。不带这个选项时立即退出。
- **工作区**：无论客户端请求哪个目录，所有会话都在带示例文件的临时工作区中运行（`README.md`、`NOTES.txt`、`src/parser.rs`、`src/lib.rs`、`docs/design.md`、`data/big.log`、`logo.png`、`scratch/src/parser.rs` 和 `scratch/src/lib.rs`），不会改动你的文件。预置了 3 条历史会话。
- **模型**：`mock/mock-1`（默认；200k 上下文、图片、推理、变体 `low`、`medium`、`high`）和 `mock/mock-fast`（64k）。多一个智能体 `reviewer`。
- **权限**：`edit` 和 `write` 询问（`scratch/` 下允许）。`delete` 允许，但延迟 5 秒。`bash` 询问，以 `echo`、`ls`、`cat`、`seq`、`sleep`、`printf`、`sed -i` 开头的命令允许，`curl` 拒绝。`webfetch` 关闭。
- **限制**：mock 并不拦截所有操作。*导出*会写到客户端给出的路径，`!` shell 命令会真的运行命令，所以两者都可能写到或运行到工作区之外。脚本调用的工具只会碰工作区。目录选择器限定在工作区内，`~` 就是工作区。
- **脚本**：提示词的第一个词选择脚本，不区分大小写。其他文字运行 `default`。提示词 `help` 会列出全部脚本。

| 提示词 | 内容 |
|---|---|
| `help` | 列出全部脚本 |
| `hello` | 一句话回复 |
| `default` | 其他文字：回显提示词 |
| `markdown` | 标题、列表、表格、引用、链接、多种语言的代码块 |
| `reasoning` | 长推理，然后简短回答 |
| `article` | 带很多代码块的长文章（约 18 秒） |
| `slow` | 极慢的流式输出，约一分钟，用于逐帧检查 |
| `tools` | `read`、读不存在的文件（出错）、`grep`、`find`、需要批准的 `edit`（带 diff）、成功的 `bash` 和以 3 退出的 `bash` |
| `stream-args` | 参数慢速流式生成的 `write` |
| `permission` | 等待批准的 `edit`：`a` 允许、`s` 本会话都允许、`d` 拒绝 |
| `delay` | 带 5 秒倒计时的 `delete`（`esc` 取消） |
| `running` | 持续输出 8 秒的 `bash` 命令 |
| `longrun` | 一条 `bash` 命令，200 秒内每 10 秒输出一行 `step N/20: tests::parser_NN ... ok`，最后回答 `All 20 tests pass.`（`esc` 取消）。没有窗口打开时它的输出也会保留 |
| `bash-fail` | 以 1 退出的 `bash` 命令 |
| `denied` | 被配置拒绝的调用 |
| `cancel` | 永不结束的命令，用 `esc` 取消 |
| `todo` | 待办一项一项打勾，大约在 5、7、9 秒，方便在多个窗口里看它同步 |
| `image` | 读取 `logo.png`，工具结果带图片 |
| `long` | 输出 400 行的工具，然后是长回答 |
| `retry` | 两次 529 错误（重试中），然后成功（约 6 秒） |
| `outage` | 先跑两个工具，然后同一个请求连续失败四次（529、529、连接被重置、503），等待约 2、4、8、16 秒；之后是一次 `edit`、一次 `bash` 和回答（约 30 秒） |
| `fail` | 401 错误，重试后以错误结束（约 6 秒） |
| `fill` | 上下文接近 95%，下一条提示会先自动压缩 |
| `compact-tool` | 模型调用 `compact_context` |
| `peer` | 创建 `reviewer` 会话 `helper`，它用 `send_message` 回报 |
| `everything` | `tools`、`todo` 和 `image` 连成一个长回合 |
| `question` | 模型提出两个问题（一个单选、一个多选，都带默认值），并复述你的回答 |
| `question-one` | 一个带默认值的单选问题，`enter` 采纳默认值 |
| `stale` | 文件在 `read` 和 `edit` 之间被改动：真实的 `edit` 工具把第一次修改对到移位后的行，拒绝第二次，模型重新 `read` 后重试 |
| `orchestrate` | 主会话先列计划，再创建 `scout`、`fixer`、`tester`、`docs` 四个助手，收齐它们的五份报告（第五份来自最后创建的 `reviewer`），最后给出总结（约 25 秒） |
| `orchestrate-long` | 和 `orchestrate` 相同，但每个助手运行一条 `bash` 命令，每 3 秒输出一行进度，持续 60 到 80 秒；总结约在 100 秒结束 |

`orchestrate` 约 25 秒，需要批准一次（fixer 的 `edit`）。时间线固定：助手在 1.5、3、4.5、6 秒出现，fixer 在 6.4 秒请求批准，主会话在 7.3 秒左右空闲，报告大约在 10.5 秒（`scout`）、12 秒（`docs`）、15.5 秒（`tester`）和 16.5 秒（`fixer`，在你于 13 秒批准之后 3.5 秒）到达。18 秒创建 reviewer，21 秒收到它的报告，25 秒总结结束。批准得更早，fixer 的报告及其后的一切都会提前，此时 `fixer` 的报告可能先于 `tester` 的。在 `tester` 报告之前（约 15 秒前）给它发一条消息，报告就变成 `Tests added for the parser fix. Also covered: empty input.`主会话忙碌时到达的报告会在它的下一次请求时并入对话，计数不受影响。`orchestrate` 之后的文字不会被使用。它启动的其他脚本（`orch-scout`、`orch-fixer`、`orch-tester`、`orch-docs`、`orch-review`、`orchestrate-report`）也会出现在 `help` 里，但不是给人输入的。

`orchestrate-long` 同样需要批准一次，约在 8 秒；在 10 秒批准时，助手大约在 66 秒（`scout`）、76 秒（`docs`）、79 秒（`tester`）和 91 秒（`fixer`）报告，reviewer 在 96 秒报告，100 秒总结结束。主会话从 7 秒起空闲，四个助手一直忙到各自报告。它可以单独使用。来自其他会话的消息，由「对话的第一个脚本名加 `-report`」命名的脚本回答，所以 `orchestrate-long` 对应 `orchestrate-long-report`；没有这样的脚本时用 `ack`。

`stale` 和 `outage` 会改动它们编辑的示例文件，并且能在同一个后端里重复运行：修改会把名字或行改回去。`stale` 和 `outage` 操作 `scratch/src/parser.rs` 和 `scratch/src/lib.rs`，这是一个带 `parse_line` 函数的小 crate，因为 `scratch/` 里的编辑和 `sed -i` 不需要批准。`stale` 在 `read` 和 `edit` 之间用 `sed -i` 在文件顶部插入三行注释，并改动 `scratch/src/lib.rs` 的第 12 行。`edit` 工具的回答（`Recovered: file changed since read; line numbers remapped.`、`Edit rejected for ... (tag #..., current #...)`）是它自己的文字。`outage` 里的连接重置是到模拟提供方的套接字真的被重置，所以错误行是 HTTP 客户端的（`network error: ... Connection reset by peer`，带本机回环地址）。等待时间是引擎的退避，在 2、4、8、16 秒上下最多浮动 20%。

## GUI

`ref-gui` 在 `gui/` 中（C++23、Qt Quick、CMake），与 Rust 工作区分开构建。需要支持 C++23 的编译器（GCC 13、较新的 Clang 或 Visual Studio 2022）、CMake 3.25 和 Ninja。依赖库由 `gui/scripts/` 中的两个脚本安装，CI 也运行它们；版本固定在 `gui/scripts/versions.sh`（Qt 6.12.0 通过 aqtinstall 安装官方预编译包；extra-cmake-modules、KSyntaxHighlighting 和 QCoro 从源码编译）：

```bash
export REF_DEPS_DIR=$HOME/ref-deps                 # 默认：gui/.deps
gui/scripts/install-qt.sh
gui/scripts/build-deps.sh
cargo build -p ref-server                          # 有两个测试针对真实后端运行
cmake -S gui -B gui/build -G Ninja \
  -DCMAKE_PREFIX_PATH="$(gui/scripts/qt-dir.sh);$REF_DEPS_DIR/prefix" \
  -DREF_SERVER=$PWD/target/debug/ref-server
cmake --build gui/build
QT_QPA_PLATFORM=offscreen QML_IMPORT_PATH=$REF_DEPS_DIR/prefix/lib/qml \
  ctest --test-dir gui/build --output-on-failure
```

`ref-gui` 在运行时加载 KSyntaxHighlighting 的 QML 模块。它位于 `$REF_DEPS_DIR/prefix/lib/qml`，所以从构建目录运行程序时，这个目录必须在 `QML_IMPORT_PATH` 中。部署后的包自带这个模块。

### 部署

`gui/cmake/Deploy.cmake` 包含安装规则。`cmake --install` 复制程序，并通过 Qt 的部署 API 复制它需要的 Qt 库、插件和 QML 模块。QML 模块通过扫描 QML 源码中的 import 找到，所以新增 import 不需要修改任何清单。配置时加 `-DCMAKE_INSTALL_BINDIR=.`，得到 `ref-gui` 在顶层的平铺文件夹：

```bash
cmake -S gui -B gui/build -G Ninja -DCMAKE_INSTALL_BINDIR=. -DCMAKE_PREFIX_PATH=…
cmake --build gui/build
cmake --install gui/build --prefix stage
scripts/package.sh --version v0.2.0 --target x86_64-unknown-linux-gnu \
  --bin-dir target/release --out dist --gui-dir stage      # 需要先完成 Rust 构建，见上文
```

`cmake --install` 还会删掉 Basic 以外的 Qt Quick Controls 样式（`gui/cmake/PruneControlsStyles.cmake`）：`ref-gui` 自己设置 Basic 样式，否则对 QML import 的扫描会把五种样式都部署进去。

- **Linux**：库放在 `lib/`，程序的运行路径是 `$ORIGIN/lib`；`qt.conf` 让 Qt 到 `plugins/` 和 `qml/` 中查找。桌面用不到的插件（QML 调试、嵌入式显示、Wayland 辅助插件）不会部署。
- **macOS**：`macdeployqt` 生成 `ref-gui.app`。`package.sh` 把它改名为 `ref.app`，把 `ref-server` 放进 `Contents/MacOS`，再运行 `scripts/sign-macos-app.sh`：由内向外重新做 ad-hoc 签名（没有开发者证书），并用 `codesign --verify --deep --strict` 检查。`all` 压缩包里的 `ref-server` 是指向 app 内那一份的符号链接。
- **Windows**：`windeployqt` 把 DLL 和 Visual C++ 运行库复制到 `ref-gui.exe` 旁边，不含软件 OpenGL 后备。程序使用 GUI 子系统（不弹控制台窗口），并带有图标和版本资源。

`scripts/verify-gui-package.sh DIR` 检查解压后的 `ref-gui` 压缩包，必须在它所构建的平台上运行。CI 在打包后运行它。它检查：不依赖构建机的环境时每个库都能找到、KSyntaxHighlighting 模块在包里、Controls 样式只有 Basic、`ref-gui --smoke --mock` 能打开会话（Linux 上在 `xvfb-run` 下）。Linux 上还检查包对系统的要求，macOS 上检查签名以及没有引用 app 之外的路径，Windows 上检查 `ref-gui.exe` 使用 GUI 子系统。

**Linux 的要求。** C++23 需要 GCC 13，而 Ubuntu 22.04 没有，所以 CI 在 Ubuntu 24.04 上构建 Linux 的 GUI，`ref`、`ref-web` 和 `ref-server` 仍在 22.04 上构建。在那里构建出的程序需要 glibc 2.38：Ubuntu 24.04 的头文件会把 `strtol` 等调用换成 glibc 2.38 引入的 `__isoc23_*` 符号，aqtinstall 安装的 arm64 版 Qt 也是在 Ubuntu 24.04 上构建的，同样需要 2.38（x86-64 版 Qt 最多需要 2.34）。要针对更旧的 glibc 构建 GUI，需要另一套工具链和 Qt 构建，项目目前没有。用 GCC 13 编译的程序需要 `libstdc++` 中的 `CXXABI_1.3.14`，Ubuntu 22.04 和 Debian 12 的 GCC 12 没有它。因此，只要包里有东西需要高于 glibc 2.38、`GLIBCXX_3.4.32` 或 `CXXABI_1.3.14`，`verify-gui-package.sh` 就会失败；README 写的是同样的数字。如果更新的编译器或 Qt 抬高了其中某项，请同时修改脚本开头的常量和 README。

## 持续集成与发布

`.github/workflows/` 中有两个 GitHub Actions 工作流。两者都先用 `.github/actions/` 中的 action 构建前端（Node 24、带缓存的 pnpm、`pnpm install --frozen-lockfile`、`pnpm build`）。

- **`ci.yml`** 在每次推送和拉取请求时运行：在 Linux 上执行 `cargo fmt --check` 和 `cargo clippy -D warnings`，在 Linux、macOS（Intel）和 Windows 上执行 `cargo test`，运行前端的 `pnpm check`，并在同样三个系统上构建 GUI 并运行它的测试（`ctest`）。GUI 的依赖库来自 `gui/scripts`，按这些脚本的哈希缓存。测试或前端检查失败时，失败输出会在运行摘要中再显示一次。
- **`release.yml`** 在推送 `v*` 标签时运行。它的任务依次是：构建一次前端；在原生 runner 上为六个目标构建 `ref`、`ref-server` 和 `ref-web`（每个目标一次 `cargo build --release --workspace`）；为每个目标构建并部署 GUI，用 `scripts/package.sh` 打出四个压缩包，再用 `scripts/verify-gui-package.sh` 检查 GUI 压缩包；生成 `SHA256SUMS` 文件，标签触发时发布 GitHub Release。每个打包任务都会在运行摘要中列出各压缩包的体积。

| 目标 | Runner（Linux 的 GUI 与打包） | 压缩包 |
|---|---|---|
| `x86_64-unknown-linux-gnu` | Ubuntu 22.04（24.04） | `.tar.gz` |
| `aarch64-unknown-linux-gnu` | Ubuntu 22.04 arm64（24.04 arm64） | `.tar.gz` |
| `x86_64-apple-darwin` | macOS 15 Intel | `.tar.gz` |
| `aarch64-apple-darwin` | macOS 15 | `.tar.gz` |
| `x86_64-pc-windows-msvc` | Windows Server 2025 | `.zip` |
| `aarch64-pc-windows-msvc` | Windows 11 arm64 | `.zip` |

每个目标生成四个压缩包。每个压缩包里是一个同名文件夹，包含该界面、`ref-server`、两个 README、许可证和 `docs/`。

| 压缩包 | 程序 |
|---|---|
| `ref-tui-<version>-<target>` | `ref`、`ref-server` |
| `ref-gui-<version>-<target>` | GUI（见[部署](#部署)）、`ref-server` |
| `ref-web-<version>-<target>` | `ref-web`、`ref-server` |
| `ref-all-<version>-<target>` | `ref`、`ref-web`、GUI、`ref-server` |

在 `cargo build --release` 之后手动打包：

```bash
scripts/package.sh --version v0.2.0 --target x86_64-unknown-linux-gnu \
  --bin-dir target/release --out dist [--gui-dir stage]
```

不带 `--gui-dir`（`cmake --install` 的结果文件夹，见[部署](#部署)）时不生成 `gui` 压缩包，`all` 里也没有 GUI。

发布新版本：

```bash
# 1. 修改 Cargo.toml 中的版本（[workspace.package] version），并运行
#    `cargo update --workspace` 刷新 Cargo.lock（CI 用 --locked 构建）。
#    把发布说明写进 .github/release-notes/v0.2.0.md。提交并推送。
# 2. 打标签并推送标签：
git tag v0.2.0
git push origin v0.2.0
```

`ref-gui` 的版本、Windows 和 macOS 的元数据以及压缩包名称都由标签和 `Cargo.toml` 决定。标签与 `Cargo.toml` 中的版本不一致时，工作流的第一个任务会失败。发布说明取自文件 `.github/release-notes/<标签>.md`；文件不存在时，由 GitHub 根据提交生成。

手动运行 Release 工作流（Actions → Release → Run workflow）会构建压缩包和 `SHA256SUMS` 并附加到该次运行，不会发布。
