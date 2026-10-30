# ref-gui 架构与实现

这份文档帮你读懂当前代码：它是怎么分层的，各部分为什么这样写。建议按章节顺序读，每一章都对应一组源文件。ref-gui 是 ref 的 Qt Quick 前端，它只是 ref-server 的一个客户端，自己不调用任何模型；界面和动效部分最初来自 qutie（LightBlueCube），协议、连接、会话这几层是为 ref 重写的。

## 1. 分层

```
┌──────────────────────────────────────────────────────────────────┐
│ src/app  QML 界面 + ChatSession + main                            │
│   Main ─ ChatView ─ {User,Image,Markdown,Code,Reasoning,Tool,     │
│        │            Error}Block                                   │
│        └ Composer, ThemeToggle, 回到最新按钮, 状态条              │
│   Theme / Motion (单例)        SystemFonts (C++ 单例)             │
├────────────────────── ChatSession ───────────────────────────────┤
│ src/session  与界面框架无关的会话层（不依赖 Gui，可以单测）        │
│   SessionController ─ ConversationFeed ─ StreamPacer              │
│                         └ ConversationModel ─ blocks, FenceSplitter│
├──────────────────────────────────────────────────────────────────┤
│ src/client  连接后端                                              │
│   Backend（找到/拉起后端、断线重连）─ ServerClient（一条 WebSocket）│
│   launcher, instance_files, spawn_lock, process_alive, mock_backend│
├──────────────────────────────────────────────────────────────────┤
│ src/proto   ref 线协议：类型、解析、请求构造（只依赖 Qt Core）     │
├──────────────────────────────────────────────────────────────────┤
│ src/cli     命令行解析（只产出 LaunchOptions，不碰界面）           │
└──────────────────────────────────────────────────────────────────┘
```

规则如下：

- QML 只能看到 `ChatSession`。它暴露了 `blocks`（`QAbstractItemModel*`）、`busy`、`ready`、`connectionState`、`statusText`、`title`，以及 `send()`、`stop()`、`permissionReply()`。
- `src/session`、`src/client`、`src/proto` 里的类型不注册给 QML，所以它们的接口改了也不会波及界面。
- 每一层只调用紧挨着的下一层。`proto` 不知道 Qt 网络；`client` 不知道会话怎么显示。
- 没有全局状态。每个 `ChatSession` 持有自己的 `Backend` 和 `SessionController`；以后做多 tab 或分屏，就是多建几个。
- 对话内容以服务器为准。发出的 prompt 不会在本地回显，用户的消息在服务器通过 Message 事件宣布后才出现。

## 2. 构建

- 根目录的 `CMakeLists.txt` 要求 C++23（`std::expected`、协程）和 Qt 6.10 以上。版本号读自上一级 `Cargo.toml` 的 workspace 版本，经 `REF_GUI_VERSION` 传给 `setApplicationVersion`。
- `cmake/CompilerOptions.cmake` 里的 `refgui_configure_target()` 套用到我们自己的每个 target 上：警告全开并当作错误，打开 Qt strict mode 和 `QT_NO_CAST_FROM_ASCII`；MSVC 用 `/W4 /permissive- /Zc:__cplusplus /utf-8 /EHsc`；`-DREFGUI_SANITIZE=ON` 打开 ASan/UBSan。
- 依赖：Qt 6.12（含 WebSockets、Quick、LinguistTools）、QCoro 0.13（Core、Network、WebSockets）、KSyntaxHighlighting（运行时的 QML 插件，代码块高亮用）。`scripts/install-qt.sh` 和 `scripts/build-deps.sh` 会把它们装到 `REF_DEPS_DIR`（默认 `gui/.deps`），之后 `-DCMAKE_PREFIX_PATH="<deps>/Qt/6.12.0/gcc_64;<deps>/prefix"` 即可。
- 运行时要让 Qt 找到 KSyntaxHighlighting 的 QML 插件：设置 `QML_IMPORT_PATH=<deps>/prefix/lib/qml`。
- 库只依赖它真正用到的东西：`refgui_proto` 只要 Core；`refgui_client` 加 Network、WebSockets、QCoro；`refgui_session` 依赖 proto 和 client；只有可执行文件才依赖 Gui、Qml、Quick。
- `src/app` 是一个 QML 模块（URI 为 `RefGui`）。`Theme.qml` 和 `Motion.qml` 通过源文件属性 `QT_QML_SINGLETON_TYPE` 声明成单例，这个属性必须在 `qt_add_qml_module()` 之前设置。
- 翻译：界面文字在源码里是英文，QML 用 `qsTr`，C++ 用 `tr`。`translations/ref-gui_zh_CN.ts` 在构建时编成 `.qm` 放进资源（前缀 `/i18n`），启动时按系统语言加载，没有对应语言就用英文。改了文字后运行 `cmake --build build --target update_translations` 更新 `.ts`。
- 命令行：`ref-gui [DIR] [-c] [-s ID] [-w] [--agent NAME] [--model provider/model[#variant]] [--server ws://… --token T] [--mock]`。参数名与 TUI 一致。`-s` 优先于 `-c`，`-c` 优先于新建；`-w` 目前只解析并提示 "split view is not available yet"。隐藏参数 `--smoke` 供自动化测试使用：会话打开后打印 `attached <id>` 并退出 0，连不上或会话不存在则退出 1。

