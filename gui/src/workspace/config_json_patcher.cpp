#include "workspace/config_json_patcher.h"

#include <QJsonArray>
#include <QLocale>
#include <QSet>
#include <QSpan>

#include <algorithm>
#include <cmath>
#include <limits>
#include <utility>

namespace refgui::workspace
{

using namespace Qt::StringLiterals;

namespace
{

constexpr QStringView EnvOpen = u"{env:";
constexpr QStringView FileOpen = u"{file:";
constexpr int MaxJsonDepth = 128;
constexpr int IndentSpaces = 2;
constexpr qint64 MaxSafeJsInteger = 9007199254740991LL;
constexpr double MaxSafeJsIntegerDouble = 9007199254740991.0;
constexpr char16_t MaxJsonControlChar = 0x001F;
constexpr char16_t HighSurrogateFirst = 0xD800;
constexpr char16_t HighSurrogateLast = 0xDBFF;
constexpr char16_t LowSurrogateFirst = 0xDC00;
constexpr char16_t LowSurrogateLast = 0xDFFF;

bool isJsonSpace(QChar ch)
{
	return ch == u' ' || ch == u'\t' || ch == u'\r' || ch == u'\n';
}

bool isAsciiDigit(QChar ch)
{
	return ch >= u'0' && ch <= u'9';
}

int hexDigitValue(QChar ch)
{
	if(ch >= u'0' && ch <= u'9')
	{
		return ch.unicode() - u'0';
	}
	if(ch >= u'a' && ch <= u'f')
	{
		return 10 + (ch.unicode() - u'a');
	}
	if(ch >= u'A' && ch <= u'F')
	{
		return 10 + (ch.unicode() - u'A');
	}
	return -1;
}

bool advanceBlockComment(QStringView text, qsizetype& index, QString& out)
{
	while(index < text.size())
	{
		if(text[index] == u'*' && index + 1 < text.size() && text[index + 1] == u'/')
		{
			++index;
			return true;
		}
		if(text[index] == u'\n')
		{
			out.append(u'\n');
		}
		++index;
	}
	return false;
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
			if(!advanceBlockComment(text, i, out))
			{
				return std::unexpected(client::Failure{u"Unclosed block comment"_s});
			}
			continue;
		}

		out.append(ch);
	}
	return out;
}

bool canPrecedeTrailingComma(QChar prev)
{
	return prev != u'\0' && prev != u'{' && prev != u'[' && prev != u',' && prev != u':';
}

QString stripTrailingCommas(QStringView text)
{
	QString out;
	out.reserve(text.size());
	bool inString = false;
	bool escaped = false;
	QChar lastTokenChar = u'\0';

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
				lastTokenChar = u'"';
			}
			continue;
		}

		if(ch == u'"')
		{
			inString = true;
			out.append(ch);
			continue;
		}

		if(ch == u',' && canPrecedeTrailingComma(lastTokenChar))
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

		if(!isJsonSpace(ch))
		{
			lastTokenChar = ch;
		}
		out.append(ch);
	}
	return out;
}

class JsonParser
{
public:
	explicit JsonParser(QStringView text)
		: m_text(text)
	{
	}

	client::Result<OrderedJsonValue> parseDocument()
	{
		skipWhitespace();
		const auto root = parseValue(0);
		if(!root)
		{
			return root;
		}
		skipWhitespace();
		if(m_pos != m_text.size())
		{
			return fail(u"Unexpected trailing characters in JSON."_s);
		}
		return root;
	}

private:
	template<typename T = OrderedJsonValue>
	static client::Result<T> fail(QString message)
	{
		return std::unexpected(client::Failure{std::move(message)});
	}

	void skipWhitespace()
	{
		while(m_pos < m_text.size() && isJsonSpace(m_text[m_pos]))
		{
			++m_pos;
		}
	}

	bool consume(QChar expected)
	{
		if(m_pos < m_text.size() && m_text[m_pos] == expected)
		{
			++m_pos;
			return true;
		}
		return false;
	}

