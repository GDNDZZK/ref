#include "session/markdown_text.h"

#include <QCoreApplication>
#include <QGuiApplication>
#include <QList>
#include <QStringView>
#include <QTextBlock>
#include <QTextDocument>
#include <QTextFragment>

#include <algorithm>
#include <optional>

using namespace Qt::StringLiterals;

namespace refgui::session
{

namespace
{

// The length of the run of `character` that starts at `from`.
qsizetype runLength(QStringView text, qsizetype from, QChar character)
{
	qsizetype end = from;
	while(end < text.size() && text.at(end) == character)
	{
		++end;
	}
	return end - from;
}

// Where the code span opened by the `fenceLength` backticks at `from` ends (after its closing
// run), or -1 when no run of the same length follows on its line (`sameLineOnly`) or at all. A
// span can run over a line break, but not over a paragraph or other block boundary, and telling
// those apart is the renderer's business: a closing run past the line could be in another
// paragraph, and what looks like code there would let an image through.
qsizetype codeSpanEnd(const QString& text, qsizetype from, qsizetype fenceLength, bool sameLineOnly)
{
	qsizetype position = from + fenceLength;
	while(position < text.size() && !(sameLineOnly && text.at(position) == u'\n'))
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

// Copies the run of backticks at `from` to `out`: with its code span when the span closes on
// this line, else escaped where a later run could pair with it. Returns where the text goes on.
qsizetype escapeBackticks(const QString& markdown, qsizetype from, QString& out)
{
	const qsizetype fenceLength = runLength(markdown, from, u'`');
	const qsizetype end = codeSpanEnd(markdown, from, fenceLength, true);
	if(end >= 0)
	{
		out += markdown.mid(from, end - from);
		return end;
	}
	// No span on this line. The renderer might still pair these with a run further down,
	// so they are escaped unless nothing is there to pair with.
	const bool canPair = codeSpanEnd(markdown, from, fenceLength, false) >= 0;
	for(qsizetype tick = 0; tick < fenceLength; ++tick)
	{
		out += canPair ? u"\\`"_s : u"`"_s;
	}
	return from + fenceLength;
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
			continue;
		}
		if(current == u'`')
		{
			i = escapeBackticks(markdown, i, out);
			continue;
		}
		if(current == u'!' && next == u'[')
		{
			// The image becomes a link. After another "!", dropping this one would make "![" again.
			const bool afterBang = out.endsWith(u'!');
			out += afterBang ? u"\\["_s : u"["_s;
			i += 2;
			continue;
		}
		if(current == u'<' && opensTagOrComment(next))
		{
			out += u"\\<"_s;
			++i;
			continue;
		}
		out += current;
		++i;
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
	const bool isFenceChar = !content.isEmpty() && (content.front() == u'`' || content.front() == u'~');
	if(!isFenceChar)
	{
		return std::nullopt;
	}
	const QChar marker = content.front();
	const qsizetype length = runLength(content, 0, marker);
	// The info string of a backtick fence has no backtick; otherwise this is a code span.
	const bool backtickInInfo = marker == u'`' && content.mid(length).contains(u'`');
	if(length < MinFenceLength || backtickInInfo)
	{
		return std::nullopt;
	}
	return Fence{marker, length};
}

bool closesFence(QStringView line, const Fence& fence)
{
	const QStringView content = line.trimmed();
	const qsizetype length = runLength(content, 0, fence.marker);
	return length >= fence.length && length == content.size();
}

// The length of the bullet (-, * or +) that starts `content`, or 0.
qsizetype bulletMarkerLength(QStringView content)
{
	const bool isBulletMarker = !content.isEmpty() && (content.front() == u'-' || content.front() == u'*' || content.front() == u'+');
	return isBulletMarker ? 1 : 0;
}

// The length of the number and its delimiter (`1.` or `1)`) that start `content`, or 0.
qsizetype numberedMarkerLength(QStringView content)
{
	qsizetype digits = 0;
	while(digits < content.size() && content.at(digits).isDigit())
	{
		++digits;
	}
	if(digits == 0 || digits == content.size())
	{
		return 0;
	}
	const QChar delimiter = content.at(digits);
	const bool isNumberDelimiter = delimiter == u'.' || delimiter == u')';
	return isNumberDelimiter ? digits + 1 : 0;
}

// The column at which the content of the list item that starts `line` begins, or -1 when it
// starts none.
qsizetype listContentColumn(QStringView line)
{
	const QStringView content = line.trimmed();
	const qsizetype bullet = bulletMarkerLength(content);
	const qsizetype markerLength = bullet > 0 ? bullet : numberedMarkerLength(content);
	if(markerLength == 0)
	{
		return -1;
	}
	if(markerLength < content.size())
	{
		const QChar after = content.at(markerLength);
		const bool followedBySpace = after == u' ' || after == u'\t';
		if(!followedBySpace)
		{
			return -1;
		}
	}
	return indentOf(line) + markerLength + 1;
}

// Whether the Markdown renderer makes a picture of `markdown`. Parsing loads nothing: pictures
// are fetched when a document is drawn.
bool rendersImage(const QString& markdown)
{
	if(!qobject_cast<QGuiApplication*>(QCoreApplication::instance()))
		return true; // the renderer needs one; without it, assume the worst
	QTextDocument document;
	document.setMarkdown(markdown);
	for(QTextBlock block = document.begin(); block.isValid(); block = block.next())
	{
		for(QTextBlock::iterator it = block.begin(); !it.atEnd(); ++it)
		{
			if(it.fragment().charFormat().isImageFormat())
				return true;
		}
	}
	return false;
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
			const bool startsIndentedCode = relative >= IndentedCodeIndent && (afterBlank || afterCode);
			const std::optional<Fence> opening = relative <= MaxFenceIndent ? openingFence(line) : std::nullopt;
			if(startsIndentedCode)
			{
				addSegment(segments, start, end, true);
				afterCode = true;
			}
			else if(opening)
			{
				fence = opening;
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
	// What is left of image syntax is in code, as far as the analysis above can tell. The renderer has
	// the last word on that: if it still makes a picture of it, nothing stays.
	if(out.contains(u"!["_s) && rendersImage(out))
		return out.replace(u"!["_s, u"!\\["_s);
	return out;
}

} // namespace refgui::session
