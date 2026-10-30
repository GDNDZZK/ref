#include "ui/mock_driver.h"

#include "ui/conversation_model.h"

#include <QRandomGenerator>

#include <algorithm>

using namespace Qt::StringLiterals;
using namespace std::chrono_literals;

namespace qutie::ui
{
namespace
{

constexpr int MinChunkLength = 1;

} // namespace

MockDriver::MockDriver(ConversationModel& model, QObject* parent)
	: Driver(parent)
	, m_model(model)
	, m_scripts(scripts())
{
	m_ticker.setTimerType(Qt::PreciseTimer);
	connect(&m_ticker, &QTimer::timeout, this, &MockDriver::advance);
}

void MockDriver::respond(QStringView prompt)
{
	Q_ASSERT(!isRunning());

	const auto named = std::ranges::find_if(m_scripts, [prompt](const Script& script) {
		return prompt.trimmed().compare(script.name, Qt::CaseInsensitive) == 0;
	});
	const bool isNamed = named != m_scripts.end();
	const Script& script = isNamed ? *named : m_scripts.at(m_nextScript);
	if(!isNamed)
	{
		m_nextScript = (m_nextScript + 1) % m_scripts.size();
	}

	m_steps = script.steps;
	m_maxChunkLength = script.pace.maxChunkLength;
	m_stepIndex = 0;
	m_charIndex = 0;
	m_stepStarted = false;

	m_ticker.setInterval(script.pace.tickInterval);
	m_ticker.start();
	emit runningChanged();
}

void MockDriver::stop()
{
	Q_ASSERT(isRunning());

	// m_stepIndex reaches size() one tick before finish(), so check it before indexing.
	const bool toolRunning = m_stepIndex < m_steps.size() && m_stepStarted && std::holds_alternative<ToolCall>(m_steps.at(m_stepIndex));
	if(toolRunning)
	{
		m_model.endToolCall(false, u"Cancelled"_s);
	}
	finish();
}

bool MockDriver::isRunning() const
{
	return m_ticker.isActive();
}

void MockDriver::advance()
{
	if(m_stepIndex == m_steps.size())
	{
		finish();
		return;
	}

	const Step& step = m_steps.at(m_stepIndex);
	bool stepDone = false;

	if(const auto* pause = std::get_if<Pause>(&step))
	{
		stepDone = waitFor(pause->duration);
	}
	else if(const auto* reasoning = std::get_if<Reasoning>(&step))
	{
		stepDone = streamChunk(reasoning->text, &ConversationModel::appendReasoning);
	}
	else if(const auto* text = std::get_if<Text>(&step))
	{
		stepDone = streamChunk(text->text, &ConversationModel::appendAssistantText);
	}
	else if(const auto* failure = std::get_if<Failure>(&step))
	{
		m_model.endAssistantTurn();
		m_model.appendError(failure->message);
		stepDone = true;
	}
	else
	{
		const auto& tool = std::get<ToolCall>(step);
		if(!m_stepStarted)
		{
			m_model.beginToolCall(tool.title);
		}
		stepDone = waitFor(tool.duration);
		if(stepDone)
		{
			m_model.endToolCall(tool.succeeded, tool.output);
		}
	}

	if(stepDone)
	{
		++m_stepIndex;
		m_charIndex = 0;
		m_stepStarted = false;
	}
}

// Appends the next chunk of `text`; returns whether the whole text has been appended.
bool MockDriver::streamChunk(const QString& text, void (ConversationModel::*append)(QStringView))
{
	const qsizetype length = QRandomGenerator::global()->bounded(MinChunkLength, m_maxChunkLength);
	const QStringView chunk = QStringView(text).sliced(m_charIndex).first(std::min(length, text.size() - m_charIndex));

	(m_model.*append)(chunk);
	m_charIndex += chunk.size();
	return m_charIndex == text.size();
}

// Returns whether `duration` has passed since the current step started.
bool MockDriver::waitFor(std::chrono::milliseconds duration)
{
	if(!m_stepStarted)
	{
		m_stepStarted = true;
		m_stepDeadline.setRemainingTime(duration);
	}
	return m_stepDeadline.hasExpired();
}

void MockDriver::finish()
{
	m_ticker.stop();
	m_model.endAssistantTurn();
	emit runningChanged();
}

QList<MockDriver::Script> MockDriver::scripts()
{
	return {codeScript(), toolsScript(), chineseScript(), slowScript(), longScript(), errorScript()};
}

// A reply that breaks off, as when the server drops the stream.
MockDriver::Script MockDriver::errorScript()
{
	return {
		.name = u"error"_s,
		.pace = NormalPace,
		.steps = {
			Pause{400ms},
			Text{u"Checking the build output first, then I'll look at the failing test"_s},
			Failure{u"The request failed: Connection closed by the server."_s},
		},
	};
}

MockDriver::Script MockDriver::codeScript()
{
	const Step readMain = ToolCall{
		.title = u"read src/app/main.cpp"_s,
		.output = u"int main(int argc, char* argv[])\n{\n\tQGuiApplication app(argc, argv);\n\tQQmlApplicationEngine engine;\n\tengine.loadFromModule(\"Qutie\", \"Main\");\n\treturn QGuiApplication::exec();\n}"_s,
		.succeeded = true,
		.duration = 900ms,
	};

	return {
		.name = u"code"_s,
		.pace = NormalPace,
		.steps = {
			Pause{600ms},
			Reasoning{u"The user wants to see how a typical reply renders. "
					  u"I'll read the entry point first, then explain the change with a code sample."_s},
			readMain,
			Text{u"`main.cpp` loads the QML module straight from the engine. "
				 u"To make the window title follow the session, expose a property on `ChatSession`:\n\n"
				 u"```cpp\n"
				 u"class ChatSession : public QObject\n"
				 u"{\n"
				 u"\tQ_OBJECT\n"
				 u"\tQ_PROPERTY(QString title READ title NOTIFY titleChanged)\n\n"
				 u"public:\n"
				 u"\tQString title() const;\n\n"
				 u"signals:\n"
				 u"\tvoid titleChanged();\n"
				 u"};\n"
				 u"```\n\n"
				 u"Then bind it in QML:\n\n"
				 u"```qml\n"
				 u"Window {\n"
				 u"    title: session.title\n"
				 u"}\n"
				 u"```\n\n"
				 u"The window never polls; it reacts to `titleChanged`. See the "
				 u"[property binding docs](https://doc.qt.io/qt-6/qtqml-syntax-propertybinding.html) for details."_s},
		},
	};
}

MockDriver::Script MockDriver::toolsScript()
{
	const Step runTests = ToolCall{
		.title = u"bash ctest --test-dir build"_s,
		.output = u"1/3 Test #1: tst_jsonc ........   Passed    0.01 sec\n"
				  u"2/3 Test #2: tst_sse ..........   Passed    0.01 sec\n"
				  u"3/3 Test #3: tst_bench ........***Failed    2.31 sec\n\n"
				  u"67% tests passed, 1 tests failed out of 3"_s,
		.succeeded = false,
		.duration = 1600ms,
	};

	return {
		.name = u"tools"_s,
		.pace = NormalPace,
		.steps = {
			Pause{400ms},
			Text{u"## Benchmark plan\n\n"
				 u"| Case  | Input | Budget   |\n"
				 u"|-------|-------|----------|\n"
				 u"| small | 1 KB  | < 1 ms   |\n"
				 u"| large | 10 MB | < 200 ms |\n\n"
				 u"Running the existing suite first."_s},
			runTests,
			Text{u"One case fails. A quick script reproduces it outside CTest:\n\n"
				 u"```python\n"
				 u"import subprocess\n"
				 u"import time\n\n"
				 u"start = time.perf_counter()\n"
				 u"subprocess.run([\"build/tests/tst_bench\"], check=True)\n"
				 u"print(f\"{time.perf_counter() - start:.3f}s\")\n"
				 u"```\n\n"
				 u"1. Run it a few times\n"
				 u"2. Compare against the budget above\n"
				 u"3. If it stays slow, profile with `perf record`"_s},
		},
	};
}

MockDriver::Script MockDriver::chineseScript()
{
	return {
		.name = u"zh"_s,
		.pace = NormalPace,
		.steps = {
			Pause{500ms},
			Reasoning{u"用户用中文提问，我也用中文回答。需要解释为什么高频交互适合指数曲线。"_s},
			Text{u"### 为什么高频交互用指数曲线\n\n"
				 u"指数缓出（`OutExpo`）一开始速度最快，然后迅速收敛：\n\n"
				 u"- 反馈**立刻**可见，不会慢半拍\n"
				 u"- 尾巴短，结束得干脆\n"
				 u"- 没有回弹，连续触发时不会互相打架\n\n"
				 u"回弹（`OutBack`）留给低频、需要强调的时刻。\n\n"
				 u"> 动效服务于理解，而不是表演。\n\n"
				 u"```qml\n"
				 u"Behavior on height {\n"
				 u"    NumberAnimation { duration: 220; easing.type: Easing.OutExpo }\n"
				 u"}\n"
				 u"```"_s},
		},
	};
}

// A slow local model: long time to first token, then a few tokens per second.
// Exercises the caret, the thinking pulse and following at a crawl.
MockDriver::Script MockDriver::slowScript()
{
	const Step listFiles = ToolCall{
		.title = u"glob src/**/*.qml"_s,
		.output = u"src/app/Main.qml\nsrc/app/ChatView.qml\nsrc/app/Block.qml\nsrc/app/CodeBlock.qml"_s,
		.succeeded = true,
		.duration = 700ms,
	};

	return {
		.name = u"slow"_s,
		.pace = SlowPace,
		.steps = {
			Pause{2500ms},
			Reasoning{u"Running on a small local model, so this is slow. "
					  u"Look at the QML files before answering."_s},
			listFiles,
			Text{u"Four QML files. `ChatView.qml` owns scrolling:\n\n"
				 u"```qml\n"
				 u"FrameAnimation {\n"
				 u"    running: view.following\n"
				 u"}\n"
				 u"```\n\n"
				 u"Each frame closes part of the remaining distance, so the view "
				 u"glides even when tokens arrive one at a time."_s},
		},
	};
}

// Several minutes of reading: many blocks of every kind, a long code listing that
// streams past the window height and tool output long enough to scroll.
MockDriver::Script MockDriver::longScript()
{
	QString buildLog;
	constexpr int CompileSteps = 40;
	for(int i = 1; i <= CompileSteps; ++i)
	{
		buildLog += u"[%1/%2] Building CXX object src/ui/CMakeFiles/qutie_ui.dir/unit_%3.cpp.o\n"_s.arg(i).arg(CompileSteps).arg(i, 2, 10, u'0');
	}
	buildLog += u"[%1/%1] Linking CXX executable src/app/qutie"_s.arg(CompileSteps);

	QString handlers;
	constexpr int HandlerCount = 12;
	for(int i = 1; i <= HandlerCount; ++i)
	{
		handlers += u"\tcase Event::Kind%1:\n"
					u"\t\tm_state.counter += %1;\n"
					u"\t\tm_state.history.append(u\"kind %1\"_s);\n"
					u"\t\tbreak;\n"_s.arg(i);
	}

	const Step build = ToolCall{
		.title = u"bash cmake --build build"_s,
		.output = buildLog,
		.succeeded = true,
		.duration = 2200ms,
	};

	const Step grep = ToolCall{
		.title = u"grep \"positionViewAtEnd\" src/"_s,
		.output = u"src/app/ChatView.qml:18:  positionViewAtEnd()\nsrc/app/ChatView.qml:62:  view.positionViewAtEnd()"_s,
		.succeeded = true,
		.duration = 600ms,
	};

	const Step missingFile = ToolCall{
		.title = u"read src/ui/event_router.cpp"_s,
		.output = u"error: no such file or directory"_s,
		.succeeded = false,
		.duration = 500ms,
	};

	return {
		.name = u"long"_s,
		.pace = NormalPace,
		.steps = {
			Pause{500ms},
			Reasoning{u"This is a broad request. Plan:\n\n"
					  u"1. Build the project to see the current state.\n"
					  u"2. Find how scrolling is driven today.\n"
					  u"3. Sketch the event router and its dispatch table.\n"
					  u"4. Walk through the trade-offs and list follow-ups.\n\n"
					  u"Start with the build, since everything else depends on it compiling."_s},
			build,
			Text{u"## 1. Current state\n\n"
				 u"The build is clean: 40 translation units, no warnings under `-Werror`. "
				 u"That gives a safe baseline before touching the event flow.\n\n"
				 u"Next, where does the view decide to scroll?"_s},
			grep,
			Text{u"## 2. How scrolling works today\n\n"
				 u"`ChatView` used to call `positionViewAtEnd()` whenever content grew. That has three problems:\n\n"
				 u"- **It jumps.** The position changes in one frame, so the eye loses its place.\n"
				 u"- **It trusts estimates.** `contentHeight` is derived from the average row height, "
				 u"and rows far above are created and destroyed as you scroll.\n"
				 u"- **It bounces.** A line that wraps and then unwraps makes the end move up and down again.\n\n"
				 u"The replacement chases the end every frame and never lets the end move up during a turn:\n\n"
				 u"```qml\n"
				 u"FrameAnimation {\n"
				 u"    running: view.following && Math.abs(view.followTarget - view.contentY) > view.followSnap\n"
				 u"    onTriggered: {\n"
				 u"        const remaining = view.followTarget - view.contentY\n"
				 u"        const progress = 1 - Math.exp(-frameTime * 1000 / Motion.followLag)\n"
				 u"        view.contentY += remaining * progress\n"
				 u"    }\n"
				 u"}\n"
				 u"```\n\n"
				 u"Because the step is a fraction of the *remaining* distance, the motion is an exponential "
				 u"ease-out that retargets smoothly when more text arrives mid-glide."_s},
			missingFile,
			Text{u"## 3. The event router\n\n"
				 u"There is no router yet, so here is a first sketch. Events arrive from the agent loop and are "
				 u"dispatched by kind:\n\n"
				 u"```cpp\n"
				 u"// SPDX-License-Identifier: GPL-3.0-or-later\n"
				 u"#include \"ui/event_router.h\"\n\n"
				 u"using namespace Qt::StringLiterals;\n\n"
				 u"namespace qutie::ui\n"
				 u"{\n\n"
				 u"void EventRouter::dispatch(const Event& event)\n"
				 u"{\n"
				 u"\tswitch(event.kind)\n"
				 u"\t{\n"_s
				 + handlers
				 + u"\t}\n"
				   u"\temit stateChanged();\n"
				   u"}\n\n"
				   u"} // namespace qutie::ui\n"
				   u"```\n\n"
				   u"A table of handlers would be shorter, but the `switch` lets the compiler warn when a new kind "
				   u"is added and not handled."_s},
			Reasoning{u"The listing is long; a short comparison table will help the user decide."_s},
			Text{u"## 4. Trade-offs\n\n"
				 u"| Approach        | Exhaustive | Extensible | Cost per event |\n"
				 u"|-----------------|------------|------------|----------------|\n"
				 u"| `switch`        | yes        | edit code  | one jump       |\n"
				 u"| handler table   | no         | register   | one lookup     |\n"
				 u"| signal per kind | no         | connect    | one emit       |\n\n"
				 u"### Follow-ups\n\n"
				 u"1. Add `tst_event_router` covering every kind\n"
				 u"2. Route tool results through the same path\n"
				 u"3. Measure dispatch cost with 10 000 synthetic events\n"
				 u"4. Decide whether cancelled tools emit an event or only end the turn\n\n"
				 u"> Keep the router dumb: it forwards, it does not decide.\n\n"
				 u"That covers the plan. Tell me which follow-up to start with."_s},
		},
	};
}

} // namespace qutie::ui
