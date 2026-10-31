# HANDOFF — 给下一位 agent 的工作上下文

> **过时（qutie 时期）。** `llm`、`infra` 和 `ui/agent_driver` 已被 `proto`、`client`、`session` 取代，ref-gui 现在是 ref-server 的客户端。当前架构见 ARCHITECTURE.md，这份文件只保留当时的上下文。

用途：用户读完代码回来继续工作时，把本文件作为上下文。详细度对标一次上下文压缩。
最后更新：M1 后端写完，等待首次编译。

---

## 0. 工作方式（必读）

- 有 Arch 沙箱可以编译和跑测试（无显示器，用 `QT_QPA_PLATFORM=offscreen`）。工作区在沙箱里只读，而 Qt 会往源码目录写
  `src/app/.qmlls.ini`，所以先 `rsync -a --delete --exclude /build --exclude .git /work/qutie/repo/ /work/.shared/qutie-m1/src/`，
  再在 `/work/.shared/qutie-m1/build{,-clang,-asan}` 里构建。界面效果要用户在本机运行确认。用户本机的常用命令：
  `cmake --build build && ctest --test-dir build --output-on-failure && ./build/src/app/qutie`
- 用户会逐行人工 review 代码，可读性优先，diff 尽量小。
- 先读 `docs/DESIGN.md`，里面是已定决策和进度；做出新决定或完成一步后要更新它。
- 查资料先看 `llmreferences/`（先读其中的 `AGENTS.md`），里面有 qtbase、qtdeclarative、qcoro、syntax-highlighting、pi、openai-openapi 等的源码和文档。不要修改这个目录，搜索业务代码时也不要扫进去。
- 用户使用中文。界面上的文案和代码注释用英文。
- 跨层改动、新模块、改 API 之前，先给出接口设计，等用户确认后再写实现。
- 修 bug 时先说出你认为的失败原因和复现方式，再改；改的是根因，不是在各个调用处层层加防御。
- 视觉效果由用户自己运行确认，不要在沙箱里录屏、抽帧来检查动画（用户明确要求，浪费时间和 token）。沙箱只做编译和测试。
- 用户审美偏好：动效是这个产品的特色。高频交互一律用指数曲线、不要回弹；回弹只用在低频场景，或者用在不影响其他元素的局部点缀上（用户的原话是工具状态点"虽然高频但是动画不影响其他东西"，所以可以更灵动）。用户对动效细节很敏感，会截图逐帧指出问题。

## 1. 项目概况

- qutie：跨平台 GUI coding agent。C++23 + Qt Quick (QML)，CMake + Ninja，GPL-3.0-or-later（SPDX 写 `GPL-3.0-or-later`）。
- 环境：Arch Linux + KDE，依赖全部用系统包，不用 vcpkg。Qt 版本下限 6.10（由 KSyntaxHighlighting 决定），实际装的是 6.10 或更新。
- M1 在沙箱里（Qt 6.12、QCoro 0.13、GCC 16 / Clang 23）`-Werror` 下编译通过，8 个测试全过，ASan+UBSan 下也全过。
  `qutie` 和 `qutie --mock` 在 offscreen 下能启动、无 QML 报错。还没对接过真实服务端。

## 2. 已定决策（包括被否掉的方案）

