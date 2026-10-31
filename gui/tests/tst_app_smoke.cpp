// Starts the real ref-gui (offscreen) against a real ref-server and checks that it opens a
// session. Nothing here calls an LLM. Everything runs in temporary state and data
// directories, and the only process stopped is the one this test started.
#include "client/instance_files.h"

#include <QDir>
#include <QProcess>
#include <QTemporaryDir>
#include <QtTest>

using namespace Qt::StringLiterals;
using namespace std::chrono_literals;
using namespace refgui;

namespace
{

constexpr int ProgramTimeoutMs = 60000;
constexpr int ServerStartTimeoutMs = 20000;
constexpr int ServerExitTimeoutMs = 10000;

struct Run
{
	int exitCode = -1;
	QString out;
	QString err;
	bool finished = false;
};

// Stops the server process a test started. On Windows QProcess::terminate() only posts WM_CLOSE,
// which a console program never receives, so the process is killed there.
bool stopServer(QProcess& server)
{
#ifdef Q_OS_WIN
	server.kill();
#else
	server.terminate();
#endif
	return server.waitForFinished(ServerExitTimeoutMs);
}

}

class TestAppSmoke : public QObject
{
	Q_OBJECT

private slots:
	void initTestCase();
	void mockBackendOpensASession();
	void explicitServerOpensASession();
	void explicitServerRejectsUnknownSession();
	void startsTheBackendItself();
	void helpAndVersion();
	void badArgumentsFail();

private:
	QProcessEnvironment environment() const;
	Run runGui(const QStringList& arguments) const;
	// Starts a ref-server of our own and waits until it has announced itself.
	std::optional<proto::InstanceInfo> startServer(QProcess& server) const;

	QTemporaryDir m_directory;
	QString m_work;
};

void TestAppSmoke::initTestCase()
{
	QVERIFY(m_directory.isValid());
	m_work = QDir(m_directory.path()).filePath(u"work"_s);
	QVERIFY(QDir().mkpath(m_work));
	QVERIFY(QFileInfo(QStringLiteral(REF_SERVER_BINARY)).isExecutable());
	QVERIFY(QFileInfo(QStringLiteral(REF_GUI_BINARY)).isExecutable());

	// This process looks for the server's instance files with the same variables.
	qputenv("REF_STATE_DIR", QDir(m_directory.path()).filePath(u"state"_s).toLocal8Bit());
	qputenv("REF_DATA_DIR", QDir(m_directory.path()).filePath(u"data"_s).toLocal8Bit());
}

QProcessEnvironment TestAppSmoke::environment() const
{
	QProcessEnvironment env = QProcessEnvironment::systemEnvironment();
	env.insert(u"QT_QPA_PLATFORM"_s, u"offscreen"_s);
	// The program looks for a desktop notification service on the session bus. A test must never reach the
	// bus of the user's desktop, where a notification would really show: this address leads nowhere.
	env.insert(u"DBUS_SESSION_BUS_ADDRESS"_s, u"unix:path="_s + QDir(m_directory.path()).filePath(u"no-bus"_s));
	// The offscreen platform's theme asks for the families "Sans Serif" and "monospace", which macOS does not have; Qt
	// then warns that building the alias table took long. A real desktop platform does not ask for them. Anything else
	// on stderr still fails the test.
	env.insert(u"QT_LOGGING_RULES"_s, u"qt.qpa.fonts.warning=false"_s);
	// ref-gui finds ref-server on PATH when it is not next to it.
	const QString serverDirectory = QFileInfo(QStringLiteral(REF_SERVER_BINARY)).absolutePath();
	env.insert(u"PATH"_s, serverDirectory + QDir::listSeparator() + env.value(u"PATH"_s));
	return env;
}

Run TestAppSmoke::runGui(const QStringList& arguments) const
{
	QProcess gui;
	gui.setProcessEnvironment(environment());
	gui.start(QStringLiteral(REF_GUI_BINARY), arguments);
	Run run;
	run.finished = gui.waitForFinished(ProgramTimeoutMs);
	if(!run.finished)
	{
		gui.kill(); // our own child
		gui.waitForFinished();
	}
	run.exitCode = gui.exitCode();
	run.out = QString::fromUtf8(gui.readAllStandardOutput());
	run.err = QString::fromUtf8(gui.readAllStandardError());
	return run;
}

