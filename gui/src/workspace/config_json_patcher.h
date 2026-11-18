#pragma once

#include "client/result.h"

#include <QJsonObject>
#include <QJsonValue>
#include <QList>
#include <QString>
#include <QStringList>
#include <QStringView>
#include <QVariant>
#include <QVariantMap>

#include <memory>

namespace refgui::workspace
{

class OrderedJsonObject;

// Order-preserving JSON value that distinguishes 64-bit integers from IEEE-754 doubles and keeps
// object member order intact across parse -> edit -> serialize cycles.
class OrderedJsonValue
{
public:
	enum class Type
	{
		Undefined,
		Null,
		Bool,
		Int,
		Double,
		String,
		Array,
		Object,
	};

	using Array = QList<OrderedJsonValue>;

	OrderedJsonValue();
	OrderedJsonValue(const OrderedJsonValue& other);
	OrderedJsonValue(OrderedJsonValue&& other) noexcept;
	OrderedJsonValue& operator=(const OrderedJsonValue& other);
	OrderedJsonValue& operator=(OrderedJsonValue&& other) noexcept;
	~OrderedJsonValue();

	static OrderedJsonValue null();
	explicit OrderedJsonValue(bool value);
	explicit OrderedJsonValue(int value);
	explicit OrderedJsonValue(qint64 value);
	explicit OrderedJsonValue(double value);
	explicit OrderedJsonValue(QString value);
	explicit OrderedJsonValue(Array value);
	explicit OrderedJsonValue(OrderedJsonObject value);

	static OrderedJsonValue fromJsonValue(const QJsonValue& value);
	QJsonValue toJsonValue() const;
	QVariant toVariant() const;

	Type type() const { return m_type; }
	bool isUndefined() const { return m_type == Type::Undefined; }
	bool isNull() const { return m_type == Type::Null; }
	bool isBool() const { return m_type == Type::Bool; }
	bool isInt() const { return m_type == Type::Int; }
	bool isDouble() const { return m_type == Type::Double; }
	bool isNumber() const { return m_type == Type::Int || m_type == Type::Double; }
	bool isString() const { return m_type == Type::String; }
	bool isArray() const { return m_type == Type::Array; }
	bool isObject() const { return m_type == Type::Object; }

	bool toBool(bool defaultValue = false) const;
	int toInt(int defaultValue = 0) const;
	qint64 toInt64(qint64 defaultValue = 0) const;
	double toDouble(double defaultValue = 0.0) const;
	QString toString(const QString& defaultValue = {}) const;
	Array toArray() const;
	OrderedJsonObject toObject() const;

	QString toCompactJson() const;
	QString toIndentedJson() const;

	bool operator==(const OrderedJsonValue& other) const;
	bool operator!=(const OrderedJsonValue& other) const { return !(*this == other); }

private:
	Type m_type = Type::Undefined;
	bool m_bool = false;
	qint64 m_int = 0;
	double m_double = 0.0;
	QString m_string;
	std::shared_ptr<const Array> m_array;
	std::shared_ptr<const OrderedJsonObject> m_object;
};

// Insertion-ordered JSON object whose `operator==` compares exact key order as well as values.
class OrderedJsonObject
{
public:
	struct Entry
	{
		QString key;
		OrderedJsonValue value;

		bool operator==(const Entry& other) const = default;
	};

	OrderedJsonObject() = default;

	static OrderedJsonObject fromJsonObject(const QJsonObject& object);
	QJsonObject toJsonObject() const;
	QVariantMap toVariantMap() const;

	bool isEmpty() const { return m_entries.isEmpty(); }
	qsizetype size() const { return m_entries.size(); }
	const QList<Entry>& entries() const & { return m_entries; }
	QList<Entry> entries() && { return std::move(m_entries); }
	QList<Entry> entries() const && { return m_entries; }
	QStringList keys() const;

	qsizetype indexOf(QStringView key) const;
	bool contains(QStringView key) const { return indexOf(key) >= 0; }
	OrderedJsonValue value(QStringView key) const;

	// Updates `key` in place when present, appends at end when new, or removes when `value` is Undefined.
	void set(const QString& key, OrderedJsonValue value);
	bool remove(QStringView key);
	// Renames `oldKey` to `newKey` in place at its existing index.
	bool renameKey(QStringView oldKey, const QString& newKey);

	bool operator==(const OrderedJsonObject& other) const { return m_entries == other.m_entries; }
	bool operator!=(const OrderedJsonObject& other) const { return !(*this == other); }

private:
	QList<Entry> m_entries;
};

// Lossless JSON/JSONC object patcher for `ref.json`: reads, writes, renames and removes nested
// paths while keeping insertion key order, untouched sibling keys, `extraBody`, and `{env:...}` /
// `{file:...}` interpolation tokens intact.
class ConfigJsonPatcher
{
public:
	ConfigJsonPatcher() = default;
	explicit ConfigJsonPatcher(OrderedJsonObject root);
	explicit ConfigJsonPatcher(const QJsonObject& root);

	// Parses JSONC (`//`, `/* */` comments and trailing commas) into a patcher when the root is an object.
	static client::Result<ConfigJsonPatcher> parseJsonc(QStringView text);
	// Parses any single JSONC value (object, array, string, number, bool, or null) preserving key order.
	static client::Result<OrderedJsonValue> parseJsoncValue(QStringView text);

	const OrderedJsonObject& orderedRoot() const { return m_root; }
	void setOrderedRoot(OrderedJsonObject root) { m_root = std::move(root); }

	QJsonObject root() const { return m_root.toJsonObject(); }
	void setRoot(const QJsonObject& root) { m_root = OrderedJsonObject::fromJsonObject(root); }

	// Formats the root object as 2-space indented UTF-8 JSON with a trailing newline.
	QString toJson() const;

	// Reads the value at `path`, returning Undefined when `path` is empty or any segment is missing.
	OrderedJsonValue orderedValueAt(const QStringList& path) const;
	QJsonValue valueAt(const QStringList& path) const;
	bool contains(const QStringList& path) const;

	// Sets `value` at `path`, creating intermediate objects as needed and preserving sibling order.
	// Passing Undefined removes `path`.
	void setOrderedValueAt(const QStringList& path, const OrderedJsonValue& value);
	void setValueAt(const QStringList& path, const QJsonValue& value);

	// Removes the key at `path`. When `pruneEmptyAncestors` is true, intermediate objects along
	// `path` that become empty as a result of this removal are removed as well.
	void removeAt(const QStringList& path, bool pruneEmptyAncestors = true);

	// Renames `oldKey` to `newKey` in place inside the object at `parentPath`. Returns false when
	// the parent is not an object, `oldKey` is missing, `newKey` is empty, or `newKey` already exists.
	bool renameKeyAt(const QStringList& parentPath, const QString& oldKey, const QString& newKey);

	// Unique `{env:VAR}` variable names referenced in string values of `value` / `root()`, sorted ascending.
	static QStringList envReferencesIn(const OrderedJsonValue& value);
	static QStringList envReferencesIn(const QJsonValue& value);
	QStringList envReferences() const;

	// True when `text` contains a closed `{env:...}` or `{file:...}` interpolation token.
	static bool hasInterpolation(QStringView text);

private:
	OrderedJsonObject m_root;
};

} // namespace refgui::workspace
