#include "composer/completion_item_model.h"

namespace refgui::composer
{

CompletionItemModel::CompletionItemModel(QObject* parent)
	: QAbstractListModel(parent)
{
}

void CompletionItemModel::setEntries(QList<CompletionEntry> entries)
{
	if(entries == m_entries)
	{
		return;
	}
	beginResetModel();
	m_entries = std::move(entries);
	endResetModel();
}

const CompletionEntry* CompletionItemModel::entryAt(qsizetype row) const
{
	return row >= 0 && row < m_entries.size() ? &m_entries[row] : nullptr;
}

int CompletionItemModel::rowCount(const QModelIndex& parent) const
{
	return parent.isValid() ? 0 : static_cast<int>(m_entries.size());
}

QVariant CompletionItemModel::data(const QModelIndex& index, int role) const
{
	if(!index.isValid() || index.row() < 0 || index.row() >= m_entries.size())
	{
		return {};
	}
	const CompletionEntry& entry = m_entries[index.row()];
	switch(role)
	{
	case ValueRole:
		return entry.value;
	case LabelRole:
		return entry.label;
	case PrefixLabelRole:
		return entry.prefixLabel;
	case DetailRole:
		return entry.detail;
	case IconRole:
		return entry.icon;
	case IsDirectoryRole:
		return entry.isDirectory;
	case MonoRole:
		return entry.mono;
	default:
		return {};
	}
}

QHash<int, QByteArray> CompletionItemModel::roleNames() const
{
	return {{ValueRole, "value"},
	        {LabelRole, "label"},
	        {PrefixLabelRole, "prefixLabel"},
	        {DetailRole, "detail"},
	        {IconRole, "icon"},
	        {IsDirectoryRole, "isDirectory"},
	        {MonoRole, "mono"}};
}

} // namespace refgui::composer
