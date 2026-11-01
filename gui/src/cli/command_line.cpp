#include "cli/command_line.h"

#include <QCommandLineOption>
#include <QCommandLineParser>
#include <QDir>
#include <QFileInfo>
#include <QUrl>

#include <cstdlib>
#include <expected>

using namespace Qt::StringLiterals;

namespace refgui::cli
{
namespace
{

template<typename T>
using Parsed = std::expected<T, QString>;

constexpr int UsageErrorCode = 2;
const QStringList WebSocketSchemes{u"ws"_s, u"wss"_s};

// `provider/model` or `provider/model#variant`, as the terminal client reads it.
Parsed<proto::ModelRef> parseModel(const QString& text)
{
	const qsizetype hash = text.lastIndexOf(u'#');
	const QString providerAndModel = hash < 0 ? text : text.first(hash);
	const qsizetype slash = providerAndModel.indexOf(u'/');
	const QString provider = slash < 0 ? QString() : providerAndModel.first(slash);
	const QString model = slash < 0 ? QString() : providerAndModel.sliced(slash + 1);
	if(provider.isEmpty() || model.isEmpty())
	{
		return std::unexpected(u"invalid --model \"%1\"; expected provider/model[#variant]"_s.arg(text));
	}
	std::optional<QString> variant;
	if(hash >= 0)
	{
		variant = text.sliced(hash + 1);
	}
	return proto::ModelRef{provider, model, variant};
}

Parsed<QString> parseDirectory(const QStringList& positional, const QString& currentDirectory)
{
	if(positional.size() > 1)
	{
		return std::unexpected(u"too many arguments; expected at most one directory"_s);
	}
	const QString given = positional.isEmpty() ? currentDirectory : positional.first();
	const QFileInfo info(QDir(currentDirectory).absoluteFilePath(given));
	if(!info.isDir())
	{
		return std::unexpected(u"%1: not a directory"_s.arg(given));
	}
	return info.canonicalFilePath();
}

Parsed<client::Endpoint> parseServer(const QString& url, const QString& token)
{
	const QUrl parsed(url);
	if(!parsed.isValid() || !WebSocketSchemes.contains(parsed.scheme()) || parsed.host().isEmpty())
	{
		return std::unexpected(u"invalid --server \"%1\"; expected ws://host:port"_s.arg(url));
	}
	return client::Endpoint{parsed, token};
}

Exit usageError(const QString& message)
{
	return {u"ref-gui: %1\nTry 'ref-gui --help'.\n"_s.arg(message), UsageErrorCode};
}

// The options ref-gui understands.
struct Options
{
	QCommandLineOption help{{u"h"_s, u"help"_s}, u"Show this help."_s};
	QCommandLineOption version{{u"V"_s, u"version"_s}, u"Show the version."_s};
	QCommandLineOption continueLast{{u"c"_s, u"continue"_s}, u"Continue the most recent session of the directory."_s};
	QCommandLineOption session{{u"s"_s, u"session"_s}, u"Attach to a session by id (live or from history)."_s, u"ID"_s};
	QCommandLineOption watch{{u"w"_s, u"watch"_s}, u"Split view of all live background sessions."_s};
	QCommandLineOption agent{u"agent"_s, u"Agent for a new session."_s, u"NAME"_s};
	QCommandLineOption model{u"model"_s, u"Model for a new session."_s, u"provider/model[#variant]"_s};
	QCommandLineOption server{u"server"_s, u"Backend to use (ws://host:port); none is started."_s, u"URL"_s};
	QCommandLineOption token{u"token"_s, u"Token for --server."_s, u"TOKEN"_s};
	QCommandLineOption mock{u"mock"_s, u"Talk to a private `ref-server --mock` with canned scripts (development)."_s};
	// Not in --help: the integration tests start the whole program with it.
	QCommandLineOption smoke = hiddenOption(u"smoke"_s, u"Open the session, print `attached <id>` and exit."_s);

	static QCommandLineOption hiddenOption(const QString& name, const QString& description)
	{
		QCommandLineOption option(name, description);
		option.setFlags(QCommandLineOption::HiddenFromHelp);
		return option;
	}
};

void buildParser(QCommandLineParser& parser, const Options& options)
{
	parser.setApplicationDescription(u"ref-gui: graphical front end for the ref coding agent"_s);
	parser.addPositionalArgument(u"DIR"_s, u"Directory to work in (default: the current one)."_s, u"[DIR]"_s);
	parser.addOptions({options.help, options.version, options.continueLast, options.session, options.watch, options.agent, options.model, options.server, options.token, options.mock, options.smoke});
}

Parsed<SessionStart> parseStart(const QCommandLineParser& parser, const Options& options)
{
	SessionStart start;
	if(parser.isSet(options.session))
	{
		start = {.kind = SessionStart::Kind::Attach, .sessionId = parser.value(options.session)};
	}
	else if(parser.isSet(options.continueLast))
	{
		start.kind = SessionStart::Kind::Continue;
	}
	if(parser.isSet(options.agent))
	{
		start.agent = parser.value(options.agent);
	}
	if(!parser.isSet(options.model))
	{
		return start;
	}
	const Parsed<proto::ModelRef> model = parseModel(parser.value(options.model));
	if(!model)
	{
		return std::unexpected(model.error());
	}
	start.model = *model;
	return start;
}

Parsed<client::Backend::Source> parseSource(const QCommandLineParser& parser, const Options& options)
{
	if(parser.isSet(options.token) && !parser.isSet(options.server))
	{
		return std::unexpected(u"--token needs --server"_s);
	}
	if(parser.isSet(options.server) && parser.isSet(options.mock))
	{
		return std::unexpected(u"--server and --mock cannot be combined"_s);
	}
	if(parser.isSet(options.server))
	{
		const Parsed<client::Endpoint> endpoint = parseServer(parser.value(options.server), parser.value(options.token));
		if(!endpoint)
		{
			return std::unexpected(endpoint.error());
		}
		return client::Backend::Mode::Remote{*endpoint};
	}
	if(parser.isSet(options.mock))
	{
		return client::Backend::Mode::Mock{};
	}
	return client::Backend::Mode::Shared{};
}

} // namespace

ParseResult parseCommandLine(const QStringList& arguments, const QString& currentDirectory, const QString& version)
{
	const Options known;
	QCommandLineParser parser;
	buildParser(parser, known);
	if(!parser.parse(arguments))
	{
		return usageError(parser.errorText());
	}
	if(parser.isSet(known.help))
	{
		return Exit{parser.helpText(), EXIT_SUCCESS};
	}
	if(parser.isSet(known.version))
	{
		return Exit{u"ref-gui %1\n"_s.arg(version), EXIT_SUCCESS};
	}

	LaunchOptions options;
	const Parsed<QString> directory = parseDirectory(parser.positionalArguments(), currentDirectory);
	if(!directory)
	{
		return usageError(directory.error());
	}
	options.directory = *directory;

	const Parsed<SessionStart> start = parseStart(parser, known);
	if(!start)
	{
		return usageError(start.error());
	}
	options.start = *start;
	options.watch = parser.isSet(known.watch);

	const Parsed<client::Backend::Source> source = parseSource(parser, known);
	if(!source)
	{
		return usageError(source.error());
	}
	options.source = *source;

	options.smoke = parser.isSet(known.smoke);
	return options;
}

} // namespace refgui::cli
