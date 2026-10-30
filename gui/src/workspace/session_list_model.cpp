#include "workspace/session_list_model.h"

#include <QSet>

#include <algorithm>

using namespace Qt::StringLiterals;

namespace refgui::workspace
{

namespace
{

using State = proto::SessionStatus::State;

QString stateName(State state)
{
	switch(state)
	{
	case State::Idle:
		return QStringLiteral("idle");
	case State::Busy:
		return QStringLiteral("thinking");
	case State::Running:
		return QStringLiteral("running");
	case State::WaitingPermission:
		return QStringLiteral("waiting_permission");
	case State::Retrying:
		return QStringLiteral("retrying");
	case State::Compacting:
		return QStringLiteral("compacting");
	}
	Q_UNREACHABLE_RETURN(QString());
}

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

QString displayTitle(const QString& name, const QString& title, const QString& cwd)
{
	if(!name.isEmpty())
	{
		return name;
	}
	return title.isEmpty() ? lastComponent(cwd) : title;
}

} // namespace

SessionListModel::SessionListModel(Order order, bool live, QObject* parent)
	: QAbstractListModel(parent)
	, m_order(order)
	, m_live(live)
{
}

SessionListModel::Row SessionListModel::toRow(const proto::SessionInfo& info)
{
	Row row;
	row.id = info.id;
	row.name = info.name;
	row.title = info.title;
	row.cwd = info.cwd;
	row.agent = info.agent;
	row.modelLabel = info.model.variant ? info.model.model + u" · "_s + *info.model.variant : info.model.model;
	row.state = stateName(info.status.state);
	row.busySince = info.busySince.value_or(0);
	row.updatedAt = info.updatedAt;
	row.createdAt = info.createdAt;
	row.todoTotal = int(info.todos.size());
	for(const proto::Todo& todo : info.todos)
	{
		if(todo.status == proto::TodoStatus::Completed || todo.status == proto::TodoStatus::Cancelled)
		{
			++row.todoDone;
		}
	}
	row.lastError = info.lastError.value_or(QString());
	row.attached = int(info.attached);
	return row;
}

bool SessionListModel::matchesFilter(const Row& row) const
{
	if(m_filter.isEmpty())
	{
		return true;
	}
	for(const QString& field : {row.title, row.name, row.cwd, row.id})
	{
		if(field.contains(m_filter, Qt::CaseInsensitive))
		{
			return true;
		}
	}
	return false;
}

bool SessionListModel::before(const Row& a, const Row& b) const
{
	const qint64 left = m_order == Order::CreatedNewestFirst ? a.createdAt : a.updatedAt;
	const qint64 right = m_order == Order::CreatedNewestFirst ? b.createdAt : b.updatedAt;
	if(left != right)
	{
		return left > right;
	}
	return a.id < b.id;
}

void SessionListModel::setSessions(const QList<proto::SessionInfo>& sessions)
{
	m_all.clear();
	for(const proto::SessionInfo& info : sessions)
	{
		m_all.insert(info.id, info);
	}
	rebuild();
}

void SessionListModel::setFilter(const QString& filter)
{
	if(filter == m_filter)
	{
		return;
	}
	m_filter = filter;
	rebuild();
}

void SessionListModel::setCurrentId(const QString& id)
{
	if(id == m_currentId)
	{
		return;
	}
	const QString old = m_currentId;
	m_currentId = id;
	emitRowChanged(old);
	emitRowChanged(id);
}

void SessionListModel::setTileIds(const QStringList& ids)
{
	if(ids == m_tileIds)
	{
		return;
	}
	const QStringList old = m_tileIds;
	m_tileIds = ids;
	for(const QString& id : old + ids)
	{
		emitRowChanged(id);
	}
}

void SessionListModel::emitRowChanged(const QString& id)
{
	const int row = indexOf(id);
	if(row >= 0)
	{
		emit dataChanged(index(row), index(row), {IsCurrentRole, IsTileRole});
	}
}

int SessionListModel::indexOf(const QString& sessionId) const
{
	for(qsizetype i = 0; i < m_rows.size(); ++i)
	{
		if(m_rows[i].id == sessionId)
		{
			return int(i);
		}
	}
	return -1;
}

const proto::SessionInfo* SessionListModel::find(const QString& sessionId) const
{
	const auto it = m_all.constFind(sessionId);
	return it == m_all.constEnd() ? nullptr : &*it;
}

// Brings m_rows to the wanted list with the fewest row operations: removes what is gone,
// then goes through the wanted rows in order and moves, inserts or updates each one.
void SessionListModel::rebuild()
{
	QList<Row> wanted;
	for(const proto::SessionInfo& info : std::as_const(m_all))
	{
		const Row row = toRow(info);
		if(matchesFilter(row))
		{
			wanted.append(row);
		}
	}
	std::ranges::sort(wanted, [this](const Row& a, const Row& b) { return before(a, b); });

	const int oldCount = count();
	QSet<QString> keep;
	for(const Row& row : wanted)
	{
		keep.insert(row.id);
	}
	for(qsizetype i = m_rows.size() - 1; i >= 0; --i)
	{
		if(keep.contains(m_rows[i].id))
		{
			continue;
		}
		beginRemoveRows({}, int(i), int(i));
		m_rows.removeAt(i);
		endRemoveRows();
	}

	for(qsizetype i = 0; i < wanted.size(); ++i)
	{
		if(i < m_rows.size() && m_rows[i].id != wanted[i].id)
		{
			qsizetype from = -1;
			for(qsizetype j = i + 1; j < m_rows.size(); ++j)
			{
				if(m_rows[j].id == wanted[i].id)
				{
					from = j;
					break;
				}
			}
			if(from >= 0)
			{
				beginMoveRows({}, int(from), int(from), {}, int(i));
				m_rows.move(from, i);
				endMoveRows();
			}
			else
			{
				beginInsertRows({}, int(i), int(i));
				m_rows.insert(i, wanted[i]);
				endInsertRows();
				continue;
			}
		}
		if(i >= m_rows.size())
		{
			beginInsertRows({}, int(i), int(i));
			m_rows.append(wanted[i]);
			endInsertRows();
			continue;
		}
		if(m_rows[i] != wanted[i])
		{
			m_rows[i] = wanted[i];
			emit dataChanged(index(int(i)), index(int(i)));
		}
	}

	if(count() != oldCount)
	{
		emit countChanged();
	}
}

int SessionListModel::rowCount(const QModelIndex& parent) const
{
	return parent.isValid() ? 0 : int(m_rows.size());
}

QVariant SessionListModel::data(const QModelIndex& index, int role) const
{
	if(!index.isValid() || index.row() >= m_rows.size())
	{
		return {};
	}
	const Row& row = m_rows[index.row()];
	switch(role)
	{
	case SessionIdRole:
		return row.id;
	case NameRole:
		return row.name;
	case TitleRole:
		return row.title;
	case DisplayTitleRole:
		return displayTitle(row.name, row.title, row.cwd);
	case CwdRole:
		return row.cwd;
	case DirNameRole:
		return lastComponent(row.cwd);
	case AgentRole:
		return row.agent;
	case ModelLabelRole:
		return row.modelLabel;
	case StateRole:
		return row.state;
	case BusySinceRole:
		return row.busySince;
	case UpdatedAtRole:
		return row.updatedAt;
	case CreatedAtRole:
		return row.createdAt;
	case TodoDoneRole:
		return row.todoDone;
	case TodoTotalRole:
		return row.todoTotal;
	case TodoAllDoneRole:
		return row.todoTotal > 0 && row.todoDone == row.todoTotal;
	case HasPermissionRole:
		return row.state == u"waiting_permission"_s;
	case LastErrorRole:
		return row.lastError;
	case HasErrorRole:
		return !row.lastError.isEmpty();
	case AttachedRole:
		return row.attached;
	case IsCurrentRole:
		return row.id == m_currentId;
	case IsTileRole:
		return m_tileIds.contains(row.id);
	case IsLiveRole:
		return m_live;
	default:
		return {};
	}
}

QHash<int, QByteArray> SessionListModel::roleNames() const
{
	return {
		{SessionIdRole, "sessionId"},
		{NameRole, "name"},
		{TitleRole, "title"},
		{DisplayTitleRole, "displayTitle"},
		{CwdRole, "cwd"},
		{DirNameRole, "dirName"},
		{AgentRole, "agent"},
		{ModelLabelRole, "modelLabel"},
		{StateRole, "state"},
		{BusySinceRole, "busySince"},
		{UpdatedAtRole, "updatedAt"},
		{CreatedAtRole, "createdAt"},
		{TodoDoneRole, "todoDone"},
		{TodoTotalRole, "todoTotal"},
		{TodoAllDoneRole, "todoAllDone"},
		{HasPermissionRole, "hasPermission"},
		{LastErrorRole, "lastError"},
		{HasErrorRole, "hasError"},
		{AttachedRole, "attached"},
		{IsCurrentRole, "isCurrent"},
		{IsTileRole, "isTile"},
		{IsLiveRole, "isLive"},
	};
}

} // namespace refgui::workspace
