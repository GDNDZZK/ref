#pragma once

#include "proto/types.h"

#include <QAbstractListModel>
#include <QSet>

namespace refgui::composer
{

// The prompts waiting for the next safe point, as the list above the input box shows them.
// The server's queue is the truth; this adds which entries are being withdrawn.
class QueueModel : public QAbstractListModel
{
	Q_OBJECT

public:
	enum Role
	{
		IdRole = Qt::UserRole + 1,
		TextRole,
		ImageCountRole,
		SenderRole,    // who wrote it when not the user, e.g. "from build"; empty for the user's own
		RemovableRole, // the user's own prompt, which can be withdrawn
		RemovingRole,  // a withdrawal is on its way
	};

	explicit QueueModel(QObject* parent = nullptr);

	void setMessages(const QList<proto::Message>& messages);
	void setRemoving(const QString& id, bool removing);
	QString idAt(qsizetype row) const;

	int rowCount(const QModelIndex& parent = {}) const override;
	QVariant data(const QModelIndex& index, int role) const override;
	QHash<int, QByteArray> roleNames() const override;

private:
	struct Entry
	{
		QString id;
		QString text;
		int imageCount = 0;
		QString sender;
		bool removable = false;
	};

	static Entry entryOf(const proto::Message& message);

	QList<Entry> m_entries;
	QSet<QString> m_removing;
};

} // namespace refgui::composer