	client::Result<OrderedJsonValue> parseValue(int depth)
	{
		if(depth > MaxJsonDepth)
		{
			return fail(u"JSON nesting depth exceeds maximum limit."_s);
		}
		skipWhitespace();
		if(m_pos >= m_text.size())
		{
			return fail(u"Unexpected end of JSON input."_s);
		}
		const QChar ch = m_text[m_pos];
		if(ch == u'{')
		{
			return parseObject(depth + 1);
		}
		if(ch == u'[')
		{
			return parseArray(depth + 1);
		}
		if(ch == u'"')
		{
			const auto str = parseString();
			return str ? client::Result<OrderedJsonValue>(OrderedJsonValue(*str)) : fail(str.error().message);
		}
		if(ch == u't')
		{
			return parseLiteral(u"true", OrderedJsonValue(true));
		}
		if(ch == u'f')
		{
			return parseLiteral(u"false", OrderedJsonValue(false));
		}
		if(ch == u'n')
		{
			return parseLiteral(u"null", OrderedJsonValue::null());
		}
		if(ch == u'-' || isAsciiDigit(ch))
		{
			return parseNumber();
		}
		return fail(u"Unexpected token in JSON input."_s);
	}

	client::Result<OrderedJsonValue> parseLiteral(QStringView literal, OrderedJsonValue result)
	{
		if(m_text.sliced(m_pos).startsWith(literal))
		{
			m_pos += literal.size();
			return result;
		}
		return fail(u"Invalid literal in JSON input."_s);
	}

	client::Result<char16_t> parseFourHexDigits()
	{
		if(m_pos + 4 > m_text.size())
		{
			return fail<char16_t>(u"Incomplete Unicode escape in JSON string."_s);
		}
		int codeUnit = 0;
		for(int i = 0; i < 4; ++i)
		{
			const int digit = hexDigitValue(m_text[m_pos + i]);
			if(digit < 0)
			{
				return fail<char16_t>(u"Invalid hex digit in Unicode escape."_s);
			}
			codeUnit = (codeUnit << 4) | digit;
		}
		m_pos += 4;
		return static_cast<char16_t>(codeUnit);
	}

	client::Result<void> parseUnicodeEscape(QString& out)
	{
		const auto first = parseFourHexDigits();
		if(!first)
		{
			return fail<void>(first.error().message);
		}
		const char16_t unit = *first;
		if(unit >= LowSurrogateFirst && unit <= LowSurrogateLast)
		{
			return fail<void>(u"Unexpected low surrogate in JSON string."_s);
		}
		if(unit >= HighSurrogateFirst && unit <= HighSurrogateLast)
		{
			const bool hasSecondEscape = m_pos + 2 <= m_text.size() && m_text[m_pos] == u'\\' && m_text[m_pos + 1] == u'u';
			if(!hasSecondEscape)
			{
				return fail<void>(u"Unpaired high surrogate in JSON string."_s);
			}
			m_pos += 2;
			const auto second = parseFourHexDigits();
			if(!second || *second < LowSurrogateFirst || *second > LowSurrogateLast)
			{
				return fail<void>(u"Invalid UTF-16 surrogate pair in JSON string."_s);
			}
			out.append(QChar(unit));
			out.append(QChar(*second));
			return {};
		}
		out.append(QChar(unit));
		return {};
	}

	client::Result<void> parseEscape(QString& out)
	{
		if(m_pos >= m_text.size())
		{
			return fail<void>(u"Unterminated escape sequence in JSON string."_s);
		}
		const QChar esc = m_text[m_pos++];
		switch(esc.unicode())
		{
		case u'"':
		case u'\\':
		case u'/':
			out.append(esc);
			return {};
		case u'b':
			out.append(u'\b');
			return {};
		case u'f':
			out.append(u'\f');
			return {};
		case u'n':
			out.append(u'\n');
			return {};
		case u'r':
			out.append(u'\r');
			return {};
		case u't':
			out.append(u'\t');
			return {};
		case u'u':
			return parseUnicodeEscape(out);
		default:
			return fail<void>(u"Invalid escape sequence in JSON string."_s);
		}
	}

