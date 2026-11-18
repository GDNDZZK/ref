#pragma once

#include "client/result.h"

#include <QJsonObject>
#include <QJsonValue>
#include <QString>
#include <QStringList>
#include <QStringView>

namespace refgui::workspace
{

// Lossless JSON/JSONC object patcher for `ref.json`: reads, writes, renames and removes nested
// paths while keeping untouched sibling keys, `extraBody`, and `{env:...}` / `{file:...}`
// interpolation tokens intact.
class ConfigJsonPatcher
{
public:
	ConfigJsonPatcher() = default;
	explicit ConfigJsonPatcher(QJsonObject root);

	// Parses JSONC (`//`, `/* */` comments and trailing commas) into a patcher when the root is an object.
	static client::Result<ConfigJsonPatcher> parseJsonc(QStringView text);

	const QJsonObject& root() const { return m_root; }
	void setRoot(QJsonObject root) { m_root = std::move(root); }

	// Formats the root object as 2-space indented UTF-8 JSON with a trailing newline.
	QString toJson() const;

	// Reads the value at `path`, returning Undefined when `path` is empty or any segment is missing.
	QJsonValue valueAt(const QStringList& path) const;
	bool contains(const QStringList& path) const;

	// Sets `value` at `path`, creating intermediate objects as needed and preserving sibling keys.
	// Passing Undefined removes `path`.
	void setValueAt(const QStringList& path, const QJsonValue& value);

	// Removes the key at `path`. When `pruneEmptyAncestors` is true, intermediate objects along
	// `path` that become empty as a result of this removal are removed as well.
	void removeAt(const QStringList& path, bool pruneEmptyAncestors = true);

	// Renames `oldKey` to `newKey` inside the object at `parentPath`. Returns false when the
	// parent is not an object, `oldKey` is missing, `newKey` is empty, or `newKey` already exists.
	bool renameKeyAt(const QStringList& parentPath, const QString& oldKey, const QString& newKey);

	// Unique `{env:VAR}` variable names referenced in string values of `value` / `root()`, sorted ascending.
	static QStringList envReferencesIn(const QJsonValue& value);
	QStringList envReferences() const;

	// True when `text` contains a closed `{env:...}` or `{file:...}` interpolation token.
	static bool hasInterpolation(QStringView text);

private:
	QJsonObject m_root;
};

} // namespace refgui::workspace
