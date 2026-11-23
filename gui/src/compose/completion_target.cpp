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

struct WrapperSpec
{
	QLatin1StringView name;
	const QLatin1StringView* valueFlags = nullptr;
	qsizetype flagCount = 0;
	int positionalArgs = 0;
	const QLatin1StringView* noPositionalFlags = nullptr;
	qsizetype noPositionalCount = 0;
};

constexpr QLatin1StringView SudoFlags[] = {"-C"_L1, "-c"_L1, "-D"_L1, "-g"_L1, "-h"_L1, "-p"_L1, "-R"_L1, "-r"_L1, "-t"_L1, "-T"_L1, "-U"_L1, "-u"_L1, "--close-from"_L1, "--login-class"_L1, "--chdir"_L1, "--group"_L1, "--host"_L1, "--prompt"_L1, "--chroot"_L1, "--role"_L1, "--type"_L1, "--command-timeout"_L1, "--other-user"_L1, "--user"_L1};
constexpr QLatin1StringView DoasFlags[] = {"-a"_L1, "-C"_L1, "-u"_L1};
constexpr QLatin1StringView EnvFlags[] = {"-a"_L1, "-C"_L1, "-S"_L1, "-u"_L1, "--argv0"_L1, "--chdir"_L1, "--split-string"_L1, "--unset"_L1};
constexpr QLatin1StringView TimeFlags[] = {"-f"_L1, "-o"_L1, "--format"_L1, "--output"_L1};
constexpr QLatin1StringView NiceFlags[] = {"-n"_L1, "--adjustment"_L1};
constexpr QLatin1StringView IoniceFlags[] = {"-c"_L1, "-n"_L1, "--class"_L1, "--classdata"_L1};
constexpr QLatin1StringView TimeoutFlags[] = {"-k"_L1, "-s"_L1, "--kill-after"_L1, "--signal"_L1};
constexpr QLatin1StringView StraceFlags[] = {"-a"_L1, "-b"_L1, "-e"_L1, "-E"_L1, "-I"_L1, "-o"_L1, "-O"_L1, "-p"_L1, "-P"_L1, "-s"_L1, "-S"_L1, "-u"_L1, "-U"_L1};
constexpr QLatin1StringView LtraceFlags[] = {"-A"_L1, "-a"_L1, "-D"_L1, "-e"_L1, "-F"_L1, "-l"_L1, "-n"_L1, "-o"_L1, "-p"_L1, "-s"_L1, "-S"_L1, "-u"_L1, "-x"_L1};
constexpr QLatin1StringView TasksetFlags[] = {"-c"_L1, "--cpu-list"_L1};
constexpr QLatin1StringView TasksetNoPositionalFlags[] = {"-c"_L1, "--cpu-list"_L1, "-p"_L1, "--pid"_L1};
constexpr QLatin1StringView StdbufFlags[] = {"-i"_L1, "-o"_L1, "-e"_L1, "--input"_L1, "--output"_L1, "--error"_L1};
constexpr QLatin1StringView WatchFlags[] = {"-n"_L1, "--interval"_L1};
constexpr QLatin1StringView ExecFlags[] = {"-a"_L1};
constexpr QLatin1StringView XargsFlags[] = {"-a"_L1, "-d"_L1, "-E"_L1, "-e"_L1, "-I"_L1, "-i"_L1, "-L"_L1, "-l"_L1, "-n"_L1, "-P"_L1, "-s"_L1, "--arg-file"_L1, "--delimiter"_L1, "--eof"_L1, "--replace"_L1, "--max-lines"_L1, "--max-args"_L1, "--max-procs"_L1, "--max-chars"_L1, "--process-slot-var"_L1};
constexpr QLatin1StringView CaffeinateFlags[] = {"-t"_L1, "-w"_L1};
constexpr QLatin1StringView PkexecFlags[] = {"-u"_L1, "--user"_L1};

