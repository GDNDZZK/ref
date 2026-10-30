#include "compose/command_ranking.h"

#include "compose/fuzzy.h"

#include <algorithm>

using namespace Qt::StringLiterals;

namespace refgui::compose
{
namespace
{

constexpr int BuiltinBonus = 5;

struct Ranked
{
	CommandMatch match;
	int score = 0;
};

// The name or alias that matches best, with its score.
std::optional<std::pair<int, QString>> bestName(const CommandEntry& command, const QString& query)
{
	std::optional<std::pair<int, QString>> best;
	QStringList names{command.name};
	names.append(command.aliases);
	for(const QString& name : names)
	{
		const std::optional<int> score = fuzzyScore(name, query);
		if(score && (!best || *score > best->first))
		{
			best = {*score, name};
		}
	}
	return best;
}

} // namespace

QList<CommandMatch> rankCommands(const QList<CommandEntry>& commands, const QString& query, qsizetype limit)
{
	QList<Ranked> ranked;
	for(qsizetype i = 0; i < commands.size(); ++i)
	{
		const CommandEntry& command = commands[i];
		const auto best = bestName(command, query);
		if(!best)
		{
			continue;
		}
		const QString label = best->second == command.name ? u"/%1"_s.arg(command.name) : u"/%1 \u2192 /%2"_s.arg(best->second, command.name);
		ranked.append({.match = {.entry = i, .label = label}, .score = best->first + (command.builtin ? BuiltinBonus : 0)});
	}

	std::ranges::stable_sort(ranked, [](const Ranked& a, const Ranked& b) { return a.score > b.score; });

	QList<CommandMatch> matches;
	for(qsizetype i = 0; i < std::min<qsizetype>(limit, ranked.size()); ++i)
	{
		matches.append(ranked[i].match);
	}
	return matches;
}

} // namespace refgui::compose
