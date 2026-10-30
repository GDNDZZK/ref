#pragma once

#include <QAbstractListModel>
#include <QList>

class ChatSession;

// The sessions shown as tiles in split view, in the order they are laid out.
class TileModel : public QAbstractListModel
{
	Q_OBJECT

public:
	enum Role
	{
		SessionRole = Qt::UserRole + 1,
		SessionIdRole,
	};

	using QAbstractListModel::QAbstractListModel;

	// Makes the tiles `wanted`, in that order. Tiles that stay are kept.
	void setTiles(const QList<ChatSession*>& wanted);
	QList<ChatSession*> tiles() const { return m_tiles; }
	bool contains(const ChatSession* session) const { return m_tiles.contains(session); }
	int indexOf(const QString& sessionId) const;

	int rowCount(const QModelIndex& parent = {}) const override;
	QVariant data(const QModelIndex& index, int role) const override;
	QHash<int, QByteArray> roleNames() const override;

private:
	QList<ChatSession*> m_tiles;
};
