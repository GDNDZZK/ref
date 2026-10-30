#include "composer/file_model.h"

namespace refgui::composer
{
namespace
{

// Where the last component starts, ignoring the slash that ends a directory.
qsizetype nameStart(const QString& path)
{
	const qsizetype searchFrom = path.endsWith(u'/') ? path.size() - 2 : path.size() - 1;
	return path.lastIndexOf(u'/', searchFrom) + 1;
}

} // namespace

FileModel::FileModel(QObject* parent)
	: QAbstractListModel(parent)
{
}

void FileModel::setPaths(QStringList paths)
{
	if(paths == m_paths)
	{
		return;
	}
	beginResetModel();
	m_paths = std::move(paths);
	endResetModel();
}

QString FileModel::pathAt(qsizetype row) const
{
	return row >= 0 && row < m_paths.size() ? m_paths[row] : QString();
}

int FileModel::rowCount(const QModelIndex& parent) const
{
	return parent.isValid() ? 0 : static_cast<int>(m_paths.size());
}

QVariant FileModel::data(const QModelIndex& index, int role) const
{
	if(!index.isValid() || index.row() >= m_paths.size())
	{
		return {};
	}
	const QString& path = m_paths[index.row()];
	switch(role)
	{
	case PathRole:
		return path;
	case NameRole:
		return path.mid(nameStart(path)).chopped(path.endsWith(u'/') ? 1 : 0);
	case DirectoryRole:
		return path.left(nameStart(path));
	case IsDirectoryRole:
		return path.endsWith(u'/');
	default:
		return {};
	}
}

QHash<int, QByteArray> FileModel::roleNames() const
{
	return {{PathRole, "path"}, {NameRole, "name"}, {DirectoryRole, "directory"}, {IsDirectoryRole, "isDirectory"}};
}

} // namespace refgui::composer