	client::Result<QString> parseString()
	{
		++m_pos; // opening '"'
		QString out;
		while(m_pos < m_text.size())
		{
			const QChar ch = m_text[m_pos++];
			if(ch == u'"')
			{
				return out;
			}
			if(ch == u'\\')
			{
				const auto esc = parseEscape(out);
				if(!esc)
				{
					return fail<QString>(esc.error().message);
				}
				continue;
			}
			if(ch.unicode() <= MaxJsonControlChar)
			{
				return fail<QString>(u"Unescaped control character in JSON string."_s);
			}
			out.append(ch);
		}
		return fail<QString>(u"Unterminated JSON string."_s);
	}

	bool scanDigits()
	{
		if(m_pos >= m_text.size() || !isAsciiDigit(m_text[m_pos]))
		{
			return false;
		}
		while(m_pos < m_text.size() && isAsciiDigit(m_text[m_pos]))
		{
			++m_pos;
		}
		return true;
	}

	client::Result<OrderedJsonValue> parseNumber()
	{
		const qsizetype start = m_pos;
		consume(u'-');
		if(m_pos >= m_text.size() || !isAsciiDigit(m_text[m_pos]))
		{
			return fail(u"Invalid number in JSON input."_s);
		}
		if(m_text[m_pos] == u'0')
		{
			++m_pos;
			if(m_pos < m_text.size() && isAsciiDigit(m_text[m_pos]))
			{
				return fail(u"Leading zeros are not permitted in JSON numbers."_s);
			}
		}
		else
		{
			scanDigits();
		}

		bool isFloating = false;
		if(consume(u'.'))
		{
			isFloating = true;
			if(!scanDigits())
			{
				return fail(u"Expected digit after decimal point in JSON number."_s);
			}
		}
		if(m_pos < m_text.size() && (m_text[m_pos] == u'e' || m_text[m_pos] == u'E'))
		{
			isFloating = true;
			++m_pos;
			if(m_pos < m_text.size() && (m_text[m_pos] == u'+' || m_text[m_pos] == u'-'))
			{
				++m_pos;
			}
			if(!scanDigits())
			{
				return fail(u"Expected exponent digits in JSON number."_s);
			}
		}

		const QStringView token = m_text.sliced(start, m_pos - start);
		if(!isFloating)
		{
			bool intOk = false;
			const qint64 intVal = token.toLongLong(&intOk);
			if(intOk)
			{
				return OrderedJsonValue(intVal);
			}
		}
		bool dblOk = false;
		const double dblVal = token.toDouble(&dblOk);
		if(!dblOk || !std::isfinite(dblVal))
		{
			return fail(u"JSON number is out of range."_s);
		}
		return OrderedJsonValue(dblVal);
	}

	client::Result<OrderedJsonValue> parseArray(int depth)
	{
		++m_pos; // opening '['
		skipWhitespace();
		OrderedJsonValue::Array items;
		if(consume(u']'))
		{
			return OrderedJsonValue(std::move(items));
		}
		while(true)
		{
			const auto elem = parseValue(depth);
			if(!elem)
			{
				return elem;
			}
			items.append(*elem);
			skipWhitespace();
			if(consume(u','))
			{
				continue;
			}
			if(consume(u']'))
			{
				return OrderedJsonValue(std::move(items));
			}
			return fail(u"Expected ',' or ']' in JSON array."_s);
		}
	}

	client::Result<OrderedJsonValue> parseObject(int depth)
	{
		++m_pos; // opening '{'
		skipWhitespace();
		OrderedJsonObject object;
		if(consume(u'}'))
		{
			return OrderedJsonValue(std::move(object));
		}
		while(true)
		{
			skipWhitespace();
			if(m_pos >= m_text.size() || m_text[m_pos] != u'"')
			{
				return fail(u"Expected string key in JSON object."_s);
			}
			const auto key = parseString();
			if(!key)
			{
				return fail(key.error().message);
			}
			skipWhitespace();
			if(!consume(u':'))
			{
				return fail(u"Expected ':' after key in JSON object."_s);
			}
			const auto val = parseValue(depth);
			if(!val)
			{
				return val;
			}
			object.set(*key, *val);
			skipWhitespace();
			if(consume(u','))
			{
				continue;
			}
			if(consume(u'}'))
			{
				return OrderedJsonValue(std::move(object));
			}
			return fail(u"Expected ',' or '}' in JSON object."_s);
		}
	}

