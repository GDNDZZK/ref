#include "app/workspace.h"

#include <QJSEngine>

#include <utility>

using refgui::client::Backend;
using refgui::client::Result;
using namespace Qt::StringLiterals;

namespace
{

Workspace* theInstance = nullptr;

} // namespace

Workspace::Workspace(refgui::cli::LaunchOptions options, QObject* parent)
	: QObject(parent)
	, m_options(std::move(options))
	, m_cwd(m_options.directory)
	, m_backend(m_options.source, m_cwd)
{
	connect(&m_backend, &Backend::stateChanged, this, &Workspace::connectionChanged);
	connect(&m_backend, &Backend::connected, this, [this] {
		if(m_started)
		{
			return;
		}
		m_started = true;
		openFirstSession();
	});
	m_backend.start();
}

// The backend goes quiet before the chat sessions that call back into it are destroyed.
Workspace::~Workspace()
{
	m_backend.shutdown();
}

void Workspace::setInstance(Workspace* instance)
{
	theInstance = instance;
}

Workspace* Workspace::create(QQmlEngine*, QJSEngine*)
{
	Q_ASSERT(theInstance != nullptr);
	QJSEngine::setObjectOwnership(theInstance, QJSEngine::CppOwnership);
	return theInstance;
}

QString Workspace::currentId() const
{
	return m_current == nullptr ? QString() : m_current->sessionId();
}

QStringList Workspace::visibleSessionIds() const
{
	return m_current == nullptr ? QStringList() : QStringList{m_current->sessionId()};
}

void Workspace::setWindowActive(bool active)
{
	if(active == m_windowActive)
	{
		return;
	}
	m_windowActive = active;
	emit windowActiveChanged();
}

QString Workspace::connectionState() const
{
	switch(m_backend.state())
	{
	case Backend::State::Connecting:
		return u"connecting"_s;
	case Backend::State::StartingBackend:
		return u"starting"_s;
	case Backend::State::Connected:
		return u"connected"_s;
	case Backend::State::Disconnected:
		return u"disconnected"_s;
	}
	Q_UNREACHABLE_RETURN(QString());
}

QString Workspace::connectionText() const
{
	switch(m_backend.state())
	{
	case Backend::State::Connecting:
		return tr("Connecting to the ref backend…");
	case Backend::State::StartingBackend:
		return tr("Starting the ref backend…");
	case Backend::State::Disconnected:
		return tr("Disconnected: %1. Trying again…").arg(m_backend.reason());
	case Backend::State::Connected:
		return {};
	}
	Q_UNREACHABLE_RETURN(QString());
}

// Opens what the command line asked for.
QCoro::Task<> Workspace::openFirstSession()
{
	using Kind = refgui::cli::SessionStart::Kind;
	const refgui::cli::SessionStart& start = m_options.start;
	refgui::client::ServerClient& client = m_backend.client();

	QString sessionId = start.sessionId;
	if(start.kind == Kind::Continue)
	{
		const auto history = co_await client.listHistory(m_cwd, 1);
		if(!history)
		{
			setFailure(history.error());
			emit openFailed(history.error());
			co_return;
		}
		if(history->isEmpty())
		{
			emit toastRequested(u"info"_s, tr("No earlier session in this directory. Started a new one."));
		}
		else
		{
			sessionId = history->first().id;
		}
	}
	if(sessionId.isEmpty())
	{
		const auto created = co_await client.createSession(m_cwd, start.agent, start.model);
		if(!created)
		{
			setFailure(created.error());
			emit openFailed(created.error());
			co_return;
		}
		sessionId = created->id;
	}

	const Result<void> shown = co_await show(sessionId);
	if(!shown)
	{
		setFailure(shown.error());
		emit openFailed(shown.error());
		co_return;
	}
	emit opened(sessionId);
}

// Makes the session current: attaches it (in the foreground) and puts it on screen.
QCoro::Task<Result<void>> Workspace::show(const QString& sessionId)
{
	ChatSession& session = chat(sessionId);
	const Result<void> attached = co_await session.attach(true);
	if(!attached)
	{
		co_return std::unexpected(attached.error());
	}
	m_current = &session;
	emit currentChanged();
	emit visibleSessionIdsChanged();
	co_return Result<void>();
}

ChatSession& Workspace::chat(const QString& sessionId)
{
	ChatSession*& slot = m_chats[sessionId];
	if(slot == nullptr)
	{
		slot = new ChatSession(m_backend, sessionId, this);
	}
	return *slot;
}

void Workspace::setFailure(const QString& text)
{
	m_failure = text;
	emit failureTextChanged();
}