- 依赖：QCoro（异步）、KSyntaxHighlighting（用它的 QML `SyntaxHighlighter`）、QtKeychain、dtl（BSD-3，做 diff，计划放到 `3rdparty/dtl/`）。Markdown 用 Qt 自带的 md4c，grep 调用外部的 `rg`。一个库要等代码里真正用到时才写 `find_package`；目前 find 了 Qt6、QCoro6 Core 和 KF6SyntaxHighlighting。
- 不引入 toml++、valijson、nlohmann、RapidJSON。JSONC 的处理方式：把注释和尾逗号原地覆写成空格，字节偏移不变，然后交给 `QJsonDocument`。不去改 Qt 的私有解析器。
- 配置文件 `~/.config/qutie/config.jsonc`（准确地说是 `$XDG_CONFIG_HOME/qutie/config.jsonc`），程序只读不写；格式见第 8 节。程序状态存 `state.json`（放在 AppDataLocation），还没实现。
- 第一个接入的 provider 是 OpenAI 兼容的 Chat Completions。
- 先做前端，再做后端（用户的决定）。UI 现在由 MockDriver 驱动，以后换成 agent 时 QML 不用改。
- 窗口用系统原生边框，不自绘。
- 视觉：支持深色和浅色，默认跟随系统，用户点切换后改由用户决定。所有内容左对齐，只有用户消息有气泡（带 accent 色调）；从第二轮起，每条用户消息上方有一条贯穿阅读栏的分割线。
- 动效：时长和缓动全部放在 `Motion.qml`。高频交互用 `OutExpo`；`OutBack` 只用在主题切换按钮的形变。工具结束时状态点的动画在 `StatusDot.qml`：先收缩到 0.6，再用 `OutElastic` 弹回、摆动几下，同时扩散出一圈完成颜色的圆环。参数集中在这个文件顶部。
- 新块入场：淡入，同时从目标位置**上方** `Motion.enterDrop` 像素落到位。用户自己调到了 48。

## 3. 代码地图

