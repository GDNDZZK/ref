#include "workspace/toast_text.h"

#include <QChar>
#include <QList>
#include <QStringView>
#include <QTextBoundaryFinder>
#include <algorithm>
#include <string>

namespace refgui::workspace
{

namespace
{

// The length of `text` in Unicode characters.
qsizetype characterCount(QStringView text)
{
	return text.toUcs4().size();
}

} // namespace

QString oneLine(const QString& text)
{
	std::u32string kept;
	bool space = false;
	for(const char32_t c : text.toStdU32String())
	{
		if(QChar::isSpace(c))
		{
			space = true;
			continue;
		}
		if(QChar::category(c) == QChar::Other_Control)
		{
			continue;
		}
		if(space && !kept.empty())
		{
			kept.push_back(U' ');
		}
		space = false;
		kept.push_back(c);
	}
	return QString::fromStdU32String(kept);
}

QString truncated(const QString& text, int maxChars)
{
	if(characterCount(text) <= maxChars)
	{
		return text;
	}
	if(maxChars <= 0)
	{
		return {};
	}
	// Cuts only between graphemes, so that an emoji sequence or a letter with its accent stays whole.
	const qsizetype room = maxChars - 1; // the ellipsis takes one
	QTextBoundaryFinder boundaries(QTextBoundaryFinder::Grapheme, text);
	qsizetype end = 0;
	qsizetype used = 0;
	for(qsizetype next = boundaries.toNextBoundary(); next >= 0; next = boundaries.toNextBoundary())
	{
		used += characterCount(QStringView(text).sliced(end, next - end));
		if(used > room)
		{
			break;
		}
		end = next;
	}
	return text.left(end) + QChar(0x2026);
}

} // namespace refgui::workspace
