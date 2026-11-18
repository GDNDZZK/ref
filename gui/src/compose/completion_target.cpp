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

} // namespace

CompletionTarget completionTarget(const QString& text, qsizetype cursor, bool shellMode, bool forceShellEmpty)
{
	const qsizetype clampedCursor = std::clamp<qsizetype>(cursor, 0, text.size());

	if(shellMode)
	{
		qsizetype tokenStart = clampedCursor;
		while(tokenStart > 0 && !text[tokenStart - 1].isSpace())
		{
			--tokenStart;
		}
		const QString token = text.mid(tokenStart, clampedCursor - tokenStart);
		const bool firstToken = QStringView(text).left(tokenStart).trimmed().isEmpty();
		if(firstToken)
		{
			if(token.isEmpty())
			{
				if(forceShellEmpty)
				{
					return {.kind = CompletionTarget::Kind::ShellCommand, .query = {}, .start = clampedCursor, .end = clampedCursor};
				}
				return {};
			}
			if(token.contains(u'/') || token.startsWith(u'.') || token.startsWith(u'~'))
			{
				return {.kind = CompletionTarget::Kind::ShellPath, .query = token, .start = tokenStart, .end = clampedCursor};
			}
			return {.kind = CompletionTarget::Kind::ShellCommand, .query = token, .start = tokenStart, .end = clampedCursor};
		}
		return {.kind = CompletionTarget::Kind::ShellPath, .query = token, .start = tokenStart, .end = clampedCursor};
	}

	const QStringView afterSlash = QStringView(text).mid(1);
	const bool typingCommand = text.startsWith(u'/') && !hasWhitespace(afterSlash) && clampedCursor == text.size();
	if(typingCommand)
	{
		return {.kind = CompletionTarget::Kind::Command, .query = afterSlash.toString(), .start = 0, .end = clampedCursor};
	}

	qsizetype cmdStart = 0;
	while(cmdStart < text.size() && text[cmdStart].isSpace())
	{
		++cmdStart;
	}
	if(cmdStart < text.size() && text[cmdStart] == u'/')
	{
		qsizetype spaceIdx = cmdStart + 1;
		while(spaceIdx < text.size() && !text[spaceIdx].isSpace())
		{
			++spaceIdx;
		}
		if(spaceIdx > cmdStart + 1 && spaceIdx < text.size() && clampedCursor > spaceIdx)
		{
			const QString canonical = canonicalArgCommand(QStringView(text).mid(cmdStart + 1, spaceIdx - (cmdStart + 1)));
			if(!canonical.isEmpty())
			{
				if(canonical == u"close-session"_s)
				{
					qsizetype tokenStart = clampedCursor;
					while(tokenStart > spaceIdx + 1 && !text[tokenStart - 1].isSpace())
					{
						--tokenStart;
					}
					const QString query = text.mid(tokenStart, clampedCursor - tokenStart);
					return {.kind = CompletionTarget::Kind::CommandArg,
					        .command = canonical,
					        .query = query,
					        .start = tokenStart,
					        .end = clampedCursor};
				}
				if(QStringView(text).mid(clampedCursor).trimmed().isEmpty())
				{
					qsizetype argStart = spaceIdx + 1;
					while(argStart < clampedCursor && text[argStart].isSpace())
					{
						++argStart;
					}
					const QString query = text.mid(argStart, clampedCursor - argStart);
					return {.kind = CompletionTarget::Kind::CommandArg,
					        .command = canonical,
					        .query = query,
					        .start = argStart,
					        .end = clampedCursor};
				}
			}
		}
	}

	const QStringView before = QStringView(text).left(clampedCursor);
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
	return {.kind = CompletionTarget::Kind::File, .query = query.toString(), .start = at, .end = clampedCursor};
}

} // namespace refgui::compose
