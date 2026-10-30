// Adapted from an early version of qutie by LightBlueCube
#include "session/fence_splitter.h"

#include <algorithm>
#include <utility>

namespace refgui::session
{
namespace
{

constexpr qsizetype MaxFenceIndent = 3;
constexpr qsizetype MinFenceLength = 3;

qsizetype countLeading(QStringView text, QChar c)
{
	const auto end = std::ranges::find_if(text, [c](QChar x) { return x != c; });
	return end - text.begin();
}

bool isFenceMarker(QChar c)
{
	return c == u'`' || c == u'~';
}

// Also accepts '\r', so a fence line ending in CRLF still closes the block.
bool isBlank(QStringView text)
{
	return std::ranges::all_of(text, [](QChar c) { return c.isSpace(); });
}

} // namespace

QList<FenceSplitter::Event> FenceSplitter::feed(QStringView delta)
{
	QList<Event> events;
	qsizetype pos = 0;

	while(true)
	{
		const qsizetype lineBreak = delta.indexOf(u'\n', pos);
		if(lineBreak < 0)
		{
			appendToLine(delta.sliced(pos), events);
			return events;
		}

		appendToLine(delta.sliced(pos, lineBreak - pos), events);
		endLine(events);
		pos = lineBreak + 1;
	}
}

QList<FenceSplitter::Event> FenceSplitter::finish()
{
	QList<Event> events;
	endLine(events);
	return events;
}

std::optional<FenceSplitter::Fence> FenceSplitter::parseOpeningFence(QStringView line)
{
	const qsizetype indent = countLeading(line, u' ');
	if(indent > MaxFenceIndent || indent == line.size())
	{
		return std::nullopt;
	}

	const QChar marker = line[indent];
	const qsizetype length = isFenceMarker(marker) ? countLeading(line.sliced(indent), marker) : 0;
	if(length < MinFenceLength)
	{
		return std::nullopt;
	}

	// Without this rule "```a```" at the start of a line, which is inline code, would open a block.
	const QStringView info = line.sliced(indent + length).trimmed();
	if(marker == u'`' && info.contains(u'`'))
	{
		return std::nullopt;
	}

	const auto languageEnd = std::ranges::find_if(info, [](QChar c) { return c.isSpace(); });
	const QString language = info.first(languageEnd - info.begin()).toString();
	return Fence{marker, length, indent, language};
}

bool FenceSplitter::closes(const Fence& fence, QStringView line)
{
	const qsizetype indent = countLeading(line, u' ');
	if(indent > MaxFenceIndent)
	{
		return false;
	}

	const QStringView rest = line.sliced(indent);
	const qsizetype length = countLeading(rest, fence.marker);
	return length >= fence.length && isBlank(rest.sliced(length));
}

void FenceSplitter::appendToLine(QStringView text, QList<Event>& events)
{
	if(m_lineReleased)
	{
		emitText(text, events);
		return;
	}

	m_heldLine.append(text);
	if(mayBecomeFence(m_heldLine))
	{
		return;
	}

	m_lineReleased = true;
	emitText(stripCodeIndent(m_heldLine), events);
	m_heldLine.clear();
}

void FenceSplitter::endLine(QList<Event>& events)
{
	const bool lineWasHeld = !std::exchange(m_lineReleased, false);
	const QString line = std::exchange(m_heldLine, {});

	if(lineWasHeld && m_openFence && closes(*m_openFence, line))
	{
		m_openFence.reset();
		m_pendingLineBreaks = 0;
		m_atBlockStart = true;
		events.append(CodeClosed{});
		return;
	}

	if(lineWasHeld && !m_openFence)
	{
		m_openFence = parseOpeningFence(line);
		if(m_openFence)
		{
			m_pendingLineBreaks = 0;
			m_atBlockStart = true;
			events.append(CodeOpened{m_openFence->language});
			return;
		}
	}

	if(lineWasHeld)
	{
		emitText(stripCodeIndent(line), events);
	}
	++m_pendingLineBreaks;
}

// `partialLine` has not ended yet, so this asks whether some continuation could make it a fence.
bool FenceSplitter::mayBecomeFence(QStringView partialLine) const
{
	const qsizetype indent = countLeading(partialLine, u' ');
	if(indent > MaxFenceIndent)
	{
		return false;
	}

	const QStringView rest = partialLine.sliced(indent);
	if(rest.isEmpty())
	{
		return true;
	}

	if(m_openFence)
	{
		const qsizetype length = countLeading(rest, m_openFence->marker);
		return length > 0 && isBlank(rest.sliced(length));
	}

	const QChar marker = rest.front();
	if(!isFenceMarker(marker))
	{
		return false;
	}

	// A complete opening run is held until the line ends: the info string decides the rest.
	const qsizetype length = countLeading(rest, marker);
	return length >= MinFenceLength || length == rest.size();
}

// Inside a code block, removes up to as many leading spaces as the opening fence was indented.
QStringView FenceSplitter::stripCodeIndent(QStringView line) const
{
	if(!m_openFence)
	{
		return line;
	}
	return line.sliced(std::min(countLeading(line, u' '), m_openFence->indent));
}

void FenceSplitter::emitText(QStringView text, QList<Event>& events)
{
	if(text.isEmpty())
	{
		return;
	}

	// Blank lines before the first prose line separate it from a fence; leading blank
	// lines inside a code block are content.
	const bool startsProse = std::exchange(m_atBlockStart, false) && !m_openFence;
	const qsizetype lineBreaks = std::exchange(m_pendingLineBreaks, 0);
	QString released(startsProse ? 0 : lineBreaks, u'\n');
	released.append(text);

	TextAppended* previous = events.isEmpty() ? nullptr : std::get_if<TextAppended>(&events.back());
	if(previous)
	{
		previous->text.append(released);
		return;
	}
	events.append(TextAppended{std::move(released)});
}

} // namespace refgui::session
