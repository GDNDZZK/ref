#include "app/workspace.h"

#include "workspace/toast_text.h"

#include <QJSEngine>

#include <algorithm>
#include <utility>

using refgui::client::Backend;
using refgui::client::Result;
using namespace Qt::StringLiterals;

namespace
{

Workspace* theInstance = nullptr;

// The loopback names under which a backend runs on this machine.
bool isLoopback(const QString& host)
{
	return host == u"localhost"_s || host == u"127.0.0.1"_s || host == u"::1"_s || host == u"[::1]"_s;
}

} // namespace

Workspace::Workspace(refgui::cli::LaunchOptions options, QString settingsPath, QObject* parent)
	: QObject(parent)
	, m_options(std::move(options))
	, m_cwd(m_options.directory)
	, m_settings(std::move(settingsPath))
	, m_backend(m_options.source, m_cwd)
{
	connect(&m_backend, &Backend::stateChanged, this, [this] {
		emit connectionChanged();
		emit backendChanged();
		updateCommandContext();
	});
	connect(&m_backend, &Backend::connected, this, &Workspace::onBackendConnected);
	connect(&m_backend.client(), &refgui::client::ServerClient::frameReceived, this, &Workspace::toastFrom);
	connect(&m_directory, &refgui::workspace::SessionDirectory::liveChanged, this, &Workspace::onLiveChanged);
	connect(&m_directory, &refgui::workspace::SessionDirectory::historyLoadedChanged, this, &Workspace::historyLoadedChanged);
	connect(&m_directory, &refgui::workspace::SessionDirectory::failed, this, [this](const QString& reason) { toast(u"error"_s, reason); });
	connect(&m_config, &refgui::workspace::BackendConfig::changed, this, [this] {
		m_commands.setCustomCommands(m_config.config().commands);
	});
	connect(&m_config, &refgui::workspace::BackendConfig::mcpChanged, this, &Workspace::mcpServersChanged);
	connect(&m_config, &refgui::workspace::BackendConfig::failed, this, [this](const QString& reason) { toast(u"error"_s, reason); });
	connect(&m_settings, &refgui::workspace::Settings::saveFailed, this, [this](const QString& reason) {
		toast(u"warn"_s, tr("Could not save the settings: %1").arg(reason));
	});
	connect(&m_commands, &refgui::workspace::CommandRegistry::unavailable, this, [this] { toast(u"info"_s, tr("That command is not available now.")); });

	registerCommands();
	m_backend.start();
}

// The backend goes quiet before the chat sessions that call back into it are destroyed.
Workspace::~Workspace()
{
	// The sessions refer to the backend, which is a member and goes before the children do.
	qDeleteAll(findChildren<ChatSession*>(Qt::FindDirectChildrenOnly));
	m_backend.shutdown();
}

void Workspace::setInstance(Workspace* instance)
{
	theInstance = instance;
}

Workspace* Workspace::instance()
{
	return theInstance;
}

Workspace* Workspace::create(QQmlEngine*, QJSEngine* jsEngine)
{
	Q_ASSERT(theInstance != nullptr);
	QJSEngine::setObjectOwnership(theInstance, QJSEngine::CppOwnership);
	theInstance->m_engine = jsEngine;
	return theInstance;
}

QString Workspace::currentId() const
{
	return m_current == nullptr ? QString() : m_current->sessionId();
}