	QStringView m_text;
	qsizetype m_pos = 0;
};

void appendEscapedString(QString& out, QStringView text)
{
	out.append(u'"');
	for(const QChar ch : text)
	{
		switch(ch.unicode())
		{
		case u'"':
			out.append(u"\\\""_s);
			break;
		case u'\\':
			out.append(u"\\\\"_s);
			break;
		case u'\b':
			out.append(u"\\b"_s);
			break;
		case u'\f':
			out.append(u"\\f"_s);
			break;
		case u'\n':
			out.append(u"\\n"_s);
			break;
		case u'\r':
			out.append(u"\\r"_s);
			break;
		case u'\t':
			out.append(u"\\t"_s);
			break;
		default:
			if(ch.unicode() <= MaxJsonControlChar)
			{
				out.append(u"\\u%1"_s.arg(static_cast<uint>(ch.unicode()), 4, 16, QChar(u'0')));
			}
			else
			{
				out.append(ch);
			}
			break;
		}
	}
	out.append(u'"');
}

void appendJsonValue(QString& out, const OrderedJsonValue& value, bool pretty, int depth);

void appendJsonArray(QString& out, const OrderedJsonValue::Array& array, bool pretty, int depth)
{
	if(array.isEmpty())
	{
		out.append(u"[]"_s);
		return;
	}
	out.append(u'[');
	const QString childIndent = pretty ? QString((depth + 1) * IndentSpaces, u' ') : QString();
	for(qsizetype i = 0; i < array.size(); ++i)
	{
		if(i > 0)
		{
			out.append(u',');
		}
		if(pretty)
		{
			out.append(u'\n');
			out.append(childIndent);
		}
		appendJsonValue(out, array[i], pretty, depth + 1);
	}
	if(pretty)
	{
		out.append(u'\n');
		out.append(QString(depth * IndentSpaces, u' '));
	}
	out.append(u']');
}

void appendJsonObject(QString& out, const OrderedJsonObject& object, bool pretty, int depth)
{
	const auto& entries = object.entries();
	if(entries.isEmpty())
	{
		out.append(u"{}"_s);
		return;
	}
	out.append(u'{');
	const QString childIndent = pretty ? QString((depth + 1) * IndentSpaces, u' ') : QString();
	for(qsizetype i = 0; i < entries.size(); ++i)
	{
		if(i > 0)
		{
			out.append(u',');
		}
		if(pretty)
		{
			out.append(u'\n');
			out.append(childIndent);
		}
		appendEscapedString(out, entries[i].key);
		out.append(pretty ? u": "_s : u":"_s);
		appendJsonValue(out, entries[i].value, pretty, depth + 1);
	}
	if(pretty)
	{
		out.append(u'\n');
		out.append(QString(depth * IndentSpaces, u' '));
	}
	out.append(u'}');
}

void appendJsonValue(QString& out, const OrderedJsonValue& value, bool pretty, int depth)
{
	switch(value.type())
	{
	case OrderedJsonValue::Type::Undefined:
	case OrderedJsonValue::Type::Null:
		out.append(u"null"_s);
		return;
	case OrderedJsonValue::Type::Bool:
		out.append(value.toBool() ? u"true"_s : u"false"_s);
		return;
	case OrderedJsonValue::Type::Int:
		out.append(QString::number(value.toInt64()));
		return;
	case OrderedJsonValue::Type::Double:
	{
		const double d = value.toDouble();
		if(!std::isfinite(d))
		{
			out.append(u"null"_s);
			return;
		}
		const double normalized = (d == 0.0) ? 0.0 : d;
		out.append(QString::number(normalized, 'g', QLocale::FloatingPointShortest));
		return;
	}
	case OrderedJsonValue::Type::String:
		appendEscapedString(out, value.toString());
		return;
	case OrderedJsonValue::Type::Array:
		appendJsonArray(out, value.toArray(), pretty, depth);
		return;
	case OrderedJsonValue::Type::Object:
		appendJsonObject(out, value.toObject(), pretty, depth);
		return;
	}
}

