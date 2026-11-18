#include "composer/file_model.h"

#include <algorithm>

namespace refgui::composer
{
namespace
{

bool endsWithSeparator(const QString& path)
{
	return path.endsWith(u'/') || path.endsWith(u'\\');
}

// Where the last component starts, ignoring the slash or backslash that ends a directory.
qsizetype nameStart(const QString& path)
{
	const qsizetype searchFrom = endsWithSeparator(path) ? path.size() - 2 : path.size() - 1;
	if(searchFrom < 0)
	{
		return 0;
	}
	const qsizetype lastSep = std::max(path.lastIndexOf(u'/', searchFrom), path.lastIndexOf(u'\\', searchFrom));
	return lastSep + 1;
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
	const bool isDir = endsWithSeparator(path);
	switch(role)
	{
	case PathRole:
		return path;
	case NameRole:
		return path.mid(nameStart(path)).chopped(isDir ? 1 : 0);
	case DirectoryRole:
		return path.left(nameStart(path));
	case IsDirectoryRole:
		return isDir;
	default:
		return {};
	}
}

QHash<int, QByteArray> FileModel::roleNames() const
{
	return {{PathRole, "path"}, {NameRole, "name"}, {DirectoryRole, "directory"}, {IsDirectoryRole, "isDirectory"}};
}

} // namespace refgui::composer
