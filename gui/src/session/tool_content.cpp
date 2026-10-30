#include "session/tool_content.h"

#include <algorithm>

#include <QCoreApplication>
#include <QHash>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLocale>
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

// The input's value for `key` when the input has it as text, even empty text.
std::optional<QString> optionalText(const QVariantMap& input, const QString& key)
{
	const QVariant value = input.value(key);
	if(value.metaType().id() != QMetaType::QString)
		return std::nullopt;
	return value.toString();
}

std::optional<QString> bashDetail(const QVariantMap& input)
{
	const auto command = optionalText(input, u"command"_s);
	if(!command)
		return std::nullopt;
	return u"$ "_s + firstLine(*command);
}

std::optional<QString> editDetail(const QVariantMap& input)
{
	const auto path = optionalText(input, u"path"_s);
	if(!path)
		return std::nullopt;
	const qsizetype edits = listArg(input, u"edits"_s).size();
	if(edits <= 1)
		return path;
	return QCoreApplication::translate("refgui::session::tool_content", "%1 (%2 edits)").arg(*path).arg(edits);
}

std::optional<QString> grepDetail(const QVariantMap& input)
{
	const auto pattern = optionalText(input, u"pattern"_s);
	if(!pattern)
		return std::nullopt;
	const auto path = optionalText(input, u"path"_s);
	if(!path)
		return u"\"%1\""_s.arg(*pattern);
	return QCoreApplication::translate("refgui::session::tool_content", "\"%1\" in %2").arg(*pattern, *path);
}

std::optional<QString> findDetail(const QVariantMap& input)
{
	const auto pattern = optionalText(input, u"pattern"_s);
	return pattern ? pattern : optionalText(input, u"path"_s);
}

std::optional<QString> webfetchDetail(const QVariantMap& input)
{
	const auto url = optionalText(input, u"url"_s);
	if(!url)
		return std::nullopt;
	const auto format = optionalText(input, u"format"_s);
	if(!format || *format == u"markdown"_s)
		return url;
	return u"%1 (%2)"_s.arg(*url, *format);
}

std::optional<QString> recipientDetail(const QVariantMap& input)
{
	const auto to = optionalText(input, u"to"_s);
	if(!to)
		return std::nullopt;
	return u"\u2192 "_s + *to;
}

std::optional<QString> createSessionDetail(const QVariantMap& input)
{
	const auto name = optionalText(input, u"name"_s);
	if(!name)
		return std::nullopt;
	return u"%1 (%2)"_s.arg(*name, optionalText(input, u"agent"_s).value_or(QString()));
}

std::optional<QString> todoDetail(const QVariantMap& input)
{
	const QVariantList items = todos(input);
	if(items.isEmpty())
		return std::nullopt;
	const auto isDone = [](const QVariant& item) { return item.toMap().value(u"status"_s).toString() == u"completed"_s; };
	const qsizetype done = std::count_if(items.cbegin(), items.cend(), isDone);
	return QCoreApplication::translate("refgui::session::tool_content", "%1/%2 done").arg(done).arg(items.size());
}

// The detail the tool's own arguments give, if it has a rule for them.
std::optional<QString> toolDetail(const QString& toolName, const QVariantMap& input)
{
	if(toolName == u"bash"_s)
		return bashDetail(input);
	if(toolName == u"read"_s || toolName == u"write"_s || toolName == u"delete"_s)
		return optionalText(input, u"path"_s);
	if(toolName == u"edit"_s)
		return editDetail(input);
	if(toolName == u"grep"_s)
		return grepDetail(input);
	if(toolName == u"find"_s)
		return findDetail(input);
	if(toolName == u"webfetch"_s)
		return webfetchDetail(input);
	if(toolName == u"send_message"_s)
		return recipientDetail(input);
	if(toolName == u"create_session"_s)
		return createSessionDetail(input);
	if(toolName == u"read_session"_s || toolName == u"close_session"_s || toolName == u"context_window"_s)
		return optionalText(input, u"session"_s);
	if(toolName == u"todo"_s)
		return todoDetail(input);
	return std::nullopt;
}

// Terminal columns of a character, roughly: 0 for combining marks, 2 for East Asian wide ones and
// emoji, else 1. The terminal client cuts text by columns.
int columnsOf(char32_t character)
{
	struct Range
	{
		char32_t first;
		char32_t last;
	};
	static constexpr Range wide[] = {
		{0x1100, 0x115F}, {0x2E80, 0xA4CF}, {0xAC00, 0xD7A3}, {0xF900, 0xFAFF}, {0xFE30, 0xFE6F},
		{0xFF00, 0xFF60}, {0xFFE0, 0xFFE6}, {0x1F300, 0x1F64F}, {0x1F900, 0x1F9FF}, {0x20000, 0x3FFFD},
	};
	const QChar::Category category = QChar::category(character);
	if(category == QChar::Mark_NonSpacing || category == QChar::Mark_Enclosing || category == QChar::Other_Format)
		return 0;
	const bool isWide = std::any_of(std::begin(wide), std::end(wide), [character](const Range& r) { return character >= r.first && character <= r.last; });
	return isWide ? 2 : 1;
}

// `text` cut to at most `maxColumns` columns, ending in "…" when something was cut.
QString truncated(const QString& text, int maxColumns)
{
	int columns = 0;
	for(const char32_t character : text.toUcs4())
		columns += columnsOf(character);
	if(columns <= maxColumns)
		return text;

	QString result;
	int used = 0;
	for(const char32_t character : text.toUcs4())
	{
		const int width = columnsOf(character);
		if(used + width > maxColumns - 1)
			break;
		used += width;
		result += QString::fromUcs4(&character, 1);
	}
	return result + u'\u2026';
}

// Whitespace runs become one space, control characters go, nothing leads or trails.
QString oneLine(const QString& text)
{
	QString result;
	bool space = false;
	for(const QChar character : text)
	{
		if(character.isSpace())
		{
			space = true;
			continue;
		}
		if(character.category() == QChar::Other_Control)
			continue;
		if(space && !result.isEmpty())
			result += u' ';
		space = false;
		result += character;
	}
	return result;
}

// The text of a plain argument (text, number, flag) for `key=value`, if it is one.
std::optional<QString> plainValue(const QVariant& value)
{
	switch(value.metaType().id())
	{
	case QMetaType::QString:
		return oneLine(value.toString());
	case QMetaType::Bool:
		return value.toBool() ? u"true"_s : u"false"_s;
	case QMetaType::Int:
	case QMetaType::UInt:
	case QMetaType::LongLong:
	case QMetaType::ULongLong:
		return QString::number(value.toLongLong());
	case QMetaType::Double:
		return QString::number(value.toDouble(), 'g', QLocale::FloatingPointShortest);
	default:
		return std::nullopt;
	}
}

// `key=value` of the first few plain arguments, for tools without a rule of their own.
QString genericDetail(const QVariantMap& input)
{
	constexpr int MaxArguments = 3;
	constexpr int MaxValueColumns = 40;
	QStringList parts;
	for(auto it = input.cbegin(); it != input.cend() && parts.size() < MaxArguments; ++it)
	{
		if(const auto value = plainValue(it.value()))
			parts.append(u"%1=%2"_s.arg(it.key(), truncated(*value, MaxValueColumns)));
	}
	return parts.join(u' ');
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
	const auto own = toolDetail(toolName, input);
	return own ? oneLine(*own) : genericDetail(input);
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
