#pragma once

#include "client/backend.h"
#include "client/result.h"
#include "workspace/connection.h"
#include "workspace/session_list_model.h"

#include <QCoroTask>
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
	// Every live session, newest first.
	const QList<proto::SessionInfo>& liveSessions() const { return m_liveSessions; }

	// Asks the backend again. The history is fetched shortly after the last call.
	void refreshHistory();
	// Whether the persisted sessions have been listed once.
	bool historyLoaded() const { return m_historyLoaded; }
	// Drops a persisted session from the list, e.g. after it was deleted.
	void forgetPersisted(const QString& sessionId);
	// Takes `info` in, e.g. the answer to a rename, without waiting for the backend's list frame.
	void update(const proto::SessionInfo& info);

	// Requests about one session by id, attached here or not. Those that answer with the
	// session's new info take it in with update().
	QCoro::Task<Result<void>> rename(QString sessionId, QString name);
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
	QCoro::Task<Result<proto::DirListing>> listDirs(QString path);
	// Follows the events of every live session (split view) or not. Sent again after a
	// reconnect, since the new connection knows nothing of the old subscription.
	QCoro::Task<Result<void>> setWatchAll(bool enabled);

signals:
	// The live list changed (a session came, went or changed).
	void liveChanged();
	void historyLoadedChanged();
	// A request for the lists failed.
	void failed(const QString& reason);

private:
	// Waits for a request answering with a session's info and takes the info in.
	QCoro::Task<Result<void>> adopt(QCoro::Task<Result<proto::SessionInfo>> request);
	// Sends watch_all again on a new connection; nobody waits for it, so a failure is logged.
	QCoro::Task<> resubscribe();
	QCoro::Task<> loadLive();
	QCoro::Task<> loadHistory();
	void setLive(QList<proto::SessionInfo> sessions);
	void applyHistory();
	void onFrame(const proto::ServerFrame& frame);

	client::Backend& m_backend;
	SessionListModel m_live;
	SessionListModel m_history;
	SessionListModel m_choices;
	QList<proto::SessionInfo> m_liveSessions;
	QList<proto::SessionInfo> m_persisted; // as the backend listed them; may include live ones
	QTimer m_historyTimer;
	int m_historyRequest = 0;
	bool m_historyLoaded = false;
	bool m_watchAll = false;
};

} // namespace refgui::workspace
