#include "app/tile_stage.h"

#include "app/chat_session.h"
#include "app/tile_model.h"

#include <QTimer>

TileStage::TileStage(const TileModel* source, QObject* parent)
	: QAbstractListModel(parent)
	, m_source(source)
{
	connect(source, &QAbstractItemModel::rowsInserted, this, &TileStage::sync);
	connect(source, &QAbstractItemModel::rowsRemoved, this, &TileStage::sync);
	connect(source, &QAbstractItemModel::rowsMoved, this, &TileStage::sync);
	connect(source, &QAbstractItemModel::modelReset, this, &TileStage::sync);
}

void TileStage::setLingerMs(int lingerMs)
{
	if(m_lingerMs == lingerMs)
	{
		return;
	}
	m_lingerMs = lingerMs;
	emit lingerMsChanged();
}

int TileStage::rowOf(const ChatSession* session) const
{
	for(int i = 0; i < m_rows.size(); ++i)
	{
		if(m_rows[i].session == session)
		{
			return i;
		}
	}
	return -1;
}

void TileStage::sync()
{
	const QList<ChatSession*> wanted = m_source->tiles();
	const bool nothingShown = m_placed == 0;

	for(int i = 0; i < m_rows.size(); ++i)
	{
		Row& row = m_rows[i];
		const int place = row.session ? int(wanted.indexOf(row.session.data())) : -1;
		if(place < 0)
		{
			if(!row.leaving)
			{
				markLeaving(i);
			}
			continue;
		}
		if(row.leaving || row.place != place)
		{
			row.leaving = false;
			row.place = place;
			emit dataChanged(index(i), index(i), {LeavingRole, PlaceRole});
		}
	}

	if(nothingShown && !wanted.isEmpty())
	{
		setOpening(true);
		QTimer::singleShot(m_lingerMs, this, [this] { setOpening(false); });
	}
	for(int place = 0; place < wanted.size(); ++place)
	{
		if(rowOf(wanted[place]) >= 0)
		{
			continue;
		}
		beginInsertRows({}, int(m_rows.size()), int(m_rows.size()));
		m_rows.append({wanted[place], wanted[place]->sessionId(), false, false, place});
		endInsertRows();
	}
	setPlaced(int(wanted.size()));
}

// The row keeps its place, so that the tile can stay where it was while it fades.
void TileStage::markLeaving(int row)
{
	m_rows[row].leaving = true;
	emit dataChanged(index(row), index(row), {LeavingRole});
	const QString sessionId = m_rows[row].sessionId;
	QTimer::singleShot(m_lingerMs, this, [this, sessionId] { retire(sessionId); });
}

void TileStage::retire(const QString& sessionId)
{
	for(int i = 0; i < m_rows.size(); ++i)
	{
		if(m_rows[i].sessionId == sessionId && m_rows[i].leaving)
		{
			const Row row = m_rows[i];
			beginRemoveRows({}, i, i);
			m_rows.removeAt(i);
			endRemoveRows();
			if(row.deleteSession && row.session)
			{
				row.session->deleteLater();
			}
			return;
		}
	}
}

void TileStage::deleteWhenLeft(ChatSession* session)
{
	const int row = rowOf(session);
	if(row < 0 || !m_rows[row].leaving)
	{
		session->deleteLater();
		return;
	}
	m_rows[row].deleteSession = true;
}

void TileStage::setOpening(bool opening)
{
	if(m_opening == opening)
	{
		return;
	}
	m_opening = opening;
	emit openingChanged();
}

void TileStage::setPlaced(int placed)
{
	if(m_placed == placed)
	{
		return;
	}
	m_placed = placed;
	emit placedChanged();
}

int TileStage::rowCount(const QModelIndex& parent) const
{
	return parent.isValid() ? 0 : int(m_rows.size());
}

QVariant TileStage::data(const QModelIndex& index, int role) const
{
	if(!index.isValid() || index.row() >= m_rows.size())
	{
		return {};
	}
	const Row& row = m_rows[index.row()];
	switch(role)
	{
	case SessionRole:
		return QVariant::fromValue(row.session.data());
	case SessionIdRole:
		return row.sessionId;
	case LeavingRole:
		return row.leaving;
	case PlaceRole:
		return row.place;
	default:
		return {};
	}
}

QHash<int, QByteArray> TileStage::roleNames() const
{
	return {{SessionRole, "session"}, {SessionIdRole, "sessionId"}, {LeavingRole, "leaving"}, {PlaceRole, "place"}};
}
