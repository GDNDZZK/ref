#include "workspace/stash_controller.h"

#include "workspace/toast_text.h"

#include <QPointer>
#include <QSet>
#include <QVariantMap>

#include <algorithm>
#include <utility>

using namespace Qt::StringLiterals;

namespace refgui::workspace
{
namespace
{

QString lastComponent(const QString& path)
{
	qsizetype end = path.size();
	while(end > 1 && (path[end - 1] == u'/' || path[end - 1] == u'\\'))
	{
		--end;
	}
	const qsizetype start = std::max(path.lastIndexOf(u'/', end - 1), path.lastIndexOf(u'\\', end - 1)) + 1;
	const QString name = path.sliced(start, end - start);
	return name.isEmpty() ? path : name;
}

QString sessionDisplayTitle(bool live, const proto::SessionInfo& info)
{
	const QString oneLineTitle = oneLine(info.title);
	if(live)
	{
		if(!info.name.isEmpty())
		{
			return info.name;
		}
		return oneLineTitle.isEmpty() ? lastComponent(info.cwd) : oneLineTitle;
	}
	if(!oneLineTitle.isEmpty())
	{
		return oneLineTitle;
	}
	return info.name.isEmpty() ? lastComponent(info.cwd) : info.name;
}

QString stateName(proto::SessionStatus::State state)
{
	using State = proto::SessionStatus::State;
	switch(state)
	{
	case State::Idle:
		return u"idle"_s;
	case State::Busy:
		return u"thinking"_s;
	case State::Running:
		return u"running"_s;
	case State::WaitingPermission:
		return u"waiting_permission"_s;
	case State::WaitingQuestion:
		return u"waiting_question"_s;
	case State::Retrying:
		return u"retrying"_s;
	case State::Compacting:
		return u"compacting"_s;
	}
	return u"idle"_s;
}

std::optional<QString> optionalName(const QString& name)
{
	const QString trimmed = name.trimmed();
	return trimmed.isEmpty() ? std::nullopt : std::optional<QString>(trimmed);
}

// Lifetime invariant: `m_counter` and `m_currentSerial` are members of `m_owner` (`StashController`).
// Coroutine frames can outlive `m_owner` (or span `reset()`), so `~InFlightGuard` checks `m_owner`
// via `QPointer` before touching either reference and skips decrementing when `reset()` bumped `m_serial`.
class InFlightGuard
{
public:
	InFlightGuard(int& counter, const int& currentSerial, int expectedSerial, StashController* owner)
		: m_counter(counter)
		, m_currentSerial(currentSerial)
		, m_expectedSerial(expectedSerial)
		, m_owner(owner)
	{
		if(m_owner)
		{
			++m_counter;
			emit m_owner->busyChanged();
		}
	}
	~InFlightGuard()
	{
		if(m_owner && m_currentSerial == m_expectedSerial)
		{
			if(m_counter > 0)
			{
				--m_counter;
			}
			emit m_owner->busyChanged();
		}
	}

private:
	int& m_counter;
	const int& m_currentSerial;
	int m_expectedSerial = 0;
	QPointer<StashController> m_owner;
};

} // namespace

StashController::StashController(Connection& connection, SessionDirectory& directory, QObject* parent)
	: QObject(parent)
	, m_connection(connection)
	, m_backend(connection.backend())
	, m_directory(directory)
{
	connect(&m_connection, &Connection::connected, this, &StashController::refresh);
	connect(&m_backend.client(), &client::ServerClient::frameReceived, this, &StashController::onFrame);
	connect(&m_directory, &SessionDirectory::liveChanged, this, &StashController::rebuildVariant);
	connect(&m_directory, &SessionDirectory::historyUpdated, this, &StashController::rebuildVariant);
}

void StashController::reset()
{
	++m_serial;
	++m_refreshSerial;
	m_loading = false;
	m_inFlight = 0;
	const bool hadGroups = !m_groups.isEmpty();
	m_groups.clear();
	m_groupsVariant.clear();
	emit busyChanged();
	if(hadGroups)
	{
		emit groupsChanged();
	}
}

void StashController::refresh()
{
	if(!m_connection.isConnected())
	{
		return;
	}
	refreshTask();
}

template<typename T, typename OnOk>
QCoro::Task<> StashController::run(QCoro::Task<client::Result<T>> request, OnOk onOk)
{
	const QPointer<StashController> self(this);
	const int serial = m_serial;
	const InFlightGuard guard(m_inFlight, m_serial, serial, this);
	const auto result = co_await std::move(request);
	if(!self || serial != m_serial)
	{
		co_return;
	}
	if(!result)
	{
		emit toastRequested(ToastLevel::Error, result.error());
		co_return;
	}
	onOk(*result);
}

void StashController::stashAll(const QString& name)
{
	run(m_backend.client().stashAll(optionalName(name)), [this](const proto::StashGroupResult& res) {
		setGroups(res.groups);
		if(res.group)
		{
			const int count = static_cast<int>(res.group->members.size());
			emit toastRequested(ToastLevel::Info, tr("Stashed %n session(s) into \"%1\"", "", count).arg(res.group->name));
		}
		else
		{
			emit toastRequested(ToastLevel::Info, tr("No running sessions to stash."));
		}
	});
}

void StashController::createGroup(const QString& name, const QStringList& sessionIds, bool stashLive)
{
	run(m_backend.client().createGroup(optionalName(name), sessionIds, stashLive), [this](const proto::StashGroupResult& res) {
		setGroups(res.groups);
	});
}

void StashController::renameGroup(const QString& groupId, const QString& name)
{
	const QString trimmed = name.trimmed();
	if(groupId.isEmpty() || trimmed.isEmpty())
	{
		return;
	}
	run(m_backend.client().renameGroup(groupId, trimmed), [this](const QList<proto::StashGroup>& groups) {
		setGroups(groups);
	});
}

void StashController::setGroupLocked(const QString& groupId, bool locked)
{
	if(groupId.isEmpty())
	{
		return;
	}
	run(m_backend.client().setGroupLocked(groupId, locked), [this](const QList<proto::StashGroup>& groups) {
		setGroups(groups);
	});
}

void StashController::deleteGroup(const QString& groupId)
{
	if(groupId.isEmpty())
	{
		return;
	}
	run(m_backend.client().deleteGroup(groupId), [this](const QList<proto::StashGroup>& groups) {
		setGroups(groups);
	});
}

void StashController::addToGroup(const QString& groupId, const QString& sessionId, int position, bool stashLive)
{
	if(groupId.isEmpty() || sessionId.isEmpty())
	{
		return;
	}
	const std::optional<quint32> pos = position >= 0 ? std::optional<quint32>(static_cast<quint32>(position)) : std::nullopt;
	run(m_backend.client().addToGroup(groupId, sessionId, pos, stashLive), [this](const QList<proto::StashGroup>& groups) {
		setGroups(groups);
	});
}

void StashController::moveGroupMember(const QString& fromGroupId, const QString& toGroupId, const QString& sessionId, int position, bool stashLive)
{
	if(toGroupId.isEmpty() || sessionId.isEmpty())
	{
		return;
	}
	const std::optional<QString> from = fromGroupId.isEmpty() ? std::nullopt : std::optional<QString>(fromGroupId);
	const quint32 pos = static_cast<quint32>(std::max(0, position));
	run(m_backend.client().moveGroupMember(from, toGroupId, sessionId, pos, stashLive), [this](const QList<proto::StashGroup>& groups) {
		setGroups(groups);
	});
}

void StashController::removeFromGroup(const QString& groupId, const QString& sessionId)
{
	if(groupId.isEmpty() || sessionId.isEmpty())
	{
		return;
	}
	run(m_backend.client().removeFromGroup(groupId, sessionId), [this](const QList<proto::StashGroup>& groups) {
		setGroups(groups);
	});
}

void StashController::mergeGroups(const QString& sourceGroupId, const QString& targetGroupId)
{
	if(sourceGroupId.isEmpty() || targetGroupId.isEmpty() || sourceGroupId == targetGroupId)
	{
		return;
	}
	run(m_backend.client().mergeGroups(sourceGroupId, targetGroupId), [this](const QList<proto::StashGroup>& groups) {
		setGroups(groups);
	});
}

void StashController::restoreGroup(const QString& groupId)
{
	if(groupId.isEmpty())
	{
		return;
	}
	run(m_backend.client().restoreGroup(groupId), [this](const proto::RestoreGroupResult& res) {
		setGroups(res.groups);
		const int count = static_cast<int>(res.restoredIds.size());
		emit toastRequested(ToastLevel::Info, count > 0 ? tr("Restored %n session(s)", "", count) : tr("Group restored (all members already live)."));
	});
}

void StashController::restoreGroupMember(const QString& groupId, const QString& sessionId)
{
	if(groupId.isEmpty() || sessionId.isEmpty())
	{
		return;
	}
	run(m_backend.client().restoreGroupMember(groupId, sessionId), [this](const proto::RestoreGroupMemberResult& res) {
		setGroups(res.groups);
		if(res.restored)
		{
			emit toastRequested(ToastLevel::Info, tr("Restored session to background."));
		}
	});
}

void StashController::assignSessionToGroups(const QString& sessionId, const QStringList& targetGroupIds)
{
	if(sessionId.isEmpty())
	{
		return;
	}
	assignSessionTask(sessionId, targetGroupIds);
}

void StashController::createGroupFromMember(const QString& fromGroupId, const QString& sessionId, const QString& name)
{
	if(sessionId.isEmpty())
	{
		return;
	}
	createGroupFromMemberTask(fromGroupId, sessionId, optionalName(name));
}

QStringList StashController::groupIdsForSession(const QString& sessionId) const
{
	QStringList ids;
	for(const proto::StashGroup& group : m_groups)
	{
		for(const proto::StashMember& member : group.members)
		{
			if(member.sessionId == sessionId)
			{
				ids.append(group.id);
				break;
			}
		}
	}
	return ids;
}

void StashController::onFrame(const proto::ServerFrame& frame)
{
	if(const auto* changed = std::get_if<proto::GroupsChangedFrame>(&frame))
	{
		setGroups(changed->groups);
	}
}

void StashController::setGroups(QList<proto::StashGroup> groups)
{
	m_groups = std::move(groups);
	rebuildVariant();
}

void StashController::rebuildVariant()
{
	QVariantList next;
	next.reserve(m_groups.size());
	for(const proto::StashGroup& group : m_groups)
	{
		next.append(groupToMap(group));
	}
	if(next != m_groupsVariant)
	{
		m_groupsVariant = std::move(next);
		emit groupsChanged();
	}
}

QVariantMap StashController::groupToMap(const proto::StashGroup& group) const
{
	QVariantList membersList;
	membersList.reserve(group.members.size());
	for(const proto::StashMember& member : group.members)
	{
		membersList.append(memberToMap(member));
	}
	return {
		{u"id"_s, group.id},
		{u"name"_s, group.name},
		{u"locked"_s, group.locked},
		{u"createdAt"_s, group.createdAt},
		{u"updatedAt"_s, group.updatedAt},
		{u"members"_s, membersList},
	};
}

QVariantMap StashController::memberToMap(const proto::StashMember& member) const
{
	const proto::SessionInfo* liveInfo = m_directory.find(member.sessionId);
	const proto::SessionInfo* knownInfo = liveInfo ? liveInfo : m_directory.findAny(member.sessionId);
	const proto::SessionInfo& info = knownInfo ? *knownInfo : member.info;
	const bool live = liveInfo != nullptr || info.live;
	const QString parentId = info.parent.value_or(QString());
	QString creatorName;
	QString rootId;
	QString rootName;
	if(!parentId.isEmpty())
	{
		if(const proto::SessionInfo* creator = m_directory.findAny(parentId))
		{
			creatorName = sessionDisplayTitle(creator->live, *creator);
		}
		else
		{
			creatorName = parentId;
		}
		QSet<QString> visited{member.sessionId};
		QString cur = parentId;
		while(!cur.isEmpty() && !visited.contains(cur))
		{
			visited.insert(cur);
			const proto::SessionInfo* ancestor = m_directory.findAny(cur);
			if(ancestor == nullptr || !ancestor->parent || ancestor->parent->isEmpty())
			{
				break;
			}
			cur = *ancestor->parent;
		}
		const proto::SessionInfo* root = m_directory.findAny(cur);
		rootId = cur;
		rootName = root != nullptr ? sessionDisplayTitle(root->live, *root) : cur;
	}
	return {
		{u"sessionId"_s, member.sessionId},
		{u"position"_s, static_cast<int>(member.position)},
		{u"title"_s, info.title},
		{u"name"_s, info.name},
		{u"displayTitle"_s, sessionDisplayTitle(live, info)},
		{u"cwd"_s, info.cwd},
		{u"dirName"_s, lastComponent(info.cwd)},
		{u"live"_s, live},
		{u"activity"_s, stateName(info.status.state)},
		{u"hasError"_s, info.lastError.has_value() && !info.lastError->isEmpty()},
		{u"hasPermission"_s, info.status.state == proto::SessionStatus::State::WaitingPermission || info.status.state == proto::SessionStatus::State::WaitingQuestion},
		{u"parentId"_s, parentId},
		{u"creatorName"_s, creatorName},
		{u"rootId"_s, rootId},
		{u"rootName"_s, rootName},
		{u"updatedAt"_s, info.updatedAt},
	};
}

QCoro::Task<> StashController::refreshTask()
{
	const QPointer<StashController> self(this);
	const int serial = m_serial;
	const int refreshSerial = ++m_refreshSerial;
	if(!m_loading)
	{
		m_loading = true;
		emit busyChanged();
	}
	const auto result = co_await m_backend.client().listGroups();
	if(!self || serial != m_serial || refreshSerial != m_refreshSerial)
	{
		co_return;
	}
	m_loading = false;
	emit busyChanged();
	if(!result)
	{
		if(!client::isConnectionLost(result.error()))
		{
			emit toastRequested(ToastLevel::Error, result.error());
		}
		co_return;
	}
	setGroups(*result);
}

QCoro::Task<> StashController::assignSessionTask(QString sessionId, QStringList targetGroupIds)
{
	const QPointer<StashController> self(this);
	const int serial = m_serial;
	const InFlightGuard guard(m_inFlight, m_serial, serial, this);
	const QStringList currentIds = groupIdsForSession(sessionId);
	const QSet<QString> currentSet(currentIds.begin(), currentIds.end());
	const QSet<QString> targetSet(targetGroupIds.begin(), targetGroupIds.end());

	for(const QString& gid : targetGroupIds)
	{
		if(!currentSet.contains(gid))
		{
			const auto added = co_await m_backend.client().addToGroup(gid, sessionId, std::nullopt, true);
			if(!self || serial != m_serial)
			{
				co_return;
			}
			if(!added)
			{
				emit toastRequested(ToastLevel::Error, added.error());
				co_return;
			}
			setGroups(*added);
		}
	}
	for(const QString& gid : currentIds)
	{
		if(!targetSet.contains(gid))
		{
			const auto removed = co_await m_backend.client().removeFromGroup(gid, sessionId);
			if(!self || serial != m_serial)
			{
				co_return;
			}
			if(!removed)
			{
				emit toastRequested(ToastLevel::Error, removed.error());
				co_return;
			}
			setGroups(*removed);
		}
	}
}

QCoro::Task<> StashController::createGroupFromMemberTask(QString fromGroupId, QString sessionId, std::optional<QString> name)
{
	const QPointer<StashController> self(this);
	const int serial = m_serial;
	const InFlightGuard guard(m_inFlight, m_serial, serial, this);
	const auto created = co_await m_backend.client().createGroup(std::move(name), {sessionId}, true);
	if(!self || serial != m_serial)
	{
		co_return;
	}
	if(!created)
	{
		emit toastRequested(ToastLevel::Error, created.error());
		co_return;
	}
	setGroups(created->groups);
	if(!fromGroupId.isEmpty() && (!created->group || created->group->id != fromGroupId))
	{
		const auto removed = co_await m_backend.client().removeFromGroup(std::move(fromGroupId), std::move(sessionId));
		if(!self || serial != m_serial)
		{
			co_return;
		}
		if(!removed)
		{
			emit toastRequested(ToastLevel::Error, removed.error());
			co_return;
		}
		setGroups(*removed);
	}
}

} // namespace refgui::workspace
