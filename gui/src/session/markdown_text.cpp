#include "session/markdown_text.h"

#include <QList>
#include <QStringView>

#include <algorithm>
#include <optional>

using namespace Qt::StringLiterals;

namespace refgui::session
{

namespace
{

// The length of the run of `character` that starts at `from`.
qsizetype runLength(const QString& text, qsizetype from, QChar character)
{
	qsizetype end = from;
	while(end < text.size() && text.at(end) == character)
	{
		++end;
	}
	return end - from;
}

// Where the code span opened by the `fenceLength` backticks at `from` ends (after its closing
// run), or -1 when it is never closed.
qsizetype codeSpanEnd(const QString& text, qsizetype from, qsizetype fenceLength)
{
	qsizetype position = from + fenceLength;
	while(position < text.size())
	{
		if(text.at(position) != u'`')
		{
			++position;
			continue;
		}
		const qsizetype run = runLength(text, position, u'`');
		if(run == fenceLength)
		{
			return position + run;
		}
		position += run;
	}
	return -1;
}

bool opensTagOrComment(QChar next)
{
	return next.isLetter() || next == u'/' || next == u'!' || next == u'?';
}

// Escapes tags and turns images into links in running text; code spans stay as they are.
QString displayInline(const QString& markdown)
{
	QString out;
	out.reserve(markdown.size());
	qsizetype i = 0;
	while(i < markdown.size())
	{
		const QChar current = markdown.at(i);
		const QChar next = i + 1 < markdown.size() ? markdown.at(i + 1) : QChar();

		if(current == u'\\' && !next.isNull())
		{
			out += markdown.mid(i, 2);
			i += 2;
		}
		else if(current == u'`')
		{
			const qsizetype fenceLength = runLength(markdown, i, u'`');
			const qsizetype end = codeSpanEnd(markdown, i, fenceLength);
			const qsizetype length = end < 0 ? fenceLength : end - i;
			out += markdown.mid(i, length);
			i += length;
		}
		else if(current == u'!' && next == u'[')
		{
			++i; // the image becomes a link
		}
		else if(current == u'<' && opensTagOrComment(next))
		{
			out += u"\\<"_s;
			++i;
		}
		else
		{
			out += current;
			++i;
		}
	}
	return out;
}

constexpr qsizetype TabWidth = 4;
constexpr qsizetype IndentedCodeIndent = 4; // columns past the enclosing list item
constexpr qsizetype MaxFenceIndent = 3;
constexpr qsizetype MinFenceLength = 3;

// A code fence that is open: ``` or ~~~, at least `length` long.
struct Fence
{
	QChar marker;
	qsizetype length;
};

// Columns of leading white space.
qsizetype indentOf(QStringView line)
{
	qsizetype columns = 0;
	for(const QChar character : line)
	{
		if(character == u' ')
		{
			++columns;
		}
		else if(character == u'\t')
		{
			columns += TabWidth - columns % TabWidth;
		}
		else
		{
			break;
		}
	}
	return columns;
}

std::optional<Fence> openingFence(QStringView line)
{
	const QStringView content = line.trimmed();
	if(content.isEmpty() || (content.front() != u'`' && content.front() != u'~'))
	{
		return std::nullopt;
	}
	const QChar marker = content.front();
	qsizetype length = 0;
	while(length < content.size() && content.at(length) == marker)
	{
		++length;
	}
	// The info string of a backtick fence has no backtick; otherwise this is a code span.
	if(length < MinFenceLength || (marker == u'`' && content.mid(length).contains(u'`')))
	{
		return std::nullopt;
	}
	return Fence{marker, length};
}

bool closesFence(QStringView line, const Fence& fence)
{
	const QStringView content = line.trimmed();
	qsizetype length = 0;
	while(length < content.size() && content.at(length) == fence.marker)
	{
		++length;
	}
	return length >= fence.length && length == content.size();
}

// The column at which the content of the list item that starts `line` begins, or -1 when it
// starts none.
qsizetype listContentColumn(QStringView line)
{
	const qsizetype indent = indentOf(line);
	const QStringView content = line.trimmed();
	qsizetype markerLength = 0;
	if(!content.isEmpty() && (content.front() == u'-' || content.front() == u'*' || content.front() == u'+'))
	{
		markerLength = 1;
	}
	else
	{
		while(markerLength < content.size() && content.at(markerLength).isDigit())
		{
			++markerLength;
		}
		const bool numbered = markerLength > 0 && markerLength < content.size() && (content.at(markerLength) == u'.' || content.at(markerLength) == u')');
		markerLength = numbered ? markerLength + 1 : 0;
	}
	if(markerLength == 0)
	{
		return -1;
	}
	const bool spaced = markerLength == content.size() || content.at(markerLength) == u' ' || content.at(markerLength) == u'\t';
	return spaced ? indent + markerLength + 1 : -1;
}

// A stretch of the text that is either code, which stays as it is, or running text.
struct Segment
{
	qsizetype start;
	qsizetype end;
	bool code;
};

void addSegment(QList<Segment>& segments, qsizetype start, qsizetype end, bool code)
{
	if(!segments.isEmpty() && segments.last().code == code)
	{
		segments.last().end = end;
		return;
	}
	segments.append({start, end, code});
}

// Splits the text into fenced and indented code and everything else, line by line. An unclosed
// fence runs to the end, as it does while the model is still writing it.
QList<Segment> splitCode(const QString& markdown)
{
	QList<Segment> segments;
	std::optional<Fence> fence;
	qsizetype listColumn = -1; // content column of the list item the text is in
	bool afterBlank = true;
	bool afterCode = false; // the last non-blank line was indented code

	qsizetype start = 0;
	while(start < markdown.size())
	{
		qsizetype end = markdown.indexOf(u'\n', start);
		end = end < 0 ? markdown.size() : end + 1;
		const QStringView line = QStringView(markdown).mid(start, end - start);

		if(fence)
		{
			addSegment(segments, start, end, true);
			if(closesFence(line, *fence))
			{
				fence.reset();
			}
		}
		else if(line.trimmed().isEmpty())
		{
			addSegment(segments, start, end, false);
			afterBlank = true;
		}
		else
		{
			const qsizetype indent = indentOf(line);
			if(indent < listColumn)
			{
				listColumn = -1;
			}
			const qsizetype relative = indent - std::max<qsizetype>(listColumn, 0);
			if(relative >= IndentedCodeIndent && (afterBlank || afterCode))
			{
				addSegment(segments, start, end, true);
				afterCode = true;
			}
			else if(relative <= MaxFenceIndent && openingFence(line))
			{
				fence = openingFence(line);
				addSegment(segments, start, end, true);
				afterCode = false;
			}
			else
			{
				addSegment(segments, start, end, false);
				afterCode = false;
				listColumn = std::max(listContentColumn(line), listColumn);
			}
			afterBlank = false;
		}
		start = end;
	}
	return segments;
}

} // namespace

QString markdownForDisplay(const QString& markdown)
{
	QString out;
	out.reserve(markdown.size());
	for(const Segment& segment : splitCode(markdown))
	{
		const QString part = markdown.mid(segment.start, segment.end - segment.start);
		out += segment.code ? part : displayInline(part);
	}
	return out;
}

} // namespace refgui::session