std::optional<proto::InstanceInfo> TestAppSmoke::startServer(QProcess& server) const
{
	server.setProcessEnvironment(environment());
	server.setProgram(QStringLiteral(REF_SERVER_BINARY));
	server.setArguments({u"--port"_s, u"0"_s, u"--token"_s, u"smoke-token"_s});
	server.start();
	if(!server.waitForStarted())
	{
		return std::nullopt;
	}

	// The server writes its port to <state dir>/instances/<pid>.json.
	QElapsedTimer waited;
	waited.start();
	while(waited.elapsed() < ServerStartTimeoutMs)
	{
		for(const proto::InstanceInfo& candidate : client::liveInstances(client::instancesDirectory()))
		{
			if(candidate.pid == server.processId())
			{
				return candidate;
			}
		}
		QTest::qWait(50);
	}
	return std::nullopt;
}

void TestAppSmoke::mockBackendOpensASession()
{
	const Run run = runGui({u"--mock"_s, u"--smoke"_s, m_work});

	QVERIFY2(run.finished, "timed out");
	QCOMPARE(run.exitCode, 0);
	QVERIFY2(run.out.startsWith(u"attached ses_"_s), qPrintable(run.out));
	QVERIFY2(run.err.isEmpty(), qPrintable(run.err)); // no QML warnings either
}

void TestAppSmoke::explicitServerOpensASession()
{
	QProcess server;
	const std::optional<proto::InstanceInfo> instance = startServer(server);
	QVERIFY(instance);

	const QString url = u"ws://127.0.0.1:%1"_s.arg(instance->port);
	const Run opened = runGui({u"--server"_s, url, u"--token"_s, u"smoke-token"_s, u"--smoke"_s, m_work});
	QVERIFY2(opened.finished, "timed out");
	QCOMPARE(opened.exitCode, 0);
	QVERIFY2(opened.out.startsWith(u"attached ses_"_s), qPrintable(opened.out));
	QVERIFY2(opened.err.isEmpty(), qPrintable(opened.err));

	// A wrong token never gets in; the program says so instead of waiting for ever.
	const Run refused = runGui({u"--server"_s, url, u"--token"_s, u"wrong"_s, u"--smoke"_s, m_work});
	QVERIFY2(refused.finished, "timed out");
	QCOMPARE(refused.exitCode, 1);
	QVERIFY2(!refused.err.isEmpty(), "says nothing");

	QVERIFY(stopServer(server));
}

void TestAppSmoke::explicitServerRejectsUnknownSession()
{
	QProcess server;
	const std::optional<proto::InstanceInfo> instance = startServer(server);
	QVERIFY(instance);

	const Run run = runGui({u"--server"_s,
	                        u"ws://127.0.0.1:%1"_s.arg(instance->port),
	                        u"--token"_s,
	                        u"smoke-token"_s,
	                        u"-s"_s,
	                        u"ses_does_not_exist"_s,
	                        u"--smoke"_s,
	                        m_work});

	QVERIFY2(run.finished, "timed out");
	QCOMPARE(run.exitCode, 1);
	QVERIFY2(run.err.contains(u"ses_does_not_exist"_s), qPrintable(run.err));

	QVERIFY(stopServer(server));
}

// No --server: ref-gui starts a backend of its own, which leaves again with its last client.
void TestAppSmoke::startsTheBackendItself()
{
	const Run run = runGui({u"--smoke"_s, m_work});

	QVERIFY2(run.finished, "timed out");
	QCOMPARE(run.exitCode, 0);
	QVERIFY2(run.out.startsWith(u"attached ses_"_s), qPrintable(run.out));

	QTRY_VERIFY_WITH_TIMEOUT(client::liveInstances(client::instancesDirectory()).isEmpty(), ServerExitTimeoutMs);
}

void TestAppSmoke::helpAndVersion()
{
	const Run help = runGui({u"--help"_s});
	QCOMPARE(help.exitCode, 0);
	QVERIFY(help.out.contains(u"--server"_s));

	const Run version = runGui({u"-V"_s});
	QCOMPARE(version.exitCode, 0);
	QVERIFY(version.out.startsWith(u"ref-gui "_s));
}

void TestAppSmoke::badArgumentsFail()
{
	const Run missing = runGui({u"/this/does/not/exist"_s});
	QCOMPARE(missing.exitCode, 2);
	QVERIFY(missing.err.contains(u"not a directory"_s));

	const Run model = runGui({u"--model"_s, u"nonsense"_s});
	QCOMPARE(model.exitCode, 2);
}

QTEST_GUILESS_MAIN(TestAppSmoke)
#include "tst_app_smoke.moc"
