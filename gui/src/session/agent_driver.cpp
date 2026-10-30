#include "session/agent_driver.h"

#include "llm/config.h"
#include "session/conversation_model.h"

#include <chrono>
#include <utility>

using namespace Qt::StringLiterals;
using namespace std::chrono_literals;

namespace refgui::session
{
namespace
{

constexpr std::chrono::milliseconds TickInterval = 16ms; // about a frame

// What to tell the user about how a reply ended, if anything.
std::optional<QString> noticeFor(const std::expected<llm::StopReason, llm::Error>& end)
{
	if(!end)
	{
		return end.error().message;
	}
	switch(*end)
	{
	case llm::StopReason::Stop:
	case llm::StopReason::Cancelled:
		return std::nullopt;
	case llm::StopReason::Length:
		return u"The reply was cut off: it reached the model's output limit."_s;
	case llm::StopReason::ContentFilter:
		return u"The provider's content filter stopped the reply."_s;
	}
	Q_UNREACHABLE_RETURN(std::nullopt);
}

} // namespace

AgentDriver::AgentDriver(ConversationModel& model, QString configPath, QObject* parent)
	: Driver(parent)
	, m_model(model)
	, m_configPath(std::move(configPath))
{
	m_ticker.setTimerType(Qt::PreciseTimer);
	m_ticker.setInterval(TickInterval);
	connect(&m_ticker, &QTimer::timeout, this, &AgentDriver::tick);
}

// Ends the reply in progress, if any, while the members it uses still exist.
AgentDriver::~AgentDriver()
{
	m_destroying = true;
	m_stop.request_stop();
}

void AgentDriver::respond(QStringView prompt)
{
	Q_ASSERT(!isRunning());

	m_history.append(llm::UserMessage{prompt.toString()});
	m_stop = std::stop_source();
	m_pacer = {}; // the last turn's pace says nothing about this one
	m_streamEnded = false;
	m_notice.reset();
	m_running = true;
	m_ticker.start();
	emit runningChanged();

	reply(); // runs on its own and ends the turn when done
}

// The stream may still be going or may have ended with text left to show. Either way,
// the user sees all that arrived, which is what the history keeps.
void AgentDriver::stop()
{
	Q_ASSERT(isRunning());
	m_stop.request_stop(); // ends the stream before it returns
	Q_ASSERT(m_streamEnded);

	show(m_pacer.takeAll());
	endTurn(m_notice);
}

bool AgentDriver::isRunning() const
{
	return m_running;
}

QCoro::Task<> AgentDriver::reply()
{
	if(!m_client)
	{
		std::expected<llm::Endpoint, QString> endpoint = llm::loadEndpoint(m_configPath);
		if(!endpoint)
		{
			endTurn(endpoint.error());
			co_return;
		}
		m_client.emplace(std::move(*endpoint));
	}

	const auto onEvent = [this](const llm::StreamEvent& event) { m_pacer.push(event, StreamPacer::Clock::now()); };
	const llm::Completion completion = co_await m_client->complete(m_history, onEvent, m_stop.get_token());
	if(m_destroying)
	{
		co_return; // nobody is left to see the end of the turn
	}

	// The pacer still shows the rest; tick() ends the turn once it is all out.
	m_history.append(completion.message);
	m_notice = noticeFor(completion.end);
	m_streamEnded = true;
}

void AgentDriver::tick()
{
	show(m_pacer.take(StreamPacer::Clock::now()));
	if(m_streamEnded && m_pacer.isEmpty())
	{
		endTurn(m_notice);
	}
}

void AgentDriver::show(const QList<llm::StreamEvent>& events)
{
	for(const llm::StreamEvent& event : events)
	{
		if(const auto* text = std::get_if<llm::TextDelta>(&event))
		{
			m_model.appendAssistantText(text->text);
		}
		else
		{
			m_model.appendReasoning(std::get<llm::ReasoningDelta>(event).text);
		}
	}
}

void AgentDriver::endTurn(const std::optional<QString>& notice)
{
	m_ticker.stop();
	m_model.endAssistantTurn();
	if(notice)
	{
		m_model.appendError(*notice);
	}
	m_running = false;
	emit runningChanged();
}

} // namespace refgui::session
