#include "compose/completion_target.h"

#include <algorithm>

namespace refgui::compose
{
namespace
{

bool hasWhitespace(QStringView text)
{
	return std::ranges::any_of(text, [](QChar c) { return c.isSpace(); });
}

} // namespace

CompletionTarget completionTarget(const QString& text, qsizetype cursor, bool shellMode)
{
	if(shellMode)
	{
		return {};
	}

	const QStringView afterSlash = QStringView(text).mid(1);
	const bool typingCommand = text.startsWith(u'/') && !hasWhitespace(afterSlash) && cursor == text.size();
	if(typingCommand)
	{
		return {.kind = CompletionTarget::Kind::Command, .query = afterSlash.toString(), .start = 0};
	}

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
	return {.kind = CompletionTarget::Kind::File, .query = query.toString(), .start = at};
}

} // namespace refgui::compose
