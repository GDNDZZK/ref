#include "workspace/config_json_patcher.h"

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonParseError>
#include <QSet>
#include <QSpan>

#include <algorithm>
#include <utility>

namespace refgui::workspace
{

using namespace Qt::StringLiterals;

namespace
{

constexpr QStringView EnvOpen = u"{env:";
constexpr QStringView FileOpen = u"{file:";

bool isJsonSpace(QChar ch)
{
	return ch == u' ' || ch == u'\t' || ch == u'\r' || ch == u'\n';
}

client::Result<QString> stripJsoncComments(QStringView text)
{
	QString out;
	out.reserve(text.size());
	bool inString = false;
	bool escaped = false;

	for(qsizetype i = 0; i < text.size(); ++i)
	{
		const QChar ch = text[i];
		if(inString)
		{
			out.append(ch);
			if(escaped)
			{
				escaped = false;
			}
			else if(ch == u'\\')
			{
				escaped = true;
			}
			else if(ch == u'"')
			{
				inString = false;
			}
			continue;
		}

		if(ch == u'"')
		{
			inString = true;
			out.append(ch);
			continue;
		}

		const bool lineComment = ch == u'/' && i + 1 < text.size() && text[i + 1] == u'/';
		if(lineComment)
		{
			i += 2;
			while(i < text.size() && text[i] != u'\n')
			{
				++i;
			}
			if(i < text.size())
			{
				out.append(u'\n');
			}
			continue;
		}

		const bool blockComment = ch == u'/' && i + 1 < text.size() && text[i + 1] == u'*';
		if(blockComment)
		{
			i += 2;
			bool closed = false;
			while(i < text.size())
			{
				if(text[i] == u'*' && i + 1 < text.size() && text[i + 1] == u'/')
				{
					++i;
					closed = true;
					break;
				}
				if(text[i] == u'\n')
				{
					out.append(u'\n');
				}
				++i;
			}
			if(!closed)
			{
				return std::unexpected(client::Failure{u"Unclosed block comment"_s});
			}
			continue;
		}

		out.append(ch);
	}
	return out;
}

QString stripTrailingCommas(QStringView text)
{
	QString out;
	out.reserve(text.size());
	bool inString = false;
	bool escaped = false;

	for(qsizetype i = 0; i < text.size(); ++i)
	{
		const QChar ch = text[i];
		if(inString)
		{
			out.append(ch);
			if(escaped)
			{
				escaped = false;
			}
			else if(ch == u'\\')
			{
				escaped = true;
			}
			else if(ch == u'"')
			{
				inString = false;
			}
			continue;
		}

		if(ch == u'"')
		{
			inString = true;
			out.append(ch);
			continue;
		}

		if(ch == u',')
		{
			qsizetype lookahead = i + 1;
			while(lookahead < text.size() && isJsonSpace(text[lookahead]))
			{
				++lookahead;
			}
			const bool beforeClose = lookahead < text.size() && (text[lookahead] == u'}' || text[lookahead] == u']');
			if(beforeClose)
			{
				continue;
			}
		}

		out.append(ch);
	}
	return out;
}

void setInObject(QJsonObject& object, QSpan<const QString> path, const QJsonValue& value)
{
	const QString& head = path.front();
	if(path.size() == 1)
	{
		object.insert(head, value);
		return;
	}
	const QJsonValue existing = object.value(head);
	QJsonObject child = existing.isObject() ? existing.toObject() : QJsonObject{};
	setInObject(child, path.subspan(1), value);
	object.insert(head, child);
}

void removeFromObject(QJsonObject& object, QSpan<const QString> path, bool pruneEmptyAncestors)
{
	const QString& head = path.front();
	if(!object.contains(head))
	{
		return;
	}
	if(path.size() == 1)
	{
		object.remove(head);
		return;
	}
	const QJsonValue existing = object.value(head);
	if(!existing.isObject())
	{
		return;
	}
	QJsonObject child = existing.toObject();
	removeFromObject(child, path.subspan(1), pruneEmptyAncestors);
	if(pruneEmptyAncestors && child.isEmpty())
	{
		object.remove(head);
		return;
	}
	object.insert(head, child);
}

qsizetype nextTokenStart(QStringView rest)
{
	const qsizetype envPos = rest.indexOf(EnvOpen);
	const qsizetype filePos = rest.indexOf(FileOpen);
	if(envPos < 0)
	{
		return filePos;
	}
	if(filePos < 0)
	{
		return envPos;
	}
	return std::min(envPos, filePos);
}

void collectEnvFromString(QStringView text, QSet<QString>& found)
{
	QStringView rest = text;
	while(!rest.isEmpty())
	{
		const qsizetype start = nextTokenStart(rest);
		if(start < 0)
		{
			return;
		}
		const QStringView fromToken = rest.sliced(start);
		const qsizetype closeOffset = fromToken.indexOf(u'}');
		if(closeOffset < 0)
		{
			return;
		}
		const QStringView token = fromToken.first(closeOffset);
		if(token.startsWith(EnvOpen))
		{
			const QString name = token.sliced(EnvOpen.size()).trimmed().toString();
			if(!name.isEmpty())
			{
				found.insert(name);
			}
		}
		rest = fromToken.sliced(closeOffset + 1);
	}
}

void collectEnvFromValue(const QJsonValue& value, QSet<QString>& found)
{
	if(value.isString())
	{
		collectEnvFromString(value.toString(), found);
		return;
	}
	if(value.isArray())
	{
		const QJsonArray array = value.toArray();
		for(const QJsonValue& item : array)
		{
			collectEnvFromValue(item, found);
		}
		return;
	}
	if(value.isObject())
	{
		const QJsonObject object = value.toObject();
		for(auto it = object.constBegin(); it != object.constEnd(); ++it)
		{
			collectEnvFromValue(it.value(), found);
		}
	}
}

} // namespace

ConfigJsonPatcher::ConfigJsonPatcher(QJsonObject root)
	: m_root(std::move(root))
{
}

client::Result<ConfigJsonPatcher> ConfigJsonPatcher::parseJsonc(QStringView text)
{
	const client::Result<QString> uncommented = stripJsoncComments(text);
	if(!uncommented)
	{
		return std::unexpected(uncommented.error());
	}
	const QString normalized = stripTrailingCommas(*uncommented);
	QJsonParseError parseError;
	const QJsonDocument doc = QJsonDocument::fromJson(normalized.toUtf8(), &parseError);
	if(parseError.error != QJsonParseError::NoError)
	{
		return std::unexpected(client::Failure{parseError.errorString()});
	}
	if(!doc.isObject())
	{
		return std::unexpected(client::Failure{u"Configuration root must be a JSON object."_s});
	}
	return ConfigJsonPatcher(doc.object());
}

QString ConfigJsonPatcher::toJson() const
{
	if(m_root.isEmpty())
	{
		return u"{}\n"_s;
	}
	const QString indented4 = QString::fromUtf8(QJsonDocument(m_root).toJson(QJsonDocument::Indented));
	const QStringList lines = indented4.split(u'\n');
	QStringList out;
	out.reserve(lines.size());
	for(const QString& line : lines)
	{
		qsizetype spaces = 0;
		while(spaces < line.size() && line[spaces] == u' ')
		{
			++spaces;
		}
		const qsizetype halfSpaces = (spaces / 4) * 2 + (spaces % 4);
		out.append(QString(halfSpaces, u' ') + line.sliced(spaces));
	}
	QString joined = out.join(u'\n');
	if(!joined.endsWith(u'\n'))
	{
		joined.append(u'\n');
	}
	return joined;
}

QJsonValue ConfigJsonPatcher::valueAt(const QStringList& path) const
{
	if(path.isEmpty())
	{
		return QJsonValue::Undefined;
	}
	QJsonValue current = m_root;
	for(const QString& key : path)
	{
		if(!current.isObject())
		{
			return QJsonValue::Undefined;
		}
		const QJsonObject object = current.toObject();
		const auto it = object.constFind(key);
		if(it == object.constEnd())
		{
			return QJsonValue::Undefined;
		}
		current = it.value();
	}
	return current;
}

bool ConfigJsonPatcher::contains(const QStringList& path) const
{
	return !valueAt(path).isUndefined();
}

void ConfigJsonPatcher::setValueAt(const QStringList& path, const QJsonValue& value)
{
	if(path.isEmpty())
	{
		return;
	}
	if(value.isUndefined())
	{
		removeAt(path);
		return;
	}
	setInObject(m_root, path, value);
}

void ConfigJsonPatcher::removeAt(const QStringList& path, bool pruneEmptyAncestors)
{
	if(path.isEmpty())
	{
		return;
	}
	removeFromObject(m_root, path, pruneEmptyAncestors);
}

bool ConfigJsonPatcher::renameKeyAt(const QStringList& parentPath, const QString& oldKey, const QString& newKey)
{
	if(oldKey.isEmpty() || newKey.isEmpty())
	{
		return false;
	}
	QJsonObject parentObject;
	if(parentPath.isEmpty())
	{
		parentObject = m_root;
	}
	else
	{
		const QJsonValue target = valueAt(parentPath);
		if(!target.isObject())
		{
			return false;
		}
		parentObject = target.toObject();
	}

	if(!parentObject.contains(oldKey))
	{
		return false;
	}
	if(oldKey == newKey)
	{
		return true;
	}
	if(parentObject.contains(newKey))
	{
		return false;
	}
	const QJsonValue moved = parentObject.take(oldKey);
	parentObject.insert(newKey, moved);
	if(parentPath.isEmpty())
	{
		m_root = parentObject;
	}
	else
	{
		setValueAt(parentPath, parentObject);
	}
	return true;
}

QStringList ConfigJsonPatcher::envReferencesIn(const QJsonValue& value)
{
	QSet<QString> found;
	collectEnvFromValue(value, found);
	QStringList sorted = found.values();
	std::ranges::sort(sorted);
	return sorted;
}

QStringList ConfigJsonPatcher::envReferences() const
{
	return envReferencesIn(m_root);
}

bool ConfigJsonPatcher::hasInterpolation(QStringView text)
{
	QStringView rest = text;
	while(!rest.isEmpty())
	{
		const qsizetype start = nextTokenStart(rest);
		if(start < 0)
		{
			return false;
		}
		const QStringView fromToken = rest.sliced(start);
		const qsizetype closeOffset = fromToken.indexOf(u'}');
		if(closeOffset < 0)
		{
			return false;
		}
		const QStringView token = fromToken.first(closeOffset);
		const qsizetype prefixLen = token.startsWith(EnvOpen) ? EnvOpen.size() : FileOpen.size();
		if(!token.sliced(prefixLen).trimmed().isEmpty())
		{
			return true;
		}
		rest = fromToken.sliced(closeOffset + 1);
	}
	return false;
}

} // namespace refgui::workspace
