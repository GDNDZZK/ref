#pragma once

#include <QString>

namespace refgui::compose
{

// What the pop-up list under the cursor is about.
struct CompletionTarget
{
	enum class Kind
	{
		None,
		Command,      // "/res": the whole text is a command name being typed
		CommandArg,   // "/models claude", "/cd src/", "/close-session a b": built-in command argument
		File,         // "@src/ma": a file mention (in a prompt or custom command arguments)
		ShellCommand, // first token in `!` shell mode (history + PATH binaries)
		ShellPath,    // path token in `!` shell mode (path command or any argument token)
	};

	Kind kind = Kind::None;
	QString command{};   // canonical slash command name for CommandArg
	QString query{};     // what was typed in the active token up to `cursor`
	qsizetype start = 0; // start offset in `text` of the token being replaced
	qsizetype end = 0;   // end offset in `text` of the token being replaced
};

// Finds the completion target at `cursor`.
// In shell mode, the first token offers ShellCommand (or ShellPath when it contains `/` or starts
// with `.` or `~`), and subsequent tokens offer ShellPath; an empty shell draft offers
// ShellCommand only when `forceShellEmpty` is set (e.g. Tab).
// Outside shell mode, a single `/word` at the end offers Command, built-in commands with
// completable arguments offer CommandArg, and `@` after start/whitespace offers File.
CompletionTarget completionTarget(const QString& text, qsizetype cursor, bool shellMode, bool forceShellEmpty = false);

} // namespace refgui::compose
