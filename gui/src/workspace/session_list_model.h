#pragma once

#include "proto/types.h"

#include <QAbstractListModel>
#include <QHash>
#include <QList>
#include <QString>
#include <QStringList>

namespace refgui::workspace
{

// A list of sessions for the sidebar. It follows the backend's lists by id, so a view
// keeps its scroll position and animates only the rows that really came, went or moved.
class SessionListModel : public QAbstractListModel
{
	Q_OBJECT
	Q_PROPERTY(int count READ count NOTIFY countChanged)

public:
	enum Role
	{
		SessionIdRole = Qt::UserRole + 1,
		NameRole,         // set by the user, may be empty
		TitleRole,        // made by the backend, may be empty
		DisplayTitleRole, // name, else title, else the directory's name
		CwdRole,
		DirNameRole, // last component of the directory
		AgentRole,
		ModelLabelRole, // model id and variant
		StateRole,      // idle, thinking, running, waiting_permission, retrying or compacting
		BusySinceRole,  // ms since epoch, 0 when not running
		UpdatedAtRole,
		CreatedAtRole,
		TodoDoneRole,
		TodoTotalRole,
		TodoAllDoneRole,
		HasPermissionRole,
		LastErrorRole,
		HasErrorRole,
		AttachedRole, // frontends attached to it
		IsCurrentRole,
		IsTileRole,
		IsLiveRole,
	};

	enum class Order
	{
		CreatedNewestFirst,
		UpdatedNewestFirst,
	};

	SessionListModel(Order order, bool live, QObject* parent = nullptr);

	// Replaces the sessions. Rows that stay are updated, not recreated.
	void setSessions(const QList<proto::SessionInfo>& sessions);
	// Shows only the sessions whose title, name, directory or id contains `filter`, ignoring case.
	void setFilter(const QString& filter);
	const QString& filter() const { return m_filter; }
	// The session shown in the window's main view, and the ones in split view tiles.
	void setCurrentId(const QString& id);
	void setTileIds(const QStringList& ids);

	// The row of the session, or -1 when it is not listed (also when filtered out).
	int indexOf(const QString& sessionId) const;
	// Whether the unfiltered list has the session.
	bool contains(const QString& sessionId) const { return m_all.contains(sessionId); }
	const proto::SessionInfo* find(const QString& sessionId) const;
	int count() const { return int(m_rows.size()); }

	int rowCount(const QModelIndex& parent = {}) const override;
	QVariant data(const QModelIndex& index, int role) const override;
	QHash<int, QByteArray> roleNames() const override;

signals:
	void countChanged();

private:
	// What the views show of a session; rows are compared to see whether one changed.
	struct Row
	{
		QString id;
		QString name;
		QString title;
		QString cwd;
		QString agent;
		QString modelLabel;
		QString state;
		qint64 busySince = 0;
		qint64 updatedAt = 0;
		qint64 createdAt = 0;
		int todoDone = 0;
		int todoTotal = 0;
		QString lastError;
		int attached = 0;

		bool operator==(const Row&) const = default;
	};

	static Row toRow(const proto::SessionInfo& info);
	bool matchesFilter(const Row& row) const;
	bool before(const Row& a, const Row& b) const;
	void rebuild();
	void emitRowChanged(const QString& id);

	Order m_order;
	bool m_live;
	QString m_filter;
	QString m_currentId;
	QStringList m_tileIds;
	QHash<QString, proto::SessionInfo> m_all;
	QList<Row> m_rows; // the listed ones, in order
};

} // namespace refgui::workspace
