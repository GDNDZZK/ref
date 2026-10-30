#pragma once

#include "client/backend.h"
#include "workspace/session_list_model.h"

#include <QCoroTask>
#include <QList>
#include <QObject>
#include <QTimer>

namespace refgui::workspace
{

// The sessions the backend knows: the live ones, kept up to date from the backend's
// session list frames, and the persisted ones (history) that are not live.
class SessionDirectory : public QObject
{
	Q_OBJECT

public:
	// How many persisted sessions the sidebar can search.
	static constexpr int HistoryLimit = 200;

	explicit SessionDirectory(client::Backend& backend, QObject* parent = nullptr);

	SessionListModel& live() { return m_live; }
	SessionListModel& history() { return m_history; }
	const SessionListModel& history() const { return m_history; }
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

signals:
	// The live list changed (a session came, went or changed).
	void liveChanged();
	void historyLoadedChanged();
	// A request for the lists failed.
	void failed(const QString& reason);

private:
	QCoro::Task<> loadLive();
	QCoro::Task<> loadHistory();
	void setLive(QList<proto::SessionInfo> sessions);
	void applyHistory();
	void onFrame(const proto::ServerFrame& frame);

	client::Backend& m_backend;
	SessionListModel m_live{SessionListModel::Order::CreatedNewestFirst, true};
	SessionListModel m_history{SessionListModel::Order::UpdatedNewestFirst, false};
	QList<proto::SessionInfo> m_liveSessions;
	QList<proto::SessionInfo> m_persisted; // as the backend listed them; may include live ones
	QTimer m_historyTimer;
	int m_historyRequest = 0;
	bool m_historyLoaded = false;
};

} // namespace refgui::workspace
