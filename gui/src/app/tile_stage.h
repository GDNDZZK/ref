#pragma once

#include <QAbstractListModel>
#include <QList>
#include <QPointer>
#include <QString>
#include <QtQmlIntegration>

class ChatSession;
class TileModel;

// What the split view puts on screen: the tiles of a TileModel, plus the ones that just left,
// kept for `lingerMs` so that they can animate out. A row stays where it is; its `place` is the
// position in the grid.
class TileStage : public QAbstractListModel
{
	Q_OBJECT
	QML_ELEMENT
	QML_UNCREATABLE("Owned by Workspace.")
	// Tiles that are not leaving.
	Q_PROPERTY(int placed READ placed NOTIFY placedChanged)
	// True while the tiles of a freshly opened split view arrive.
	Q_PROPERTY(bool opening READ opening NOTIFY openingChanged)
	Q_PROPERTY(int lingerMs READ lingerMs WRITE setLingerMs NOTIFY lingerMsChanged)

public:
	enum Role
	{
		SessionRole = Qt::UserRole + 1,
		SessionIdRole,
		LeavingRole,
		PlaceRole,
	};

	explicit TileStage(const TileModel* source, QObject* parent = nullptr);

	int placed() const { return m_placed; }
	bool opening() const { return m_opening; }
	int lingerMs() const { return m_lingerMs; }
	void setLingerMs(int lingerMs);

	// Deletes a session that is gone: at once when no tile shows it, else when its tile has left.
	void deleteWhenLeft(ChatSession* session);

	int rowCount(const QModelIndex& parent = {}) const override;
	QVariant data(const QModelIndex& index, int role) const override;
	QHash<int, QByteArray> roleNames() const override;

signals:
	void placedChanged();
	void openingChanged();
	void lingerMsChanged();

private:
	struct Row
	{
		QPointer<ChatSession> session;
		QString sessionId;
		bool leaving = false;
		bool deleteSession = false; // the stage deletes the session when the row goes
		int place = 0;
	};

	void sync();
	void markLeaving(int row);
	void retire(const QString& sessionId);
	void setOpening(bool opening);
	void setPlaced(int placed);
	int rowOf(const ChatSession* session) const;

	const TileModel* m_source;
	QList<Row> m_rows;
	int m_placed = 0;
	bool m_opening = false;
	int m_lingerMs = 0;
};
