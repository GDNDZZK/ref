#include "session/markdown_text.h"

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

} // namespace

QString markdownForDisplay(const QString& markdown)
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

} // namespace refgui::session