constexpr WrapperSpec ShellWrappers[] = {
	{"sudo"_L1, SudoFlags, std::size(SudoFlags), 0, nullptr, 0},
	{"doas"_L1, DoasFlags, std::size(DoasFlags), 0, nullptr, 0},
	{"env"_L1, EnvFlags, std::size(EnvFlags), 0, nullptr, 0},
	{"nohup"_L1, nullptr, 0, 0, nullptr, 0},
	{"time"_L1, TimeFlags, std::size(TimeFlags), 0, nullptr, 0},
	{"nice"_L1, NiceFlags, std::size(NiceFlags), 0, nullptr, 0},
	{"ionice"_L1, IoniceFlags, std::size(IoniceFlags), 0, nullptr, 0},
	{"timeout"_L1, TimeoutFlags, std::size(TimeoutFlags), 1, nullptr, 0},
	{"setsid"_L1, nullptr, 0, 0, nullptr, 0},
	{"strace"_L1, StraceFlags, std::size(StraceFlags), 0, nullptr, 0},
	{"ltrace"_L1, LtraceFlags, std::size(LtraceFlags), 0, nullptr, 0},
	{"chrt"_L1, nullptr, 0, 1, nullptr, 0},
	{"taskset"_L1, TasksetFlags, std::size(TasksetFlags), 1, TasksetNoPositionalFlags, std::size(TasksetNoPositionalFlags)},
	{"stdbuf"_L1, StdbufFlags, std::size(StdbufFlags), 0, nullptr, 0},
	{"watch"_L1, WatchFlags, std::size(WatchFlags), 0, nullptr, 0},
	{"command"_L1, nullptr, 0, 0, nullptr, 0},
	{"exec"_L1, ExecFlags, std::size(ExecFlags), 0, nullptr, 0},
	{"builtin"_L1, nullptr, 0, 0, nullptr, 0},
	{"xargs"_L1, XargsFlags, std::size(XargsFlags), 0, nullptr, 0},
	{"caffeinate"_L1, CaffeinateFlags, std::size(CaffeinateFlags), 0, nullptr, 0},
	{"pkexec"_L1, PkexecFlags, std::size(PkexecFlags), 0, nullptr, 0},
};

bool hasShortFlag(const QLatin1StringView* flags, qsizetype count, QChar ch)
{
	for(qsizetype i = 0; i < count; ++i)
	{
		if(flags[i].size() == 2 && flags[i][0] == u'-' && flags[i][1] == ch)
		{
			return true;
		}
	}
	return false;
}

bool hasLongFlag(const QLatin1StringView* flags, qsizetype count, QStringView flag)
{
	for(qsizetype i = 0; i < count; ++i)
	{
		if(flag == flags[i])
		{
			return true;
		}
	}
	return false;
}

const WrapperSpec* findWrapperSpec(QStringView word)
{
	const qsizetype slash = std::max(word.lastIndexOf(u'/'), word.lastIndexOf(u'\\'));
	const QStringView name = slash >= 0 ? word.mid(slash + 1) : word;
	for(const WrapperSpec& spec : ShellWrappers)
	{
		if(name == spec.name)
		{
			return &spec;
		}
	}
	return nullptr;
}

bool takesSeparateFlagArg(const WrapperSpec& spec, QStringView flag)
{
	if(!flag.startsWith(u'-') || flag.size() <= 1 || flag.contains(u'='))
	{
		return false;
	}
	if(flag.startsWith("--"_L1))
	{
		return hasLongFlag(spec.valueFlags, spec.flagCount, flag);
	}
	for(qsizetype i = 1; i < flag.size(); ++i)
	{
		if(hasShortFlag(spec.valueFlags, spec.flagCount, flag[i]))
		{
			return i == flag.size() - 1;
		}
	}
	return false;
}

bool cancelsPositional(const WrapperSpec& spec, QStringView flag)
{
	if(spec.noPositionalCount == 0 || !flag.startsWith(u'-') || flag.size() <= 1)
	{
		return false;
	}
	const qsizetype eq = flag.indexOf(u'=');
	const QStringView head = eq >= 0 ? flag.left(eq) : flag;
	if(head.startsWith("--"_L1))
	{
		return hasLongFlag(spec.noPositionalFlags, spec.noPositionalCount, head);
	}
	for(qsizetype i = 1; i < flag.size(); ++i)
	{
		if(hasShortFlag(spec.noPositionalFlags, spec.noPositionalCount, flag[i]))
		{
			return true;
		}
		if(hasShortFlag(spec.valueFlags, spec.flagCount, flag[i]))
		{
			return false;
		}
	}
	return false;
}

