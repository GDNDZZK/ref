#pragma once

#include "client/backend.h"
#include "proto/types.h"

#include <QString>
#include <QStringList>

#include <optional>
#include <variant>

namespace refgui::cli
{

// Which session to open once the backend is reachable.
struct SessionStart
{
	enum class Kind
	{
		New,
		Continue, // the newest session of the directory, else a new one
		Attach,   // `sessionId`
	};

	Kind kind = Kind::New;
	QString sessionId{};
	std::optional<QString> agent{};      // New only
	std::optional<proto::ModelRef> model{}; // New only
};

struct LaunchOptions
{
	QString directory; // absolute and canonical
	SessionStart start{};
	client::Backend::Source source{client::Backend::Mode::Shared{}};
	// Start in split view.
	bool watch = false;
	// Open the session, say so on stdout and exit. For tests that run the whole program.
	bool smoke = false;
	QStringList warnings{}; // for stderr, e.g. an option that has no effect yet
};

// The program should print `text` (stdout when `code` is 0, else stderr) and exit.
struct Exit
{
	QString text;
	int code = 0;
};

using ParseResult = std::variant<LaunchOptions, Exit>;

// `arguments` starts with the program name. Relative directories are taken from `currentDirectory`.
ParseResult parseCommandLine(const QStringList& arguments, const QString& currentDirectory, const QString& version);

} // namespace refgui::cli
