#include "compose/completion_target.h"

#include <algorithm>

using namespace Qt::StringLiterals;

namespace refgui::compose
{
namespace
{

bool hasWhitespace(QStringView text)
{
	return std::ranges::any_of(text, [](QChar c) { return c.isSpace(); });
}

bool isAsciiAlpha(QChar c)
{
	return (c >= u'a' && c <= u'z') || (c >= u'A' && c <= u'Z');
}

bool isAsciiAlnum(QChar c)
{
	return isAsciiAlpha(c) || (c >= u'0' && c <= u'9');
}

bool hasDrivePrefix(QStringView token)
{
	return token.size() >= 2 && isAsciiAlpha(token[0]) && token[1] == u':';
}

bool isShellPathToken(QStringView token)
{
	return token.contains(u'/')
	       || token.contains(u'\\')
	       || token.startsWith(u'.')
	       || token.startsWith(u'~')
	       || hasDrivePrefix(token);
}

bool isEnvAssignment(QStringView word)
{
	const qsizetype eq = word.indexOf(u'=');
	if(eq <= 0)
	{
		return false;
	}
	if(!isAsciiAlpha(word[0]) && word[0] != u'_')
	{
		return false;
	}
	for(qsizetype i = 1; i < eq; ++i)
	{
		if(!isAsciiAlnum(word[i]) && word[i] != u'_')
		{
			return false;
		}
	}
	return true;
}

qsizetype advanceShellWordEnd(QStringView text, qsizetype pos)
{
	QChar quote{};
	while(pos < text.size())
	{
		const QChar ch = text[pos];
		if(quote.isNull())
		{
			if(ch.isSpace())
			{
				break;
			}
			if(ch == u'\'' || ch == u'"')
			{
				quote = ch;
			}
			else if(ch == u'\\' && pos + 1 < text.size())
			{
				++pos;
			}
		}
		else if(ch == quote)
		{
			quote = QChar{};
		}
		else if(quote == u'"' && ch == u'\\' && pos + 1 < text.size())
		{
			++pos;
		}
		++pos;
	}
	return pos;
}

bool allWordsAreEnvAssignments(QStringView prefix)
{
	qsizetype i = 0;
	while(i < prefix.size())
	{
		while(i < prefix.size() && prefix[i].isSpace())
		{
			++i;
		}
		if(i >= prefix.size())
		{
			break;
		}
		const qsizetype wordEnd = advanceShellWordEnd(prefix, i);
		if(!isEnvAssignment(prefix.mid(i, wordEnd - i)))
		{
			return false;
		}
		i = wordEnd;
	}
	return true;
}

qsizetype operatorLengthAt(QStringView text, qsizetype pos)
{
	const QChar ch = text[pos];
	if(ch == u';')
	{
		return 1;
	}
	if(ch == u'|')
	{
		return (pos + 1 < text.size() && text[pos + 1] == u'|') ? 2 : 1;
	}
	if(ch == u'&' && pos + 1 < text.size() && text[pos + 1] == u'&')
	{
		return 2;
	}
	return 0;
}

struct ShellSegment
{
	qsizetype start = 0;
	qsizetype end = 0;
	bool hasOperator = false;
	bool insideOperator = false;
};

ShellSegment findShellSegment(QStringView text, qsizetype cursor)
{
	ShellSegment seg{.start = 0, .end = text.size()};
	QChar quote{};
	qsizetype i = 0;
	while(i < text.size())
	{
		const QChar ch = text[i];
		if(!quote.isNull())
		{
			if(quote == u'"' && ch == u'\\' && i + 1 < text.size())
			{
				i += 2;
				continue;
			}
			if(ch == quote)
			{
				quote = {};
			}
			++i;
			continue;
		}
		if(ch == u'\'' || ch == u'"')
		{
			quote = ch;
			++i;
			continue;
		}
		const qsizetype opLen = operatorLengthAt(text, i);
		if(opLen == 0)
		{
			++i;
			continue;
		}
		seg.hasOperator = true;
		if(i + opLen <= cursor)
		{
			seg.start = i + opLen;
			i += opLen;
			continue;
		}
		if(cursor > i)
		{
			seg.insideOperator = true;
		}
		seg.end = i;
		break;
	}
	return seg;
}

CompletionTarget shellCompletionTarget(const QString& text, qsizetype cursor, bool forceShellEmpty)
{
	const ShellSegment seg = findShellSegment(text, cursor);
	if(seg.insideOperator)
	{
		return {};
	}

	qsizetype tokenStart = cursor;
	while(tokenStart > seg.start && !text[tokenStart - 1].isSpace())
	{
		--tokenStart;
	}
	qsizetype tokenEnd = cursor;
	while(tokenEnd < seg.end && !text[tokenEnd].isSpace())
	{
		++tokenEnd;
	}

	const QString query = text.mid(tokenStart, cursor - tokenStart);
	const QStringView beforeToken = QStringView(text).mid(seg.start, tokenStart - seg.start);
	const bool atCommandPos = allWordsAreEnvAssignments(beforeToken);

	if(!atCommandPos)
	{
		if(query.startsWith(u'-'))
		{
			return {};
		}
		return {.kind = CompletionTarget::Kind::ShellPath, .query = query, .start = tokenStart, .end = tokenEnd};
	}

	if(query.isEmpty())
	{
		if(!forceShellEmpty)
		{
			return {};
		}
		const bool atStart = !seg.hasOperator && tokenStart == 0 && QStringView(text).mid(tokenEnd).trimmed().isEmpty();
		return {.kind = CompletionTarget::Kind::ShellCommand, .query = {}, .start = tokenStart, .end = tokenEnd, .atLineStart = atStart};
	}

	if(isEnvAssignment(query))
	{
		const qsizetype eq = query.indexOf(u'=');
		const QString valQuery = query.mid(eq + 1);
		if(!valQuery.isEmpty() && isShellPathToken(valQuery))
		{
			return {.kind = CompletionTarget::Kind::ShellPath, .query = valQuery, .start = tokenStart + eq + 1, .end = tokenEnd};
		}
		return {};
	}

	if(isShellPathToken(query))
	{
		return {.kind = CompletionTarget::Kind::ShellPath, .query = query, .start = tokenStart, .end = tokenEnd};
	}

	const bool atStart = !seg.hasOperator && tokenStart == 0 && QStringView(text).mid(tokenEnd).trimmed().isEmpty();
	return {.kind = CompletionTarget::Kind::ShellCommand, .query = query, .start = tokenStart, .end = tokenEnd, .atLineStart = atStart};
}

QString canonicalArgCommand(QStringView raw)
{
	const QString lower = raw.toString().toLower();
	if(lower == u"cd"_s || lower == u"cwd"_s)
	{
		return u"cd"_s;
	}
	if(lower == u"export"_s)
	{
		return u"export"_s;
	}
	if(lower == u"variant"_s || lower == u"variants"_s || lower == u"effort"_s)
	{
		return u"variant"_s;
	}
	if(lower == u"models"_s || lower == u"model"_s)
	{
		return u"models"_s;
	}
	if(lower == u"agents"_s || lower == u"agent"_s)
	{
		return u"agents"_s;
	}
	if(lower == u"sessions"_s || lower == u"resume"_s || lower == u"continue"_s)
	{
		return u"sessions"_s;
	}
	if(lower == u"close-session"_s || lower == u"kill"_s)
	{
		return u"close-session"_s;
	}
	return {};
}

CompletionTarget slashArgCompletionTarget(const QString& text, qsizetype cursor)
{
	qsizetype cmdStart = 0;
	while(cmdStart < text.size() && text[cmdStart].isSpace())
	{
		++cmdStart;
	}
	if(cmdStart >= text.size() || text[cmdStart] != u'/')
	{
		return {};
	}

	qsizetype spaceIdx = cmdStart + 1;
	while(spaceIdx < text.size() && !text[spaceIdx].isSpace())
	{
		++spaceIdx;
	}
	if(spaceIdx <= cmdStart + 1 || spaceIdx >= text.size() || cursor <= spaceIdx)
	{
		return {};
	}

	const QString canonical = canonicalArgCommand(QStringView(text).mid(cmdStart + 1, spaceIdx - (cmdStart + 1)));
	if(canonical.isEmpty())
	{
		return {};
	}

	if(canonical == u"close-session"_s)
	{
		qsizetype tokenStart = cursor;
		while(tokenStart > spaceIdx + 1 && !text[tokenStart - 1].isSpace())
		{
			--tokenStart;
		}
		qsizetype tokenEnd = cursor;
		while(tokenEnd < text.size() && !text[tokenEnd].isSpace())
		{
			++tokenEnd;
		}
		return {.kind = CompletionTarget::Kind::CommandArg,
		        .command = canonical,
		        .query = text.mid(tokenStart, cursor - tokenStart),
		        .start = tokenStart,
		        .end = tokenEnd};
	}

	if(!QStringView(text).mid(cursor).trimmed().isEmpty())
	{
		return {};
	}
	qsizetype argStart = spaceIdx + 1;
	while(argStart < cursor && text[argStart].isSpace())
	{
		++argStart;
	}
	return {.kind = CompletionTarget::Kind::CommandArg,
	        .command = canonical,
	        .query = text.mid(argStart, cursor - argStart),
	        .start = argStart,
	        .end = cursor};
}

CompletionTarget fileCompletionTarget(const QString& text, qsizetype cursor)
{
	const QStringView before = QStringView(text).left(cursor);
	const qsizetype at = before.lastIndexOf(u'@');
	if(at < 0)
	{
		return {};
	}
	const QStringView query = before.mid(at + 1);
	const bool startsMention = at == 0 || before[at - 1].isSpace();
	if(hasWhitespace(query) || !startsMention)
	{
		return {};
	}
	return {.kind = CompletionTarget::Kind::File, .query = query.toString(), .start = at, .end = cursor};
}

} // namespace

CompletionTarget completionTarget(const QString& text, qsizetype cursor, bool shellMode, bool forceShellEmpty)
{
	const qsizetype clampedCursor = std::clamp<qsizetype>(cursor, 0, text.size());
	if(shellMode)
	{
		return shellCompletionTarget(text, clampedCursor, forceShellEmpty);
	}

	const QStringView afterSlash = QStringView(text).mid(1);
	const bool typingCommand = text.startsWith(u'/') && !hasWhitespace(afterSlash) && clampedCursor == text.size();
	if(typingCommand)
	{
		return {.kind = CompletionTarget::Kind::Command, .query = afterSlash.toString(), .start = 0, .end = clampedCursor};
	}

	const CompletionTarget argTarget = slashArgCompletionTarget(text, clampedCursor);
	if(argTarget.kind != CompletionTarget::Kind::None)
	{
		return argTarget;
	}

	return fileCompletionTarget(text, clampedCursor);
}

} // namespace refgui::compose
