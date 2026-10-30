// A Backend destroyed while it is still connecting must end its coroutines: one that nobody
// resumes is never freed. Every case waits for the Backend to be in the middle of something, then
// destroys it. A leak shows under LeakSanitizer (the sanitizer build), a coroutine that runs on
// after its Backend as a crash there.
#include "client/backend.h"
#include "client/instance_files.h"
#include "client/spawn_lock.h"

#include <QDir>
#include <QFile>
#include <QSignalSpy>
#include <QTcpServer>
#include <QTemporaryDir>
#include <QtTest>

#include <memory>

using namespace Qt::StringLiterals;
using namespace refgui::client;

namespace
{

constexpr int StateTimeoutMs = 10000;
constexpr int AfterwardsMs = 1500; // longer than any pause of the connecting loop

// Puts `directory` first in PATH and points the state directory into it, as the launcher reads them.
class Environment
{
public:
	explicit Environment(const QString& directory)
		: m_path(qgetenv("PATH"))
		, m_state(qgetenv("REF_STATE_DIR"))
	{
		qputenv("PATH", (directory + u":"_s).toUtf8() + m_path);
		qputenv("REF_STATE_DIR", (directory + u"/state"_s).toUtf8());
	}

	~Environment()
	{
		qputenv("PATH", m_path);
		if(m_state.isNull())
		{
			qunsetenv("REF_STATE_DIR");
		}
		else
		{
			qputenv("REF_STATE_DIR", m_state);
		}
	}

private:
	QByteArray m_path;
	QByteArray m_state;
};

void writeServerScript(const QString& directory, const QString& body)
{
	QFile file(directory + u"/ref-server"_s);
	QVERIFY(file.open(QIODevice::WriteOnly));
	file.write((u"#!/bin/sh\n"_s + body).toUtf8());
	file.setPermissions(file.permissions() | QFileDevice::ExeOwner);
}

} // namespace

class TestBackendShutdown : public QObject
{
	Q_OBJECT

private slots:
	void whileTheBackendStarts();
	void whileTheMockStarts();
	void whileWaitingForTheSpawnLock();
	void whileConnecting();
};

#ifndef Q_OS_WIN

// ref-server that starts and never publishes an instance file: the launcher polls for it.
void TestBackendShutdown::whileTheBackendStarts()
{
	QTemporaryDir directory;
	QVERIFY(directory.isValid());
	writeServerScript(directory.path(), u"exit 0\n"_s);
	const Environment environment(directory.path());

	auto backend = std::make_unique<Backend>(Backend::Source(Backend::Mode::Shared{}), u"/work"_s);
	backend->start();
	QTRY_COMPARE_WITH_TIMEOUT(backend->state(), Backend::State::StartingBackend, StateTimeoutMs);

	backend.reset();
	QTest::qWait(AfterwardsMs);
}

// A mock backend that never announces its port: the process is polled.
void TestBackendShutdown::whileTheMockStarts()
{
	QTemporaryDir directory;
	QVERIFY(directory.isValid());
	writeServerScript(directory.path(), u"cat > /dev/null\n"_s); // leaves when its stdin ends
	const Environment environment(directory.path());

	auto backend = std::make_unique<Backend>(Backend::Source(Backend::Mode::Mock{}), u"/work"_s);
	backend->start();
	QTest::qWait(300);

	backend.reset();
	QTest::qWait(AfterwardsMs);
}

// Another frontend is starting the backend and holds the lock: the launcher polls for it.
void TestBackendShutdown::whileWaitingForTheSpawnLock()
{
	QTemporaryDir directory;
	QVERIFY(directory.isValid());
	const Environment environment(directory.path());
	QDir().mkpath(instancesDirectory());
	auto lock = SpawnLock::tryAcquire(instancesDirectory());
	QVERIFY(lock.has_value());

	auto backend = std::make_unique<Backend>(Backend::Source(Backend::Mode::Shared{}), u"/work"_s);
	backend->start();
	QTest::qWait(300);

	backend.reset();
	QTest::qWait(AfterwardsMs);
}

// The backend accepts the connection and never answers the handshake.
void TestBackendShutdown::whileConnecting()
{
	QTcpServer silent;
	QVERIFY(silent.listen(QHostAddress::LocalHost));
	QSignalSpy accepted(&silent, &QTcpServer::newConnection);

	auto backend = std::make_unique<Backend>(
		Backend::Source(Backend::Mode::Remote{{QUrl(u"ws://127.0.0.1:%1/"_s.arg(silent.serverPort())), u"token"_s}}), u"/work"_s);
	backend->start();
	QTRY_VERIFY_WITH_TIMEOUT(accepted.count() > 0, StateTimeoutMs);

	backend.reset();
	QTest::qWait(AfterwardsMs);
}

#else

void TestBackendShutdown::whileTheBackendStarts() { QSKIP("needs shell scripts"); }
void TestBackendShutdown::whileTheMockStarts() { QSKIP("needs shell scripts"); }
void TestBackendShutdown::whileWaitingForTheSpawnLock() { QSKIP("needs shell scripts"); }
void TestBackendShutdown::whileConnecting() { QSKIP("needs shell scripts"); }

#endif

QTEST_MAIN(TestBackendShutdown)
#include "tst_backend_shutdown.moc"