## 3. proto：线协议

`src/proto` 是 `crates/proto` 的 C++ 镜像，只有数据和解析，没有 I/O。

- `types.h`：全部协议类型，字段名用 camelCase，时间戳是毫秒。`ToolState` 带 `isFinal()`。
- `parse.h`：`Parsed<T>` 是 `std::expected<T, ParseError>`。服务器版本比客户端新时，未知的帧种类、事件种类、part 种类不算错：它们解析成可以忽略的结果（part 变成 `UnknownPart`，保持下标对齐）。`ParseError::unknown` 区分“没见过的”和“格式错的”。
- `request.h`：请求体的构造函数（hello、create_session、attach、prompt、abort、permission_reply 等）。关联用的 id 由 `ServerClient` 加。
- `tests/tst_proto.cpp` 遍历 `crates/proto/fixtures/` 里的全部样例，所以协议改了而这里没跟上，测试会红。

## 4. client：连接后端

### ServerClient

一条 WebSocket 连接。每个请求是一个协程：发一帧（带新 id），在收到同 id 的响应时恢复。其余帧（事件、sessions、toast、notify）按到达顺序通过 `frameReceived` 发出。

- 响应是在 `textMessageReceived` 的槽里**同步**恢复协程的，所以 Attach 的快照一定在紧随其后的事件之前处理完。QCoro 自带的信号 awaiter 经事件循环排队恢复，做不到这一点，所以这里自己写了 awaiter。
- 关闭连接或销毁 client 时，所有等待中的请求以 "connection closed" 失败，它们的协程当场继续运行。所以必须先 `close()`，再销毁协程会访问的对象。

### launcher：找到或拉起后端

`connectTarget()` 的逻辑与 TUI 的 `ref-client` 一致：

1. 读 `<state dir>/instances/*.json`，挑存活的实例直接连。
2. 没有就等 `spawn.lock`（Unix 用 `flock`，Windows 用 `LockFileEx`）。同一时刻只有一个前端在拉起后端，其余的等到锁之后再找一次实例，通常就找到了刚拉起的那个。
3. 拿到锁又没有实例时，选一个空闲端口，启动一个脱离进程的 `ref-server --port P --idle-timeout 0`，等它写出 instances 文件（最长等 BackendStartTimeout），再连。
4. `--server` 指定的地址直接连，不拉起任何东西。

`ref-server` 先找 ref-gui 同目录，找不到再走 `PATH`。状态目录由 `REF_STATE_DIR` 覆盖，数据目录由 `REF_DATA_DIR` 覆盖，自动化测试靠它们隔离。

### Backend

在整个程序生命期内持有连接：找到或拉起后端、连接、连接断开后等 1 秒重试。状态是 Connecting、StartingBackend、Connected、Disconnected（带原因）。每次连上都发 `connected()`，使用者据此重新 attach。`--mock` 时由 `MockBackend` 启动一个私有的 `ref-server --mock`（stdin 保持打开，关闭 stdin 它就退出）。

等待连接结束用的是自己写的 awaiter，原因同上：在信号发出的那一刻恢复，而不是之后排队，否则 Backend 在中间被销毁时，排队的事件会去恢复一个已经销毁的协程。

## 5. session：会话模型

### 一行是一个块，不是一条消息

```
user        "Fix the build"
image       "shot.png"
reasoning   "The error says ..."
markdown    "The QML module is missing a file:"
code        "qt_add_qml_module(...)"   language = "cmake"
tool        "bash ctest"               toolState, input, output, failed
error       "The request failed: ..."
meta        "Build · mock-1 · 1.1s · ↑4.3k ↓100"   消息下面的一行：用时、token、Copy、Undo
```

为什么拆成块：

1. 代码高亮需要单独一个 TextEdit。`SyntaxHighlighter` 要挂在一个 TextEdit 上，而且整个文档只能用一种语言。
2. 动效以块为单位：新块入场，旧块保持不动；流式输出时只有最后一个块在变。
3. 不同类型的块交互不同：思考过程可以折叠，工具调用可以展开输出，代码块可以复制。

