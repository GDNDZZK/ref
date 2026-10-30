// Adapted from an early version of qutie by LightBlueCube
#include "app/chat_session.h"

#include <QCommandLineParser>
#include <QDir>
#include <QGuiApplication>
#include <QQmlApplicationEngine>

#include <cstdlib>

using namespace Qt::StringLiterals;
using refgui::client::Backend;

int main(int argc, char* argv[])
{
	QGuiApplication app(argc, argv);
	QGuiApplication::setApplicationName(u"ref-gui"_s);
	QGuiApplication::setApplicationVersion(QString::fromLatin1(REF_GUI_VERSION));

	QCommandLineParser parser;
	parser.addHelpOption();
	const QCommandLineOption mockOption(u"mock"_s, u"Talk to a private `ref-server --mock` with canned scripts."_s);
	parser.addOption(mockOption);
	parser.process(app);

	const Backend::Source source = parser.isSet(mockOption) ? Backend::Source(Backend::Mode::Mock{}) : Backend::Source(Backend::Mode::Shared{});

	// Outlives the engine, whose window refers to it.
	ChatSession session(source, QDir::currentPath(), {});

	QQmlApplicationEngine engine;
	QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app, [] { QCoreApplication::exit(EXIT_FAILURE); }, Qt::QueuedConnection);
	engine.setInitialProperties({{u"session"_s, QVariant::fromValue(&session)}});
	engine.loadFromModule("RefGui", "Main");

	return QGuiApplication::exec();
}
