#include "cli/command_line.h"

#include <QCommandLineOption>
#include <QCommandLineParser>
#include <QDir>
#include <QFileInfo>
#include <QUrl>

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

} // namespace

ParseResult parseCommandLine(const QStringList& arguments, const QString& currentDirectory, const QString& version)
{
	QCommandLineParser parser;
	parser.setApplicationDescription(u"ref-gui: graphical front end for the ref coding agent"_s);
	parser.addPositionalArgument(u"DIR"_s, u"Directory to work in (default: the current one)."_s, u"[DIR]"_s);
	const QCommandLineOption helpOption({u"h"_s, u"help"_s}, u"Show this help."_s);
	const QCommandLineOption versionOption({u"V"_s, u"version"_s}, u"Show the version."_s);
	const QCommandLineOption continueOption({u"c"_s, u"continue"_s}, u"Continue the most recent session of the directory."_s);
	const QCommandLineOption sessionOption({u"s"_s, u"session"_s}, u"Attach to a session by id (live or from history)."_s, u"ID"_s);
	const QCommandLineOption watchOption({u"w"_s, u"watch"_s}, u"Split view of all live background sessions (not available yet)."_s);
	const QCommandLineOption agentOption(u"agent"_s, u"Agent for a new session."_s, u"NAME"_s);
	const QCommandLineOption modelOption(u"model"_s, u"Model for a new session."_s, u"provider/model[#variant]"_s);
	const QCommandLineOption serverOption(u"server"_s, u"Backend to use (ws://host:port); none is started."_s, u"URL"_s);
	const QCommandLineOption tokenOption(u"token"_s, u"Token for --server."_s, u"TOKEN"_s);
	const QCommandLineOption mockOption(u"mock"_s, u"Talk to a private `ref-server --mock` with canned scripts (development)."_s);
	// Not in --help: the integration tests start the whole program with it.
	QCommandLineOption smokeOption(u"smoke"_s, u"Open the session, print `attached <id>` and exit."_s);
	smokeOption.setFlags(QCommandLineOption::HiddenFromHelp);
	parser.addOptions({helpOption, versionOption, continueOption, sessionOption, watchOption, agentOption, modelOption, serverOption, tokenOption, mockOption, smokeOption});

	if(!parser.parse(arguments))
	{
		return usageError(parser.errorText());
	}
	if(parser.isSet(helpOption))
	{
		return Exit{parser.helpText(), 0};
	}
	if(parser.isSet(versionOption))
	{
		return Exit{u"ref-gui %1\n"_s.arg(version), 0};
	}

	LaunchOptions options;
	const Parsed<QString> directory = parseDirectory(parser.positionalArguments(), currentDirectory);
	if(!directory)
	{
		return usageError(directory.error());
	}
	options.directory = *directory;

	if(parser.isSet(sessionOption))
	{
		options.start = {.kind = SessionStart::Kind::Attach, .sessionId = parser.value(sessionOption)};
	}
	else if(parser.isSet(continueOption))
	{
		options.start.kind = SessionStart::Kind::Continue;
	}
	if(parser.isSet(watchOption))
	{
		options.warnings.append(u"split view is not available yet; opening a session instead"_s);
	}
	if(parser.isSet(agentOption))
	{
		options.start.agent = parser.value(agentOption);
	}
	if(parser.isSet(modelOption))
	{
		const Parsed<proto::ModelRef> model = parseModel(parser.value(modelOption));
		if(!model)
		{
			return usageError(model.error());
		}
		options.start.model = *model;
	}

	if(parser.isSet(tokenOption) && !parser.isSet(serverOption))
	{
		return usageError(u"--token needs --server"_s);
	}
	if(parser.isSet(serverOption) && parser.isSet(mockOption))
	{
		return usageError(u"--server and --mock cannot be combined"_s);
	}
	if(parser.isSet(serverOption))
	{
		const Parsed<client::Endpoint> endpoint = parseServer(parser.value(serverOption), parser.value(tokenOption));
		if(!endpoint)
		{
			return usageError(endpoint.error());
		}
		options.source = client::Backend::Mode::Remote{*endpoint};
	}
	else if(parser.isSet(mockOption))
	{
		options.source = client::Backend::Mode::Mock{};
	}

	options.smoke = parser.isSet(smokeOption);
	return options;
}

} // namespace refgui::cli