bool intEqualsDouble(qint64 intVal, double dblVal)
{
	if(!std::isfinite(dblVal) || std::trunc(dblVal) != dblVal)
	{
		return false;
	}
	if(intVal < -MaxSafeJsInteger || intVal > MaxSafeJsInteger)
	{
		return false;
	}
	if(dblVal < -MaxSafeJsIntegerDouble || dblVal > MaxSafeJsIntegerDouble)
	{
		return false;
	}
	return static_cast<qint64>(dblVal) == intVal;
}

void setInOrderedObject(OrderedJsonObject& object, QSpan<const QString> path, const OrderedJsonValue& value)
{
	const QString& head = path.front();
	if(path.size() == 1)
	{
		object.set(head, value);
		return;
	}
	const OrderedJsonValue existing = object.value(head);
	OrderedJsonObject child = existing.isObject() ? existing.toObject() : OrderedJsonObject{};
	setInOrderedObject(child, path.subspan(1), value);
	object.set(head, OrderedJsonValue(std::move(child)));
}

void removeFromOrderedObject(OrderedJsonObject& object, QSpan<const QString> path, bool pruneEmptyAncestors)
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
	const OrderedJsonValue existing = object.value(head);
	if(!existing.isObject())
	{
		return;
	}
	OrderedJsonObject child = existing.toObject();
	removeFromOrderedObject(child, path.subspan(1), pruneEmptyAncestors);
	if(pruneEmptyAncestors && child.isEmpty())
	{
		object.remove(head);
		return;
	}
	object.set(head, OrderedJsonValue(std::move(child)));
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

void collectEnvFromOrderedValue(const OrderedJsonValue& value, QSet<QString>& found)
{
	if(value.isString())
	{
		collectEnvFromString(value.toString(), found);
		return;
	}
	if(value.isArray())
	{
		const OrderedJsonValue::Array array = value.toArray();
		for(const OrderedJsonValue& item : array)
		{
			collectEnvFromOrderedValue(item, found);
		}
		return;
	}
	if(value.isObject())
	{
		const OrderedJsonObject object = value.toObject();
		for(const OrderedJsonObject::Entry& entry : object.entries())
		{
			collectEnvFromOrderedValue(entry.value, found);
		}
	}
}

} // namespace

// ---- OrderedJsonValue -----------------------------------------------------------------------

OrderedJsonValue::OrderedJsonValue() = default;
OrderedJsonValue::OrderedJsonValue(const OrderedJsonValue& other) = default;
OrderedJsonValue::OrderedJsonValue(OrderedJsonValue&& other) noexcept = default;
OrderedJsonValue& OrderedJsonValue::operator=(const OrderedJsonValue& other) = default;
OrderedJsonValue& OrderedJsonValue::operator=(OrderedJsonValue&& other) noexcept = default;
OrderedJsonValue::~OrderedJsonValue() = default;

OrderedJsonValue OrderedJsonValue::null()
{
	OrderedJsonValue val;
	val.m_type = Type::Null;
	return val;
}

OrderedJsonValue::OrderedJsonValue(bool value)
	: m_type(Type::Bool)
	, m_bool(value)
{
}

OrderedJsonValue::OrderedJsonValue(int value)
	: m_type(Type::Int)
	, m_int(value)
{
}

OrderedJsonValue::OrderedJsonValue(qint64 value)
	: m_type(Type::Int)
	, m_int(value)
{
}

OrderedJsonValue::OrderedJsonValue(double value)
	: m_type(Type::Double)
	, m_double(value)
{
}

OrderedJsonValue::OrderedJsonValue(QString value)
	: m_type(Type::String)
	, m_string(std::move(value))
{
}

OrderedJsonValue::OrderedJsonValue(Array value)
	: m_type(Type::Array)
	, m_array(std::make_shared<const Array>(std::move(value)))
{
}

OrderedJsonValue::OrderedJsonValue(OrderedJsonObject value)
	: m_type(Type::Object)
	, m_object(std::make_shared<const OrderedJsonObject>(std::move(value)))
{
}

