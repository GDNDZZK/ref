#include "compose/fuzzy.h"

using namespace Qt::StringLiterals;

namespace refgui::compose
{
namespace
{

constexpr int MatchedLetter = 10;
constexpr int ConsecutiveLetter = 15;
constexpr int AfterSeparator = 12;
constexpr int Prefix = 40;

bool isSeparator(QChar c)
{
	return c == u'/' || c == u'-' || c == u'_' || c == u' ' || c == u'.';
}

} // namespace

std::optional<int> fuzzyScore(const QString& haystack, const QString& needle)
{
	if(needle.isEmpty())
	{
		return 0;
	}

	const QString hay = haystack.toLower();
	const QString wanted = needle.toLower();
	int score = 0;
	qsizetype next = 0; // where the search for the following letter starts
	std::optional<qsizetype> last;
	for(const QChar letter : wanted)
	{
		if(letter == u' ')
		{
			continue;
		}
		const qsizetype at = hay.indexOf(letter, next);
		if(at < 0)
		{
			return std::nullopt;
		}
		next = at + 1;

		score += MatchedLetter;
		if(!last)
		{
			score -= static_cast<int>(at); // the later the first letter, the worse
		}
		else if(at == *last + 1)
		{
			score += ConsecutiveLetter;
		}
		else
		{
			score -= static_cast<int>((at - *last) / 2);
		}
		if(at == 0 || isSeparator(hay[at - 1]))
		{
			score += AfterSeparator;
		}
		last = at;
	}

	if(hay.startsWith(wanted))
	{
		score += Prefix;
	}
	return score;
}

} // namespace refgui::compose
