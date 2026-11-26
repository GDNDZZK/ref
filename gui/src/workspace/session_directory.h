#pragma once

#include "client/backend.h"
#include "client/result.h"
#include "workspace/connection.h"
#include "workspace/session_list_model.h"

#include <QCoroTask>
#include <QHash>
#include <QList>
#include <QObject>
#include <QTimer>
#include <optional>

namespace refgui::workspace
{

// The sessions the backend knows: the live ones, kept up to date from the backend's
// session list frames, and the persisted ones (history) that are not live.
class SessionDirectory : public QObject
{
	Q_OBJECT

public:
	template<typename T>
	using Result = client::Result<T>;

	// How many persisted sessions the sidebar can search.
	static constexpr int HistoryLimit = 200;

	explicit SessionDirectory(Connection& connection, QObject* parent = nullptr);

	// The live sessions, for the sidebar's running list.
	SessionListModel& live() { return m_live; }
	// The sessions on disk that are not live, filtered by the sidebar's search.
	SessionListModel& history() { return m_history; }
	// Both together with a filter of their own, for the session picker.
	SessionListModel& choices() { return m_choices; }
	const SessionListModel& choices() const { return m_choices; }
	// The live session, or null.
	const proto::SessionInfo* find(const QString& sessionId) const { return m_live.find(sessionId); }
	// Any known live or persisted session, or null.
	const proto::SessionInfo* findAny(const QString& sessionId) const;
	// Every live session, newest first.
	const QList<proto::SessionInfo>& liveSessions() const { return m_liveSessions; }

	// Every live session followed by every non-live persisted session.
	QList<proto::SessionInfo> allSessions() const;
	// Configures history filters. Updates SessionListModel immediately and triggers backend re-query when `refreshNow`.
	void setHistoryFilter(const QString& cwd, SessionListModel::CreatorFilter creatorFilter, const QString& creatorId, bool refreshNow = true);
	// Sets the history search keyword. Filters SessionListModel immediately and debounces backend re-query.
	void setHistoryQuery(const QString& query);
	QString sessionLabel(const QString& sessionId) const { return m_history.displayName(sessionId); }
	// Asks the backend again. Immediate when `immediate`, otherwise coalesced after burst changes.
	void refreshHistory(bool immediate = false);
	bool historyLoaded() const { return m_historyLoaded; }
	// Drops a persisted session from the list, e.g. after it was deleted.
	void forgetPersisted(const QString& sessionId);
	// Takes `info` in, e.g. the answer to a rename, without waiting for the backend's list frame.
	void update(const proto::SessionInfo& info);
	void setLiveOrder(const QStringList& ids);
	// Clears cached session lists and stops pending refresh timers (e.g. on backend switch).
	void reset();

	// Requests about one session by id, attached here or not. Those that answer with the
	// session's new info take it in with update().
	QCoro::Task<Result<void>> rename(QString sessionId, QString name);
	QCoro::Task<Result<proto::SessionInfo>> setPinned(QString sessionId, bool pinned);
	QCoro::Task<Result<void>> setCwd(QString sessionId, QString path);
	QCoro::Task<Result<void>> setModel(QString sessionId, proto::ModelRef model);
	QCoro::Task<Result<void>> setAgent(QString sessionId, QString agent);
	QCoro::Task<Result<void>> setVariant(QString sessionId, std::optional<QString> variant);
	QCoro::Task<Result<void>> compact(QString sessionId);
	// The answer is the path written.
	QCoro::Task<Result<QString>> exportTranscript(QString sessionId, QString path);
	// The answer is the text the command printed.
	QCoro::Task<Result<QString>> peerCommand(QString sessionId, QString command, QString args);
	// Ends a live session.
	QCoro::Task<Result<void>> close(QString sessionId);
	// Deletes a persisted session.
	QCoro::Task<Result<void>> remove(QString sessionId);
	// Starts a session in `cwd` and takes its info in.
	QCoro::Task<Result<proto::SessionInfo>> create(QString cwd, std::optional<QString> agent, std::optional<proto::ModelRef> model);
	// The most recent persisted session in `cwd`, if there is one.
	QCoro::Task<Result<std::optional<proto::SessionInfo>>> newestIn(QString cwd);
	// The directories in `path`, for choosing where a session runs.
	QCoro::Task<Result<proto::DirListing>> listDirs(QString path, std::optional<QString> cwd = {}, bool includeFiles = false);
	// Follows the events of every live session (split view) or not. Sent again after a
	// reconnect, since the new connection knows nothing of the old subscription.
	QCoro::Task<Result<void>> setWatchAll(bool enabled);

signals:
	// The live list changed (a session came, went or changed).
	void liveChanged();
	void historyLoadedChanged();
	void historyUpdated();
	// A request for the lists failed.
	void failed(const QString& reason);

private:
	// Waits for a request answering with a session's info and takes the info in.
	QCoro::Task<Result<void>> adopt(QCoro::Task<Result<proto::SessionInfo>> request);
	// Sends watch_all again on a new connection; nobody waits for it, so a failure is logged.
	QCoro::Task<> resubscribe();
	QCoro::Task<> loadLive();
	QCoro::Task<> loadHistory();
	QCoro::Task<> loadUnfilteredHistory();
	bool hasSidebarFilter() const;
	void setLive(QList<proto::SessionInfo> sessions);
	void updatePersisted(const proto::SessionInfo& info);
	void applyHistory();
	void onFrame(const proto::ServerFrame& frame);

	client::Backend& m_backend;
	SessionListModel m_live;
	SessionListModel m_history;
	SessionListModel m_choices;
	QList<proto::SessionInfo> m_liveSessions;
	QList<proto::SessionInfo> m_persisted; // unfiltered persisted sessions for allSessions() and choices()
	QList<proto::SessionInfo> m_sidebarPersisted; // sidebar-filtered persisted sessions for history()
	QHash<QString, proto::SessionInfo> m_knownSessions;
	QTimer m_historyTimer;
	QTimer m_historySearchTimer;
	QString m_historyCwd;
	QString m_historyCreator;
	QString m_historyQuery;
	int m_historyRequest = 0;
	int m_unfilteredHistoryRequest = 0;
	bool m_historyLoaded = false;
	bool m_watchAll = false;
};

} // namespace refgui::workspace
