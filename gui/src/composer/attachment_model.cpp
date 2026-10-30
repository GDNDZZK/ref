#include "composer/attachment_model.h"

#include <QSet>

namespace refgui::composer
{

AttachmentModel::AttachmentModel(QObject* parent)
	: QAbstractListModel(parent)
{
}

void AttachmentModel::setAttachments(const QList<compose::PromptDraft::Attachment>& attachments)
{
	QSet<QString> tokens;
	for(const auto& attachment : attachments)
	{
		tokens.insert(attachment.token);
	}
	// A token always stands for the same image, so rows only come and go. Those that left
	// first, so that the view can animate each of them out.
	for(qsizetype row = m_attachments.size() - 1; row >= 0; --row)
	{
		if(!tokens.contains(m_attachments[row].token))
		{
			beginRemoveRows({}, static_cast<int>(row), static_cast<int>(row));
			m_attachments.removeAt(row);
			endRemoveRows();
		}
	}
	for(qsizetype row = 0; row < attachments.size(); ++row)
	{
		if(row >= m_attachments.size() || m_attachments[row].token != attachments[row].token)
		{
			beginInsertRows({}, static_cast<int>(row), static_cast<int>(row));
			m_attachments.insert(row, attachments[row]);
			endInsertRows();
		}
	}
}

int AttachmentModel::rowCount(const QModelIndex& parent) const
{
	return parent.isValid() ? 0 : static_cast<int>(m_attachments.size());
}

QVariant AttachmentModel::data(const QModelIndex& index, int role) const
{
	if(!index.isValid() || index.row() >= m_attachments.size())
	{
		return {};
	}
	const auto& attachment = m_attachments[index.row()];
	switch(role)
	{
	case TokenRole:
		return attachment.token;
	case NameRole:
		return attachment.image.name.value_or(attachment.token);
	case SourceRole:
		return QStringLiteral("data:%1;base64,%2").arg(attachment.image.mime, attachment.image.data);
	default:
		return {};
	}
}

QHash<int, QByteArray> AttachmentModel::roleNames() const
{
	return {{TokenRole, "token"}, {NameRole, "name"}, {SourceRole, "source"}};
}

} // namespace refgui::composer
