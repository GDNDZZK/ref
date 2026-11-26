#include "app/workspace.h"

#include "client/result.h"
#include "workspace/toast_text.h"

#include <QJSEngine>
#include <QLoggingCategory>
#include <QScopedValueRollback>
#include <QUrl>

#include <algorithm>
#include <utility>
using refgui::client::Result;
using namespace Qt::StringLiterals;

namespace
{

Q_LOGGING_CATEGORY(lcWorkspace, "refgui.app.workspace")

Workspace* theInstance = nullptr;

refgui::client::Backend::Source resolveInitialSource(const refgui::cli::LaunchOptions& options, const refgui::workspace::Settings& settings)
{
	if(std::holds_alternative<refgui::client::Backend::Mode::Shared>(options.source) && !settings.backendUrl().isEmpty())
	{
		const QUrl url(settings.backendUrl());
		if(url.isValid() && (url.scheme() == u"ws"_s || url.scheme() == u"wss"_s) && !url.host().isEmpty())
		{
			return refgui::client::Backend::Mode::Remote{{url, settings.backendToken()}};
		}
	}
	return options.source;
}

QString maskSecretToken(const QString& token)
{
	if(token.isEmpty())
	{
		return {};
	}
	if(token.size() <= 4)
	{
		return u"••••"_s;
	}
	return u"••••"_s + token.right(4);
}

} // namespace

