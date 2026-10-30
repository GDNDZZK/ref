#pragma once

#include "compose/prompt_draft.h"

#include <QAbstractListModel>

namespace refgui::composer
{

// The images attached to the prompt being written, as the strip above the text shows them.
class AttachmentModel : public QAbstractListModel
{
	Q_OBJECT

public:
	enum Role
	{
		TokenRole = Qt::UserRole + 1, // "[Image 2]"
		NameRole,                     // the file name, or the token for a pasted image
		SourceRole,                   // a data: URL the Image item can show
	};

	explicit AttachmentModel(QObject* parent = nullptr);

	// Brings the rows in line with the attachments, by their tokens: rows come and go, none is replaced.
	void setAttachments(const QList<compose::PromptDraft::Attachment>& attachments);

	int rowCount(const QModelIndex& parent = {}) const override;
	QVariant data(const QModelIndex& index, int role) const override;
	QHash<int, QByteArray> roleNames() const override;

private:
	QList<compose::PromptDraft::Attachment> m_attachments;
};

} // namespace refgui::composer
