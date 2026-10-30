#include "cli/command_line.h"

#include <QDir>
#include <QTemporaryDir>
#include <QTest>

using namespace Qt::StringLiterals;
using namespace refgui::cli;
using refgui::client::Backend;

namespace
{

const QString Version = u"1.2.3"_s;

ParseResult parse(const QStringList& arguments, const QString& currentDirectory)
{
	return parseCommandLine(QStringList{u"ref-gui"_s} + arguments, currentDirectory, Version);
}

}

class TestCommandLine : public QObject
{
	Q_OBJECT

private slots:
	void initTestCase();
	void defaultsToANewSessionInTheCurrentDirectory();
	void directoryIsCheckedAndMadeCanonical();
	void sessionBeatsContinueBeatsNew();
	void watchOnlyWarns();
	void readsAgentAndModel();
	void rejectsMalformedModels_data();
	void rejectsMalformedModels();
	void explicitServerCarriesTheToken();
	void rejectsBadServerOptions_data();
	void rejectsBadServerOptions();
	void mockUsesAPrivateBackend();
	void helpAndVersionExit();
	void unknownOptionIsAUsageError();
	void smokeIsHiddenFromHelp();

private:
	QTemporaryDir m_directory;
	QString m_root; // canonical
};

void TestCommandLine::initTestCase()
{
	QVERIFY(m_directory.isValid());
	QVERIFY(QDir(m_directory.path()).mkdir(u"sub"_s));
	m_root = QDir(m_directory.path()).canonicalPath();
}

void TestCommandLine::defaultsToANewSessionInTheCurrentDirectory()
{
	const ParseResult result = parse({}, m_root);

	const auto& options = std::get<LaunchOptions>(result);
	QCOMPARE(options.directory, m_root);
	QCOMPARE(options.start.kind, SessionStart::Kind::New);
	QVERIFY(std::holds_alternative<Backend::Mode::Shared>(options.source));
	QVERIFY(!options.smoke);
	QVERIFY(options.warnings.isEmpty());
}

void TestCommandLine::directoryIsCheckedAndMadeCanonical()
{
	const auto relative = std::get<LaunchOptions>(parse({u"sub/../sub"_s}, m_root));
	QCOMPARE(relative.directory, m_root + u"/sub"_s);

	const auto missing = std::get<Exit>(parse({u"nowhere"_s}, m_root));
	QCOMPARE(missing.code, 2);
	QVERIFY(missing.text.contains(u"nowhere: not a directory"_s));

	const auto twice = std::get<Exit>(parse({u"sub"_s, u"sub"_s}, m_root));
	QCOMPARE(twice.code, 2);
}

void TestCommandLine::sessionBeatsContinueBeatsNew()
{
	const auto continued = std::get<LaunchOptions>(parse({u"-c"_s}, m_root));
	QCOMPARE(continued.start.kind, SessionStart::Kind::Continue);

	const auto attached = std::get<LaunchOptions>(parse({u"-c"_s, u"--session"_s, u"ses_1"_s}, m_root));
	QCOMPARE(attached.start.kind, SessionStart::Kind::Attach);
	QCOMPARE(attached.start.sessionId, u"ses_1"_s);

	const auto longForm = std::get<LaunchOptions>(parse({u"--continue"_s}, m_root));
	QCOMPARE(longForm.start.kind, SessionStart::Kind::Continue);
}

void TestCommandLine::watchOnlyWarns()
{
	const auto options = std::get<LaunchOptions>(parse({u"-w"_s}, m_root));

	QCOMPARE(options.start.kind, SessionStart::Kind::New);
	QCOMPARE(options.warnings.size(), 1);
	QVERIFY(options.warnings.first().contains(u"split view is not available yet"_s));
}

void TestCommandLine::readsAgentAndModel()
{
	const auto options = std::get<LaunchOptions>(parse({u"--agent"_s, u"plan"_s, u"--model"_s, u"anthropic/claude-sonnet-4#high"_s}, m_root));

	QCOMPARE(options.start.agent, u"plan"_s);
	QVERIFY(options.start.model);
	QCOMPARE(options.start.model->provider, u"anthropic"_s);
	QCOMPARE(options.start.model->model, u"claude-sonnet-4"_s);
	QCOMPARE(options.start.model->variant, u"high"_s);

	const auto plain = std::get<LaunchOptions>(parse({u"--model"_s, u"openai/gpt-5"_s}, m_root));
	QVERIFY(!plain.start.model->variant);

	// The model name may itself contain slashes.
	const auto nested = std::get<LaunchOptions>(parse({u"--model"_s, u"router/meta/llama"_s}, m_root));
	QCOMPARE(nested.start.model->model, u"meta/llama"_s);
}

