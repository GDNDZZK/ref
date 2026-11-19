#include "session/table_splitter.h"

#include <algorithm>
#include <utility>

using namespace Qt::StringLiterals;

namespace refgui::session
{
namespace
{

constexpr qsizetype MaxTableIndent = 3;

qsizetype countLeading(QStringView text, QChar ch)
{
	const auto end = std::ranges::find_if(text, [ch](QChar c) { return c != ch; });
	return end - text.begin();
}

QStringView stripTrailingCr(QStringView line)
{
	return line.endsWith(u'\r') ? line.chopped(1) : line;
}

bool isBlockInterrupt(QStringView line)
{
	const qsizetype indent = countLeading(line, u' ');
	if(indent > MaxTableIndent)
	{
		return true;
	}
	const QStringView rest = line.sliced(indent);
	if(rest.isEmpty())
	{
		return false;
	}
	if(rest.front() == u'>')
	{
		return true;
	}
	if(rest.front() == u'#')
	{
		const qsizetype hashes = countLeading(rest, u'#');
		return hashes <= 6 && (hashes == rest.size() || rest[hashes].isSpace());
	}
	if(rest.front() == u'-' || rest.front() == u'*' || rest.front() == u'+')
	{
		return rest.size() > 1 && rest[1].isSpace();
	}
	qsizetype digits = 0;
	while(digits < rest.size() && rest[digits].isDigit())
	{
		++digits;
	}
	if(digits > 0 && digits + 1 < rest.size())
	{
		const QChar delim = rest[digits];
		return (delim == u'.' || delim == u')') && rest[digits + 1].isSpace();
	}
	return false;
}

bool mayBeTableHeaderPrefix(QStringView partialLine)
{
	const qsizetype indent = countLeading(partialLine, u' ');
	if(indent > MaxTableIndent)
	{
		return false;
	}
	const QStringView rest = partialLine.sliced(indent);
	return rest.isEmpty() || rest.front() == u'|';
}

bool mayBeSeparatorPrefix(QStringView partialLine)
{
	const QStringView clean = stripTrailingCr(partialLine);
	return std::ranges::all_of(clean, [](QChar c) {
		return c == u' ' || c == u'\t' || c == u'|' || c == u'-' || c == u':';
	});
}

QString unescapeCell(QStringView cell)
{
	QString out = cell.trimmed().toString();
	out.replace(u"\\|"_s, u"|"_s);
	return out;
}

QStringList normalizeRowCells(QStringList cells, qsizetype columnCount)
{
	if(cells.size() > columnCount)
	{
		cells.resize(columnCount);
		return cells;
	}
	while(cells.size() < columnCount)
	{
		cells.append(QString());
	}
	return cells;
}

} // namespace

std::optional<QStringList> TableSplitter::splitTableRow(QStringView line)
{
	const QStringView trimmed = line.trimmed();
	if(trimmed.isEmpty())
	{
		return std::nullopt;
	}

	QList<qsizetype> pipes;
	qsizetype backtickRun = 0;
	qsizetype i = 0;
	while(i < trimmed.size())
	{
		const QChar ch = trimmed[i];
		if(ch == u'\\' && i + 1 < trimmed.size())
		{
			i += 2;
			continue;
		}
		if(ch == u'`')
		{
			const qsizetype run = countLeading(trimmed.sliced(i), u'`');
			if(backtickRun == 0)
			{
				backtickRun = run;
			}
			else if(run == backtickRun)
			{
				backtickRun = 0;
			}
			i += run;
			continue;
		}
		if(ch == u'|' && backtickRun == 0)
		{
			pipes.append(i);
		}
		++i;
	}

	if(pipes.isEmpty())
	{
		return std::nullopt;
	}

	QStringList cells;
	qsizetype start = 0;
	for(const qsizetype pipePos : pipes)
	{
		cells.append(unescapeCell(trimmed.sliced(start, pipePos - start)));
		start = pipePos + 1;
	}
	cells.append(unescapeCell(trimmed.sliced(start)));

	if(pipes.first() == 0)
	{
		cells.removeFirst();
	}
	if(!cells.isEmpty() && pipes.size() > 1 && pipes.last() == trimmed.size() - 1)
	{
		cells.removeLast();
	}
	if(pipes.size() == 1 && pipes.first() == trimmed.size() - 1 && trimmed.size() > 1)
	{
		cells.removeLast();
	}
	if(cells.isEmpty())
	{
		return std::nullopt;
	}
	return cells;
}

std::optional<QStringList> TableSplitter::parseSeparatorRow(QStringView line, qsizetype expectedCols)
{
	const std::optional<QStringList> cells = splitTableRow(line);
	if(!cells || cells->size() != expectedCols)
	{
		return std::nullopt;
	}

	QStringList alignments;
	alignments.reserve(expectedCols);
	for(const QString& rawCell : *cells)
	{
		QStringView cell = QStringView(rawCell).trimmed();
		if(cell.isEmpty())
		{
			return std::nullopt;
		}
		const bool leftColon = cell.startsWith(u':');
		const bool rightColon = cell.endsWith(u':');
		if(leftColon)
		{
			cell = cell.sliced(1);
		}
		if(rightColon && !cell.isEmpty())
		{
			cell = cell.chopped(1);
		}
		if(cell.isEmpty() || !std::ranges::all_of(cell, [](QChar c) { return c == u'-'; }))
		{
			return std::nullopt;
		}
		if(leftColon && rightColon)
		{
			alignments.append(u"center"_s);
		}
		else if(rightColon)
		{
			alignments.append(u"right"_s);
		}
		else
		{
			alignments.append(u"left"_s);
		}
	}
	return alignments;
}

QList<TableSplitter::Event> TableSplitter::feed(QStringView delta)
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

QList<TableSplitter::Event> TableSplitter::finish()
{
	QList<Event> events;
	if(m_state == State::Prose)
	{
		if(!m_heldLine.isEmpty())
		{
			emitText(m_heldLine, events);
			m_heldLine.clear();
		}
		m_proseLine.clear();
		m_lineReleased = false;
		m_emittedLineChars = 0;
		m_pendingLineBreaks = 0;
		m_atBlockStart = true;
		return events;
	}

	if(m_state == State::CandidateHeader)
	{
		const QString pending = std::exchange(m_heldLine, {});
		const QStringView clean = stripTrailingCr(pending);
		const std::optional<QStringList> alignments = parseSeparatorRow(clean, m_headerCells.size());
		if(alignments)
		{
			m_pendingLineBreaks = 0;
			m_atBlockStart = true;
			m_tableCols = m_headerCells.size();
			events.append(TableStarted{.headers = std::move(m_headerCells),
			                           .alignments = *alignments,
			                           .rawMarkdown = m_headerRaw + u'\n' + clean.toString(),
			                           .replaceChars = m_headerReplaceChars});
			closeTable(events);
			return events;
		}
		rejectCandidateHeader(events);
		if(!pending.isEmpty())
		{
			emitText(pending, events);
		}
		m_proseLine.clear();
		m_lineReleased = false;
		m_emittedLineChars = 0;
		m_pendingLineBreaks = 0;
		m_atBlockStart = true;
		return events;
	}
	// State::InTable
	const QString pending = std::exchange(m_heldLine, {});
	const QStringView clean = stripTrailingCr(pending);
	if(!clean.trimmed().isEmpty() && !isBlockInterrupt(clean))
	{
		if(const std::optional<QStringList> cells = splitTableRow(clean))
		{
			events.append(TableRowAppended{.cells = normalizeRowCells(*cells, m_tableCols), .rawLine = clean.toString()});
			closeTable(events);
			return events;
		}
	}
	closeTable(events);
	if(!clean.trimmed().isEmpty())
	{
		emitText(pending, events);
	}
	return events;
}

void TableSplitter::appendToLine(QStringView text, QList<Event>& events)
{
	if(m_state == State::InTable)
	{
		m_heldLine.append(text);
		return;
	}
	if(m_state == State::CandidateHeader)
	{
		m_heldLine.append(text);
		if(mayBeSeparatorPrefix(m_heldLine))
		{
			return;
		}
		const QString retry = std::exchange(m_heldLine, {});
		rejectCandidateHeader(events);
		appendToLine(retry, events);
		return;
	}

	if(m_lineReleased)
	{
		m_proseLine.append(text);
		m_emittedLineChars += text.size();
		emitText(text, events);
		return;
	}

	m_heldLine.append(text);
	if(mayBeTableHeaderPrefix(m_heldLine))
	{
		return;
	}

	m_lineReleased = true;
	m_proseLine = std::exchange(m_heldLine, {});
	m_emittedLineChars = m_proseLine.size();
	emitText(m_proseLine, events);
}

void TableSplitter::endLine(QList<Event>& events)
{
	if(m_state == State::InTable)
	{
		const QString line = std::exchange(m_heldLine, {});
		const QStringView clean = stripTrailingCr(line);
		if(!clean.trimmed().isEmpty() && !isBlockInterrupt(clean))
		{
			if(const std::optional<QStringList> cells = splitTableRow(clean))
			{
				events.append(TableRowAppended{.cells = normalizeRowCells(*cells, m_tableCols), .rawLine = clean.toString()});
				return;
			}
		}
		closeTable(events);
		if(!clean.trimmed().isEmpty())
		{
			appendToLine(line, events);
			endLine(events);
		}
		return;
	}

	if(m_state == State::CandidateHeader)
	{
		const QString line = std::exchange(m_heldLine, {});
		const QStringView clean = stripTrailingCr(line);
		const std::optional<QStringList> alignments = parseSeparatorRow(clean, m_headerCells.size());
		if(alignments)
		{
			m_pendingLineBreaks = 0;
			m_atBlockStart = true;
			m_state = State::InTable;
			m_tableCols = m_headerCells.size();
			events.append(TableStarted{.headers = std::move(m_headerCells),
			                           .alignments = *alignments,
			                           .rawMarkdown = m_headerRaw + u'\n' + clean.toString(),
			                           .replaceChars = m_headerReplaceChars});
			m_headerRaw.clear();
			m_headerFromProse = false;
			m_headerReplaceChars = 0;
			return;
		}
		rejectCandidateHeader(events);
		appendToLine(line, events);
		endLine(events);
		return;
	}

	// State::Prose
	const bool hadEmitted = std::exchange(m_lineReleased, false);
	const QString line = hadEmitted ? std::exchange(m_proseLine, {}) : std::exchange(m_heldLine, {});
	m_proseLine.clear();
	const qsizetype emittedChars = std::exchange(m_emittedLineChars, 0);
	const QStringView clean = stripTrailingCr(line);

	if(clean.trimmed().isEmpty())
	{
		++m_pendingLineBreaks;
		return;
	}

	if(!isBlockInterrupt(clean))
	{
		if(const std::optional<QStringList> cells = splitTableRow(clean))
		{
			m_state = State::CandidateHeader;
			m_headerRaw = clean.toString();
			m_headerCells = *cells;
			m_headerFromProse = hadEmitted;
			m_headerReplaceChars = hadEmitted ? emittedChars : 0;
			return;
		}
	}

	appendProseLine(hadEmitted ? QStringView() : QStringView(line), events);
}

void TableSplitter::rejectCandidateHeader(QList<Event>& events)
{
	m_state = State::Prose;
	if(!m_headerFromProse)
	{
		emitText(m_headerRaw, events);
	}
	else
	{
		m_atBlockStart = false;
	}
	++m_pendingLineBreaks;
	m_headerRaw.clear();
	m_headerCells.clear();
	m_headerFromProse = false;
	m_headerReplaceChars = 0;
}

void TableSplitter::closeTable(QList<Event>& events)
{
	events.append(TableClosed{});
	m_state = State::Prose;
	m_atBlockStart = true;
	m_pendingLineBreaks = 0;
	m_tableCols = 0;
	m_headerRaw.clear();
	m_headerCells.clear();
	m_headerFromProse = false;
	m_headerReplaceChars = 0;
}

void TableSplitter::appendProseLine(QStringView line, QList<Event>& events)
{
	if(!line.isEmpty())
	{
		emitText(line, events);
	}
	++m_pendingLineBreaks;
}

void TableSplitter::emitText(QStringView text, QList<Event>& events)
{
	if(text.isEmpty())
	{
		return;
	}
	const bool startsProse = std::exchange(m_atBlockStart, false);
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
