#include "infra/sse.h"

#include <algorithm>
#include <utility>

namespace refgui::sse
{
namespace
{

constexpr QByteArrayView Utf8Bom = "\xEF\xBB\xBF";
constexpr QByteArrayView DefaultEventType = "message";
constexpr QByteArrayView EventField = "event";
constexpr QByteArrayView DataField = "data";

bool isLineBreak(char c)
{
	return c == '\r' || c == '\n';
}

} // namespace

// Lines end with CRLF, LF or CR. A CRLF may be split across two chunks:
//
//   chunk 1: "data: a\r"     -> line "data: a", remember the CR
//   chunk 2: "\ndata: b\n"   -> skip the '\n', then line "data: b"
QList<Event> Parser::feed(QByteArrayView chunk)
{
	QList<Event> completed;
	qsizetype pos = 0;

	while(pos < chunk.size())
	{
		const bool completesCrLf = m_previousLineEndedWithCr && chunk[pos] == '\n';
		m_previousLineEndedWithCr = false;
		if(completesCrLf)
		{
			++pos;
			continue;
		}

		const char* lineBreak = std::find_if(chunk.begin() + pos, chunk.end(), isLineBreak);
		if(lineBreak == chunk.end())
		{
			m_line.append(chunk.sliced(pos));
			break;
		}

		const qsizetype lineEnd = lineBreak - chunk.begin();
		m_line.append(chunk.sliced(pos, lineEnd - pos));
		m_previousLineEndedWithCr = *lineBreak == '\r';
		pos = lineEnd + 1;

		processLine(m_line, completed);
		m_line.clear();
	}
	return completed;
}

void Parser::processLine(QByteArrayView line, QList<Event>& completed)
{
	if(m_isFirstLine)
	{
		m_isFirstLine = false;
		if(line.startsWith(Utf8Bom))
		{
			line = line.sliced(Utf8Bom.size());
		}
	}

	if(line.isEmpty())
	{
		dispatch(completed);
		return;
	}
	if(line.startsWith(':'))
	{
		return; // comment, typically a keep-alive ping
	}

	// "field:value", "field: value" (one leading space dropped) or bare "field" (empty value).
	const qsizetype colon = line.indexOf(':');
	const QByteArrayView field = colon < 0 ? line : line.first(colon);
	QByteArrayView value = colon < 0 ? QByteArrayView() : line.sliced(colon + 1);
	if(value.startsWith(' '))
	{
		value = value.sliced(1);
	}

	if(field == EventField)
	{
		m_eventType = value.toByteArray();
	}
	else if(field == DataField)
	{
		m_data.append(value);
		m_data.append('\n');
	}
}

void Parser::dispatch(QList<Event>& completed)
{
	QByteArray type = std::exchange(m_eventType, {});
	QByteArray data = std::exchange(m_data, {});

	// A block without any `data:` line (only comments or `event:`) produces no event.
	if(data.isEmpty())
	{
		return;
	}

	data.chop(1); // the '\n' appended after the last `data:` line
	if(type.isEmpty())
	{
		type = DefaultEventType.toByteArray();
	}
	completed.append(Event{std::move(type), std::move(data)});
}

} // namespace refgui::sse