void TestCommandLine::rejectsMalformedModels_data()
{
	QTest::addColumn<QString>("model");
	QTest::newRow("no slash") << u"gpt"_s;
	QTest::newRow("no provider") << u"/gpt"_s;
	QTest::newRow("no model") << u"openai/"_s;
	QTest::newRow("only a variant") << u"#high"_s;
}

void TestCommandLine::rejectsMalformedModels()
{
	QFETCH(QString, model);

	const auto exit = std::get<Exit>(parse({u"--model"_s, model}, m_root));

	QCOMPARE(exit.code, 2);
	QVERIFY(exit.text.contains(u"invalid --model"_s));
}

void TestCommandLine::explicitServerCarriesTheToken()
{
	const auto options = std::get<LaunchOptions>(parse({u"--server"_s, u"ws://127.0.0.1:4242"_s, u"--token"_s, u"secret"_s}, m_root));

	const auto* remote = std::get_if<Backend::Mode::Remote>(&options.source);
	QVERIFY(remote);
	QCOMPARE(remote->endpoint.url.port(), 4242);
	QCOMPARE(remote->endpoint.token, u"secret"_s);

	const auto withoutToken = std::get<LaunchOptions>(parse({u"--server"_s, u"ws://host:1"_s}, m_root));
	QVERIFY(std::get<Backend::Mode::Remote>(withoutToken.source).endpoint.token.isEmpty());
}

void TestCommandLine::rejectsBadServerOptions_data()
{
	QTest::addColumn<QStringList>("arguments");
	QTest::newRow("not a websocket url") << QStringList{u"--server"_s, u"http://host:1"_s};
	QTest::newRow("no host") << QStringList{u"--server"_s, u"ws://"_s};
	QTest::newRow("token alone") << QStringList{u"--token"_s, u"t"_s};
	QTest::newRow("server with mock") << QStringList{u"--server"_s, u"ws://host:1"_s, u"--mock"_s};
}

void TestCommandLine::rejectsBadServerOptions()
{
	QFETCH(QStringList, arguments);

	const auto exit = std::get<Exit>(parse(arguments, m_root));

	QCOMPARE(exit.code, 2);
	QVERIFY(exit.text.startsWith(u"ref-gui: "_s));
}

void TestCommandLine::mockUsesAPrivateBackend()
{
	const auto options = std::get<LaunchOptions>(parse({u"--mock"_s}, m_root));

	QVERIFY(std::holds_alternative<Backend::Mode::Mock>(options.source));
}

void TestCommandLine::helpAndVersionExit()
{
	const auto help = std::get<Exit>(parse({u"-h"_s}, m_root));
	QCOMPARE(help.code, 0);
	for(const QString& option : {u"--continue"_s, u"--session"_s, u"--watch"_s, u"--agent"_s, u"--model"_s, u"--server"_s, u"--token"_s, u"--mock"_s})
	{
		QVERIFY2(help.text.contains(option), qPrintable(option));
	}

	const auto version = std::get<Exit>(parse({u"-V"_s}, m_root));
	QCOMPARE(version.code, 0);
	QCOMPARE(version.text, u"ref-gui 1.2.3\n"_s);
}

void TestCommandLine::unknownOptionIsAUsageError()
{
	const auto exit = std::get<Exit>(parse({u"--bogus"_s}, m_root));

	QCOMPARE(exit.code, 2);
	QVERIFY(exit.text.contains(u"bogus"_s));
}

void TestCommandLine::smokeIsHiddenFromHelp()
{
	const auto options = std::get<LaunchOptions>(parse({u"--smoke"_s}, m_root));
	QVERIFY(options.smoke);

	const auto help = std::get<Exit>(parse({u"--help"_s}, m_root));
	QVERIFY(!help.text.contains(u"smoke"_s));
}

QTEST_GUILESS_MAIN(TestCommandLine)
#include "tst_command_line.moc"
