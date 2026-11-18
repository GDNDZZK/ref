#pragma once

#include <QAbstractListModel>
#include <QList>
#include <QString>

namespace refgui::composer
{

struct CompletionEntry
{
	QString value{};
	QString label{};
	QString prefixLabel{};
	QString detail{};
	QString icon{};
	bool isDirectory = false;
	bool mono = false;
	bool trailingSpace = false;

	bool operator==(const CompletionEntry&) const = default;
};

// Entries offered by the pop-up list for slash command arguments and `!` shell completions.
class CompletionItemModel : public QAbstractListModel
{
	Q_OBJECT

public:
	enum Role
	{
		ValueRole = Qt::UserRole + 1,
		LabelRole,
		PrefixLabelRole,
		DetailRole,
		IconRole,
		IsDirectoryRole,
		MonoRole,
	};

	explicit CompletionItemModel(QObject* parent = nullptr);

	void setEntries(QList<CompletionEntry> entries);
	const CompletionEntry* entryAt(qsizetype row) const;
	const QList<CompletionEntry>& entries() const { return m_entries; }

	int rowCount(const QModelIndex& parent = {}) const override;
	QVariant data(const QModelIndex& index, int role) const override;
	QHash<int, QByteArray> roleNames() const override;

private:
	QList<CompletionEntry> m_entries;
};

} // namespace refgui::composer