enum class ShellPosition
{
	Command,
	WrapperArg,
	Arg,
};

ShellPosition classifyShellPosition(QStringView prefix)
{
	const WrapperSpec* activeWrapper = nullptr;
	bool wrapperFlagsDone = false;
	bool expectingFlagArg = false;
	int positionalRemaining = 0;
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
		const QStringView word = prefix.mid(i, wordEnd - i);
		i = wordEnd;
		if(expectingFlagArg)
		{
			expectingFlagArg = false;
			continue;
		}
		if(activeWrapper != nullptr && !wrapperFlagsDone)
		{
			if(word == "--"_L1)
			{
				wrapperFlagsDone = true;
				continue;
			}
			if(word.startsWith(u'-') && word.size() > 1)
			{
				if(cancelsPositional(*activeWrapper, word))
				{
					positionalRemaining = 0;
				}
				expectingFlagArg = takesSeparateFlagArg(*activeWrapper, word);
				continue;
			}
		}
		if(positionalRemaining == 0 && isEnvAssignment(word))
		{
			continue;
		}
		if(positionalRemaining > 0)
		{
			--positionalRemaining;
			continue;
		}
		if(const WrapperSpec* nextWrapper = findWrapperSpec(word))
		{
			activeWrapper = nextWrapper;
			wrapperFlagsDone = false;
			expectingFlagArg = false;
			positionalRemaining = nextWrapper->positionalArgs;
			continue;
		}
		return ShellPosition::Arg;
	}
	return (expectingFlagArg || positionalRemaining > 0) ? ShellPosition::WrapperArg : ShellPosition::Command;
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
	const ShellPosition position = classifyShellPosition(beforeToken);
	if(position == ShellPosition::WrapperArg)
	{
		return {};
	}
	if(position == ShellPosition::Arg)
	{
		if(isShellPathToken(query))
		{
			return {.kind = CompletionTarget::Kind::ShellPath, .query = query, .start = tokenStart, .end = tokenEnd};
		}
		return {.kind = CompletionTarget::Kind::ShellArg, .query = query, .start = tokenStart, .end = tokenEnd};
	}
	if(query.startsWith(u'-'))
	{
		return {};
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

CompletionTarget slashCommandCompletionTarget(const QString& text, qsizetype cursor)
{
	qsizetype tokenStart = cursor;
	while(tokenStart > 0 && !text[tokenStart - 1].isSpace())
	{
		--tokenStart;
	}
	qsizetype tokenEnd = cursor;
	while(tokenEnd < text.size() && !text[tokenEnd].isSpace())
	{
		++tokenEnd;
	}
	if(cursor <= tokenStart || tokenStart >= tokenEnd || text[tokenStart] != u'/')
	{
		return {};
	}
	const QStringView afterSlash = QStringView(text).mid(tokenStart + 1, tokenEnd - (tokenStart + 1));
	if(afterSlash.contains(u'/') || afterSlash.contains(u'\\'))
	{
		return {};
	}
	return {.kind = CompletionTarget::Kind::Command,
	        .query = text.mid(tokenStart + 1, cursor - (tokenStart + 1)),
	        .start = tokenStart,
	        .end = tokenEnd};
}

} // namespace

CompletionTarget completionTarget(const QString& text, qsizetype cursor, bool shellMode, bool forceShellEmpty)
{
	const qsizetype clampedCursor = std::clamp<qsizetype>(cursor, 0, text.size());
	if(shellMode)
	{
		return shellCompletionTarget(text, clampedCursor, forceShellEmpty);
	}
	const CompletionTarget argTarget = slashArgCompletionTarget(text, clampedCursor);
	if(argTarget.kind != CompletionTarget::Kind::None)
	{
		return argTarget;
	}
	const CompletionTarget cmdTarget = slashCommandCompletionTarget(text, clampedCursor);
	if(cmdTarget.kind != CompletionTarget::Kind::None)
	{
		return cmdTarget;
	}
	return fileCompletionTarget(text, clampedCursor);
}

} // namespace refgui::compose