QStringList Workspace::visibleSessionIds() const
{
	if(m_split)
	{
		QStringList ids;
		for(const ChatSession* tile : m_tiles.tiles())
		{
			if(m_zoomed.isEmpty() || tile->sessionId() == m_zoomed)
			{
				ids.append(tile->sessionId());
			}
		}
		return ids;
	}
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

bool Workspace::isBackendLocal() const
{
	const auto* remote = std::get_if<Backend::Mode::Remote>(&m_options.source);
	return remote == nullptr || isLoopback(remote->endpoint.url.host());
}

// Restarting stops every session of the backend, so it waits until none is working.
bool Workspace::canRestartBackend() const
{
	if(!isProtocolOutdated())
	{
		return false;
	}
	const auto& live = m_directory.liveSessions();
	return std::ranges::none_of(live, [](const refgui::proto::SessionInfo& info) { return info.status.state != refgui::proto::SessionStatus::State::Idle; });
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

void Workspace::retranslate()
{
	m_commands.retranslate();
	emit connectionChanged();
}

void Workspace::onBackendConnected()
{
	emit backendChanged();
	if(!m_started)
	{
		m_started = true;
		openFirstSession();
		return;
	}
	if(m_split)
	{
		m_backend.client().watchAll(true); // the new connection knows nothing of the old subscription
	}
}

void Workspace::toastFrom(const refgui::proto::ServerFrame& frame)
{
	const auto* toastFrame = std::get_if<refgui::proto::ToastFrame>(&frame);
	if(toastFrame == nullptr)
	{
		return;
	}
	using Level = refgui::proto::ToastFrame::Level;
	switch(toastFrame->level)
	{
	case Level::Info:
		toast(u"info"_s, toastFrame->text);
		return;
	case Level::Warn:
		toast(u"warn"_s, toastFrame->text);
		return;
	case Level::Error:
		toast(u"error"_s, toastFrame->text);
		return;
	}
}

void Workspace::toast(const QString& level, const QString& text)
{
	emit toastRequested(level, text);
}

void Workspace::openDialog(const QString& id, const QVariantMap& args)
{
	m_dialog = id;
	m_dialogArgs = args;
	emit dialogChanged();
}

void Workspace::closeDialog()
{
	if(m_dialog.isEmpty())
	{
		return;
	}
	m_dialog.clear();
	m_dialogArgs.clear();
	emit dialogChanged();
}

// ---- Opening, closing and switching sessions --------------------------------------------

// Opens what the command line asked for.
QCoro::Task<> Workspace::openFirstSession()
{
	using Kind = refgui::cli::SessionStart::Kind;
	const refgui::cli::SessionStart& start = m_options.start;
	refgui::client::ServerClient& client = m_backend.client();

	const auto fail = [this](const QString& reason) {
		m_failure = reason;
		emit failureTextChanged();
		emit openFailed(reason);
	};

	QString sessionId = start.sessionId;
	if(start.kind == Kind::Continue)
	{
		const auto history = co_await client.listHistory(m_cwd, 1);
		if(!history)
		{
			fail(history.error());
			co_return;
		}
		if(history->isEmpty())
		{
			toast(u"info"_s, tr("No earlier session in this directory. Started a new one."));
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
			fail(created.error());
			co_return;
		}
		m_directory.update(*created);
		sessionId = created->id;
	}

	const Result<void> shown = co_await show(sessionId);
	if(!shown)
	{
		fail(shown.error());
		co_return;
	}
	emit opened(sessionId);
	if(m_options.watch)
	{
		toggleSplit();
	}
}

ChatSession* Workspace::session(const QString& sessionId)
{
	return m_chats.value(sessionId, nullptr); // never creates one: a closed session must not come back
}

ChatSession& Workspace::chat(const QString& sessionId)
{
	ChatSession*& slot = m_chats[sessionId];
	if(slot == nullptr)
	{
		slot = new ChatSession(m_backend, sessionId, this);
		connect(slot, &ChatSession::errorReported, this, [this, sessionId](const QString& text) {
			// The current session draws its errors in the conversation; the others need a toast.
			if(sessionId == currentId() && !m_split)
			{
				return;
			}
			toast(u"error"_s, refgui::workspace::toastText(u"%1: %2"_s.arg(nameOf(sessionId), refgui::workspace::oneLine(text))));
		});
	}
	return *slot;
}

QString Workspace::nameOf(const QString& sessionId) const
{
	const refgui::proto::SessionInfo* info = m_directory.find(sessionId);
	if(info == nullptr)
	{
		return sessionId;
	}
	return !info->name.isEmpty() ? info->name : !info->title.isEmpty() ? info->title : sessionId;
}

// The session a command acts on: the one asked for, else the focused tile, else the current one.
QString Workspace::targetId(const QString& sessionId) const
{
	if(!sessionId.isEmpty())
	{
		return sessionId;
	}
	if(m_split && !m_focusedTile.isEmpty())
	{
		return m_focusedTile;
	}
	return currentId();
}

void Workspace::setCurrent(ChatSession* session)
{
	m_current = session;
	m_directory.live().setCurrentId(currentId());
	m_directory.history().setCurrentId(currentId());
	if(m_split && session != nullptr)
	{
		setFocusedTile(session->sessionId());
		syncTiles();
	}
	emit currentChanged();
	emit visibleSessionIdsChanged();
	updateCommandContext();
}

void Workspace::openSession(const QString& sessionId)
{
	if(sessionId.isEmpty())
	{
		return;
	}
	if(m_split && m_tiles.indexOf(sessionId) >= 0)
	{
		focusTile(sessionId);
		return;
	}
	if(sessionId == currentId())
	{
		return;
	}
	[](Workspace* self, QString id) -> QCoro::Task<> {
		const Result<void> shown = co_await self->show(std::move(id));
		if(!shown)
		{
			self->toast(u"error"_s, shown.error());
		}
	}(this, sessionId);
}

// Makes the session current: attaches it and puts it on screen.
QCoro::Task<Workspace::Done> Workspace::show(QString sessionId)
{
	const int serial = ++m_showSerial;
	ChatSession& target = chat(sessionId);
	const Result<void> attached = co_await target.attach(!m_split);
	if(!attached)
	{
		co_return std::unexpected(attached.error());
	}
	if(serial != m_showSerial && &target != m_current && !m_split)
	{
		// The user has moved on to another session meanwhile.
		co_await target.detach();
		co_return Result<void>();
	}
	ChatSession* old = m_current;
	setCurrent(&target);
	if(old != nullptr && old != &target && !m_split && old->isAttached() && !m_leaving.contains(old->sessionId()))
	{
		co_await old->detach();
	}
	co_return Result<void>();
}

void Workspace::newSession(const QString& cwd, const QJSValue& cb)
{
	createAndShow(cwd.isEmpty() ? m_cwd : cwd, wrap(cb));
}

QCoro::Task<> Workspace::createAndShow(QString cwd, Reply reply)
{
	const auto created = co_await m_backend.client().createSession(cwd, std::nullopt, std::nullopt);
	if(!created)
	{
		report(reply, false, created.error());
		co_return;
	}
	m_directory.update(*created);
	const Result<void> shown = co_await show(created->id);
	if(!shown)
	{
		report(reply, false, shown.error());
		co_return;
	}
	report(reply, true, created->id);
}

void Workspace::closeSession(const QString& sessionId)
{
	if(sessionId.isEmpty())
	{
		return;
	}
	closeTask(sessionId);
}

QCoro::Task<> Workspace::closeTask(QString sessionId)
{
	const refgui::proto::SessionInfo* info = m_directory.find(sessionId);
	const QString cwd = info == nullptr ? m_cwd : info->cwd;
	const Result<void> closed = co_await m_backend.client().close(sessionId);
	if(!closed)
	{
		toast(u"error"_s, closed.error());
		co_return;
	}
	handleGone(sessionId, cwd);
}

// A session ended, closed here or elsewhere: forgets it, and when it was the current one
// moves to another session, or starts a new one in its directory.
void Workspace::handleGone(const QString& sessionId, const QString& cwd)
{
	if(!m_chats.contains(sessionId) || m_leaving.contains(sessionId))
	{
		return;
	}
	m_leaving.insert(sessionId);
	if(sessionId != currentId())
	{
		dropChat(sessionId);
		return;
	}
	leaveCurrent(sessionId, cwd);
}

QCoro::Task<> Workspace::leaveCurrent(QString closedId, QString cwd)
{
	QString next;
	for(const refgui::proto::SessionInfo& info : m_directory.liveSessions())
	{
		if(info.id != closedId && !m_leaving.contains(info.id))
		{
			next = info.id;
			break;
		}
	}
	if(next.isEmpty())
	{
		co_await createAndShow(cwd.isEmpty() ? m_cwd : cwd, {});
	}
	else
	{
		const Result<void> shown = co_await show(next);
		if(!shown)
		{
			toast(u"error"_s, shown.error());
		}
	}
	if(currentId() == closedId)
	{
		setCurrent(nullptr);
	}
	dropChat(closedId);
}

void Workspace::dropChat(const QString& sessionId)
{
	// While the backend still lists the session, nothing may bring it back (see syncTiles).
	if(!m_seenLive.contains(sessionId))
	{
		m_leaving.remove(sessionId);
	}
	ChatSession* gone = m_chats.take(sessionId);
	if(gone == nullptr)
	{
		return;
	}
	if(m_split)
	{
		syncTiles();
	}
	m_stage.deleteWhenLeft(gone); // its tile may still be fading out
}

// Reacts to the backend's session list: sessions that disappeared were closed.
void Workspace::onLiveChanged()
{
	QHash<QString, QString> now;
	for(const refgui::proto::SessionInfo& info : m_directory.liveSessions())
	{
		now.insert(info.id, info.cwd);
	}
	const QHash<QString, QString> before = std::exchange(m_seenLive, now);
	for(auto it = before.cbegin(); it != before.cend(); ++it)
	{
		if(!now.contains(it.key()))
		{
			handleGone(it.key(), it.value());
		}
	}
	// Closed sessions the backend no longer lists cannot come back.
	m_leaving.removeIf([this, &now](const QString& id) { return !now.contains(id) && !m_chats.contains(id); });
	if(m_split)
	{
		syncTiles();
	}
	updateCommandContext();
	emit backendChanged(); // canRestartBackend depends on the sessions' states
}
