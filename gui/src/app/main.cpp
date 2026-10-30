// Adapted from an early version of qutie by LightBlueCube
#include "app/chat_session.h"
#include "cli/command_line.h"

#include <QDir>
#include <QGuiApplication>
#include <QLocale>
#include <QQmlApplicationEngine>
#include <QTextStream>
#include <QTimer>
#include <QTranslator>

#include <chrono>
#include <cstdlib>

using namespace Qt::StringLiterals;
using namespace std::chrono_literals;

namespace
{

// How long --smoke waits for the session to open; the backend may have to start first.
constexpr std::chrono::milliseconds SmokeTimeout = 30s;
// After the session opened, the window gets this long to create its rows, so that QML
// warnings about them show up on stderr before the program exits.
constexpr std::chrono::milliseconds SmokeSettle = 400ms;

void print(QTextStream& stream, const QString& text)
{
	stream << text;
	stream.flush();
}

// For --smoke: ends the program as soon as the session is open (or cannot be).
void exitWhenOpened(ChatSession& session)
{
	QObject::connect(&session, &ChatSession::opened, &session, [](const QString& sessionId) {
		QTextStream out(stdout);
		print(out, u"attached %1\n"_s.arg(sessionId));
		QTimer::singleShot(SmokeSettle, [] { QCoreApplication::exit(EXIT_SUCCESS); });
	});
	QObject::connect(&session, &ChatSession::openFailed, &session, [](const QString& reason) {
		QTextStream err(stderr);
		print(err, u"ref-gui: %1\n"_s.arg(reason));
		QCoreApplication::exit(EXIT_FAILURE);
	});
	QTimer::singleShot(SmokeTimeout, &session, [] {
		QTextStream err(stderr);
		print(err, u"ref-gui: no session after %1 s\n"_s.arg(SmokeTimeout.count() / 1000));
		QCoreApplication::exit(EXIT_FAILURE);
	});
}

} // namespace

int main(int argc, char* argv[])
{
	QGuiApplication app(argc, argv);
	QGuiApplication::setApplicationName(u"ref-gui"_s);
	const QString version = QString::fromLatin1(REF_GUI_VERSION);
	QGuiApplication::setApplicationVersion(version);

	// The interface follows the system language; English is the fallback.
	QTranslator translator;
	if(translator.load(QLocale(), u"ref-gui"_s, u"_"_s, u":/i18n"_s))
	{
		QCoreApplication::installTranslator(&translator);
	}

	const refgui::cli::ParseResult parsed = refgui::cli::parseCommandLine(QCoreApplication::arguments(), QDir::currentPath(), version);
	if(const auto* exit = std::get_if<refgui::cli::Exit>(&parsed))
	{
		QTextStream stream(exit->code == 0 ? stdout : stderr);
		print(stream, exit->text);
		return exit->code;
	}
	const auto& options = std::get<refgui::cli::LaunchOptions>(parsed);
	QTextStream err(stderr);
	for(const QString& warning : options.warnings)
	{
		print(err, u"ref-gui: %1\n"_s.arg(warning));
	}

	// Outlives the engine, whose window refers to it.
	ChatSession session(options.source, options.directory, options.start);
	if(options.smoke)
	{
		exitWhenOpened(session);
	}

	QQmlApplicationEngine engine;
	QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app, [] { QCoreApplication::exit(EXIT_FAILURE); }, Qt::QueuedConnection);
	engine.setInitialProperties({{u"session"_s, QVariant::fromValue(&session)}});
	engine.loadFromModule("RefGui", "Main");

	return QGuiApplication::exec();
}
