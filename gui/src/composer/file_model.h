#pragma once

#include <QAbstractListModel>
#include <QStringList>

namespace refgui::composer
{

// The files the `@` list offers: paths relative to the session's directory, best match first.
// Directories end with a slash.
class FileModel : public QAbstractListModel
{
	Q_OBJECT

public:
	enum Role
	{
		PathRole = Qt::UserRole + 1, // "src/main.rs"
		NameRole,                    // the last component: "main.rs", "src" for "src/"
		DirectoryRole,               // the parent: "src/"; empty at the top
		IsDirectoryRole,
	};

	explicit FileModel(QObject* parent = nullptr);

	void setPaths(QStringList paths);
	QString pathAt(qsizetype row) const;

	int rowCount(const QModelIndex& parent = {}) const override;
	QVariant data(const QModelIndex& index, int role) const override;
	QHash<int, QByteArray> roleNames() const override;

private:
	QStringList m_paths;
};

} // namespace refgui::composer
