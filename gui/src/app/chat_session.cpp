#include "app/chat_session.h"

#include "llm/config.h"
#include "ui/agent_driver.h"
#include "ui/mock_driver.h"

namespace
{

std::unique_ptr<qutie::ui::Driver> makeDriver(ChatSession::Replies replies, qutie::ui::ConversationModel& model)
{
	switch(replies)
	{
	case ChatSession::Replies::FromModel:
		return std::make_unique<qutie::ui::AgentDriver>(model, qutie::llm::configPath());
	case ChatSession::Replies::FromMock:
		return std::make_unique<qutie::ui::MockDriver>(model);
	}
	Q_UNREACHABLE_RETURN(nullptr);
}

} // namespace

ChatSession::ChatSession(Replies replies, QObject* parent)
	: QObject(parent)
	, m_model(this)
	, m_driver(makeDriver(replies, m_model))
{
	connect(m_driver.get(), &qutie::ui::Driver::runningChanged, this, &ChatSession::busyChanged);
}

ChatSession::~ChatSession() = default;

QAbstractItemModel* ChatSession::blocks()
{
	return &m_model;
}

bool ChatSession::isBusy() const
{
	return m_driver->isRunning();
}

void ChatSession::send(const QString& text)
{
	Q_ASSERT(!isBusy() && !text.trimmed().isEmpty());

	m_model.appendUserMessage(text);
	m_driver->respond(text);
}

void ChatSession::stop()
{
	m_driver->stop();
}