OrderedJsonValue OrderedJsonValue::fromJsonValue(const QJsonValue& value)
{
	switch(value.type())
	{
	case QJsonValue::Undefined:
		return {};
	case QJsonValue::Null:
		return null();
	case QJsonValue::Bool:
		return OrderedJsonValue(value.toBool());
	case QJsonValue::Double:
	{
		const double d = value.toDouble();
		if(std::isfinite(d) && std::trunc(d) == d && d >= -MaxSafeJsIntegerDouble && d <= MaxSafeJsIntegerDouble)
		{
			return OrderedJsonValue(static_cast<qint64>(d));
		}
		return OrderedJsonValue(d);
	}
	case QJsonValue::String:
		return OrderedJsonValue(value.toString());
	case QJsonValue::Array:
	{
		const QJsonArray src = value.toArray();
		Array dst;
		dst.reserve(src.size());
		for(const QJsonValue& item : src)
		{
			dst.append(fromJsonValue(item));
		}
		return OrderedJsonValue(std::move(dst));
	}
	case QJsonValue::Object:
		return OrderedJsonValue(OrderedJsonObject::fromJsonObject(value.toObject()));
	}
	return {};
}

QJsonValue OrderedJsonValue::toJsonValue() const
{
	switch(m_type)
	{
	case Type::Undefined:
		return QJsonValue(QJsonValue::Undefined);
	case Type::Null:
		return QJsonValue(QJsonValue::Null);
	case Type::Bool:
		return QJsonValue(m_bool);
	case Type::Int:
		return QJsonValue(m_int);
	case Type::Double:
		return QJsonValue(m_double);
	case Type::String:
		return QJsonValue(m_string);
	case Type::Array:
	{
		QJsonArray out;
		if(m_array)
		{
			for(const OrderedJsonValue& item : *m_array)
			{
				out.append(item.toJsonValue());
			}
		}
		return out;
	}
	case Type::Object:
		return m_object ? QJsonValue(m_object->toJsonObject()) : QJsonValue(QJsonObject{});
	}
	return QJsonValue(QJsonValue::Undefined);
}

QVariant OrderedJsonValue::toVariant() const
{
	switch(m_type)
	{
	case Type::Undefined:
		return {};
	case Type::Null:
		return QVariant::fromValue(nullptr);
	case Type::Bool:
		return QVariant(m_bool);
	case Type::Int:
		return QVariant(m_int);
	case Type::Double:
		return QVariant(m_double);
	case Type::String:
		return QVariant(m_string);
	case Type::Array:
	{
		QVariantList out;
		if(m_array)
		{
			out.reserve(m_array->size());
			for(const OrderedJsonValue& item : *m_array)
			{
				out.append(item.toVariant());
			}
		}
		return out;
	}
	case Type::Object:
		return m_object ? QVariant(m_object->toVariantMap()) : QVariant(QVariantMap{});
	}
	return {};
}

bool OrderedJsonValue::toBool(bool defaultValue) const
{
	return m_type == Type::Bool ? m_bool : defaultValue;
}

int OrderedJsonValue::toInt(int defaultValue) const
{
	if(m_type == Type::Int)
	{
		if(m_int >= std::numeric_limits<int>::min() && m_int <= std::numeric_limits<int>::max())
		{
			return static_cast<int>(m_int);
		}
		return defaultValue;
	}
	if(m_type == Type::Double && std::isfinite(m_double) && std::trunc(m_double) == m_double
	   && m_double >= std::numeric_limits<int>::min() && m_double <= std::numeric_limits<int>::max())
	{
		return static_cast<int>(m_double);
	}
	return defaultValue;
}

qint64 OrderedJsonValue::toInt64(qint64 defaultValue) const
{
	if(m_type == Type::Int)
	{
		return m_int;
	}
	if(m_type == Type::Double && std::isfinite(m_double) && std::trunc(m_double) == m_double
	   && m_double >= -MaxSafeJsIntegerDouble && m_double <= MaxSafeJsIntegerDouble)
	{
		return static_cast<qint64>(m_double);
	}
	return defaultValue;
}

double OrderedJsonValue::toDouble(double defaultValue) const
{
	if(m_type == Type::Double)
	{
		return m_double;
	}
	if(m_type == Type::Int)
	{
		return static_cast<double>(m_int);
	}
	return defaultValue;
}

QString OrderedJsonValue::toString(const QString& defaultValue) const
{
	return m_type == Type::String ? m_string : defaultValue;
}

