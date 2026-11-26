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
	Q_PROPERTY(int pinnedCount READ pinnedCount NOTIFY pinnedCountChanged)
	// Shows only the sessions whose title, name, directory or id contains it, ignoring case.
	Q_PROPERTY(QString filter READ filter WRITE setFilter NOTIFY filterChanged)
	Q_PROPERTY(CreatorFilter creatorFilter READ creatorFilter WRITE setCreatorFilter NOTIFY creatorFilterChanged)
	Q_PROPERTY(QString creatorId READ creatorId WRITE setCreatorId NOTIFY creatorIdChanged)
	Q_PROPERTY(QString cwdFilter READ cwdFilter WRITE setCwdFilter NOTIFY cwdFilterChanged)
public:
	enum class CreatorFilter
	{
		All,
		UserCreated,
		ByCreator,
	};
	Q_ENUM(CreatorFilter)

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
		ParentIdRole,
		CreatorNameRole,
		RootIdRole,
		RootNameRole,
		RootColorIndexRole,
		PinnedRole,
		PinnedAtRole,
	};
	explicit SessionListModel(QObject* parent = nullptr);

	// Replaces the sessions. Live ones come first, the newest created first; the others follow,
	// the most recently updated first. Rows that stay are updated, not recreated.
	void setSessions(const QList<proto::SessionInfo>& sessions);
	void setFilter(const QString& filter);
	const QString& filter() const { return m_filter; }
	void setCreatorFilter(CreatorFilter filter);
	CreatorFilter creatorFilter() const { return m_creatorFilter; }
	void setCreatorId(const QString& id);
	const QString& creatorId() const { return m_creatorId; }
	void setCwdFilter(const QString& cwd);
	const QString& cwdFilter() const { return m_cwdFilter; }
	void setFilters(const QString& cwd, CreatorFilter creatorFilter, const QString& creatorId);
	void setKnownSessions(const QList<proto::SessionInfo>& sessions);
	QString displayName(const QString& sessionId) const;
	void setCurrentId(const QString& id);
	void setTileIds(const QStringList& ids);
	void setLiveOrder(const QStringList& ids);

	// The row of the session, or -1 when it is not listed (also when filtered out).
	Q_INVOKABLE int indexOf(const QString& sessionId) const;
	Q_INVOKABLE bool isLiveAt(int row) const;
	Q_INVOKABLE bool isPinnedAt(int row) const;
	// Whether the unfiltered list has the session.
	bool contains(const QString& sessionId) const { return m_all.contains(sessionId); }
	const proto::SessionInfo* find(const QString& sessionId) const;
	int count() const { return int(m_rows.size()); }
	int pinnedCount() const { return m_pinnedCount; }
	// All roles of the session (also when filtered out) as a map with the role names as keys; empty when unknown.
	Q_INVOKABLE QVariantMap get(const QString& sessionId) const;
	Q_INVOKABLE QVariantMap rowAt(int row) const;
	int rowCount(const QModelIndex& parent = {}) const override;
	QVariant data(const QModelIndex& index, int role) const override;
	QHash<int, QByteArray> roleNames() const override;

signals:
	void countChanged();
	void pinnedCountChanged();
	void filterChanged();
	void creatorFilterChanged();
	void creatorIdChanged();
	void cwdFilterChanged();
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
		proto::SessionStatus::State state = proto::SessionStatus::State::Idle;
		qint64 busySince = 0;
		qint64 updatedAt = 0;
		qint64 createdAt = 0;
		int todoDone = 0;
		int todoTotal = 0;
		QString lastError;
		int attached = 0;
		bool live = false;
		QString parentId;
		QString creatorName;
		QString creatorRawName;
		QString creatorTitle;
		QString rootId;
		QString rootName;
		int rootColorIndex = -1;
		bool pinned = false;
		qint64 pinnedAt = 0;
		bool operator==(const Row&) const = default;
	};

	const proto::SessionInfo* findKnown(const QString& sessionId) const;
	Row toRow(const proto::SessionInfo& info) const;
	QVariant roleData(const Row& row, int role) const;
	bool matchesFilter(const Row& row) const;
	bool before(const Row& a, const Row& b) const;
	void rebuild();
	void removeGone(const QList<Row>& wanted);
	qsizetype findRowFrom(qsizetype start, const QString& sessionId) const;
	void placeRow(qsizetype i, const Row& wanted);
	void emitRowChanged(const QString& id);

	QString m_filter;
	CreatorFilter m_creatorFilter = CreatorFilter::All;
	QString m_creatorId;
	QString m_cwdFilter;
	QString m_currentId;
	QStringList m_tileIds;
	QHash<QString, int> m_liveOrder;
	QHash<QString, proto::SessionInfo> m_all;
	QHash<QString, proto::SessionInfo> m_known;
	QList<Row> m_rows; // the listed ones, in order
	QHash<QString, int> m_liveRootColorIndices;
	QHash<QString, int> m_allRootColorIndices;
	int m_pinnedCount = 0;
};

} // namespace refgui::workspace