Workspace::Workspace(refgui::cli::LaunchOptions options, QString settingsPath, QObject* parent)
	: QObject(parent)
	, m_options(std::move(options))
	, m_cwd(m_options.directory)
	, m_settings(std::move(settingsPath))
	, m_startupUiScale(m_settings.uiScale())
	, m_connection(resolveInitialSource(m_options, m_settings), m_cwd)
	, m_splitFilter(u"all"_s)
	, m_cliRemoteSource(std::holds_alternative<refgui::client::Backend::Mode::Remote>(m_options.source))
	, m_cliMockSource(std::holds_alternative<refgui::client::Backend::Mode::Mock>(m_options.source))
{
	connect(&m_connection, &refgui::workspace::Connection::stateChanged, this, [this] {
		emit connectionChanged();
		emit backendChanged();
		updateCommandContext();
	});
	connect(&m_connection, &refgui::workspace::Connection::connected, this, &Workspace::onBackendConnected);
	connect(&m_connection, &refgui::workspace::Connection::toastReceived, this, &Workspace::toastFrom);
	connect(&m_connection, &refgui::workspace::Connection::configChangedReceived, this, &Workspace::onConfigChanged);
	connect(&m_directory, &refgui::workspace::SessionDirectory::liveChanged, this, &Workspace::onLiveChanged);
	connect(&m_directory, &refgui::workspace::SessionDirectory::historyLoadedChanged, this, &Workspace::historyLoadedChanged);
	connect(&m_directory, &refgui::workspace::SessionDirectory::historyUpdated, this, [this] {
		updateHistoryCreatorLabel();
		emit splitCreatorsChanged();
		if(m_splitFilter != u"all"_s && m_splitFilter != u"user"_s)
		{
			emit splitFilterChanged();
		}
	});
	connect(&m_directory, &refgui::workspace::SessionDirectory::failed, this, [this](const QString& reason) { toast(refgui::workspace::ToastLevel::Error, reason); });
	connect(&m_config, &refgui::workspace::BackendConfig::changed, this, [this] {
		m_commands.setCustomCommands(m_config.config().commands);
	});
	connect(&m_config, &refgui::workspace::BackendConfig::mcpChanged, this, &Workspace::mcpServersChanged);
	connect(&m_config, &refgui::workspace::BackendConfig::failed, this, [this](const QString& reason) { toast(refgui::workspace::ToastLevel::Error, reason); });
	connect(&m_configEditor, &refgui::workspace::ConfigEditorController::mutationStarted, this, &Workspace::beginLocalConfigMutation);
	connect(&m_configEditor, &refgui::workspace::ConfigEditorController::mutationFinished, this, &Workspace::endLocalConfigMutation);
	connect(&m_configEditor, &refgui::workspace::ConfigEditorController::toastRequested, this, &Workspace::toast);
	connect(&m_stash, &refgui::workspace::StashController::toastRequested, this, &Workspace::toast);
	connect(&m_stash, &refgui::workspace::StashController::restoreGroupMemberFinished, this, [this](const QString& sessionId, bool restored) {
		if(restored || m_seenLive.contains(sessionId))
		{
			return;
		}
		if(!m_pendingRestoreOrderIds.remove(sessionId))
		{
			return;
		}
		QStringList order = m_settings.sidebarRunningOrder();
		if(order.removeAll(sessionId) > 0)
		{
			m_settings.setSidebarRunningOrder(order);
		}
	});
	connect(&m_settings, &refgui::workspace::Settings::saveFailed, this, [this](const QString& reason) {
		toast(refgui::workspace::ToastLevel::Warn, tr("Could not save the settings: %1").arg(reason));
	});
	connect(&m_commands, &refgui::workspace::CommandRegistry::unavailable, this, [this] { toastCommandUnavailable(); });

	connect(&m_settings, &refgui::workspace::Settings::sidebarRunningOrderChanged, this, [this] {
		m_directory.setLiveOrder(m_settings.sidebarRunningOrder());
	});
	m_directory.setLiveOrder(m_settings.sidebarRunningOrder());

	m_historyCreatorFilter = m_settings.sidebarHistoryCreatorFilter();
	connect(&m_settings, &refgui::workspace::Settings::sidebarHistoryCreatorFilterChanged, this, [this] {
		if(m_updatingHistoryFilter)
		{
			return;
		}
		m_historyCreatorFilter = m_settings.sidebarHistoryCreatorFilter();
		m_historyCreatorId.clear();
		m_lastHistoryCreatorLabel.clear();
		applyHistoryFilter(true);
		emit historyFilterChanged();
	});
	connect(&m_settings, &refgui::workspace::Settings::sidebarHistoryCurrentDirChanged, this, [this] {
		if(m_updatingHistoryFilter)
		{
			return;
		}
		applyHistoryFilter(true);
		emit historyFilterChanged();
	});

	connect(&m_settings, &refgui::workspace::Settings::messageWindowSizeChanged, this, [this] {
		const int size = m_settings.messageWindowSize();
		for(ChatSession* chatSession : std::as_const(m_chats))
		{
			if(chatSession != nullptr)
			{
				chatSession->setMessageWindowSize(size);
			}
		}
	});

	m_effectiveCwd = m_cwd;
	m_config.setCwd(m_effectiveCwd);
	applyHistoryFilter(false);
	registerCommands();
	m_connection.start();
}

