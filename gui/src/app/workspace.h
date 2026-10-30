#pragma once

#include "app/chat_session.h"
#include "app/tile_model.h"
#include "app/tile_stage.h"
#include "cli/launch_options.h"
#include "client/backend.h"
#include "workspace/backend_config.h"
#include "workspace/command_registry.h"
#include "workspace/session_directory.h"
#include "workspace/settings.h"

#include <QCoroTask>
#include <QHash>
#include <QJSValue>
#include <QObject>
#include <QSet>
#include <QVariantMap>
#include <QtQmlIntegration>

#include <functional>

class QJSEngine;
class QQmlEngine;

// Everything the window shows, above single sessions: the connection to the backend, the
// sessions opened in this window, which one is current, split view, and the window-wide
// actions (the commands). The one instance is created in main() and handed to QML as the
// singleton `Workspace`.
//
// Requests to the backend that need an answer on screen take a JavaScript callback
// `cb(ok, result)`; `result` is the reason when `ok` is false. Without a callback a failure
// is shown as a toast.
class Workspace : public QObject
{
	Q_OBJECT
	QML_ELEMENT
	QML_SINGLETON
	// Connection and the current session.
	Q_PROPERTY(ChatSession* current READ current NOTIFY currentChanged)
	Q_PROPERTY(QString currentId READ currentId NOTIFY currentChanged)
	Q_PROPERTY(QString connectionState READ connectionState NOTIFY connectionChanged)
	Q_PROPERTY(QString connectionText READ connectionText NOTIFY connectionChanged)
	Q_PROPERTY(QString failureText READ failureText NOTIFY failureTextChanged)
	Q_PROPERTY(QString cwd READ cwd NOTIFY cwdChanged)
	Q_PROPERTY(QStringList visibleSessionIds READ visibleSessionIds NOTIFY visibleSessionIdsChanged)
	Q_PROPERTY(bool windowActive READ isWindowActive WRITE setWindowActive NOTIFY windowActiveChanged)
	// What the backend offers and the session lists.
	Q_PROPERTY(refgui::workspace::Settings* settings READ settings CONSTANT)
	Q_PROPERTY(refgui::workspace::BackendConfig* config READ config CONSTANT)
	Q_PROPERTY(QVariantList mcpServers READ mcpServers NOTIFY mcpServersChanged)
	Q_PROPERTY(refgui::workspace::SessionListModel* sessions READ sessions CONSTANT)
	Q_PROPERTY(refgui::workspace::SessionListModel* history READ history CONSTANT)
	// Live and persisted sessions in one list with a filter of its own, for the session picker.
	Q_PROPERTY(refgui::workspace::SessionListModel* sessionChoices READ sessionChoices CONSTANT)
	Q_PROPERTY(bool historyLoaded READ historyLoaded NOTIFY historyLoadedChanged)
	// Split view.
	Q_PROPERTY(QAbstractItemModel* tiles READ tiles CONSTANT)
	Q_PROPERTY(TileStage* tileStage READ tileStage CONSTANT)
	Q_PROPERTY(bool splitView READ isSplitView NOTIFY splitViewChanged)
	Q_PROPERTY(QString focusedTileId READ focusedTileId NOTIFY focusedTileChanged)
	Q_PROPERTY(QString zoomedId READ zoomedId NOTIFY zoomedChanged)
	// The dialog on screen: "" for none, else palette, settings, sessions, models, agents,
	// variants, help, mcp, peers, export, cd, new-in, close, rename or delete-history.
	Q_PROPERTY(QString dialog READ dialog NOTIFY dialogChanged)
	Q_PROPERTY(QVariantMap dialogArgs READ dialogArgs NOTIFY dialogChanged)
	// The backend.
	Q_PROPERTY(bool backendLocal READ isBackendLocal CONSTANT)
	Q_PROPERTY(bool protocolOutdated READ isProtocolOutdated NOTIFY backendChanged)
	Q_PROPERTY(QString backendVersion READ backendVersion NOTIFY backendChanged)
	Q_PROPERTY(int protocolVersion READ protocolVersion NOTIFY backendChanged)
	Q_PROPERTY(int requiredProtocol READ requiredProtocol CONSTANT)
	Q_PROPERTY(bool canRestartBackend READ canRestartBackend NOTIFY backendChanged)

public:
	// `settingsPath` is the file the settings are kept in.
	Workspace(refgui::cli::LaunchOptions options, QString settingsPath, QObject* parent = nullptr);
	~Workspace() override;

	// QML gets the instance given to setInstance().
	static void setInstance(Workspace* instance);
	static Workspace* instance();
	static Workspace* create(QQmlEngine* qmlEngine, QJSEngine* jsEngine);

