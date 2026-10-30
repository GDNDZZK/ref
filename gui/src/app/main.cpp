#include "app/chat_session.h"

#include <QCommandLineParser>
#include <QGuiApplication>
#include <QQmlApplicationEngine>

#include <cstdlib>

using namespace Qt::StringLiterals;

int main(int argc, char* argv[])
{
	QGuiApplication app(argc, argv);
	QGuiApplication::setApplicationName(u"qutie"_s);

	QCommandLineParser parser;
	parser.addHelpOption();
	const QCommandLineOption mockOption(u"mock"_s, u"Reply with canned scripts instead of the configured model."_s);
	parser.addOption(mockOption);
	parser.process(app);

	// Outlives the engine, whose window refers to it.
	ChatSession session(parser.isSet(mockOption) ? ChatSession::Replies::FromMock : ChatSession::Replies::FromModel);

	QQmlApplicationEngine engine;
	QObject::connect(&engine, &QQmlApplicationEngine::objectCreationFailed, &app, [] { QCoreApplication::exit(EXIT_FAILURE); }, Qt::QueuedConnection);
	engine.setInitialProperties({{u"session"_s, QVariant::fromValue(&session)}});
	engine.loadFromModule("Qutie", "Main");

	return QGuiApplication::exec();
}
