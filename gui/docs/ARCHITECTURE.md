# qutie 架构与实现

这份文档帮你读懂当前代码：它是怎么分层的，各部分为什么这样写。建议按章节顺序读，每一章都对应一组源文件。

## 1. 分层

```
┌─────────────────────────────────────────────────────────────────┐
│ src/app  QML 界面 + ChatSession                                  │
│   Main ─ ChatView ─ {User,Markdown,Code,Reasoning,Tool,Error}Block│
│        └ Composer, ThemeToggle, 回到最新按钮                      │
│   Theme / Motion (单例)        SystemFonts (C++ 单例)             │
├──────────────────────── ChatSession ─────────────────────────────┤
│ src/ui   与界面框架无关的视图模型（不依赖 Gui，可以单测）         │
│   ConversationModel ◄── Driver: MockDriver (--mock) | AgentDriver │
│        └ FenceSplitter                                           │
├──────────────────────────────────────────────────────────────────┤
│ src/llm  和模型对话                                              │
│   config（读 endpoint）  OpenAiClient ─ openai_wire（纯函数）     │
├──────────────────────────────────────────────────────────────────┤
│ src/infra  通用基础设施                                          │
│   jsonc（配置解析）   sse（流式响应解析）  http_stream（流式 POST）│
└──────────────────────────────────────────────────────────────────┘
```

规则如下：

- QML 只能看到 `ChatSession`。它暴露了 `blocks`（类型是 `QAbstractItemModel*`）、`busy`、`send()`、`stop()`。
- `src/ui` 里的类型不注册给 QML，所以它们的接口改了也不会波及界面。
- 每一层只调用紧挨着的下一层：AgentDriver 不直接碰网络，OpenAiClient 不碰 model。
- 没有全局状态。每个 ChatSession 持有自己的 driver，driver 持有自己的 client，client 持有自己的 `QNetworkAccessManager`。以后做多 tab，就是多建几个 ChatSession。

## 2. 构建

- 根目录的 `CMakeLists.txt` 要求 C++23（因为用到 `std::expected`）和 Qt 6.10。
- `cmake/CompilerOptions.cmake` 里的 `qutie_configure_target()` 会套用到我们自己的每个 target 上，内容是：警告全开并当作错误（`-Werror`），打开 Qt strict mode（关掉 6.10 之前已废弃的 API），以及 `QT_NO_CAST_FROM_ASCII`（任何 `char*` 转 `QString` 都必须写明编码）。
- 库只依赖它真正用到的东西：`qutie_infra` 依赖 Core、Network 和 QCoro::Core；`qutie_llm` 依赖 infra；`qutie_ui` 依赖 llm。只有可执行文件才依赖 Gui、Qml、Quick。
- QCoro 用的是系统包（Arch 上是 0.13），`find_package(QCoro6 COMPONENTS Core)`。
- `src/app` 是一个 QML 模块（URI 为 `Qutie`）。`Theme.qml` 和 `Motion.qml` 通过源文件属性 `QT_QML_SINGLETON_TYPE` 声明成单例，这个属性必须在 `qt_add_qml_module()` 之前设置。
- 代码块高亮用的 `org.kde.syntaxhighlighting` 是运行时加载的 QML 插件，CMake 里只用 `find_package` 确认它已安装，不链接它。

## 3. infra

### jsonc

`parse()` 把 JSONC 转成 `QJsonDocument`。做法是在原字节上把注释和尾逗号覆写成空格，再交给 Qt 解析。字节偏移不变，所以 Qt 报错时给出的位置可以直接对应回源文件的行和列。

核心是一个单遍状态机 `lowerToJson()`，状态转换图写在源文件开头。有两个细节：

- 尾逗号的判断：遇到逗号时先记下它的位置（`pendingComma`），等看到下一个 token 再决定。如果下一个是 `]` 或 `}`，就把这个逗号抹掉。
- 紧跟在 `[`、`{`、`,` 后面的逗号不会被抹掉，这样 `[,]`、`[1,,]` 这类本来就非法的写法仍然由 Qt 报错。

### sse

`sse::Parser::feed()` 是一个增量解析器，每喂进一段网络数据，就返回这段数据里凑齐了的事件。实现遵循 WHATWG 规范：

