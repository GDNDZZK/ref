#pragma once

#include "proto/types.h"

#include <QAbstractListModel>
#include <QHash>
#include <QList>
#include <QString>
#include <QStringList>
#include <QVariantMap>

namespace refgui::workspace
{

// A list of sessions for the sidebar. It follows the backend's lists by id, so a view
// keeps its scroll position and animates only the rows that really came, went or moved.
class SessionListModel : public QAbstractListModel
{
	Q_OBJECT
	Q_PROPERTY(int count READ count NOTIFY countChanged)
	// Shows only the sessions whose title, name, directory or id contains it, ignoring case.
	Q_PROPERTY(QString filter READ filter WRITE setFilter NOTIFY filterChanged)

public:
	enum Role
	{
		SessionIdRole = Qt::UserRole + 1,
		NameRole,         // set by the user, may be empty
		TitleRole,        // made by the backend, may be empty
		DisplayTitleRole, // live: name, else title, else the directory; stored: title (one line), else name, else the directory
		CwdRole,
		DirNameRole, // last component of the directory
		AgentRole,
		ModelLabelRole, // model id and variant
		ProviderRole,
		ModelIdRole,
		VariantRole, // empty without
		StateRole,      // "activity" in QML: idle, thinking, running, waiting_permission, waiting_question, retrying or compacting
		BusySinceRole,  // ms since epoch, 0 when not running
		UpdatedAtRole,
		CreatedAtRole,
		TodoDoneRole,
		TodoTotalRole,
		TodoAllDoneRole,
		HasPermissionRole, // blocked on the user: a permission or a question
		LastErrorRole,
		HasErrorRole,
		AttachedRole, // frontends attached to it
		IsCurrentRole,
		IsTileRole,
		IsLiveRole,
	};

	explicit SessionListModel(QObject* parent = nullptr);

	// Replaces the sessions. Live ones come first, the newest created first; the others follow,
	// the most recently updated first. Rows that stay are updated, not recreated.
	void setSessions(const QList<proto::SessionInfo>& sessions);
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
	// All roles of the session (also when filtered out) as a map with the role names as keys; empty when unknown.
	Q_INVOKABLE QVariantMap get(const QString& sessionId) const;

	int rowCount(const QModelIndex& parent = {}) const override;
	QVariant data(const QModelIndex& index, int role) const override;
	QHash<int, QByteArray> roleNames() const override;

signals:
	void countChanged();
	void filterChanged();

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
		QString provider;
		QString modelId;
		QString variant;
		QString state;
		qint64 busySince = 0;
		qint64 updatedAt = 0;
		qint64 createdAt = 0;
		int todoDone = 0;
		int todoTotal = 0;
		QString lastError;
		int attached = 0;
		bool live = false;

		bool operator==(const Row&) const = default;
	};

	static Row toRow(const proto::SessionInfo& info);
	QVariant roleData(const Row& row, int role) const;
	bool matchesFilter(const Row& row) const;
	bool before(const Row& a, const Row& b) const;
	void rebuild();
	void emitRowChanged(const QString& id);

	QString m_filter;
	QString m_currentId;
	QStringList m_tileIds;
	QHash<QString, proto::SessionInfo> m_all;
	QList<Row> m_rows; // the listed ones, in order
};

} // namespace refgui::workspace
