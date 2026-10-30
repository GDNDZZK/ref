// Adapted from an early version of qutie by LightBlueCube
#include "app/chat_session.h"

#include "llm/config.h"
#include "session/agent_driver.h"
#include "session/mock_driver.h"

namespace
{

std::unique_ptr<refgui::session::Driver> makeDriver(ChatSession::Replies replies, refgui::session::ConversationModel& model)
{
	switch(replies)
	{
	case ChatSession::Replies::FromModel:
		return std::make_unique<refgui::session::AgentDriver>(model, refgui::llm::configPath());
	case ChatSession::Replies::FromMock:
		return std::make_unique<refgui::session::MockDriver>(model);
	}
	Q_UNREACHABLE_RETURN(nullptr);
}

} // namespace

ChatSession::ChatSession(Replies replies, QObject* parent)
	: QObject(parent)
	, m_model(this)
	, m_driver(makeDriver(replies, m_model))
{
	connect(m_driver.get(), &refgui::session::Driver::runningChanged, this, &ChatSession::busyChanged);
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