OrderedJsonValue::Array OrderedJsonValue::toArray() const
{
	return (m_type == Type::Array && m_array) ? *m_array : Array{};
}

OrderedJsonObject OrderedJsonValue::toObject() const
{
	return (m_type == Type::Object && m_object) ? *m_object : OrderedJsonObject{};
}

QString OrderedJsonValue::toCompactJson() const
{
	if(m_type == Type::Undefined)
	{
		return {};
	}
	QString out;
	appendJsonValue(out, *this, false, 0);
	return out;
}

QString OrderedJsonValue::toIndentedJson() const
{
	if(m_type == Type::Undefined)
	{
		return {};
	}
	QString out;
	appendJsonValue(out, *this, true, 0);
	return out;
}

bool OrderedJsonValue::operator==(const OrderedJsonValue& other) const
{
	if(m_type == other.m_type)
	{
		switch(m_type)
		{
		case Type::Undefined:
		case Type::Null:
			return true;
		case Type::Bool:
			return m_bool == other.m_bool;
		case Type::Int:
			return m_int == other.m_int;
		case Type::Double:
			return m_double == other.m_double;
		case Type::String:
			return m_string == other.m_string;
		case Type::Array:
			if(m_array == other.m_array)
			{
				return true;
			}
			return m_array && other.m_array && *m_array == *other.m_array;
		case Type::Object:
			if(m_object == other.m_object)
			{
				return true;
			}
			return m_object && other.m_object && *m_object == *other.m_object;
		}
	}
	if(m_type == Type::Int && other.m_type == Type::Double)
	{
		return intEqualsDouble(m_int, other.m_double);
	}
	if(m_type == Type::Double && other.m_type == Type::Int)
	{
		return intEqualsDouble(other.m_int, m_double);
	}
	return false;
}

// ---- OrderedJsonObject ----------------------------------------------------------------------

OrderedJsonObject OrderedJsonObject::fromJsonObject(const QJsonObject& object)
{
	OrderedJsonObject out;
	for(auto it = object.constBegin(); it != object.constEnd(); ++it)
	{
		out.set(it.key(), OrderedJsonValue::fromJsonValue(it.value()));
	}
	return out;
}

QJsonObject OrderedJsonObject::toJsonObject() const
{
	QJsonObject out;
	for(const Entry& entry : m_entries)
	{
		out.insert(entry.key, entry.value.toJsonValue());
	}
	return out;
}

QVariantMap OrderedJsonObject::toVariantMap() const
{
	QVariantMap out;
	for(const Entry& entry : m_entries)
	{
		out.insert(entry.key, entry.value.toVariant());
	}
	return out;
}

QStringList OrderedJsonObject::keys() const
{
	QStringList out;
	out.reserve(m_entries.size());
	for(const Entry& entry : m_entries)
	{
		out.append(entry.key);
	}
	return out;
}

qsizetype OrderedJsonObject::indexOf(QStringView key) const
{
	for(qsizetype i = 0; i < m_entries.size(); ++i)
	{
		if(m_entries[i].key == key)
		{
			return i;
		}
	}
	return -1;
}

OrderedJsonValue OrderedJsonObject::value(QStringView key) const
{
	const qsizetype idx = indexOf(key);
	return idx >= 0 ? m_entries[idx].value : OrderedJsonValue{};
}

void OrderedJsonObject::set(const QString& key, OrderedJsonValue value)
{
	if(value.isUndefined())
	{
		remove(key);
		return;
	}
	const qsizetype idx = indexOf(key);
	if(idx >= 0)
	{
		m_entries[idx].value = std::move(value);
		return;
	}
	m_entries.append(Entry{key, std::move(value)});
}

bool OrderedJsonObject::remove(QStringView key)
{
	const qsizetype idx = indexOf(key);
	if(idx < 0)
	{
		return false;
	}
	m_entries.removeAt(idx);
	return true;
}

bool OrderedJsonObject::renameKey(QStringView oldKey, const QString& newKey)
{
	if(oldKey.isEmpty() || newKey.isEmpty())
	{
		return false;
	}
	const qsizetype oldIdx = indexOf(oldKey);
	if(oldIdx < 0)
	{
		return false;
	}
	if(oldKey == newKey)
	{
		return true;
	}
	if(contains(newKey))
	{
		return false;
	}
	m_entries[oldIdx].key = newKey;
	return true;
}

