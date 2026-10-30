#include "composer/queue_model.h"

#include <QCoreApplication>

#include <variant>

namespace refgui::composer
{
namespace
{

QString senderOf(const proto::MessageSource& source)
{
	using Kind = proto::MessageSource::Kind;
	const QString name = source.fromName.isEmpty() ? source.fromId : source.fromName;
	switch(source.kind)
	{
	case Kind::Human:
		return {};
	case Kind::Peer:
	case Kind::Spawn:
		return QCoreApplication::translate("refgui::composer", "from %1").arg(name);
	case Kind::Reminder:
		return QCoreApplication::translate("refgui::composer", "Reminder");
	case Kind::Compaction:
		return QCoreApplication::translate("refgui::composer", "Summary");
	case Kind::Shell:
		return QCoreApplication::translate("refgui::composer", "Shell");
	}
	return {};
}

} // namespace

QueueModel::QueueModel(QObject* parent)
	: QAbstractListModel(parent)
{
}

QueueModel::Entry QueueModel::entryOf(const proto::Message& message)
{
	Entry entry;
	entry.id = message.id;
	entry.sender = senderOf(message.source);
	entry.removable = message.source.kind == proto::MessageSource::Kind::Human;
	QStringList texts;
	for(const proto::Part& part : message.parts)
	{
		if(const auto* text = std::get_if<proto::TextPart>(&part))
		{
			texts.append(text->text);
		}
		else if(std::holds_alternative<proto::ImagePart>(part))
		{
			++entry.imageCount;
		}
	}
	entry.text = texts.join(u'\n');
	return entry;
}

void QueueModel::setMessages(const QList<proto::Message>& messages)
{
	QList<Entry> entries;
	QSet<QString> ids;
	for(const proto::Message& message : messages)
	{
		entries.append(entryOf(message));
		ids.insert(message.id);
	}
	m_removing.intersect(ids);

	// Rows that left go first, so the view can animate each of them out.
	for(qsizetype row = m_entries.size() - 1; row >= 0; --row)
	{
		if(!ids.contains(m_entries[row].id))
		{
			beginRemoveRows({}, static_cast<int>(row), static_cast<int>(row));
			m_entries.removeAt(row);
			endRemoveRows();
		}
	}

	// What is left must be in the new order; the server only appends, but do not trust that.
	QList<QString> kept;
	for(const Entry& entry : m_entries)
	{
		kept.append(entry.id);
	}
	QList<QString> expected;
	QSet<QString> known(kept.cbegin(), kept.cend());
	for(const Entry& entry : entries)
	{
		if(known.contains(entry.id))
		{
			expected.append(entry.id);
		}
	}
	if(kept != expected)
	{
		beginResetModel();
		m_entries = entries;
		endResetModel();
		return;
	}

	for(qsizetype row = 0; row < entries.size(); ++row)
	{
		if(row >= m_entries.size() || m_entries[row].id != entries[row].id)
		{
			beginInsertRows({}, static_cast<int>(row), static_cast<int>(row));
			m_entries.insert(row, entries[row]);
			endInsertRows();
		}
		else if(m_entries[row].text != entries[row].text || m_entries[row].imageCount != entries[row].imageCount)
		{
			m_entries[row] = entries[row];
			emit dataChanged(index(static_cast<int>(row)), index(static_cast<int>(row)));
		}
	}
}

void QueueModel::setRemoving(const QString& id, bool removing)
{
	if(removing == m_removing.contains(id))
	{
		return;
	}
	if(removing)
	{
		m_removing.insert(id);
	}
	else
	{
		m_removing.remove(id);
	}
	for(qsizetype row = 0; row < m_entries.size(); ++row)
	{
		if(m_entries[row].id == id)
		{
			const QModelIndex changed = index(static_cast<int>(row));
			emit dataChanged(changed, changed, {RemovingRole});
		}
	}
}

QString QueueModel::idAt(qsizetype row) const
{
	return row >= 0 && row < m_entries.size() ? m_entries[row].id : QString();
}

int QueueModel::rowCount(const QModelIndex& parent) const
{
	return parent.isValid() ? 0 : static_cast<int>(m_entries.size());
}

QVariant QueueModel::data(const QModelIndex& index, int role) const
{
	if(!index.isValid() || index.row() >= m_entries.size())
	{
		return {};
	}
	const Entry& entry = m_entries[index.row()];
	switch(role)
	{
	case IdRole:
		return entry.id;
	case TextRole:
		return entry.text;
	case ImageCountRole:
		return entry.imageCount;
	case SenderRole:
		return entry.sender;
	case RemovableRole:
		return entry.removable;
	case RemovingRole:
		return m_removing.contains(entry.id);
	default:
		return {};
	}
}

QHash<int, QByteArray> QueueModel::roleNames() const
{
	return {{IdRole, "id"},
	        {TextRole, "text"},
	        {ImageCountRole, "imageCount"},
	        {SenderRole, "sender"},
	        {RemovableRole, "removable"},
	        {RemovingRole, "removing"}};
}

} // namespace refgui::composer