// Closing the connection fails every request that is out and so resumes the coroutines waiting for them
// (leaving split view, closing a session, a session's attach...). They run to their end at once, since a closed
// client answers new requests with an error, and they need the sessions and the other members alive. So the
// connection closes first; nothing can resume after it. The sessions refer to the backend, which is a member
// and goes before the children do, so they are deleted here.
Workspace::~Workspace()
{
	m_connection.shutdown();
	qDeleteAll(findChildren<ChatSession*>(Qt::FindDirectChildrenOnly));
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

QString Workspace::effectiveCwd() const
{
	const QString id = targetId({});
	if(!id.isEmpty())
	{
		if(const auto* sessionInfo = m_directory.find(id); sessionInfo != nullptr && !sessionInfo->cwd.isEmpty())
		{
			return sessionInfo->cwd;
		}
		if(const auto* chatSession = m_chats.value(id, nullptr); chatSession != nullptr && !chatSession->cwd().isEmpty())
		{
			return chatSession->cwd();
		}
	}
	return m_cwd;
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

bool Workspace::isProtocolOutdated() const
{
	return m_connection.isProtocolOutdated();
}

bool Workspace::isBackendLocal() const
{
	return m_connection.isLocal();
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

QString Workspace::backendUrl() const
{
	return m_connection.backendUrl();
}

QString Workspace::maskedBackendToken() const
{
	return maskSecretToken(m_connection.backendToken());
}

QString Workspace::backendSource() const
{
	if(m_temporaryBackend)
	{
		return u"temporary"_s;
	}
	if(m_cliRemoteSource)
	{
		return u"direct"_s;
	}
	if(m_cliMockSource)
	{
		return u"mock"_s;
	}
	if(!m_settings.backendUrl().isEmpty())
	{
		return u"saved"_s;
	}
	return u"auto"_s;
}

QString Workspace::editableBackendUrl() const
{
	if(m_temporaryBackend)
	{
		return m_temporaryBackend->url.toString();
	}
	if(!m_settings.backendUrl().isEmpty())
	{
		return m_settings.backendUrl();
	}
	if(m_cliRemoteSource)
	{
		return m_connection.backendUrl();
	}
	return {};
}

QString Workspace::currentEditableToken() const
{
	if(m_temporaryBackend)
	{
		return m_temporaryBackend->token;
	}
	if(!m_settings.backendUrl().isEmpty())
	{
		return m_settings.backendToken();
	}
	if(m_cliRemoteSource)
	{
		return m_connection.backendToken();
	}
	return {};
}

bool Workspace::hasEditableBackendToken() const
{
	return !currentEditableToken().isEmpty();
}

QString Workspace::maskedEditableBackendToken() const
{
	return maskSecretToken(currentEditableToken());
}

void Workspace::setFloatingMode(bool floating)
{
	if(floating == m_floatingMode)
	{
		return;
	}
	m_floatingMode = floating;
	emit floatingModeChanged();
}

void Workspace::activateFromNotification(const QString& sessionId)
{
	setFloatingMode(false);
	openSession(sessionId);
}

bool Workspace::anySessionWorking() const
{
	for(const refgui::proto::SessionInfo& info : m_directory.liveSessions())
	{
		if(info.status.state != refgui::proto::SessionStatus::State::Idle)
		{
			return true;
		}
	}
	for(const ChatSession* chatSession : m_chats)
	{
		if(chatSession != nullptr && chatSession->isBusy())
		{
			return true;
		}
	}
	return false;
}

QString Workspace::connectionState() const
{
	switch(m_connection.state())
	{
	case refgui::workspace::Connection::State::Connecting:
		return u"connecting"_s;
	case refgui::workspace::Connection::State::StartingBackend:
		return u"starting"_s;
	case refgui::workspace::Connection::State::Connected:
		return u"connected"_s;
	case refgui::workspace::Connection::State::Disconnected:
		return u"disconnected"_s;
	}
	Q_UNREACHABLE_RETURN(QString());
}

QString Workspace::connectionText() const
{
	switch(m_connection.state())
	{
	case refgui::workspace::Connection::State::Connecting:
		return tr("Connecting to the ref backend…");
	case refgui::workspace::Connection::State::StartingBackend:
		return tr("Starting the ref backend…");
	case refgui::workspace::Connection::State::Disconnected:
		return tr("Disconnected: %1. Trying again…").arg(m_connection.reason());
	case refgui::workspace::Connection::State::Connected:
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
	}
}

void Workspace::toastFrom(const refgui::proto::ToastFrame& frame)
{
	using Level = refgui::proto::ToastFrame::Level;
	switch(frame.level)
	{
	case Level::Info:
		toast(refgui::workspace::ToastLevel::Info, frame.text);
		return;
	case Level::Warn:
		toast(refgui::workspace::ToastLevel::Warn, frame.text);
		return;
	case Level::Error:
		toast(refgui::workspace::ToastLevel::Error, frame.text);
		return;
	}
}

void Workspace::toast(const QString& level, const QString& text)
{
	emit toastRequested(level, text);
}

void Workspace::toastCommandUnavailable()
{
	toast(refgui::workspace::ToastLevel::Info, tr("That command is not available now."));
}

void Workspace::toastWarnings(const QStringList& warnings)
{
	if(warnings.isEmpty())
	{
		return;
	}
	toast(refgui::workspace::ToastLevel::Warn, refgui::workspace::toastText(warnings.join(u"; "_s)));
}

void Workspace::onConfigChanged(const QStringList& warnings)
{
	if(m_config.isReloading())
	{
		// ReloadConfig's own response brings both the refreshed config and warnings.
		return;
	}
	m_config.refresh();
	if(m_localMutationsInFlight == 0)
	{
		toastWarnings(warnings);
	}
}

QCoro::Task<> Workspace::detachUnwaited(ChatSession* session)
{
	const QString id = session->sessionId();
	logDropped(co_await session->detach(), "detach"_L1, id);
}

void Workspace::logDropped(const Result<void>& result, QLatin1StringView action, const QString& sessionId)
{
	if(!result && !refgui::client::isConnectionLost(result.error()))
	{
		qCWarning(lcWorkspace).noquote() << action << "of" << sessionId << "failed:" << result.error();
	}
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

	const auto fail = [this](const QString& reason) {
		m_failure = reason;
		emit failureTextChanged();
		emit openFailed(reason);
	};

	if(start.kind == Kind::None)
	{
		if(m_options.watch)
		{
			toggleSplit();
		}
		emit opened(QString());
		co_return;
	}

	QString sessionId = start.sessionId;
	if(start.kind == Kind::Continue)
	{
		const auto newest = co_await m_directory.newestIn(m_cwd);
		if(!newest)
		{
			fail(newest.error());
			co_return;
		}
		if(newest->has_value())
		{
			sessionId = (*newest)->id;
		}
		else
		{
			toast(refgui::workspace::ToastLevel::Info, tr("No earlier session in this directory. Started a new one."));
			const auto created = co_await m_directory.create(m_cwd, start.agent, start.model);
			if(!created)
			{
				fail(created.error());
				co_return;
			}
			setCwd(created->cwd);
			sessionId = created->id;
		}
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

ChatSession* Workspace::findSession(const QString& sessionId) const
{
	return sessionId.isEmpty() ? nullptr : m_chats.value(sessionId, nullptr);
}

ChatSession* Workspace::session(const QString& sessionId)
{
	return findSession(sessionId); // never creates one: a closed session must not come back
}

ChatSession& Workspace::chat(const QString& sessionId)
{
	ChatSession*& slot = m_chats[sessionId];
	if(slot == nullptr)
	{
		slot = new ChatSession(m_connection.backend(), m_config, sessionId, this, &m_directory);
		slot->setMessageWindowSize(m_settings.messageWindowSize());
		connect(slot, &ChatSession::infoChanged, this, &Workspace::syncConfigCwd);
		connect(slot->composer(), &refgui::composer::ComposerModel::toastRequested, this, &Workspace::toast);
		connect(slot, &ChatSession::messagesEvicted, this, [this, sessionId](const QStringList& ids) {
			emit sessionMessagesEvicted(sessionId, ids);
		});
		connect(slot, &ChatSession::errorReported, this, [this, sessionId](const QString& text) {
			// The current session draws its errors in the conversation; the others need a toast.
			if(sessionId == currentId() && !m_split)
			{
				return;
			}
			toast(refgui::workspace::ToastLevel::Error, refgui::workspace::toastText(u"%1: %2"_s.arg(nameOf(sessionId), refgui::workspace::oneLine(text))));
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
	if(!info->name.isEmpty())
	{
		return info->name;
	}
	if(!info->title.isEmpty())
	{
		return info->title;
	}
	return sessionId;
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

// The sidebar marks the session in front: in split view the one whose tile has the focus.
void Workspace::updateHighlight()
{
	const QString id = m_split && !m_focusedTile.isEmpty() ? m_focusedTile : currentId();
	m_directory.live().setCurrentId(id);
	m_directory.history().setCurrentId(id);
	if(id != m_frontId)
	{
		m_frontId = id;
		emit frontSessionChanged();
	}
	syncConfigCwd();
}

void Workspace::setCwd(const QString& cwd)
{
	if(cwd.isEmpty() || cwd == m_cwd)
	{
		return;
	}
	m_cwd = cwd;
	emit cwdChanged();
	syncConfigCwd();
}

void Workspace::syncConfigCwd()
{
	const QString next = effectiveCwd();
	if(next == m_effectiveCwd)
	{
		return;
	}
	m_effectiveCwd = next;
	emit effectiveCwdChanged();
	m_config.setCwd(m_effectiveCwd);
	if(m_settings.sidebarHistoryCurrentDir())
	{
		applyHistoryFilter(true);
	}
}

QString Workspace::historyCreatorLabel() const
{
	return m_historyCreatorId.isEmpty() ? QString() : m_directory.sessionLabel(m_historyCreatorId);
}

void Workspace::setHistoryFilter(const QString& creatorFilter, bool currentDir, const QString& creatorId)
{
	QString nextMode = refgui::workspace::Settings::HistoryCreatorAll;
	QString nextCreatorId;
	if(creatorFilter == u"user"_s)
	{
		nextMode = refgui::workspace::Settings::HistoryCreatorUser;
	}
	else if(creatorFilter == u"creator"_s && !creatorId.isEmpty())
	{
		nextMode = u"creator"_s;
		nextCreatorId = creatorId;
	}

	const bool changed = nextMode != m_historyCreatorFilter
	                     || nextCreatorId != m_historyCreatorId
	                     || currentDir != m_settings.sidebarHistoryCurrentDir();
	m_historyCreatorFilter = nextMode;
	m_historyCreatorId = nextCreatorId;
	m_lastHistoryCreatorLabel = historyCreatorLabel();

	{
		const QScopedValueRollback guard(m_updatingHistoryFilter, true);
		if(nextMode == refgui::workspace::Settings::HistoryCreatorAll || nextMode == refgui::workspace::Settings::HistoryCreatorUser)
		{
			m_settings.setSidebarHistoryCreatorFilter(nextMode);
		}
		m_settings.setSidebarHistoryCurrentDir(currentDir);
	}

	applyHistoryFilter(true);
	if(changed)
	{
		emit historyFilterChanged();
	}
}

void Workspace::setHistorySearch(const QString& query)
{
	const QString trimmed = query.trimmed();
	m_directory.setHistoryQuery(trimmed);
	if(trimmed != m_historySearch)
	{
		m_historySearch = trimmed;
		emit historySearchChanged();
	}
}

void Workspace::filterHistoryByCreator(const QString& sessionId)
{
	if(sessionId.isEmpty())
	{
		return;
	}
	m_settings.setSidebarHistoryExpanded(true);
	setHistoryFilter(u"creator"_s, m_settings.sidebarHistoryCurrentDir(), sessionId);
}

void Workspace::applyHistoryFilter(bool refreshNow)
{
	using CreatorFilter = refgui::workspace::SessionListModel::CreatorFilter;
	CreatorFilter filter = CreatorFilter::All;
	if(m_historyCreatorFilter == refgui::workspace::Settings::HistoryCreatorUser)
	{
		filter = CreatorFilter::UserCreated;
	}
	else if(m_historyCreatorFilter == u"creator"_s && !m_historyCreatorId.isEmpty())
	{
		filter = CreatorFilter::ByCreator;
	}
	const QString cwd = m_settings.sidebarHistoryCurrentDir() ? m_effectiveCwd : QString();
	m_directory.setHistoryFilter(cwd, filter, m_historyCreatorId, refreshNow);
}

void Workspace::updateHistoryCreatorLabel()
{
	if(m_historyCreatorId.isEmpty())
	{
		return;
	}
	const QString label = historyCreatorLabel();
	if(label != m_lastHistoryCreatorLabel)
	{
		m_lastHistoryCreatorLabel = label;
		emit historyFilterChanged();
	}
}
void Workspace::setCurrent(ChatSession* session)
{
	m_current = session;
	updateHighlight();
	if(m_split)
	{
		syncTiles();
	}
	emit currentChanged();
	emit visibleSessionIdsChanged();
	emit splitCreatorsChanged();
	updateCommandContext();
}

void Workspace::openSession(const QString& sessionId)
{
	if(sessionId.isEmpty())
	{
		return;
	}
	if(m_split)
	{
		if(!m_floatingMode && m_tiles.indexOf(sessionId) >= 0)
		{
			if(!m_zoomed.isEmpty() && m_zoomed != sessionId)
			{
				zoomTile(sessionId);
			}
			else
			{
				focusTile(sessionId);
			}
			return;
		}
		leaveSplit(false, sessionId);
		return;
	}
	if(sessionId == currentId())
	{
		return;
	}
	[](Workspace* self, QString id) -> QCoro::Task<> {
		const Result<void> shown = co_await self->show(std::move(id));
		self->toastFailure(shown);
	}(this, sessionId);
}

// Makes the session current: attaches it and puts it on screen.
QCoro::Task<Workspace::Done> Workspace::show(QString sessionId)
{
	const int serial = ++m_showSerial;
	ChatSession& target = chat(sessionId);
	const Result<void> attached = co_await target.attach(true);
	if(!attached)
	{
		co_return std::unexpected(attached.error());
	}
	const bool superseded = serial != m_showSerial && &target != m_current;
	if(superseded)
	{
		// The user has moved on to another session or detached meanwhile.
		if(m_split && m_tiles.indexOf(sessionId) >= 0)
		{
			logDropped(co_await target.background(), "background"_L1, sessionId);
		}
		else
		{
			logDropped(co_await target.detach(), "detach"_L1, sessionId);
		}
		co_return Result<void>();
	}
	ChatSession* old = m_current;
	setCurrent(&target);
	const bool shouldDetachOld = old != nullptr && old != &target && old->isAttached() && !m_leaving.contains(old->sessionId());
	if(shouldDetachOld)
	{
		const QString oldId = old->sessionId();
		logDropped(co_await old->detach(), "detach"_L1, oldId);
	}
	co_return Result<void>();
}

void Workspace::detachCurrent()
{
	++m_showSerial;
	if(m_split)
	{
		if(!m_zoomed.isEmpty())
		{
			unzoom();
		}
		if(ChatSession* old = m_current; old != nullptr)
		{
			const QString id = old->sessionId();
			const bool downgradeToTile = old->isAttached() && !m_leaving.contains(id);
			if(downgradeToTile)
			{
				m_unzooming.insert(id);
			}
			setCurrent(nullptr);
			if(downgradeToTile)
			{
				backgroundInSplit(old);
			}
		}
		return;
	}
	ChatSession* old = m_current;
	if(old == nullptr)
	{
		return;
	}
	setCurrent(nullptr);
	if(old->isAttached() && !m_leaving.contains(old->sessionId()))
	{
		detachUnwaited(old);
	}
}

QCoro::Task<> Workspace::backgroundInSplit(ChatSession* session)
{
	const QString id = session->sessionId();
	logDropped(co_await session->background(), "background"_L1, id);
	m_unzooming.remove(id);
}

void Workspace::newSession(const QString& cwd, const QJSValue& cb)
{
	createAndShow(cwd.isEmpty() ? m_cwd : cwd, wrap(cb));
}

QCoro::Task<> Workspace::createAndShow(QString cwd, Reply reply)
{
	const auto created = co_await m_directory.create(cwd, m_options.start.agent, m_options.start.model);
	if(!created)
	{
		report(reply, false, created.error().message);
		co_return;
	}
	setCwd(created->cwd);
	const Result<void> shown = co_await show(created->id);
	if(!shown)
	{
		report(reply, false, shown.error().message);
		co_return;
	}
	report(reply, true, created->id);
}

void Workspace::requestCloseSession(const QString& sessionId)
{
	if(sessionId.isEmpty())
	{
		return;
	}
	const refgui::proto::SessionInfo* info = m_directory.find(sessionId);
	const bool working = info != nullptr && info->status.state != refgui::proto::SessionStatus::State::Idle;
	if(working)
	{
		openDialog(u"close"_s, {{u"sessionId"_s, sessionId}});
		return;
	}
	closeSession(sessionId);
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
	const Result<void> closed = co_await m_directory.close(sessionId);
	if(toastFailure(closed))
	{
		co_return;
	}
	handleGone(sessionId);
}

void Workspace::moveRunningSession(const QString& sessionId, int toIndex)
{
	const auto& live = m_directory.live();
	const int count = live.count();
	if(sessionId.isEmpty() || count <= 1)
	{
		return;
	}
	QStringList order;
	order.reserve(count);
	int fromIndex = -1;
	for(int row = 0; row < count; ++row)
	{
		const QString id = live.data(live.index(row), refgui::workspace::SessionListModel::SessionIdRole).toString();
		if(id == sessionId)
		{
			fromIndex = row;
		}
		order.append(id);
	}
	if(fromIndex < 0)
	{
		return;
	}
	const int pinnedCount = live.pinnedCount();
	const bool movingPinned = live.isPinnedAt(fromIndex);
	const int minIndex = movingPinned ? 0 : pinnedCount;
	const int maxIndex = movingPinned ? std::max(0, pinnedCount - 1) : count - 1;
	const int clamped = std::clamp(toIndex, minIndex, maxIndex);
	if(clamped == fromIndex)
	{
		return;
	}
	order.move(fromIndex, clamped);
	preservePendingRestoreOrder(order);
	m_settings.setSidebarRunningOrder(order);
}

void Workspace::restoreStashedSessionAt(const QString& groupId, const QString& sessionId, int targetIndex, bool pinned)
{
	if(groupId.isEmpty() || sessionId.isEmpty())
	{
		return;
	}
	if(targetIndex < 0)
	{
		m_stash.restoreGroupMember(groupId, sessionId);
		return;
	}
	const auto& live = m_directory.live();
	const int count = live.count();
	QStringList order;
	order.reserve(count + 1);
	int existingIndex = -1;
	for(int row = 0; row < count; ++row)
	{
		const QString id = live.data(live.index(row), refgui::workspace::SessionListModel::SessionIdRole).toString();
		if(id == sessionId)
		{
			existingIndex = row;
		}
		order.append(id);
	}
	if(existingIndex >= 0)
	{
		const int toIndex = existingIndex < targetIndex ? targetIndex - 1 : targetIndex;
		moveRunningSession(sessionId, toIndex);
		m_stash.restoreGroupMember(groupId, sessionId);
		return;
	}
	const int pinnedCount = live.pinnedCount();
	const int minIndex = pinned ? 0 : pinnedCount;
	const int maxIndex = pinned ? pinnedCount : count;
	const int clamped = std::clamp(targetIndex, minIndex, maxIndex);
	order.removeAll(sessionId);
	order.insert(clamped, sessionId);
	m_pendingRestoreOrderIds.insert(sessionId);
	preservePendingRestoreOrder(order);
	m_settings.setSidebarRunningOrder(order);
	m_stash.restoreGroupMember(groupId, sessionId);
}

void Workspace::preservePendingRestoreOrder(QStringList& order) const
{
	if(m_pendingRestoreOrderIds.isEmpty())
	{
		return;
	}
	const QStringList previous = m_settings.sidebarRunningOrder();
	for(qsizetype i = 0; i < previous.size(); ++i)
	{
		const QString& id = previous.at(i);
		if(!m_pendingRestoreOrderIds.contains(id) || order.contains(id))
		{
			continue;
		}
		int insertAt = 0;
		for(qsizetype prevIdx = i - 1; prevIdx >= 0; --prevIdx)
		{
			const int anchorIndex = static_cast<int>(order.indexOf(previous.at(prevIdx)));
			if(anchorIndex >= 0)
			{
				insertAt = anchorIndex + 1;
				break;
			}
		}
		order.insert(insertAt, id);
	}
}

// A session ended, closed here or elsewhere: forgets it, and when it was the current one in single view
// switches to another live session or leaves the workspace empty.
void Workspace::handleGone(const QString& sessionId)
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
	leaveCurrent(sessionId);
}

QCoro::Task<> Workspace::leaveCurrent(QString closedId)
{
	if(m_split)
	{
		setCurrent(nullptr);
		dropChat(closedId);
		co_return;
	}
	QString next;
	for(const refgui::proto::SessionInfo& info : m_directory.liveSessions())
	{
		if(info.id != closedId && !m_leaving.contains(info.id))
		{
			next = info.id;
			break;
		}
	}
	if(!next.isEmpty())
	{
		const Result<void> shown = co_await show(next);
		toastFailure(shown);
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
	emit sessionDropped(gone);
	m_stage.deleteWhenLeft(gone); // its tile may still be fading out
}

// Reacts to the backend's session list: sessions that disappeared were closed.
void Workspace::onLiveChanged()
{
	QHash<QString, QString> now;
	QSet<QString> nowPinned;
	struct NewlyPinned
	{
		QString id;
		qint64 pinnedAt = 0;
	};
	QList<NewlyPinned> newlyPinned;
	for(const refgui::proto::SessionInfo& info : m_directory.liveSessions())
	{
		now.insert(info.id, info.cwd);
		if(info.isPinned())
		{
			nowPinned.insert(info.id);
			if(m_seenLive.contains(info.id) && !m_pinnedLive.contains(info.id))
			{
				newlyPinned.append({info.id, info.pinnedAt.value_or(0)});
			}
		}
	}
	const QHash<QString, QString> before = std::exchange(m_seenLive, now);
	m_pinnedLive = std::move(nowPinned);
	for(auto it = before.cbegin(); it != before.cend(); ++it)
	{
		if(!now.contains(it.key()))
		{
			handleGone(it.key());
		}
	}
	// Closed sessions the backend no longer lists cannot come back.
	m_leaving.removeIf([this, &now](const QString& id) { return !now.contains(id) && !m_chats.contains(id); });
	m_pendingRestoreOrderIds.removeIf([&now](const QString& id) { return now.contains(id); });
	const QStringList currentOrder = m_settings.sidebarRunningOrder();
	if(!currentOrder.isEmpty())
	{
		QStringList pruned;
		pruned.reserve(currentOrder.size());
		for(const QString& id : currentOrder)
		{
			if(now.contains(id) || m_pendingRestoreOrderIds.contains(id))
			{
				pruned.append(id);
			}
		}
		if(!newlyPinned.isEmpty())
		{
			std::ranges::sort(newlyPinned, [](const NewlyPinned& a, const NewlyPinned& b) {
				return a.pinnedAt < b.pinnedAt;
			});
			for(const NewlyPinned& pinnedInfo : newlyPinned)
			{
				pruned.removeAll(pinnedInfo.id);
				pruned.prepend(pinnedInfo.id);
			}
		}
		if(pruned != currentOrder)
		{
			m_settings.setSidebarRunningOrder(pruned);
		}
	}
	if(m_split)
	{
		syncTiles();
	}
	updateCommandContext();
	syncConfigCwd();
	emit splitCreatorsChanged();
	emit backendChanged(); // canRestartBackend depends on the sessions' states
}