- 全程处理原始字节，不做解码。分隔符都是 ASCII，被拆开的 UTF-8 字符会在行缓冲里重新拼完整。
- 一个 CRLF 被拆在两段数据之间时，用 `m_previousLineEndedWithCr` 记住前一段末尾的 CR，避免把后面的 LF 当成一个空行，否则事件会被提前派发。
- 文件开头的 BOM 只在第一行检查。`id` 和 `retry` 字段直接忽略，它们只在浏览器的 EventSource 里用于断线重连。

### http_stream

`http::postStreaming()` 是一个 QCoro 协程：发出 POST，响应体到一段就交给 `onChunk` 一段。状态码不是 2xx 时，响应体不交给回调，而是收集起来作为错误信息返回（`Error{status, message}`）。`status` 为 0 表示传输本身失败，比如连不上、超时、被取消。

取消用 `std::stop_token`：请求停止时调用 `reply->abort()`。这里有一个关键点：

```
request_stop() → reply->abort() → 同步发出 finished() → Finished awaiter 直接恢复协程
               → postStreaming 结束 → OpenAiClient::complete 结束 → AgentDriver::reply 结束
               → request_stop() 返回
```

也就是说，`request_stop()` 返回时，整条协程链都已经跑完了。QCoro 自带的 `co_await reply` 是通过 queued connection 恢复协程的，要等到下一轮事件循环才会继续执行。如果 driver 在析构函数里调用 stop，协程恢复时 driver 已经没了。所以这里自己写了一个 `Finished` awaiter，直接在 `finished()` 信号发出时恢复协程。也正因为恢复发生在 reply 自己的信号发射过程中，reply 只能用 `deleteLater` 释放。

## 4. llm：和模型对话

### openai_wire

这个模块只有纯函数和一个解码器，没有 I/O，所以可以直接拿录好的 SSE 数据来测。

- `requestBody()`：把 history 拼成请求 JSON。推理内容会回传给模型，用的是它到达时的字段名（`reasoning_content`、`reasoning` 等都不在 OpenAI 的 schema 里，各家服务端各取各的名字）。没有正文的 assistant 消息不发，因为服务端会拒绝 content 为空的 assistant 消息。
- `StreamDecoder::decode()`：每次喂一个 SSE 事件的 data，返回其中的增量，同时把完整的消息拼起来。一旦知道了结果（收到 finish_reason、流里报了错、或者 data 无法解析），之后的事件全部忽略。

### OpenAiClient

`complete(history, onEvent, stop)` 把 http_stream、sse::Parser 和 StreamDecoder 串起来，返回 `Completion`：

```cpp
struct Completion {
	AssistantMessage message;              // 收到多少算多少，出错或取消时也有
	std::expected<StopReason, Error> end;  // 正常结束的原因（含 Cancelled），或者错误
};
```

判定回复怎么结束的优先级：流里明确说了的（finish_reason 或 error）优先；其次看是否被取消；再看传输是否失败；如果流正常结束但从没收到 finish_reason，也算错误，因为回复不完整。

### config

`loadEndpoint(path)` 读取 `config.jsonc` 里的 `endpoint`，返回 `Endpoint` 或者一段给用户看的错误文字。文件不存在时，错误文字里会附上一个配置示例。

```jsonc
{
  "endpoint": {
    "baseUrl": "http://localhost:8080/v1",
    "model": "qwen3-coder",
    "apiKey": "sk-..." // 可选；暂时明文
  }
}
```

## 5. ui：会话模型

### 一行是一个块，不是一条消息

```
user        "Fix the build"
reasoning   "The error says ..."
markdown    "The QML module is missing a file:"
code        "qt_add_qml_module(...)"   language = "cmake"
tool        "bash ctest"               output, failed
error       "The request failed: ..."
```

为什么拆成块：

1. 代码高亮需要单独一个 TextEdit。`SyntaxHighlighter` 要挂在一个 TextEdit 上，而且整个文档只能用一种语言。
2. 动效以块为单位：新块入场，旧块保持不动；流式输出时只有最后一个块在变。
3. 不同类型的块交互不同：思考过程可以折叠，工具调用可以展开输出，代码块可以复制。

### ConversationModel

写入接口（driver 调用这些）：

```cpp
appendUserMessage(text)
appendReasoning(delta)        // 和下面几个调用可以任意交错
appendAssistantText(delta)    // 内部交给 FenceSplitter 切成 markdown / code 块
beginToolCall(title)
endToolCall(succeeded, output)
endAssistantTurn()
appendError(message)          // 一轮结束后追加，说明这一轮为什么失败或被截断
```

几条不变量：

