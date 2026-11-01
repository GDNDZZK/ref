#include "proto/json_reader.h"

using namespace Qt::StringLiterals;

namespace refgui::proto::json
{

void fail(const QString& path, const QString& problem)
{
	throw Failure{ParseError{.message = u"%1: %2"_s.arg(path, problem)}};
}

void failUnknown(const QString& path, const QString& problem)
{
	throw Failure{ParseError{.message = u"%1: %2"_s.arg(path, problem), .unknown = true}};
}

QString Fields::childPath(QLatin1StringView key) const
{
	return m_path.isEmpty() ? QString(key) : m_path + u'.' + key;
}

bool Fields::isAbsent(QLatin1StringView key) const
{
	const QJsonValue found = m_object.value(key);
	return found.isNull() || found.isUndefined();
}

QString Fields::string(QLatin1StringView key) const
{
	const QJsonValue found = m_object.value(key);
	if(!found.isString())
	{
		fail(childPath(key), u"expected a string"_s);
	}
	return found.toString();
}

qint64 Fields::integer(QLatin1StringView key) const
{
	const QJsonValue found = m_object.value(key);
	if(!found.isDouble())
	{
		fail(childPath(key), u"expected a number"_s);
	}
	return found.toInteger();
}

QJsonObject Fields::object(QLatin1StringView key) const
{
	const QJsonValue found = m_object.value(key);
	if(!found.isObject())
	{
		fail(childPath(key), u"expected an object"_s);
	}
	return found.toObject();
}

QJsonArray Fields::array(QLatin1StringView key) const
{
	const QJsonValue found = m_object.value(key);
	if(!found.isArray())
	{
		fail(childPath(key), u"expected an array"_s);
	}
	return found.toArray();
}

quint64 Fields::unsignedInteger(QLatin1StringView key) const
{
	return static_cast<quint64>(integer(key));
}

Fields Fields::child(QLatin1StringView key) const
{
	return Fields(object(key), childPath(key));
}

std::optional<QString> Fields::optionalString(QLatin1StringView key) const
{
	if(isAbsent(key))
	{
		return std::nullopt;
	}
	return string(key);
}

std::optional<qint64> Fields::optionalInteger(QLatin1StringView key) const
{
	if(isAbsent(key))
	{
		return std::nullopt;
	}
	return integer(key);
}

std::optional<QJsonObject> Fields::optionalObject(QLatin1StringView key) const
{
	if(isAbsent(key))
	{
		return std::nullopt;
	}
	return object(key);
}

qint64 Fields::integerOr(QLatin1StringView key, qint64 fallback) const
{
	return optionalInteger(key).value_or(fallback);
}

quint64 Fields::unsignedOr(QLatin1StringView key, quint64 fallback) const
{
	return static_cast<quint64>(integerOr(key, static_cast<qint64>(fallback)));
}

std::optional<Fields> Fields::optionalChild(QLatin1StringView key) const
{
	if(isAbsent(key))
	{
		return std::nullopt;
	}
	return child(key);
}

bool Fields::booleanOr(QLatin1StringView key, bool fallback) const
{
	if(isAbsent(key))
	{
		return fallback;
	}
	const QJsonValue found = m_object.value(key);
	if(!found.isBool())
	{
		fail(childPath(key), u"expected a boolean"_s);
	}
	return found.toBool();
}

QJsonArray Fields::arrayOrEmpty(QLatin1StringView key) const
{
	if(isAbsent(key))
	{
		return {};
	}
	return array(key);
}

QJsonValue Fields::value(QLatin1StringView key) const
{
	const QJsonValue found = m_object.value(key);
	return found.isUndefined() ? QJsonValue() : found;
}

} // namespace refgui::proto::json
