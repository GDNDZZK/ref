#include "session/tool_content.h"

#include <algorithm>

#include <QCoreApplication>
#include <QHash>
#include <QJsonDocument>
#include <QJsonObject>
#include <QRegularExpression>

using namespace Qt::StringLiterals;

namespace refgui::session::tool_content
{

namespace
{

// The input's value for `key` when it is a list, else empty.
QVariantList listArg(const QVariantMap& input, const QString& key)
{
	const QVariant value = input.value(key);
	return value.metaType().id() == QMetaType::QVariantList ? value.toList() : QVariantList();
}

// The input's value for `key` when it is text, else empty.
QString textArg(const QVariantMap& input, const QString& key)
{
	const QVariant value = input.value(key);
	return value.metaType().id() == QMetaType::QString ? value.toString() : QString();
}

QString bashDetail(const QVariantMap& input)
{
	const QString command = textArg(input, u"command"_s);
	return command.isEmpty() ? QString() : u"$ "_s + firstLine(command);
}

QString editDetail(const QVariantMap& input)
{
	const QString path = textArg(input, u"path"_s);
	const qsizetype edits = listArg(input, u"edits"_s).size();
	if(edits <= 1)
		return path;
	return QCoreApplication::translate("refgui::session::tool_content", "%1 (%2 edits)").arg(path).arg(edits);
}

QString grepDetail(const QVariantMap& input)
{
	const QString pattern = textArg(input, u"pattern"_s);
	const QString path = textArg(input, u"path"_s);
	if(path.isEmpty())
		return u"\"%1\""_s.arg(pattern);
	return QCoreApplication::translate("refgui::session::tool_content", "\"%1\" in %2").arg(pattern, path);
}

QString findDetail(const QVariantMap& input)
{
	const QString pattern = textArg(input, u"pattern"_s);
	return pattern.isEmpty() ? textArg(input, u"path"_s) : pattern;
}

QString recipientDetail(const QVariantMap& input)
{
	const QString to = textArg(input, u"to"_s);
	return to.isEmpty() ? QString() : u"\u2192 "_s + to;
}

QString todoDetail(const QVariantMap& input)
{
	const QVariantList items = todos(input);
	if(items.isEmpty())
		return {};
	const auto isDone = [](const QVariant& item) { return item.toMap().value(u"status"_s).toString() == u"completed"_s; };
	const qsizetype done = std::count_if(items.cbegin(), items.cend(), isDone);
	return QCoreApplication::translate("refgui::session::tool_content", "%1/%2 done").arg(done).arg(items.size());
}

// The row of a diff line.
QVariantMap diffRow(const QString& line)
{
	QString kind = u"context"_s;
	if(line.startsWith(u"+++"_s) || line.startsWith(u"---"_s))
		kind = u"file"_s;
	else if(line.startsWith(u"@@"_s))
		kind = u"hunk"_s;
	else if(line.startsWith(u'+'))
		kind = u"added"_s;
	else if(line.startsWith(u'-'))
		kind = u"removed"_s;
	const bool marked = kind == u"added"_s || kind == u"removed"_s;
	return {
		{u"text"_s, marked ? line.mid(1) : line},
		{u"sign"_s, marked ? line.left(1) : QString()},
		{u"kind"_s, kind},
	};
}

} // namespace

QString iconName(const QString& toolName)
{
	static const QHash<QString, QString> icons{
		{u"read"_s, u"file-text"_s},
		{u"edit"_s, u"pencil"_s},
		{u"write"_s, u"file-plus"_s},
		{u"delete"_s, u"trash"_s},
		{u"bash"_s, u"terminal"_s},
		{u"grep"_s, u"search"_s},
		{u"find"_s, u"folder-search"_s},
		{u"webfetch"_s, u"globe"_s},
		{u"todo"_s, u"todo"_s},
		{u"create_session"_s, u"chat-plus"_s},
		{u"send_message"_s, u"chat-send"_s},
		{u"message_status"_s, u"chat-send"_s},
		{u"list_sessions"_s, u"chat-list"_s},
		{u"close_session"_s, u"chat-x"_s},
		{u"read_session"_s, u"chat"_s},
	};
	const auto known = icons.constFind(toolName);
	if(known != icons.cend())
		return *known;
	return toolName.startsWith(u"mcp_"_s) ? u"mcp"_s : u"tool"_s;
}

QVariantMap parseInput(const QString& inputJson)
{
	if(inputJson.isEmpty())
		return {};
	const QJsonDocument document = QJsonDocument::fromJson(inputJson.toUtf8());
	return document.isObject() ? document.object().toVariantMap() : QVariantMap();
}

QVariantList todos(const QVariantMap& input)
{
	static const QStringList knownStatuses{u"in_progress"_s, u"completed"_s, u"cancelled"_s};
	QVariantList result;
	for(const QVariant& entry : listArg(input, u"todos"_s))
	{
		const QVariantMap item = entry.toMap();
		const QString content = textArg(item, u"content"_s).trimmed();
		if(content.isEmpty())
			continue;
		const QString status = textArg(item, u"status"_s);
		result.append(QVariantMap{
			{u"content"_s, content},
			{u"status"_s, knownStatuses.contains(status) ? status : u"pending"_s},
		});
	}
	return result;
}

QString detail(const QString& toolName, const QVariantMap& input)
{
	if(toolName == u"bash"_s)
		return bashDetail(input);
	if(toolName == u"read"_s || toolName == u"write"_s || toolName == u"delete"_s)
		return textArg(input, u"path"_s);
	if(toolName == u"edit"_s)
		return editDetail(input);
	if(toolName == u"grep"_s)
		return grepDetail(input);
	if(toolName == u"find"_s)
		return findDetail(input);
	if(toolName == u"webfetch"_s)
		return textArg(input, u"url"_s);
	if(toolName == u"send_message"_s)
		return recipientDetail(input);
	if(toolName == u"create_session"_s)
		return textArg(input, u"name"_s);
	if(toolName == u"read_session"_s || toolName == u"close_session"_s)
		return textArg(input, u"session"_s);
	if(toolName == u"todo"_s)
		return todoDetail(input);
	return {};
}

QVariantList listing(const QString& output)
{
	static const QRegularExpression header(uR"(\A\[(.*)#([0-9A-Fa-f]{4})\]\z)"_s);
	static const QRegularExpression numbered(uR"(\A([ *]?)(\d+):(.*)\z)"_s);

	struct Group
	{
		QString path;
		QString tag;
		QVariantList rows;
		QVariantList notes;
	};
	QList<Group> groups;

	for(const QString& line : output.split(u'\n'))
	{
		if(line.isEmpty())
			continue;
		const QRegularExpressionMatch heading = header.match(line);
		if(heading.hasMatch())
		{
			groups.append({heading.captured(1), heading.captured(2), {}, {}});
			continue;
		}
		if(groups.isEmpty())
			groups.append(Group{});
		Group& group = groups.last();
		const QRegularExpressionMatch row = numbered.match(line);
		if(row.hasMatch())
		{
			const QString marker = row.captured(1);
			const QString kind = marker == u"*"_s ? u"match"_s : marker == u" "_s ? u"context"_s : u"line"_s;
			group.rows.append(QVariantMap{{u"number"_s, row.captured(2)}, {u"text"_s, row.captured(3)}, {u"kind"_s, kind}});
		}
		else if(line == u"..."_s)
			group.rows.append(QVariantMap{{u"number"_s, QString()}, {u"text"_s, u"\u22ef"_s}, {u"kind"_s, u"gap"_s}});
		else
			group.notes.append(line);
	}

	QVariantList result;
	for(const Group& group : groups)
		result.append(QVariantMap{{u"path"_s, group.path}, {u"tag"_s, group.tag}, {u"rows"_s, group.rows}, {u"notes"_s, group.notes}});
	return result;
}

QVariantList diffRows(const QString& diff)
{
	QStringList lines = diff.split(u'\n');
	if(!lines.isEmpty() && lines.last().isEmpty())
		lines.removeLast();
	QVariantList rows;
	rows.reserve(lines.size());
	for(const QString& line : lines)
		rows.append(diffRow(line));
	return rows;
}

QVariantMap splitPath(const QString& path)
{
	const bool isDirectory = path.endsWith(u'/');
	const QString trimmed = isDirectory ? path.chopped(1) : path;
	const qsizetype cut = trimmed.lastIndexOf(u'/');
	return {
		{u"directory"_s, path.left(cut + 1)},
		{u"name"_s, path.mid(cut + 1)},
		{u"isDirectory"_s, isDirectory},
	};
}

QString sizeText(qint64 bytes)
{
	constexpr qint64 Kibibyte = 1024;
	constexpr qint64 Mebibyte = Kibibyte * Kibibyte;
	if(bytes >= Mebibyte)
		return u"%1 MiB"_s.arg(static_cast<double>(bytes) / Mebibyte, 0, 'f', 1);
	if(bytes >= Kibibyte)
		return u"%1 KiB"_s.arg(static_cast<double>(bytes) / Kibibyte, 0, 'f', 1);
	return u"%1 B"_s.arg(bytes);
}

QString firstLine(const QString& text)
{
	const qsizetype end = text.indexOf(u'\n');
	return end < 0 ? text : text.left(end);
}

QString firstText(const QVariantMap& input, const QStringList& keys)
{
	for(const QString& key : keys)
	{
		const QString text = textArg(input, key);
		if(!text.isEmpty())
			return text;
	}
	return {};
}

} // namespace refgui::session::tool_content