- 同一时刻最多只有一个块在接收内容（`m_streamingRow`），它的 `streaming` role 为 true。
- 只用 `rowsInserted` 和带 role 列表的 `dataChanged` 通知变化，从来不 reset。一旦 reset，ListView 会重建所有行，入场动画就全部失效了。
- 在思考、正文、工具调用之间切换时，会调用 `endTextSegment()`：先让 splitter 把扣住的最后一行放出来，再把它重置。所以代码围栏不会跨过一次工具调用。

### FenceSplitter

它把流式到达的 Markdown 切成三种事件：`CodeOpened(language)`、`CodeClosed` 和 `TextAppended(text)`，规则按 CommonMark 的围栏代码块：

- 围栏是至少 3 个连续的 ``` ` ``` 或 `~`，前面最多缩进 3 个空格。
- info string 的第一个词是语言名。
- 闭合围栏必须用同一种字符，长度不少于开头那一行，后面只能跟空白。

流式处理的难点是：一行没结束之前，不知道它是不是围栏。处理方式如下：

- 一行一旦可以确定不是围栏（比如第一个非空字符是字母），就立刻放出，不必等到行尾。
- 有可能成为围栏的行先扣住（`m_heldLine`），等这一行结束再判断。
- 换行符延迟放出（`m_pendingLineBreaks`），等到后面确实还有内容时才补上。这样块的末尾不会挂着分隔用的空行。

测试里每个用例都会跑两遍，一遍整段输入，一遍逐字符输入，两种方式的结果必须完全一致。

### MockDriver

它用一个 `QTimer` 按 tick 回放脚本，每个 tick 输出一小段随机长度的文字。每个脚本自带名字和速度（`Pace`）。在输入框里发脚本名就会播放对应脚本，发别的内容则按顺序轮换：

| 脚本 | 覆盖的内容 |
|---|---|
| `code` | 思考过程、工具调用、两个代码块、链接 |
| `tools` | 表格、失败的工具调用、Python 代码、有序列表 |
| `zh` | 中文排版、引用、粗体 |
| `slow` | 约 4 tok/s 的输出速度，首个 token 前等待 2.5 秒 |
| `long` | 多段思考过程、41 行的工具输出、约 60 行的代码、两张表格 |
| `error` | 输出一半后断开，追加 error 块 |

### AgentDriver

真实后端的 driver，实现了和 MockDriver 相同的 `Driver` 接口。

- 持有对话的 history。用户发一条，就追加一条 `UserMessage`；回复结束（不管成功、出错还是被取消），就把收到的那部分 `AssistantMessage` 追加进去。所以下一轮模型看到的内容，和用户在屏幕上看到的一致。
- 配置在发第一条消息时读取。读取失败就显示一个 error 块，下一条消息再读一次，用户改好配置文件后不用重启。
- 增量不是一到就写进 model，而是先交给 `StreamPacer`，由一个 16ms 的 ticker 每帧取出该显示的部分写入。真实服务端常常隔一两百毫秒发一大段，直接写进去就是一段一段蹦出来。pacer 根据最近几次到达的间隔估计下一段什么时候到，把积压的文字在这段时间里匀速放完，所以文字以稳定的速度流出，最多比网络慢 200ms。切分按字素边界，不会拆开 emoji。
- 流结束后，要等 pacer 放空才结束这一轮，然后才追加 error 块。用户按停止时，剩下的文字一次全部放出，屏幕上看到的和 history 保持一致。
- 析构时如果回复还在进行，就先置 `m_destroying` 再请求停止。按 http_stream 一节说的，停止是同步完成的，`reply()` 看到这个标志后就不再写 model。

## 6. app：界面

### ChatSession 与 SystemFonts

- `ChatSession` 是 QML 唯一能直接访问的业务对象，它持有 model 和 driver。它在 `main()` 里创建（根据 `--mock` 参数选择 driver），再通过 `setInitialProperties` 交给 `Main.qml` 的 `required property ChatSession session`。QML 不能自己创建 ChatSession（`QML_UNCREATABLE`）。
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
- `cacheBuffer` 设得很大，所有行常驻。ListView 默认会销毁屏幕外的行，并按平均行高估算它们的高度；高的代码块一进出视口，估算值就会变，滚动条长度跟着变，拖动滚动条时内容来回跳。

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
| `UserBlock.qml` | 气泡宽度贴合文字；从第二轮开始，上方加一条分割线，新发送的消息分割线从左往右展开（`added()`） |
| `MarkdownBlock.qml` | `TextEdit.MarkdownText` 渲染；流式输出时末尾有一个跟着走的脉冲点（`StreamCaret`） |
| `CodeBlock.qml` | PlainText 加 `SyntaxHighlighter`；语言名匹配不到时按纯文本显示；复制按钮 |
| `ReasoningBlock.qml` | 输出时自动展开，结束后自动折叠，也可以手动点击开合；"Thinking" 字样在输出时呼吸闪烁 |
| 吸顶标题 | 思考块和工具块展开时，标题行贴在视口顶部，直到整个块滚出去（`Block.pinOffset()`）。吸顶时点击折叠，视图会跟着块变矮往回滚，标题停在原处不动 |
| `ToolBlock.qml` | 状态点（`StatusDot.qml`：运行中脉冲，完成后变成绿色或红色，弹性回弹并扩散一圈圆环）；有输出时可以展开查看 |
| `ErrorBlock.qml` | 浅红底卡片，左边一个"!"圆标（`AlertBadge.qml`）。出错很少见，但一定要让用户注意到：卡片以中性底色入场，落定后圆标用 `Flourish` 弹出并扩散一圈红色圆环，同时错误底色以硬边圆形从圆标处匀速扩散，盖满卡片即结束。动作都从一个点向外扩散，和工具状态点同一种语言；回弹只发生在圆标周围几个像素。用户正往上翻时出错，回到最新按钮的右上角会弹出一个红色"!"徽标，按钮消失时清掉 |
| `Flourish.qml` | "发生了"的局部点缀：`play()` 让已显示的目标收缩后用 `OutElastic` 弹回并扩散圆环；`enter()` 用于新出现的目标，从 40% 大小淡入并同时弹开（从 0 放大会像一个膨胀的点）。工具状态点、错误圆标、回到最新按钮的徽标共用 |
| `Pulse.qml` | 上面三处"进行中"闪烁共用的组件。停止时从当前透明度直接缓动回 1，不跑完当前周期；用 `alwaysRunToEnd` 的话，结束后最多还会再闪一次。周期由 `period` 设置，默认 `Motion.pulse`，Thinking 标题用更慢的 `Motion.calmPulse` |

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
- 滚动条的 handle 自绘，颜色取自 Theme。Basic 风格默认用系统 palette，不跟随主题。
- 输入框高度随内容增长，最大 220px。

## 7. 测试

| 测试 | 覆盖 |
|---|---|
| `tst_jsonc` | 各种扩展语法、该报错的非法输入、报错位置的行列号 |
| `tst_sse` | 各种换行符、跨 chunk 的 CRLF 和 BOM、注释、多行 data、逐字节拆分 |
| `tst_config` | 正常配置、apiKey 可省略、缺字段、语法错误、非 http 地址、文件不存在 |
| `tst_openai_wire` | 请求 JSON、推理内容按原字段回传、跳过空 assistant 消息；解码正文和推理、重复字段只取一个、finish_reason 映射、流中报错、无法解析的 data |
| `tst_openai_client` | 用假服务端：正常流、请求头和路径、逐字节拆分（含 UTF-8）、HTTP 500、连接中途断开、连不上、取消同步完成 |
| `tst_fence_splitter` | 15 个用例，每个都分别按整段和逐字符各跑一遍；正文立即放出；可能是围栏的行先扣住 |
| `tst_conversation_model` | 块的切分、交错输出、同时只有一个块在 streaming、更新过程中不 reset（配合 `QAbstractItemModelTester`）、error 块 |
| `tst_stream_pacer` | 首段按最大延迟摊开、成段到达时每帧放出量稳定、逐 token 的流几乎不加延迟、类型顺序、不拆字素、空闲后不攒额度、takeAll |
| `tst_agent_driver` | 用假服务端：回复写进 model、缺配置、中途失败、取消后部分回复留在 history 里、回复进行中销毁 driver |

`tests/fake_server.h` 是一个跑在 `QTcpServer` 上的最小 HTTP/1.1 服务端，用 chunked 编码一段一段回放响应，结尾可以选正常结束（Complete）、中途断开（Drop）或一直挂着（Hold）。

QML 部分没有自动化测试，目前靠人工运行 mock 脚本来检查。

## 8. 已知限制

- MarkdownBlock 每来一段增量就整段重新解析 Markdown，超长正文的性能还没测过。
- 所有行常驻内存，超长会话的开销还没测过；到做会话持久化时再评估。
- 末尾的脉冲点依赖 `positionToRectangle`，Markdown 以列表或表格结尾时位置可能偏。
