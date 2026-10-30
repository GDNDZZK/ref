#pragma once

#include "proto/parse.h"

#include <QJsonArray>
#include <QJsonObject>
#include <QString>
#include <QStringView>

#include <utility>
#include <optional>

// Reading typed fields out of a JSON object for the parse functions. Failures throw
// Failure, which the parse functions turn into a ParseError at their boundary, so the
// field-by-field code reads straight through.
namespace refgui::proto::json
{

struct Failure
{
	ParseError error;
};

[[noreturn]] void fail(const QString& path, const QString& problem);
[[noreturn]] void failUnknown(const QString& path, const QString& problem);

// Runs `parse` and returns its result, or the error it threw.
template<typename Parse>
auto guarded(Parse&& parse) -> Parsed<decltype(parse())>
{
	try
	{
		return parse();
	}
	catch(const Failure& failure)
	{
		return std::unexpected(failure.error);
	}
}

// One JSON object and the path leading to it, for error messages.
class Fields
{
public:
	Fields(QJsonObject object, QString path)
		: m_object(std::move(object))
		, m_path(std::move(path))
	{
	}

	const QString& path() const { return m_path; }

	// Required fields.
	QString string(QLatin1StringView key) const;
	qint64 integer(QLatin1StringView key) const;
	QJsonObject object(QLatin1StringView key) const;
	QJsonArray array(QLatin1StringView key) const;

	// Optional fields: missing and null both give the default.
	std::optional<QString> optionalString(QLatin1StringView key) const;
	std::optional<qint64> optionalInteger(QLatin1StringView key) const;
	std::optional<QJsonObject> optionalObject(QLatin1StringView key) const;
	qint64 integerOr(QLatin1StringView key, qint64 fallback) const;
	bool booleanOr(QLatin1StringView key, bool fallback) const;
	QJsonArray arrayOrEmpty(QLatin1StringView key) const;
	QJsonValue value(QLatin1StringView key) const; // null when missing

	QString childPath(QLatin1StringView key) const;

	// Parses every element of the array under `key` (required) with `parse(Fields)`.
	template<typename T, typename Parse>
	QList<T> list(QLatin1StringView key, Parse&& parse) const
	{
		return elements<T>(array(key), key, parse);
	}
	template<typename T, typename Parse>
	QList<T> listOrEmpty(QLatin1StringView key, Parse&& parse) const
	{
		return elements<T>(arrayOrEmpty(key), key, parse);
	}

private:
	template<typename T, typename Parse>
	QList<T> elements(const QJsonArray& array, QLatin1StringView key, Parse& parse) const
	{
		QList<T> result;
		result.reserve(array.size());
		for(qsizetype i = 0; i < array.size(); ++i)
		{
			const QString path = childPath(key) + QStringLiteral("[%1]").arg(i);
			if(!array.at(i).isObject())
			{
				fail(path, QStringLiteral("expected an object"));
			}
			result.append(parse(Fields(array.at(i).toObject(), path)));
		}
		return result;
	}

	QJsonObject m_object;
	QString m_path;
};

} // namespace refgui::proto::json
