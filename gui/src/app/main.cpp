// Adapted from an early version of qutie by LightBlueCube
#include "app/app_translator.h"
#include "app/notification_center_type.h"
#include "app/workspace.h"
#include "cli/command_line.h"
#include "client/instance_files.h"
#include "icons/icon_provider.h"
#include "notify/notifier.h"

#include <QDir>
#include <QGuiApplication>
#include <QLocale>
#include <QQmlApplicationEngine>
#include <QWindow>
#include <QTextStream>
#include <QTimer>

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

// For --smoke: ends the program as soon as the session is open, or the backend cannot be
// reached or has no such session.
void exitWhenOpened(Workspace& workspace)
{
	QObject::connect(&workspace, &Workspace::opened, &workspace, [](const QString& sessionId) {
		QTextStream out(stdout);
		print(out, u"attached %1\n"_s.arg(sessionId));
		QTimer::singleShot(SmokeSettle, [] { QCoreApplication::exit(EXIT_SUCCESS); });
	});
	QObject::connect(&workspace, &Workspace::openFailed, &workspace, [](const QString& reason) {
		QTextStream err(stderr);
		print(err, u"ref-gui: %1\n"_s.arg(reason));
		QCoreApplication::exit(EXIT_FAILURE);
	});
	QObject::connect(&workspace, &Workspace::connectionChanged, &workspace, [&workspace] {
		if(workspace.connectionState() != u"disconnected"_s)
		{
			return;
		}
		QTextStream err(stderr);
		print(err, u"ref-gui: %1\n"_s.arg(workspace.connectionText()));
		QCoreApplication::exit(EXIT_FAILURE);
	});
	QTimer::singleShot(SmokeTimeout, &workspace, [] {
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
	QGuiApplication::setDesktopFileName(u"ref-gui"_s); // ties the window to the desktop entry

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
	Workspace workspace(options, refgui::client::stateDirectory() + u"/gui.json"_s);
	Workspace::setInstance(&workspace);

	// The interface follows the language setting; English is the fallback.
	AppTranslator translator;
	translator.setLanguage(workspace.settings()->language());
	if(options.smoke)
	{
		exitWhenOpened(workspace);
	}

	// Tells the backend where the user looks and shows what it sends. The settings supply the kinds.
	refgui::notify::NotificationCenter notifications(workspace.backend(), refgui::notify::createPlatformNotifier());
	NotificationCenterForeign::instance = &notifications;
	const auto reportFocus = [&] { notifications.setFocus(workspace.isWindowActive(), workspace.visibleSessionIds()); };
	QObject::connect(&workspace, &Workspace::windowActiveChanged, &notifications, reportFocus);
	QObject::connect(&workspace, &Workspace::visibleSessionIdsChanged, &notifications, reportFocus);
	reportFocus();
	const auto reportKinds = [&] { notifications.setKinds(workspace.settings()->notifyKinds()); };
	QObject::connect(workspace.settings(), &refgui::workspace::Settings::notifyKindsChanged, &notifications, reportKinds);
	reportKinds();
	QObject::connect(&notifications, &refgui::notify::NotificationCenter::sessionRequested, &workspace, &Workspace::openSession);

	QQmlApplicationEngine engine;
	QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app, [] { QCoreApplication::exit(EXIT_FAILURE); }, Qt::QueuedConnection);
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);
	QObject::connect(workspace.settings(), &refgui::workspace::Settings::languageChanged, &app, [&] {
		translator.setLanguage(workspace.settings()->language());
		engine.retranslate();
		workspace.retranslate();
	});
	engine.loadFromModule("RefGui", "Main");
	if(!engine.rootObjects().isEmpty())
	{
		notifications.setWindow(qobject_cast<QWindow*>(engine.rootObjects().first()));
	}

	return QGuiApplication::exec();
}
