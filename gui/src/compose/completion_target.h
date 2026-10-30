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
		Command, // "/res": the whole text is a command name being typed
		File,    // "@src/ma": a file mention
	};

	Kind kind = Kind::None;
	QString query{};     // what was typed after the `/` or `@`
	qsizetype start = 0; // where the `/` or `@` is
};

// Finds the target at `cursor`. Shell mode has none. A command is only offered while the
// text is a single word starting with `/` and the cursor is at its end; an `@` starts a
// mention when it is at the start or follows whitespace, up to the next whitespace.
CompletionTarget completionTarget(const QString& text, qsizetype cursor, bool shellMode);

} // namespace refgui::compose
