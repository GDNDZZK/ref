#pragma once

#include <QByteArray>
#include <QByteArrayView>
#include <QList>

namespace refgui::sse
{

struct Event
{
	QByteArray type; // "message" unless the stream set an `event:` field
	QByteArray data; // UTF-8; consecutive `data:` lines are joined with '\n'
};

// Incremental parser for `text/event-stream` (WHATWG HTML, "Interpreting an event stream").
//
// Operates on raw bytes: every delimiter is ASCII, so a UTF-8 sequence split across
// chunks is reassembled inside the line buffer before anyone decodes it.
// `id` and `retry` fields are ignored; they only drive EventSource reconnection.
class Parser
{
public:
	// Consumes the next chunk of the stream and returns the events it completed.
	QList<Event> feed(QByteArrayView chunk);

private:
	void processLine(QByteArrayView line, QList<Event>& completed);
	void dispatch(QList<Event>& completed);

	QByteArray m_line; // bytes of the current, not yet terminated line
	QByteArray m_eventType;
	QByteArray m_data;
	bool m_previousLineEndedWithCr = false; // a '\n' right after it completes a CRLF, not a new line
	bool m_isFirstLine = true;              // the only place a UTF-8 BOM may appear
};

} // namespace refgui::sse