	ChatSession* current() const { return m_current; }
	QString currentId() const;
	// "connecting", "starting", "connected" or "disconnected".
	QString connectionState() const;
	// What to tell the user about the connection. Empty while connected.
	QString connectionText() const;
	// Why the session could not be opened. Empty when all is well.
	QString failureText() const { return m_failure; }
	// The directory new sessions start in.
	const QString& cwd() const { return m_cwd; }
	// The sessions the user can see now: the current one, or every tile in split view.
	QStringList visibleSessionIds() const;
	// The window is in the foreground. Set by the window; the notification logic needs it with the above.
	bool isWindowActive() const { return m_windowActive; }
	void setWindowActive(bool active);

	refgui::workspace::Settings* settings() { return &m_settings; }
	refgui::workspace::BackendConfig* config() { return &m_config; }
	refgui::workspace::CommandRegistry* commands() { return &m_commands; }
	QVariantList mcpServers() const { return m_config.mcpServers(); }
	refgui::workspace::SessionListModel* sessions() { return &m_directory.live(); }
	refgui::workspace::SessionListModel* history() { return &m_directory.history(); }
	refgui::workspace::SessionListModel* sessionChoices() { return &m_directory.choices(); }
	bool historyLoaded() const { return m_directory.historyLoaded(); }
	QAbstractItemModel* tiles() { return &m_tiles; }
	TileStage* tileStage() { return &m_stage; }
	bool isSplitView() const { return m_split; }
	const QString& focusedTileId() const { return m_focusedTile; }
	const QString& zoomedId() const { return m_zoomed; }
	const QString& dialog() const { return m_dialog; }
	const QVariantMap& dialogArgs() const { return m_dialogArgs; }
	bool isBackendLocal() const;
	bool isProtocolOutdated() const { return m_backend.hello().protocol < refgui::proto::ProtocolVersion && m_backend.state() == refgui::client::Backend::State::Connected; }
	QString backendVersion() const { return m_backend.hello().serverVersion; }
	int protocolVersion() const { return m_backend.hello().protocol; }
	static constexpr int requiredProtocol() { return refgui::proto::ProtocolVersion; }
	// Restarting is only offered when no session would lose work.
	bool canRestartBackend() const;

	// The chat session for `sessionId`, made when the window does not have it yet. Null for an empty id.
	Q_INVOKABLE ChatSession* session(const QString& sessionId);

	// Opens a session (live or from history) and makes it current. In split view it also gets the focus.
	Q_INVOKABLE void openSession(const QString& sessionId);
	// A new session in `cwd` (the window's directory when empty), made current.
	Q_INVOKABLE void newSession(const QString& cwd = {}, const QJSValue& cb = {});
	// Closes the session; its conversation stays in history. Closing the last one opens a new session in its directory.
	Q_INVOKABLE void closeSession(const QString& sessionId);
	// Removes the session from history for good.
	Q_INVOKABLE void deleteHistory(const QString& sessionId);
	Q_INVOKABLE void renameSession(const QString& sessionId, const QString& name, const QJSValue& cb = {});
	Q_INVOKABLE void setSessionCwd(const QString& sessionId, const QString& path, const QJSValue& cb = {});
	Q_INVOKABLE void setModel(const QString& sessionId, const QString& provider, const QString& model, const QString& variant = {});
	Q_INVOKABLE void setAgent(const QString& sessionId, const QString& agent);
	// An empty variant rotates to the model's next one.
	Q_INVOKABLE void setVariant(const QString& sessionId, const QString& variant = {});
	// The next (`direction` 1) or previous (-1) agent.
	Q_INVOKABLE void cycleAgent(const QString& sessionId, int direction);
	Q_INVOKABLE void compact(const QString& sessionId);
	Q_INVOKABLE void exportTranscript(const QString& sessionId, const QString& path, const QJSValue& cb);
	// `command` is peers, peers-name, peers-inbox or peers-outbox; `cb` gets the text to show.
	Q_INVOKABLE void peerCommand(const QString& sessionId, const QString& command, const QString& args, const QJSValue& cb);
	// The sub-directories of `path`; `cb` gets {path, dirs}.
	Q_INVOKABLE void listDirs(const QString& path, const QJSValue& cb);
	Q_INVOKABLE void refreshMcp() { m_config.refreshMcp(); }
	Q_INVOKABLE void refreshHistory() { m_directory.refreshHistory(); }

	Q_INVOKABLE void toggleSplit();
	Q_INVOKABLE void focusTile(const QString& sessionId);
	Q_INVOKABLE void zoomTile(const QString& sessionId);
	Q_INVOKABLE void unzoom();

