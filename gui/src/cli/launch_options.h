#pragma once

#include "client/backend.h"
#include "proto/types.h"

#include <QString>
#include <QStringList>

#include <cstdlib>
#include <optional>
#include <variant>

namespace refgui::cli
{

// Which session to open once the backend is reachable.
struct SessionStart
{
	enum class Kind
	{
		None,     // start without opening a session
		Continue, // the newest session of the directory, else a new one
		Attach,   // `sessionId`
	};

	Kind kind = Kind::None;
	QString sessionId{};
	std::optional<QString> agent{};        // default for new sessions
	std::optional<proto::ModelRef> model{}; // default for new sessions
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

// The program should print `text` (stdout when `code` is EXIT_SUCCESS, else stderr) and exit.
struct Exit
{
	QString text;
	int code = EXIT_SUCCESS;
};

using ParseResult = std::variant<LaunchOptions, Exit>;

} // namespace refgui::cli