### ConversationModel

模型直接吃协议数据：

```cpp
reset(messages)                       // 换成另一个会话的历史，唯一会 reset 的操作
replaceHistory(messages)              // 同一会话的历史被改写：只删除、插入有差别的行
prependMessages(older)                // 在最前面插入更早的消息
setMessage(message)                   // 新消息，或按 id 整条替换
setPart(messageId, index, part)       // 替换或追加一个 part
appendDelta(messageId, index, delta)  // 文本、思考、工具参数的增量
removeMessage(messageId)
```

一条消息怎么变成块（`blocks.cpp`）：

- 用户消息：所有文本 part 用空行连成一个 user 块，每张图片一个 image 块（带图片 id，缩略图按需从后端取）。来源不是用户本人时带 `source` 和 `sender`（对等会话、派生任务、提醒、压缩摘要、shell 输出）。
- 每条消息末尾多一个 meta 块（压缩摘要消息除外），装着时间、agent、模型、token、interrupted / failed 标记和 Copy / Undo 的动作。
- 助手消息：Text part 交给 `FenceSplitter` 切成 markdown / code 块（一个 part 一个 splitter，围栏不跨 part）；Reasoning part 是一个 reasoning 块；Tool part 是一个 tool 块；`error` 不为空且不是 "aborted"（忽略大小写，与 TUI 一致）时末尾再加一个 error 块。
- `streaming`：消息没结束时，最后一个文本或思考 part 在流式；工具在状态不是终态时算流式。所以同一时刻最多一个块在接收内容（运行中的工具除外）。
- `opensTurn`：user 块、来源是本人/对等/派生、且上面有行时为真，界面据此画分割线。prepend 和删除第一条消息时会对受影响的行发 `dataChanged`。

内部结构：`m_rows` 是平铺的块列表，`data()` 直接按行取；每条消息记着自己占几行（`MessageNode`），每个 part 记着自己占几行并带着它的 splitter。更新时把受影响的块重新算出来，和旧的逐行比较（`replaceRows`）：内容变了的发带 role 列表的 `dataChanged`，多出来的插入，少了的删除。除了 `reset()`，不会 reset 模型，ListView 的入场动画才不会失效。`replaceHistory()`（Reset 事件，如压缩后的新历史）从第一个有差别的行起删除再插入，前面没变的行原地不动；被删除的行在 `Block` 里淡出再收起高度。

模型还给每行算出两个与消息内容无关的状态（`refreshContext()`）：`stale`（在最后一次压缩之前，模型已看不到，界面变淡）和 `contextStart`（压缩摘要本身）。`ToolSummary`（有几个工具在等授权、最早的是哪个、最后一个工具是否在延迟倒计时）随行的变化更新，授权键和状态行读它。

交给界面显示的 Markdown（Markdown / 思考 / shell / 压缩摘要块）过 `markdownForDisplay()`（`markdown_text.cpp`）：`![alt](src)` 变成 `[alt](src)`，开标签或注释的 `<` 转义成文字；行内代码、围栏代码（含还没闭合的）和缩进代码原样保留。这样模型的内容不会让 Qt 的 Markdown 渲染器去取图片或解释 HTML。

Delta 指向还没见过的 part 时，按 TUI 的 `apply_delta` 规则补建（中间缺的补空文本 part；工具参数的 purpose 从流式 JSON 里提前解析出来显示）。

### FenceSplitter

它把流式到达的 Markdown 切成三种事件：`CodeOpened(language)`、`CodeClosed` 和 `TextAppended(text)`，规则按 CommonMark 的围栏代码块：