	// The user picked a theme (system, light or dark). The window changes it under a crossfade
	// and then sets Settings.theme.
	Q_INVOKABLE void requestTheme(const QString& theme) { emit themeChangeRequested(theme); }
	// Shows a toast. `level` is info, warn or error.
	Q_INVOKABLE void showToast(const QString& level, const QString& text) { toast(level, text); }
	// The row of a live or listed session as a map (see SessionList), empty when unknown.
	Q_INVOKABLE QVariantMap info(const QString& sessionId) const { return m_directory.choices().get(sessionId); }

	Q_INVOKABLE void openDialog(const QString& id, const QVariantMap& args = {});
	Q_INVOKABLE void closeDialog();

	// Restarts the backend (stops it; the connection then starts a new one). Only when canRestartBackend.
	Q_INVOKABLE void restartBackend();

	// The language changed: texts built here are built again.
	void retranslate();

signals:
	void currentChanged();
	void connectionChanged();
	void failureTextChanged();
	void cwdChanged();
	void visibleSessionIdsChanged();
	void windowActiveChanged();
	void mcpServersChanged();
	void historyLoadedChanged();
	void splitViewChanged();
	void focusedTileChanged();
	void zoomedChanged();
	void dialogChanged();
	void backendChanged();
	void themeChangeRequested(const QString& theme);
	// A message for the user that is not about one session. `level` is info, warn or error.
	void toastRequested(const QString& level, const QString& text);
	// The user asked for a new session in a folder of their choice; the window shows a folder dialog
	// (the backend is on this machine). A remote backend gets the `new-in` dialog instead.
	void newSessionFolderRequested();
	// The first session is open.
	void opened(const QString& sessionId);
	// The first session could not be opened.
	void openFailed(const QString& reason);

private:
	using Done = refgui::client::Result<void>;
	// What a request that was asked for with a callback tells: success, and the answer or the reason.
	using Reply = std::function<void(bool ok, const QVariant& payload)>;

	// Opening, closing and switching.
	QCoro::Task<> openFirstSession();
	QCoro::Task<Done> show(QString sessionId);
	QCoro::Task<> createAndShow(QString cwd, Reply reply);
	QCoro::Task<> closeTask(QString sessionId);
	QCoro::Task<> leaveCurrent(QString closedId, QString cwd);
	void handleGone(const QString& sessionId, const QString& cwd);
	void setCurrent(ChatSession* session);
	void onLiveChanged();
	void dropChat(const QString& sessionId);
	ChatSession& chat(const QString& sessionId);
	QString nameOf(const QString& sessionId) const;
	QString targetId(const QString& sessionId) const;
	void onBackendConnected();

	// Management requests.
	Reply wrap(const QJSValue& cb) const;
	void report(const Reply& reply, bool ok, const QVariant& payload);
	void toast(const QString& level, const QString& text);
	void toastFrom(const refgui::proto::ServerFrame& frame);
	template<typename T, typename OnOk>
	QCoro::Task<> perform(QCoro::Task<refgui::client::Result<T>> request, Reply reply, OnOk onOk);
	void exportTo(const QString& sessionId, const QString& path, Reply reply);
	void peerText(const QString& sessionId, const QString& command, const QString& args, Reply reply);

	// Split view.
	QCoro::Task<> enterSplit();
	QCoro::Task<> leaveSplit();
	QCoro::Task<> attachTile(ChatSession* tile);
	QCoro::Task<> zoomTask(QString sessionId);
	QCoro::Task<> unzoomTask();
	void syncTiles();
	QStringList wantedTileIds() const;
	void setFocusedTile(const QString& sessionId);

	// Commands.
	void registerCommands();
	void updateCommandContext();
	void runOnSession(const QString& sessionId, const char* method);
	void runCustomCommand(const QString& name, const QString& args, const QString& sessionId);

	refgui::cli::LaunchOptions m_options;
	QString m_cwd;
	refgui::workspace::Settings m_settings;
	refgui::workspace::CommandRegistry m_commands;
	// The chat sessions refer to the backend, so it is declared before them.
	refgui::client::Backend m_backend;
	refgui::workspace::SessionDirectory m_directory{m_backend};
	refgui::workspace::BackendConfig m_config{m_backend};
	QHash<QString, ChatSession*> m_chats;
	TileModel m_tiles;
	TileStage m_stage{&m_tiles};
	ChatSession* m_current = nullptr;
	QHash<QString, QString> m_seenLive; // sessions that were in the live list, with their directory; one that leaves it was closed
	QSet<QString> m_leaving; // closed sessions whose chat session is about to go
	QString m_failure;
	QString m_dialog;
	QVariantMap m_dialogArgs;
	QString m_focusedTile;
	QString m_zoomed;
	QSet<QString> m_tileAttaching;
	QJSEngine* m_engine = nullptr;
	int m_showSerial = 0;
	bool m_split = false;
	bool m_started = false;
	bool m_windowActive = false;
};