// ---- ConfigJsonPatcher ----------------------------------------------------------------------

ConfigJsonPatcher::ConfigJsonPatcher(OrderedJsonObject root)
	: m_root(std::move(root))
{
}

ConfigJsonPatcher::ConfigJsonPatcher(const QJsonObject& root)
	: m_root(OrderedJsonObject::fromJsonObject(root))
{
}

client::Result<OrderedJsonValue> ConfigJsonPatcher::parseJsoncValue(QStringView text)
{
	const client::Result<QString> uncommented = stripJsoncComments(text);
	if(!uncommented)
	{
		return std::unexpected(uncommented.error());
	}
	const QString normalized = stripTrailingCommas(*uncommented);
	JsonParser parser(normalized);
	return parser.parseDocument();
}

client::Result<ConfigJsonPatcher> ConfigJsonPatcher::parseJsonc(QStringView text)
{
	const auto parsed = parseJsoncValue(text);
	if(!parsed)
	{
		return std::unexpected(parsed.error());
	}
	if(!parsed->isObject())
	{
		return std::unexpected(client::Failure{u"Configuration root must be a JSON object."_s});
	}
	return ConfigJsonPatcher(parsed->toObject());
}

QString ConfigJsonPatcher::toJson() const
{
	if(m_root.isEmpty())
	{
		return u"{}\n"_s;
	}
	QString out;
	appendJsonObject(out, m_root, true, 0);
	out.append(u'\n');
	return out;
}

OrderedJsonValue ConfigJsonPatcher::orderedValueAt(const QStringList& path) const
{
	if(path.isEmpty())
	{
		return {};
	}
	OrderedJsonValue current(m_root);
	for(const QString& key : path)
	{
		if(!current.isObject())
		{
			return {};
		}
		current = current.toObject().value(key);
		if(current.isUndefined())
		{
			return {};
		}
	}
	return current;
}

QJsonValue ConfigJsonPatcher::valueAt(const QStringList& path) const
{
	return orderedValueAt(path).toJsonValue();
}

bool ConfigJsonPatcher::contains(const QStringList& path) const
{
	return !orderedValueAt(path).isUndefined();
}

void ConfigJsonPatcher::setOrderedValueAt(const QStringList& path, const OrderedJsonValue& value)
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
	setInOrderedObject(m_root, path, value);
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
	setOrderedValueAt(path, OrderedJsonValue::fromJsonValue(value));
}

void ConfigJsonPatcher::removeAt(const QStringList& path, bool pruneEmptyAncestors)
{
	if(path.isEmpty())
	{
		return;
	}
	removeFromOrderedObject(m_root, path, pruneEmptyAncestors);
}

bool ConfigJsonPatcher::renameKeyAt(const QStringList& parentPath, const QString& oldKey, const QString& newKey)
{
	if(oldKey.isEmpty() || newKey.isEmpty())
	{
		return false;
	}
	if(parentPath.isEmpty())
	{
		return m_root.renameKey(oldKey, newKey);
	}
	const OrderedJsonValue target = orderedValueAt(parentPath);
	if(!target.isObject())
	{
		return false;
	}
	OrderedJsonObject parentObject = target.toObject();
	if(!parentObject.renameKey(oldKey, newKey))
	{
		return false;
	}
	setOrderedValueAt(parentPath, OrderedJsonValue(std::move(parentObject)));
	return true;
}

QStringList ConfigJsonPatcher::envReferencesIn(const OrderedJsonValue& value)
{
	QSet<QString> found;
	collectEnvFromOrderedValue(value, found);
	QStringList sorted = found.values();
	std::ranges::sort(sorted);
	return sorted;
}

QStringList ConfigJsonPatcher::envReferencesIn(const QJsonValue& value)
{
	return envReferencesIn(OrderedJsonValue::fromJsonValue(value));
}

QStringList ConfigJsonPatcher::envReferences() const
{
	return envReferencesIn(OrderedJsonValue(m_root));
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