- 围栏是至少 3 个连续的 ``` ` ``` 或 `~`，前面最多缩进 3 个空格。
- info string 的第一个词是语言名。
- 闭合围栏必须用同一种字符，长度不少于开头那一行，后面只能跟空白。

流式处理的难点是：一行没结束之前，不知道它是不是围栏。处理方式如下：

- 一行一旦可以确定不是围栏（比如第一个非空字符是字母），就立刻放出，不必等到行尾。
- 有可能成为围栏的行先扣住（`m_heldLine`），等这一行结束再判断。
- 换行符延迟放出（`m_pendingLineBreaks`），等到后面确实还有内容时才补上。这样块的末尾不会挂着分隔用的空行。
- part 不再流式（消息结束，或后面又来了新 part）时，调用 `finish()` 放出扣住的最后一行。

测试里每个用例都会跑两遍，一遍整段输入，一遍逐字符输入，两种方式的结果必须完全一致。

### StreamPacer 与 ConversationFeed

真实服务端常常隔一两百毫秒发一大段，直接写进去就是一段一段蹦出来。`StreamPacer` 把积压的文字匀速放出：每次到达时按最近几次到达的间隔估计下一段什么时候到，让积压刚好在那时放完，所以文字以稳定的速度流出，最多比网络慢 200ms。切分按字素边界，不会拆开 emoji。事件带着目标（消息、part 下标、文本或思考），相邻同目标的才合并。

`ConversationFeed` 把会话事件应用到模型上，并决定哪些走 pacer：

- 文本和思考的 Delta 走 pacer，由 16ms 的 ticker 每帧取出该显示的部分写入模型。工具参数的 Delta 立即写入。
- “服务器说过的内容”等于已显示的文字加 pacer 里还没放出的文字。整条 Message 或整个 Part 到达时，如果新文本以这个内容开头，只把多出来的部分交给 pacer，其余字段（时间、状态、其他 part）立即更新；不是这个关系，或者事件改变了结构（新增 part、删除消息），就先放空 pacer 再应用，保证显示顺序不乱。
- 所以回复结束的 Message 事件不会把还没放完的文字一下子切出来。
- `hasPending()` 为真表示还有文字在路上。`ChatSession.busy` 是“会话忙，或还有文字没显示完”，沿用 qutie 的行为：忙时发送按钮变成停止。

### SessionController

一个会话的控制器：`create()`、`attach()`、`continueLast()`、`send()`、`abort()`、`permissionReply()`、`undo()`、`loadOlder()`、`getImage()`。订阅 `ServerClient::frameReceived`，按 session_id 过滤事件。

- Attach 响应之前到达的事件不缓冲也不处理：它们要么已经在快照里，要么根本不会发来。这一点依赖服务器的行为，没有对着真实服务器的并发场景验证过。
- 每次 `Backend::connected()` 后，如果已经有会话，自动重新 attach，快照替换整个历史；失败发 `reattachFailed`。
- 工具等待授权的状态在 tool part 里（`awaiting_permission`），所以 Permission、PermissionResolved 事件不处理。
- `loadOlder()` 取比当前最早一条更早的 100 条消息，先 `prependMessages()` 再清 `loadingOlder`，视图据此知道新增的行是历史、不播入场动画；`moreBefore` 来自快照、Reset 事件和每次翻页的响应。
- `undo()` 回退到某条用户消息之前，返回被撤销的提示（文字和图片），`ChatSession` 把它交给输入框（`draftRestored`）。

### tool_content

`session/tool_content.{h,cpp}` 把工具调用读成界面要的结构：参数 JSON 解析、图标名、header 上的一行摘要（命令、路径、模式、`n/m done` 等）、todo 清单、read / grep 输出（`[path#TAG]` 头、`N:`、`*N:`、` N:` 行和 `...`）、diff 行、路径拆分、大小文字。格式是 `crates/tools` 写出来的那些，不认识的内容返回空，卡片退回到原样的输入和输出。`app/tool_info.h` 把这些函数作为 QML 单例 `ToolInfo` 暴露。有单元测试（`tst_tool_content`），WebUI 的 `web/src/model/toolContent.ts` 是同一份逻辑。

## 6. app：界面

### ChatSession 与 SystemFonts

- `ChatSession` 是 QML 唯一能直接访问的业务对象，它持有 `Backend` 和 `SessionController`（析构时先 `shutdown()`，让挂起的请求先失败，再销毁它们依赖的成员）。`main()` 解析命令行后创建它，通过 `setInitialProperties` 交给 `Main.qml` 的 `required property ChatSession session`。QML 不能自己创建 ChatSession（`QML_UNCREATABLE`）。
- 后端一连上就按命令行打开会话（新建、继续上一个或按 id 附着）；连接状态和失败信息通过 `statusText` 显示在窗口顶部的状态条里，`ready`（已连接且有会话）为假时输入框不能发送。
- `SystemFonts` 只做一件事：把系统等宽字体告诉 QML。`"monospace"` 这种字体名只是 fontconfig 的别名，在 Windows 和 macOS 上无效，所以要用 `QFontDatabase::systemFont(FixedFont)` 去取。

### Theme 与 Motion

- 所有颜色、字号、间距都放在 `Theme.qml`，所有时长和缓动曲线都放在 `Motion.qml`，组件里不允许出现硬编码的数字。
- `Theme.dark` 一开始绑定到系统的配色方案，用户点切换按钮后改成手动值，从此不再跟随系统。

动效的分工：

| 场景 | 用法 |
|---|---|
| 悬停、按下、聚焦 | `Motion.quick` + `OutExpo` |
| 入场、展开折叠、让位 | `Motion.standard` + `OutExpo` |
| 主题切换、分割线展开、收回保留的空间 | `Motion.slow` + `OutExpo` |
| 主题按钮的太阳/月亮形变 | `OutBack`（回弹，只用于低频场景） |
| 工具结束时的状态点、错误圆标（`Flourish`） | 收缩后用 `OutElastic` 弹回并摆动，同时扩散一圈圆环。虽然高频，但只在点周围几像素内动，不影响布局 |
| 流式输出时块的高度、跟随滚动、滚轮滚动 | 逐帧指数追赶（见下一节） |

### 逐帧指数追赶

目标值会连续变化的动画，不能用 `Behavior`：`Behavior` 每次收到新目标都会停掉当前动画、重新开始，而新动画要等到下一帧才开始推进。如果目标每一帧都在变，比如外层块的高度跟着内层折叠动画一起变，属性就会一直停在原地不动。

所以这类场景用 `FrameAnimation` 在每一帧推进一步：

```
remaining = target - current
current  += remaining * (1 - e^(-frameTime / followLag))
```

每帧走完剩余距离的固定比例，效果就是一条指数缓出曲线。中途目标变了也没关系，下一帧接着追，不会出现停顿；而且这个比例按帧间隔换算，帧率不同时速度也一致。剩余距离小于 0.5px 时，直接落到目标值上。

用到这个做法的三处：

- `Block.qml`：块在流式输出时，高度追赶内容的真实高度。块输出结束后会继续追到位才停，这样思考块完成后自动折叠时，外层高度也能平滑跟上。
- `ChatView.qml`：跟随滚动时 `contentY` 追赶底部位置，滚轮滚动时追赶 `wheelTarget`。

滚动的两处每一步都取整到整数像素，并且每帧至少走 1px（`scrollStep()`）。原因是 ListView 每次重新布局都会把带小数的 contentY 取整。thinking 折叠期间行高每帧都在变，每帧都会重新布局，于是每帧不到 1px 的那点前进全被吃掉，追赶会停在离底部几像素的地方；等折叠结束才补完这一段，看起来就像贴好之后又抽了一下。

### ChatView：跟随与保持

跟随：

- `following` 这个状态只由用户的手势改变：开始滚动或按住滚动条时置为 false；手势结束时，停在底部就恢复为 true。
- 底部位置从 footer 读取，footer 紧跟在最后一行后面。底部留白也用 footer 来做，因为 `positionViewAtEnd()` 不计入 `bottomMargin`。
- `cacheBuffer` 设为 `retainedDistance`（20000 px，约 200 行），视口上下这个范围内的行常驻。ListView 销毁屏幕外的行后会按平均行高估算它们的高度；高的代码块一进出视口，估算值就会变，滚动条长度跟着变，拖动滚动条时内容来回跳。多数会话在这个范围内，所以几何量是精确的。全部常驻太费内存：mock 会话 800 行时，常驻全部的峰值是 256 MB，默认缓存是 124 MB（每行约 0.25 MB）。
- 向上翻页：`ChatView` 的 `onContentYChanged` 在离顶部不到 `olderTriggerDistance`（400 px）、有更早的消息、不在加载、没有待重试的错误、且不在跟随时发 `loadOlderRequested`。顶部的 `OlderPill` 显示加载中、失败（点击重试）。前面插入行时 ListView 自己保持阅读位置，不需要补偿代码；`prepending` 为真期间新增的行不播入场动画。

滚轮：

Flickable 自带的滚轮动画，每一格都从当前位置重新开始，上一格剩下的距离会被丢掉，所以快速滚动时反而走得比慢速少。现在 `WheelHandler` 接管所有滚轮事件：滚轮每一格把 `wheelTarget` 推进固定距离，contentY 用逐帧追赶去接近它，快速连续滚动的距离会累加；触控板手势按 pixelDelta 直接移动内容。区分滚轮和触控板看的是事件的 scroll phase，不看设备类型：Wayland 下 Qt 会把鼠标滚轮也报告成 "touchpad" 设备。

保持（防止来回跳）：

```
回复进行中且在跟随时：
  highWater = max(highWater, naturalEnd)     底部到过的最远位置
  heldSpace = highWater - naturalEnd         内容让出来的高度，加到 footer 上
这一轮结束后：
  heldSpace 用 Motion.slow 动画收回到 0
```

例子：一行先折成两行显示，输出完又变回一行。如果不做保持，视图会先跟着下移、再弹回来；做了保持，让出来的那一行高度先留着，等这一轮结束后再平滑收回。

### 入场动画

`Block` 在 `ListView.onAdd` 里给自己做入场动画：opacity 从 0 到 1，同时一个 `Translate` 从 `-Motion.enterDrop` 回到 0。行的 `y` 始终由 ListView 实时布局。

这里没有用 ListView 的 `add` Transition。Transition 会在插入那一刻把行的目标位置定死，动画期间行的位置变化只是被记下来，等动画结束才生效。例如 thinking 块折叠时紧接着插入了 tool 块：tool 块会先落在折叠前的位置，动画结束后再瞬移上去。改成 Translate 之后，入场偏移叠加在实时布局之上，不会出现这个问题。

只有真正新插入的行会触发 `onAdd`；子类可以通过 `added()` 信号得知（UserBlock 用它触发分割线展开）。

### 各个块

| 文件 | 要点 |
|---|---|
| `Block.qml` | 所有块的基类：在阅读栏里居中；流式输出时高度做追赶；`clip` 防止内容在追赶过程中溢出 |
| `UserBlock.qml` | 气泡宽度贴合文字；开启新一轮的消息（`opensTurn`）上方加一条分割线，新发送的消息分割线从左往右展开（`added()`）；不是用户自己输入的消息（对等会话、派生任务、提醒、压缩摘要、shell 输出）上方有一行说明来源 |
| `ImageBlock.qml` | 消息里的图片：`ImageStrip` + `Thumbnail`（72 px 高），经 `image://chatimage/<会话 id>/<图片 id>` 取图（`ChatImageProvider`，异步，64 MB 缓存），点击发 `imageClicked`，由 `SessionPane` 里的 `Lightbox` 显示大图 |
| `MetaBlock.qml` | 消息下面的一行（`MetaText`、`MetaAction`）：角色 / agent / 模型 / 用时 / token，interrupted / failed 标记，hover 显示 Copy，用户消息有 Undo；比消息本身淡 |
| `MarkdownBlock.qml` | `TextEdit.MarkdownText` 渲染；流式输出时末尾有一个跟着走的脉冲点（`StreamCaret`） |
| `CodeBlock.qml` | PlainText 加 `SyntaxHighlighter`；语言名匹配不到时按纯文本显示；复制按钮 |
| `ReasoningBlock.qml` | 输出时自动展开，结束后自动折叠，也可以手动点击开合；"Thinking" 字样在输出时呼吸闪烁 |
| 吸顶标题 | 思考块和工具块展开时，标题行贴在视口顶部，直到整个块滚出去（`Block.pinOffset()`）。吸顶时点击折叠，视图会跟着块变矮往回滚，标题停在原处不动 |
| `ToolBlock.qml` | 工具卡片。头部：`ToolStateSlot`（16×16 的状态槽，8 种状态；运行中脉冲，完成 / 出错时 `Flourish`）、类型图标、名称、purpose 和摘要、Cancel 按钮（延迟倒计时中，带 `CountdownRing`）、状态文字、用时。折叠的失败卡片显示输出首行；等待授权时强制展开，边框变 warning 色，下方是 `PermissionSection`（允许 / 本会话内允许 / 拒绝，带 a / s / d 键帽）。正文由 `ToolBody` 按工具分派（见下） |
| `StatusLine.qml` | 输入框上方一行：活动文字（`ShimmerText`）、本轮用时、重试倒计时、上下文占用条；信息来自 `ChatSession` 的 `activity`、`retry*`、`context*` 属性 |
| `ErrorBlock.qml` | 浅红底卡片，左边一个"!"圆标（`AlertBadge.qml`）。出错很少见，但一定要让用户注意到：卡片以中性底色入场，落定后圆标用 `Flourish` 弹出并扩散一圈红色圆环，同时错误底色以硬边圆形从圆标处匀速扩散，盖满卡片即结束。动作都从一个点向外扩散，和工具状态点同一种语言；回弹只发生在圆标周围几个像素。用户正往上翻时出错，回到最新按钮的右上角会弹出一个红色"!"徽标，按钮消失时清掉 |
| `Flourish.qml` | "发生了"的局部点缀：`play()` 让已显示的目标收缩后用 `OutElastic` 弹回并扩散圆环；`enter()` 用于新出现的目标，从 40% 大小淡入并同时弹开（从 0 放大会像一个膨胀的点）。工具状态点、错误圆标、回到最新按钮的徽标共用 |
| `Pulse.qml` | 上面三处"进行中"闪烁共用的组件。停止时从当前透明度直接缓动回 1，不跑完当前周期；用 `alwaysRunToEnd` 的话，结束后最多还会再闪一次。周期由 `period` 设置，默认 `Motion.pulse`，Thinking 标题用更慢的 `Motion.calmPulse` |

### 工具正文

`ToolBody` 按工具名选一个正文组件：`DiffBody`（edit / write 的 diff）、`BashBody`（命令、输出、退出码和 exit / timed out / cancelled / truncated 标签；运行中只显示输出末尾 20 行）、`ReadBody`、`GrepBody`、`FindBody`、`WebfetchBody`、`SessionBody`（send_message 等跨会话工具）、`TodoBody`（清单，常驻展开）、其余用 `GenericBody`（输入 JSON 加输出）。结果缺了某个工具的布局所需的内容时也退回 `GenericBody`。共用件：`ToolBodyBase`（内边距和分段间距）、`ToolSection`（带标题的一段）、`OutputText`（200 行预览加 “Show more” 和复制）、`LineView`（带行号和符号列、可横向滚动的行表）、`CommandBox`、`ToolChip`、`CopyButton`。所有正文都从 `ToolBlock` 读数据（`call`），解析走 `ToolInfo`。

### 授权按键

`ChatSession.permissionKeysActive` 在有工具等待授权、且距第一次出现已过 350 ms（`PermissionGuard`）时为真。`SessionPane` 里的 `permissionKeys` 此时抢走焦点，把 a / Enter、s、d / Esc 交给 `ChatSession.answerOldestPermission()`，其他单字符键吞掉；答完把焦点还给输入框。同一个 call id 只回复一次（失败可以重试）。`SessionPane.keysEnabled` 在分屏里由分块是否有焦点决定。

### 来自外部的文字

模型的输出、工具的输出和其他会话发来的消息都不可信，界面按下面几条处理：

- 除了 Markdown 块（`MarkdownBlock`、`ReasoningBlock`、`UserBlock` 里的 shell 和压缩摘要）和 `ShimmerText`，QML 里所有 `Text` 都是 `Text.PlainText`，`TextEdit` 必须写明 `textFormat`；`tst_plain_text` 检查这一点。`ProseText` 默认也是纯文本。
- Markdown 先过 `markdownForDisplay()`（见 session 一节），再交给渲染器。
- `LocalOnlyNetwork`（`local_only_network.cpp`）是 QML 引擎的网络工厂，只放行 `qrc` 和 `data`；后端的 WebSocket 不经过它。即使上面哪一层漏了，图片地址也不会被取。
- `Links.open()` 只打开 http、https、mailto。

### 主题切换

```
grabToImage(page) → 截图盖到最上层 → Theme.toggle() → 截图淡出
```

所有颜色是同时完成交叉淡化的，不需要给每个元素单独写 `ColorAnimation`。有几个要注意的地方：

- 必须持有 `ItemGrabResult` 对象，否则截图的 URL 会失效。
- 窗口的背景色不在截图范围内，所以 page 里垫了一个背景 Rectangle。
- 切换按钮放在截图的上层，保证它自己的形变动画在切换过程中看得见。

### Composer

- 按 Enter 发送，Shift+Enter 换行；输入法正在组字时按 Enter 不会发送。
- 回复进行中，发送按钮会变成停止按钮，箭头和方块之间用旋转加缩放的动画切换。进入停止状态用 `Motion.ease`（先快后慢）；回到发送状态用 `Motion.exitEase`（`InCubic`，先慢后快），让方块的退场看得见，否则 ease-out 几乎在头几帧就走完了，看起来像直接切换。按钮背景色的过渡用 `Motion.slow`。
- 回到最新按钮叠在发送按钮正上方，竖直中线对齐（`Composer.sendButtonCenterX`）。

### 图标与滚动条

- 箭头图标统一用 `Chevron.qml` 绘制：90 度尖角、圆角端点，通过旋转改变朝向。不用字符，因为字体度量会让它在垂直方向上对不齐。根元素是普通 Item，Shape 只作为子元素，否则 Shape 按路径设置自己的尺寸，会和路径依赖尺寸形成绑定循环。
- 工具和界面里的图标是 `assets/icons/` 的 SVG，经 `IconProvider` 加载：`image://icons/<名字>-<网格>?color=<hex>`，`Icon.qml` 包了一层，名字为空时不加载。
- 滚动条的 handle 自绘，颜色取自 Theme。Basic 风格默认用系统 palette，不跟随主题。
- 输入框高度随内容增长，最大 220px。


## 7. 测试

| 测试 | 覆盖 |
|---|---|
| `tst_proto` | 遍历 `crates/proto/fixtures/` 的全部样例；未知种类的帧、事件、part 的处理；格式错误的报错 |
| `tst_server_client` | 响应按 id 对应请求、乱序响应、后端报错的文字、Attach 响应先于后面的帧处理、帧按到达顺序、连接关闭时等待中的请求失败 |
| `tst_launcher` | 实例文件的读取和存活判断、拉起锁、已有实例直接连、拉起后等待实例出现、显式地址、超时 |
| `tst_fence_splitter` | 每个用例分别按整段和逐字符各跑一遍；正文立即放出；可能是围栏的行先扣住 |
| `tst_conversation_model` | 消息到块的映射、交错输出、同时只有一个块在流式、delta 不 reset（配合 `QAbstractItemModelTester`）、part 替换和追加、工具状态变化、error 块（忽略 aborted）、来源和 `opensTurn`、删除、prepend、reset |
| `tst_stream_pacer` | 首段按最大延迟摊开、成段到达时每帧放出量稳定、逐 token 的流几乎不加延迟、目标顺序、不拆字素、空闲后不攒额度、takeAll、按 part 查询待放出的文字 |
| `tst_conversation_feed` | delta 被匀速放出、工具参数立即显示、整个 part 延长已显示的文字时只放出多的部分、不连续的文字立即替换、新 part 等前面的文字放完、回复结束的 Message 不截断文字、reset 丢弃待放出的文字、删除前先放空 |
| `tst_session_controller` | 用假后端：创建并 attach、attach 失败、事件流和 busy 的变化、忽略其他会话的事件、发送不回显、abort、continue、重连后重新 attach、错误和 toast |
| `tst_tool_content` | 工具调用的解析：参数、todo、摘要、read / grep 列表、diff 行、路径拆分、大小文字 |
| `tst_markdown_text` | 图片变链接、标签转义、行内 / 围栏（含未闭合、`~~~`、列表里）/ 缩进代码原样保留 |
| `tst_network_policy` | 组件的网络工厂只放行 qrc 和 data，对着本地 server 计数，确认模型内容不会触发请求 |
| `tst_plain_text` | 来自外部的文字一律按纯文本显示：除白名单（Markdown 块、`ShimmerText`）外 QML 里的 Text / TextEdit 都要声明格式 |
| `tst_command_line` | 目录检查和规范化、`-s` / `-c` / 新建的优先级、`-w`、`--agent`、`--model` 的格式、`--server` 和 `--token`、`--mock`、帮助和版本、隐藏的 `--smoke` |
| `tst_mock_session` | 对着真实的 `ref-server --mock`：markdown 和代码块流式显示、工具调用、等待授权后允许或拒绝 |
| `tst_app_smoke` | 整个程序（offscreen）：`--mock`、显式 `--server`、错误的 token、不存在的会话、自动拉起后端并在退出后自动消失、帮助和版本、错误参数；要求 stderr 里没有 QML 警告 |

`tst_mock_session` 和 `tst_app_smoke` 只在配置时提供了 `-DREF_SERVER=<path>/ref-server`（`cargo build -p ref-server` 的产物）才会注册。它们不调用任何模型，自己启动的进程自己结束，状态和数据目录都用临时目录，不会碰用户正在运行的后端。`tests/fake_backend.h` 是一个跑在 loopback WebSocket 上的假后端，其余测试用它脚本化服务器的行为。

QML 部分没有自动化测试：`tst_app_smoke` 只检查界面能加载、不报警告。动画和观感靠人工运行（可以用 `ref-gui --mock`，在输入框里发 `help` 看有哪些脚本，比如 `tools`、`permission`、`markdown`、`long`、`fail`）来检查。

## 8. 已知限制

- 对话在后端断开期间保持原样，恢复连接后用快照整段替换。
- MarkdownBlock 每来一段增量就整段重新解析 Markdown，超长正文的性能还没测过。
- 超过 `retainedDistance`（约 200 行）的长会话里，视口外的行会被销毁，滚动条长度按平均行高估算，拖动滚动条时可能有轻微的跳动。
- 没有专门规则的工具，卡片头部的 `key=value` 摘要按键名的字母顺序取前 3 个简单参数；终端客户端按服务器 JSON 里的键顺序取。协议层把参数解析成 `QJsonObject`，它会给键排序，所以顺序丢了。
- bash 退出码非 0 时卡片头部的状态仍是 done（退出码只在正文标签里显示）。
- 末尾的脉冲点依赖 `positionToRectangle`，Markdown 以列表或表格结尾时位置可能偏。
- Windows 上的拉起锁、`CREATE_NO_WINDOW` 拉起和图标渲染没有在 Windows 上验证过。
- 如果 ref-gui 在拉起后端之后、连上之前就退出，后端会留着（它的空闲超时只在第一次连接之后才生效）。TUI 有同样的限制。