```
CMakeLists.txt              C++23, find_package(Qt6 6.10 Core Gui Qml Quick Test), qt_standard_project_setup
cmake/CompilerOptions.cmake qutie_configure_target(): 警告全开 + -Werror, QT_ENABLE_STRICT_MODE_UP_TO=0x060A00,
                            QT_NO_CAST_FROM_ASCII, 选项 QUTIE_SANITIZE
.clang-format / .clang-tidy 用户提供；tab 缩进, Allman, 列宽 140, `if(` 无空格, PointerAlignment Left
src/infra/   (qutie_infra: Qt6::Core, Qt6::Network, QCoro::Core)
  jsonc.{h,cpp}             parse(QByteArrayView) -> std::expected<QJsonDocument, ParseError{line,column,message}>
                            单遍状态机 lowerToJson(): Code/String/StringEscape/LineComment/BlockComment
  sse.{h,cpp}               sse::Parser::feed(QByteArrayView) -> QList<Event{type,data}>; WHATWG 规则, 纯字节,
                            CRLF 跨 chunk, BOM, 忽略 id/retry
  http_stream.{h,cpp}       http::postStreaming(): QCoro 协程, 2xx 的 body 分块回调, 非 2xx 收集成 Error{status,message};
                            stop_token -> reply->abort(), 自定义 awaiter 让 abort 时同步完成 (坑 15)
src/llm/     (qutie_llm: 依赖 qutie_infra)
  message.h                 UserMessage / AssistantMessage{text,reasoning,reasoningField} / StreamEvent / StopReason /
                            Completion{message, expected<StopReason,Error> end} / Endpoint
  openai_wire.{h,cpp}       纯函数: requestBody(), errorMessage(), StreamDecoder (SSE data -> delta + outcome)
  openai_client.{h,cpp}     OpenAiClient: 持有 Endpoint 和 QNetworkAccessManager; complete() 拼起 http + sse + decoder
  config.{h,cpp}            configPath() / loadEndpoint() / parseEndpoint(); 错误是给用户看的文字
src/ui/      (qutie_ui: Qt6::Core + qutie_llm; QML 看不到这一层的类型)
  fence_splitter.{h,cpp}    流式 Markdown -> CodeOpened / CodeClosed / TextAppended 事件, CommonMark 围栏规则
  conversation_model.{h,cpp} QAbstractListModel, 一行 = 一个块 (user/reasoning/markdown/code/tool/error)
  driver.h                  Driver 接口 (QObject): respond / stop / isRunning / runningChanged
  mock_driver.{h,cpp}       脚本回放, QTimer 驱动; 脚本: code, tools, zh, slow, long, error
  stream_pacer.{h,cpp}      把成段到达的 delta 匀速放出 (见坑 17); 纯逻辑, 时间由调用方传入
  agent_driver.{h,cpp}      真实后端: 持有 history 和 OpenAiClient; delta 经 StreamPacer, 16ms ticker 取出写 model;
                            流结束且 pacer 放空后才 endTurn (再追加 error 块); stop() 立即放出剩余
src/app/     (可执行文件 qutie + QML 模块 URI Qutie)
  main.cpp                  --mock 参数; ChatSession 在 C++ 创建, 通过 setInitialProperties 交给 Main.session
  chat_session.{h,cpp}      QML_UNCREATABLE; blocks / busy / send() / stop(); 持有 model 和 unique_ptr<Driver>
  system_fonts.h            QML_SINGLETON; fixed = QFontDatabase::systemFont(FixedFont)
  Theme.qml / Motion.qml    单例 (CMake 里设了 QT_QML_SINGLETON_TYPE)
  Main.qml                  窗口, 主题快照交叉淡化, 回到最新按钮, Composer
  ChatView.qml              ListView: 跟随, 保持, 滚轮追赶, 主题化滚动条, DelegateChooser
  Chevron.qml               绘制的 ">" 图标
  Pulse.qml                 "进行中"的呼吸闪烁; 停止时从当前透明度缓动回 1, 不跑完当前周期
                            (StatusDot / ReasoningBlock 标题 / StreamCaret 共用)
  Block.qml                 所有行的基类: 居中阅读栏 + 流式高度逐帧追赶
  UserBlock / MarkdownBlock / CodeBlock / ReasoningBlock / ToolBlock / ErrorBlock.qml
  Flourish.qml              "发生了"点缀, 两种: play() 对已显示的目标, 收缩到 0.6 后 OutElastic 弹回 + 扩散圆环;
                            enter() 对新出现的目标, 从 0.4 淡入并同时弹开 (用户: 从 0 放大看着怪, 入场要用淡入)
                            (StatusDot 完成、ErrorBlock 徽标、回到最新按钮的警示徽标共用)
  AlertBadge.qml            绘制的 "!" 圆标, 按尺寸缩放
  ProseText / StreamCaret / StatusDot / Composer / SendButton / TextButton / CircleButton / ThemeToggle.qml
tests/  tst_jsonc, tst_sse, tst_config, tst_openai_wire, tst_openai_client, tst_fence_splitter,
        tst_conversation_model, tst_agent_driver (QTest, QTEST_GUILESS_MAIN)
        fake_server.h: QTcpServer 上的假 HTTP/1.1 服务端, chunked 回放, 结尾可选 Complete / Drop / Hold
docs/   DESIGN.md (决策 + 进度), HANDOFF.md (本文件), ARCHITECTURE.md (写给用户看的)
```

## 4. 数据流

```
用户输入 -> Composer.submitted -> ChatSession.send(text)
         -> ConversationModel.appendUserMessage(text)
         -> Driver.respond(text)
            MockDriver: 输入是脚本名就播放该脚本, 否则按顺序轮换; 每个 tick 写 model
            AgentDriver: history += UserMessage; 首次读配置 (失败 -> error 块, 下次再读)
              -> OpenAiClient.complete(history, onEvent, stop)
                 -> http::postStreaming -> sse::Parser -> openai::StreamDecoder -> onEvent(delta)
              -> delta 进 m_pending, 16ms 后 flush 成 appendReasoning / appendAssistantText
              -> 完成: history += 部分或完整的 AssistantMessage; endAssistantTurn; 需要时 appendError
         -> runningChanged -> ChatSession.busyChanged
ConversationModel: appendAssistantText -> FenceSplitter.feed -> 事件 -> 新建或追加块
  每次切换 reasoning / text / tool 时调用 endTextSegment()(先 splitter.finish(), 再重置), 所以围栏不会跨越工具调用
  同一时刻最多只有一个块处于 streaming 状态 (m_streamingRow); 只发 rowsInserted 和 dataChanged(指定 role), 从不 reset
```

## 5. 关键实现细节和踩过的坑（不要再踩）

1. 入场动画不用 ListView 的 `add` Transition。Transition 会把行固定在插入那一刻算出的目标位置（`qquickitemviewtransition.cpp:336-343`，过渡期间 `moveTo` 只记录 `nextTransitionTo`），等动画结束才跳到真实位置。thinking 折叠后紧接着插入的 tool 块，会先停在折叠前的 y 上，然后瞬移上去。现在的做法：`Block` 在 `ListView.onAdd` 里给自己的 `Translate` 和 opacity 做动画，`y` 仍由 ListView 实时布局。附带一个坑：用 Transition 时 `ViewTransition` 必须通过 Transition 的 id 读取，直接写会拿到空的附加对象。子类需要知道"新插入"时，用 `Block.added()` 信号（UserBlock 的分割线就是这样）。
2. `cacheBuffer: 1e7`，所有行常驻。如果不这样做，ListView 会用平均行高估算屏幕外的行（`qquicklistview.cpp:491-500`），高的代码块进出视口时，`contentHeight`/`originY` 会跳，表现是滚动条长度变化、拖动滚动条时来回跳。代价是会话很长时内存和创建开销会增加，到 M4 有会话持久化时需要重新评估（虚拟化或分页）。`following` 仍然只由用户手势改变（`movementStarted` 时置 false，`movementEnded` 时设为 `atYEnd`，拖动滚动条、滚轮同理），底部位置取 `footerItem.y`。
3. `positionViewAtEnd()` 不计入 `bottomMargin`，所以底部留白用 footer 来做。
4. 目标会连续变化的场景，不要用 `Behavior`：目标一变它就停掉并重启动画（`qquickbehavior.cpp:383-389`），新动画要等到下一帧才开始推进。目标每帧都变时，高度会一直停在原地（long 脚本里 thinking 块的卡顿就是这个原因）。这类场景改用 `FrameAnimation` 逐帧追赶，公式统一放在 `Motion.chase(current, target, frameTime)`。目前有三处用到：跟随滚动、滚轮滚动、`Block` 的流式高度。`Block` 用 `Motion.arrived()` 判断到达（误差小于 0.5px）。两处滚动追赶则走 `ChatView.scrollStep()`/`scrollArrived()`，按整数像素前进，误差小于 1px 算到达，原因见 5b。不要用 `!==` 判断到达：Flickable 对 fuzzy 相等的 contentY 写入会直接忽略（`qquickflickable.cpp:879`）。
4b. 鼠标滚轮：Flickable 的默认处理是每一格滚轮都从当前位置重新开始一段 `OutExpo` 动画（`qquickflickable.cpp:1811-1813`），上一格剩下没走完的距离会被丢掉，所以滚得越快反而走得越少。现在由 `WheelHandler` 接管所有滚轮事件：滚轮的每一格累加到 `wheelTarget` 上，contentY 逐帧追赶它；触控板事件（带 scroll phase）则把 pixelDelta 直接加到 contentY 上。两者都走 `ChatView.scrollTo()`，滚到底时切换为 `following`。每格距离沿用 Flickable 的 `wheelScrollLines * 24`。**不能按设备类型区分滚轮和触控板**：在 Wayland 上，只要 compositor 支持 pointer gestures，Qt 就会把整个 seat 的指针设备报告成一个名为 "touchpad" 的 TouchPad 设备（`qwaylandinputdevice.cpp:421`、`qwaylandwindow.cpp:1249`），连真实的鼠标滚轮也算在内。`WheelHandler` 默认只接受 Mouse 类型的设备，于是滚轮事件会漏给 Flickable（用户的日志确认了这一点）。现在改为按 `phase` 区分，和 Flickable 内部的判断方式一致（`qquickflickable.cpp:1771`）。Unverified：接管之后，Flickable 在触控板松手时可能有的惯性滑动是否丢失，要看实际手感（Wayland 本身不发送 ScrollMomentum）。
5a. `holdEnd()` 不可重入。footer 高度一变，ListView 会同步重新放置 footer（`qquickitemview.cpp:1299-1304` → `updateFooter`），用的是行的当前高度。如果这些高度在上次布局之后又变了，footer 会在 `heldSpace` 还在写入的过程中再次移动，`onNaturalEndChanged` 重入，footer 的 height 绑定就会报 binding loop。现在的处理：重入时只置 `holdStale`，由外层循环再算一遍。footer 高度本身不影响 footer 位置，所以第二遍就会稳定。具体是 Qt 的哪条调用路径触发了重入，没有完全确认（Unverified）。
5b. 滚动追赶按整数像素走（`ChatView.scrollStep()`/`scrollArrived()`）。ListView 每次 layout 都会调 `fixupPosition()`（`qquickitemview.cpp:2099-2100`），而 Flickable 的 fixup 会把落在范围内的小数 contentY 取整（`qquickflickable.cpp:537-549`）。thinking 折叠期间行高每帧都变，每帧都要重新布局，追赶每帧前进的那一点小数会被取整吃掉。在 120Hz 下，剩 4px 时每帧只走约 0.46px，结果 contentY 停在离目标 4px 的地方。等行高稳定、不再布局后，追赶才把这 4px 走完，就是用户看到的"贴完之后又往下滚一小段"。用户的日志（`cy` 在 chase 写入的 764.5 和 fixup 写回的 764.0 之间来回、持续约 30 帧）和逐帧录屏都证实了这一点。修法：每步取整，并且每帧至少走 1px；误差小于 1px 就算到达。滚轮追赶也走同一套逻辑。另外试过一种无效的做法（先加大 `heldSpace` 再 `forceLayout()`），已回退。
5. 防回跳（"保持"）：回复进行中、并且处于跟随状态时，记录底部到过的最远位置 `highWater`；内容变矮让出来的高度记为 `heldSpace`，加到 footer 上。这一轮结束后，用 `Motion.slow` 时长的 `OutExpo` 把 `heldSpace` 收回到 0。只在跟随时更新 `highWater`，因为用户往上翻时 `naturalEnd` 只是估算值。重新开始跟随或开始新一轮时，`highWater = heldEnd`。跟随的目标位置被限制在 `originY + contentHeight` 以内，否则 Flickable 会把 contentY 夹住，追赶永远到不了目标。
6. `Block` 的高度：块刚创建时 `easedHeight = naturalHeight`。只有 streaming 期间（加上结束后追上真实高度之前）才走追赶，这样 thinking 块输出结束后自动折叠，外层高度也能平滑跟上。
7. 主题切换：对 `page` 调用 `grabToImage`，把截图盖在最上层，然后切换 `Theme.dark`，再把截图淡出。必须持有 `ItemGrabResult`（`themeSnapshot.grab`），否则 `result.url` 会失效。窗口背景色不会被截进图里，所以 page 里垫了一个 Rectangle。ThemeToggle 放在截图上方，保证它自身的形变动画在切换时可见。
8. FenceSplitter：能确定是正文的部分立即放出；可能成为围栏的行先扣住，直到这一行结束再判断。换行符延迟放出，所以块的末尾不会带着分隔用的空行。围栏后面开始正文时，丢掉前导空行；代码块内部的前导空行保留。闭合围栏允许后面跟 `\r`。开始行以反引号做围栏时，info string 里不能有反引号（否则会把行内代码误判成围栏）。
9. MarkdownBlock 每次更新都整段重新解析 Markdown（`TextEdit.MarkdownText`）。CodeBlock 用的是 PlainText 加 `SyntaxHighlighter`，definition 直接用围栏上的语言名，`Repository::definitionForName` 会转成小写并匹配别名；匹配不到就是纯文本。
10. ChatView 设了 `acceptedButtons: Qt.NoButton`，鼠标拖动用来选文字，滚轮、触控板和触摸照常滚动（这个属性 6.9 起才有）。
11. `ConversationModel::Block` 的成员带默认初始化 `{}`，是为了在用指定初始化器省略字段时不触发 Clang 的 `-Wmissing-field-initializers`。
12. Composer：按 Enter 发送，Shift+Enter 换行；`inputMethodComposing` 时 Enter 不发送。
12b. 按钮按下时只缩放内部的 `face`，不缩放带 handler 的根元素（CircleButton、SendButton、TextButton 都是这样）。TapHandler 判断按下和松开是否落在按钮上，用的是父元素的 `contains()`（`qquickpointerhandler.cpp:606`、`qquicktaphandler.cpp:89-91`），而坐标换算会把父元素的 scale 算进去。如果按下时把根元素缩到 0.9，原本落在边缘的松开点就到了按钮外面，于是有按压回弹、却不触发 tapped。根据代码推断，用户的现象符合这个机制，没有在运行时验证（Unverified）。CircleButton 通过 `color` 属性暴露 face 的颜色（ThemeToggle 用它画月牙的缺口），并用 `default property alias content: face.data` 把子元素放进 face 里，这样图标会跟着一起缩放。
13. 图标统一用 `Chevron.qml` 绘制（Canvas，圆角端点，通过旋转改变朝向），不用字符：字符的基线和字体度量会导致垂直方向对不齐。不用 Shape：整个落在祖先裁剪区之外的 Shape 仍会被画出来（所有渲染器都如此），滚出聊天区的卡片的箭头会盖在标题栏上。用在发送按钮、回到最新按钮、思考块和工具块的展开箭头上。
14. 滚动条的 handle 自绘，颜色取 `Theme.scrollHandle`/`scrollHandlePressed`。Basic 风格的默认 handle 用的是系统 palette，不跟随 Theme。回到最新按钮的 x 与 `Composer.sendButtonCenterX` 对齐（尺寸 34，发送按钮 32，用户调过）。
15. 取消必须同步完成。QCoro 对 `QNetworkReply` 的 awaiter 通过 queued connection 恢复协程（`qcoronetworkreply.cpp:69-77`），被 abort 的请求要等下一轮事件循环才会结束；如果 driver 在析构函数里 stop，协程会在 driver 已销毁后恢复，造成 use-after-free。`QNetworkReplyHttpImpl::abort()` 会在返回前同步发出 `finished()`（`qnetworkreplyhttpimpl.cpp:278-298`），所以 `http_stream.cpp` 用自定义的 `Finished` awaiter 直接在这次 emit 里恢复协程。连锁效果：`request_stop()` 返回时，`postStreaming`、`OpenAiClient::complete`、`AgentDriver::reply` 整条链都已经跑完。AgentDriver 析构时先置 `m_destroying`，再 `request_stop()`，`reply()` 恢复后看到这个标志就不再写 model。reply 对象用 `deleteLater` 释放，因为恢复点就在它自己的 signal emit 里面。`stop_callback` 在 `co_await` 之后就销毁，这样回调不会在协程帧之外被触发。这条结论来自读源码，靠 `tst_openai_client::stopCompletesAtOnce` 和 `tst_agent_driver::destroyingMidReplyIsSafe` 验证（Unverified，等用户跑测试）。
17. 流式节奏（StreamPacer）：真实端点常常每隔一两百毫秒发一大段，直接写进 model 就是一段一段蹦出来。pacer 每次收到数据时，
    用最近几次到达间隔的 EMA（限制在 [16ms, 200ms]）估计下一段什么时候到，把释放速率设为"积压 ÷ 间隔"，这样每段刚好在下一段
    到来前放完，流速稳定，最多比流慢 200ms。4ms 内连续到达的事件算同一次网络读取，不计入间隔。没有放出去的额度上限是积压量，
    所以空闲之后来的新段不会一下子全蹦出来。按字素边界切（QTextBoundaryFinder），emoji 不会被拆开。每轮开始时重置 pacer。
    AgentDriver：流结束后不立即 endTurn，由 ticker 等 pacer 放空再结束、再追加 error 块；stop() 同步结束流后用 takeAll()
    把剩余部分一次放出，屏幕和 history 保持一致。StreamCaret 的位置也从 Behavior 改成了逐帧追赶（坑 4）。
18. 错误提示动画（替换掉了原来的左右摇晃，用户认为它和其他动效不搭）：卡片照常入场；落定后徽标用 `Flourish.enter()`
    从 40% 淡入并弹开，扩散红色圆环（从 0 放大被用户否掉）。卡片以中性底色入场；徽标弹开的同时，错误底色以硬边圆形从徽标处匀速扩散，直到盖满卡片（用户的方案：匀速、硬边、
    不羽化）。`spread` 0→1 线性，时长按 `spreadSpeed`（px/ms）由半径换算，卡片宽窄不同速度一致。圆用 RadialGradient 里
    相隔 1px 的两个 stop 画出硬边，填充形状是内缩一个边框宽度的 PathRectangle，所以圆被卡片圆角裁住、边框在上层；
    边框颜色同步过渡。否掉的版本："红色圆盘铺满后整体淡出"（像闪一下）、"渐弱亮带 + 羽化前沿"（用户仍不满意）。用户不在底部时（`!following`），
    ErrorBlock 让 ChatView 发出 `errorOutOfSight()`，回到最新按钮右上角的边缘处弹出一个红色 "!" 徽标（同一个 Flourish）；
    按钮消失时清掉。视觉效果等用户确认（Unverified）。
19. 吸顶标题：ReasoningBlock 和 ToolBlock 展开时，标题行贴在视口顶部，直到块的末尾把它推出去
    （`Block.pinOffset(headerHeight)` = clamp(contentY - y, 0, height - headerHeight)，标题的 y 绑定到它，z 在正文之上，
    下面垫一块底色挡住滚过的正文，吸顶时底边画一条分割线）。ToolBlock 的垫底在卡片边框内，圆角随离卡片顶端/末端的距离变化。
    在吸顶状态下点击折叠：先 `holdHeader()`，折叠动画期间块每变矮一次，就把 contentY 拉回到 `y + height - headerHeight`，
    标题停在原处，下面的行收上来；高度 Behavior 结束时 `releaseHeader()`。不常驻是因为之后的任何高度变化都会把视图拉回去。
16. ChatSession 由 C++ 创建，通过 `setInitialProperties` 传给 `Main.qml` 的 `required property ChatSession session`，所以标成 `QML_UNCREATABLE`。原因：构造时需要知道 `--mock` 参数；而且 session 必须比 engine 活得久（在 `main()` 里先于 engine 声明）。

## 6. 可以调的参数（集中在两个单例里）

- `Motion.qml`：quick 120、standard 260、slow 480、pulse 1100、calmPulse 1800（Thinking 标题用，经 `Pulse.period` 传入），ease 为 OutExpo、emphasisEase 为 OutBack、exitEase 为 InCubic（目前只用于发送按钮从停止切回发送），enterDrop 48（用户设定），followLag 70。
- `Theme.qml`：颜色（dark / light 两套）、syntaxTheme（"Breeze Dark" / "Breeze Light"）、字号 15/12/13、圆角 6/10/18、pagePadding 24、blockSpacing 14、turnGap 18、contentWidth 760。
- `MockDriver::NormalPace` 为 16ms 一次、每次 1–7 个字符（约 60 tok/s）；`SlowPace` 为 150ms 一次、每次 1–4 个字符（约 4 tok/s）。

## 7. 已知风险和未验证事项

- 所有行常驻（`cacheBuffer`）：会话长到几百个块以上时的内存和首次加载开销还没测过。
- MarkdownBlock 每个 delta 都整段重新解析。正文很长时（几千行）可能会卡，还没压测过。
- StreamPacer 的参数（MaxLag 200ms、平滑系数 0.25、BurstWindow 4ms）只在单测里用合成时间验证过，真实端点下的手感等用户确认。
- M1 的协程生命周期（坑 15）：`stopCompletesAtOnce` 和 `destroyingMidReplyIsSafe` 在 ASan+UBSan 下通过。真实服务端下还没验证。
- QCoro 用的是系统包 0.13.0，`find_package(QCoro6 COMPONENTS Core)`。Qt6::Network 只链接到 qutie_infra。
- `StreamCaret` 依赖 `positionToRectangle(length)`。Markdown 末尾如果是列表或表格，位置可能不太对。

## 8. M1 后端（已实现，等首次编译和真实服务端验证）

用户确认的决定：
- API key 先明文写在 config 里（`endpoint.apiKey`，可选），以后有设置界面再改用 QtKeychain。
- 配置文件先用最小格式：`{"endpoint": {"baseUrl", "model", "apiKey"?}}`。`max_tokens` 先不发，以后加进配置。
- 出错时追加一个 `error` 块，动画要醒目（方案见坑 18；最初的"闪红 + 摇晃"已被用户否掉）。
- history 和屏幕保持一致：出错或被取消的回复，已经显示的部分留在 history 里。
- `--mock` 继续用 MockDriver（加了 `error` 脚本）。等后端稳定后可以删掉。
- 推理内容要回传给模型（因为支持多轮对话），用它到达时的字段名回传。空闲超时暂定 5 分钟。

和最初草案相比的改动（都是 M1 用不上的部分，先不做）：
- 工具调用、SystemMessage、ToolResult、Usage 推迟到 M2。`StopReason` 新增 `Cancelled`。
- `complete()` 返回 `Completion{message, expected<StopReason, Error> end}`。不管成功、取消还是出错，都带上已经收到的那部分 message，因为 history 要用它。
- `OpenAiClient` 自己持有 `QNetworkAccessManager`，每个 session 一个，不用全局单例，多 tab 时互不影响。
- 流中途的 `{"error":...}`、无法解析的 data、不认识的 finish_reason，都算错误。流结束了却没有收到 finish_reason，也算错误（"The connection closed before the reply was complete."）。先收到了 finish_reason、之后连接才断的，算成功。
- 每个 chunk 里，推理内容只取 `reasoning_content` / `reasoning` / `reasoning_text` 中第一个非空的字段（有的服务会在两个字段里重复发同一段，pi 也是这么处理的）。
- 回传时，没有正文的 assistant 消息不发（比如推理到一半就被取消的回复），因为服务端会拒绝 content 为空的 assistant 消息。

M2 时要做的：在 AgentDriver 和 llm 之间加 agent loop；工具调用分片按 `index` 拼接，`id`/`name` 可能晚到（schema 只要求 `index`，见 `openapi.yaml:41214-41242`）；加 faux provider 时再抽 Provider 接口。

## 9. 里程碑总表

M0 骨架（完成）→ UI v1（完成）→ M1 SSE + OpenAI 客户端 → M2 agent loop + 只读工具 + faux provider → M3 写工具、bash、权限 → M4 会话 JSONL 持久化 + 会话列表左栏 → M5 GUI 打磨 → M6 Anthropic + 上下文压缩 → M7 MCP → M8 子 agent、沙箱。
架构参考：`llmreferences/pi/packages/{ai,agent,coding-agent}`。
