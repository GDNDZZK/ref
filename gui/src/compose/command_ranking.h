#pragma once

#include <QList>
#include <QString>
#include <QStringList>

namespace refgui::compose
{

// A command as the `/` list sees it.
struct CommandEntry
{
	QString name;          // without the slash
	QStringList aliases{}; // other slash names
	QString description{};
	QString shortcut{};
	bool builtin = true;   // custom commands rank a little below built-in ones of equal match
};

struct CommandMatch
{
	qsizetype entry = 0; // index into the list that was ranked
	QString label;       // "/name", or "/alias -> /name" when an alias matched better
};

// The commands matching `query` (the text after the slash), best first, at most `limit`.
// Fuzzy match on the name and every alias; the name wins a tie.
QList<CommandMatch> rankCommands(const QList<CommandEntry>& commands, const QString& query, qsizetype limit);

} // namespace refgui::compose
