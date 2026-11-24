#pragma once

#include "client/backend.h"
#include "client/result.h"
#include "proto/types.h"
#include "workspace/connection.h"
#include "workspace/session_directory.h"

#include <QCoroTask>
#include <QList>
#include <QObject>
#include <QString>
#include <QStringList>
#include <QVariantList>
#include <QVariantMap>

namespace refgui::workspace
{

// Manages stashed session groups for the workspace and exposes reactive group/member state to QML.
class StashController : public QObject
{
	Q_OBJECT
	Q_PROPERTY(int groupCount READ groupCount NOTIFY groupsChanged)
	Q_PROPERTY(QVariantList groups READ groupsVariant NOTIFY groupsChanged)
	Q_PROPERTY(bool loading READ isLoading NOTIFY busyChanged)
	Q_PROPERTY(bool busy READ isBusy NOTIFY busyChanged)

public:
	StashController(Connection& connection, SessionDirectory& directory, QObject* parent = nullptr);

	int groupCount() const { return static_cast<int>(m_groups.size()); }
	const QList<proto::StashGroup>& groups() const { return m_groups; }
	const QVariantList& groupsVariant() const { return m_groupsVariant; }
	bool isLoading() const { return m_loading; }
	bool isBusy() const { return m_inFlight > 0; }

	void reset();

	Q_INVOKABLE void refresh();
	Q_INVOKABLE void stashAll(const QString& name = {});
	Q_INVOKABLE void createGroup(const QString& name, const QStringList& sessionIds, bool stashLive = true);
	Q_INVOKABLE void renameGroup(const QString& groupId, const QString& name);
	Q_INVOKABLE void setGroupLocked(const QString& groupId, bool locked);
	Q_INVOKABLE void deleteGroup(const QString& groupId);
	Q_INVOKABLE void addToGroup(const QString& groupId, const QString& sessionId, int position = -1, bool stashLive = true);
	Q_INVOKABLE void moveGroupMember(const QString& fromGroupId, const QString& toGroupId, const QString& sessionId, int position, bool stashLive = true);
	Q_INVOKABLE void removeFromGroup(const QString& groupId, const QString& sessionId);
	Q_INVOKABLE void mergeGroups(const QString& sourceGroupId, const QString& targetGroupId);
	Q_INVOKABLE void restoreGroup(const QString& groupId);
	Q_INVOKABLE void restoreGroupMember(const QString& groupId, const QString& sessionId);
	Q_INVOKABLE void assignSessionToGroups(const QString& sessionId, const QStringList& targetGroupIds);
	Q_INVOKABLE void createGroupFromMember(const QString& fromGroupId, const QString& sessionId, const QString& name = {});
	Q_INVOKABLE QStringList groupIdsForSession(const QString& sessionId) const;

signals:
	void groupsChanged();
	void busyChanged();
	void toastRequested(const QString& level, const QString& text);

private:
	void onFrame(const proto::ServerFrame& frame);
	void setGroups(QList<proto::StashGroup> groups);
	void rebuildVariant();
	QVariantMap groupToMap(const proto::StashGroup& group) const;
	QVariantMap memberToMap(const proto::StashMember& member) const;

	template<typename T, typename OnOk>
	QCoro::Task<> run(QCoro::Task<client::Result<T>> request, OnOk onOk);
	QCoro::Task<> refreshTask();
	QCoro::Task<> assignSessionTask(QString sessionId, QStringList targetGroupIds);
	QCoro::Task<> createGroupFromMemberTask(QString fromGroupId, QString sessionId, std::optional<QString> name);

	Connection& m_connection;
	client::Backend& m_backend;
	SessionDirectory& m_directory;
	QList<proto::StashGroup> m_groups;
	QVariantList m_groupsVariant;
	bool m_loading = false;
	int m_inFlight = 0;
	int m_serial = 0;
	int m_refreshSerial = 0;
};

} // namespace refgui::workspace
