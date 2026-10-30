#include "app/tile_model.h"

#include "app/chat_session.h"

void TileModel::setTiles(const QList<ChatSession*>& wanted)
{
	for(qsizetype i = m_tiles.size() - 1; i >= 0; --i)
	{
		if(wanted.contains(m_tiles[i]))
		{
			continue;
		}
		beginRemoveRows({}, int(i), int(i));
		m_tiles.removeAt(i);
		endRemoveRows();
	}
	for(qsizetype i = 0; i < wanted.size(); ++i)
	{
		if(i < m_tiles.size() && m_tiles[i] == wanted[i])
		{
			continue;
		}
		const qsizetype from = m_tiles.indexOf(wanted[i], i);
		if(from >= 0)
		{
			beginMoveRows({}, int(from), int(from), {}, int(i));
			m_tiles.move(from, i);
			endMoveRows();
			continue;
		}
		beginInsertRows({}, int(i), int(i));
		m_tiles.insert(i, wanted[i]);
		endInsertRows();
	}
}

int TileModel::indexOf(const QString& sessionId) const
{
	for(qsizetype i = 0; i < m_tiles.size(); ++i)
	{
		if(m_tiles[i]->sessionId() == sessionId)
		{
			return int(i);
		}
	}
	return -1;
}

int TileModel::rowCount(const QModelIndex& parent) const
{
	return parent.isValid() ? 0 : int(m_tiles.size());
}

QVariant TileModel::data(const QModelIndex& index, int role) const
{
	if(!index.isValid() || index.row() >= m_tiles.size())
	{
		return {};
	}
	ChatSession* session = m_tiles[index.row()];
	switch(role)
	{
	case SessionRole:
		return QVariant::fromValue(session);
	case SessionIdRole:
		return session->sessionId();
	default:
		return {};
	}
}

QHash<int, QByteArray> TileModel::roleNames() const
{
	return {{SessionRole, "session"}, {SessionIdRole, "sessionId"}};
}
