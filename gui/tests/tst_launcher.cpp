#include "client/instance_files.h"
#include "client/launcher.h"
#include "client/mock_backend.h"
#include "client/spawn_lock.h"
#include "coro_test.h"

#include <QCoreApplication>
#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QTcpServer>
#include <QTemporaryDir>
#include <QtTest>

using namespace Qt::StringLiterals;
using namespace std::chrono_literals;
using namespace refgui::client;
using refgui::proto::InstanceInfo;
using refgui::test::await;

namespace
{

constexpr quint32 DeadPid = 0x7ffffff0; // above any pid the kernel hands out

void writeInstance(const QString& directory, const QString& name, quint32 pid, quint16 port, qint64 startedAt, const QString& bind = u"127.0.0.1"_s)
{
	const QJsonObject instance{{u"pid"_s, static_cast<qint64>(pid)},
	                           {u"port"_s, port},
	                           {u"bind"_s, bind},
	                           {u"allow_hosts"_s, QJsonArray()},
	                           {u"idle_timeout"_s, 0},
	                           {u"started_at"_s, startedAt},
	                           {u"version"_s, u"0.0.0"_s},
	                           {u"token"_s, u"tok%1"_s.arg(port)}};
	QFile file(QDir(directory).filePath(name + u".json"_s));
	QVERIFY(file.open(QIODevice::WriteOnly));
	file.write(QJsonDocument(instance).toJson());
}

// Puts an environment variable back the way it was.
class EnvironmentGuard
{
public:
	explicit EnvironmentGuard(const char* name)
		: m_name(name)
		, m_wasSet(qEnvironmentVariableIsSet(name))
		, m_value(qgetenv(name))
	{
	}
	~EnvironmentGuard()
	{
		if(m_wasSet)
		{
			qputenv(m_name, m_value);
		}
		else
		{
			qunsetenv(m_name);
		}
	}

private:
	const char* m_name;
	bool m_wasSet;
	QByteArray m_value;
};

} // namespace

class TestLauncher : public QObject
{
	Q_OBJECT

private slots:
	void stateDirectoryFollowsTheSameRulesAsTheBackend()
	{
		const EnvironmentGuard stateDir("REF_STATE_DIR");
		const EnvironmentGuard xdgState("XDG_STATE_HOME");
		const EnvironmentGuard home("HOME");

		qputenv("REF_STATE_DIR", "/explicit/state");
		qputenv("XDG_STATE_HOME", "/xdg/state");
		QCOMPARE(stateDirectory(), u"/explicit/state"_s);
		QCOMPARE(instancesDirectory(), u"/explicit/state/instances"_s);

		qunsetenv("REF_STATE_DIR");
		QCOMPARE(stateDirectory(), u"/xdg/state/ref"_s);

		qunsetenv("XDG_STATE_HOME");
		qputenv("HOME", "/home/someone");
		QCOMPARE(stateDirectory(), u"/home/someone/.local/state/ref"_s);
	}

	void endpointsAreLoopbackForWildcardBinds_data()
	{
		QTest::addColumn<QString>("bind");
		QTest::addColumn<QString>("url");
		QTest::newRow("loopback") << u"127.0.0.1"_s << u"ws://127.0.0.1:4000/"_s;
		QTest::newRow("any ipv4") << u"0.0.0.0"_s << u"ws://127.0.0.1:4000/"_s;
		QTest::newRow("any ipv6") << u"::"_s << u"ws://127.0.0.1:4000/"_s;
		QTest::newRow("bracketed any ipv6") << u"[::]"_s << u"ws://127.0.0.1:4000/"_s;
		QTest::newRow("empty") << u""_s << u"ws://127.0.0.1:4000/"_s;
		QTest::newRow("ipv6 loopback") << u"::1"_s << u"ws://[::1]:4000/"_s;
		QTest::newRow("host name") << u"example.test"_s << u"ws://example.test:4000/"_s;
	}
	void endpointsAreLoopbackForWildcardBinds()
	{
		QFETCH(QString, bind);
		QFETCH(QString, url);
		const Endpoint endpoint = endpointFor({.pid = 1, .port = 4000, .bind = bind, .startedAt = 0, .token = u"t"_s});
		QCOMPARE(endpoint.url.toString(), url);
		QCOMPARE(endpoint.token, u"t"_s);
	}

	void liveInstancesAreNewestFirstAndDeadOnesAreRemoved()
	{
		QTemporaryDir directory;
		const auto me = static_cast<quint32>(QCoreApplication::applicationPid());
		writeInstance(directory.path(), u"old"_s, me, 4001, 100);
		writeInstance(directory.path(), u"new"_s, me, 4002, 200);
		writeInstance(directory.path(), u"dead"_s, DeadPid, 4003, 300);
		QFile garbage(directory.filePath(u"garbage.json"_s));
		QVERIFY(garbage.open(QIODevice::WriteOnly));
		garbage.write("{not json");
		garbage.close();

		const QList<InstanceInfo> live = liveInstances(directory.path());

		QCOMPARE(live.size(), 2);
		QCOMPARE(live.at(0).port, quint16{4002});
		QCOMPARE(live.at(1).port, quint16{4001});
		QVERIFY(!QFile::exists(directory.filePath(u"dead.json"_s)));
		QVERIFY(QFile::exists(directory.filePath(u"garbage.json"_s)));
	}

	void liveInstancesOfAMissingDirectoryIsEmpty()
	{
		QVERIFY(liveInstances(u"/nonexistent/ref/instances"_s).isEmpty());
	}

	void spawnLockIsExclusive()
	{
		QTemporaryDir directory;
		auto first = SpawnLock::tryAcquire(directory.path());
		QVERIFY(first.has_value());

		const auto second = SpawnLock::tryAcquire(directory.path());
		QVERIFY(!second.has_value());
		QCOMPARE(second.error(), SpawnLock::Failure::HeldElsewhere);

		first->reset();
		QVERIFY(SpawnLock::tryAcquire(directory.path()).has_value());
	}

	void spawnLockInAnUnwritablePlaceIsUnavailable()
	{
		QTemporaryDir directory;
		QFile blocker(directory.filePath(u"file"_s));
		QVERIFY(blocker.open(QIODevice::WriteOnly));
		blocker.close();

		const auto lock = SpawnLock::tryAcquire(directory.filePath(u"file/inside"_s)); // under a regular file
		QVERIFY(!lock.has_value());
		QCOMPARE(lock.error(), SpawnLock::Failure::Unavailable);
	}

	void freePortIsInRangeAndBindable()
	{
		const quint16 port = launcher::pickFreePort();
		QVERIFY(port >= 10000);
		QTcpServer server;
		QVERIFY(server.listen(QHostAddress::LocalHost, port));
	}

	void explicitTargetConnectsOnlyToThatEndpoint()
	{
		QList<QUrl> tried;
		const launcher::TryConnect connect = [&](const Endpoint& endpoint, std::chrono::milliseconds) -> QCoro::Task<bool> {
			tried << endpoint.url;
			co_return true;
		};
		const Endpoint wanted{.url = QUrl(u"ws://remote.test:9/"_s), .token = u"t"_s};

		const auto connected = await(launcher::connectTarget(launcher::Explicit{wanted}, connect, [] {}, {})).value();

		QVERIFY(connected.has_value());
		QCOMPARE(connected->url, wanted.url);
		QCOMPARE(tried, QList<QUrl>{wanted.url});
	}

	void explicitTargetThatRefusesIsAnError()
	{
		const launcher::TryConnect refuse = [](const Endpoint&, std::chrono::milliseconds) -> QCoro::Task<bool> { co_return false; };

		const auto connected = await(launcher::connectTarget(launcher::Explicit{{.url = QUrl(u"ws://remote.test:9/"_s)}}, refuse, [] {}, {})).value();

		QVERIFY(!connected.has_value());
		QVERIFY2(connected.error().contains(u"ws://remote.test:9/"_s), qPrintable(connected.error()));
	}

	void autoTargetTriesTheNewestInstanceFirstAndSkipsOnesThatRefuse()
	{
		QTemporaryDir directory;
		const auto me = static_cast<quint32>(QCoreApplication::applicationPid());
		writeInstance(directory.path(), u"a"_s, me, 4001, 100);
		writeInstance(directory.path(), u"b"_s, me, 4002, 200);
		writeInstance(directory.path(), u"c"_s, me, 4003, 300);
		QList<quint16> tried;
		const launcher::TryConnect connect = [&](const Endpoint& endpoint, std::chrono::milliseconds) -> QCoro::Task<bool> {
			tried << static_cast<quint16>(endpoint.url.port());
			co_return endpoint.url.port() == 4002; // the newest one is not listening any more
		};

		const auto connected = await(launcher::connectTarget(launcher::Auto{}, connect, [] {}, directory.path())).value();

		QVERIFY(connected.has_value());
		QCOMPARE(connected->url.port(), 4002);
		QCOMPARE(connected->token, u"tok4002"_s);
		QCOMPARE(tried, (QList<quint16>{4003, 4002}));
	}

	void announcementGivesTheEndpoint()
	{
		const auto endpoint = MockBackend::parseAnnouncement(R"({"port":41873,"token":"abc"})" "\n");
		QVERIFY(endpoint.has_value());
		QCOMPARE(endpoint->url.toString(), u"ws://127.0.0.1:41873/"_s);
		QCOMPARE(endpoint->token, u"abc"_s);
	}

	void announcementThatIsNotOneIsAnError()
	{
		QVERIFY(!MockBackend::parseAnnouncement("listening on 41873\n").has_value());
		QVERIFY(!MockBackend::parseAnnouncement(R"({"port":0,"token":"abc"})").has_value());
	}

#ifdef Q_OS_UNIX
	void mockBackendIsStartedAndKilledWithItsObject()
	{
		QTemporaryDir directory;
		const QString script = writeScript(directory, u"announce"_s, u"echo '{\"port\":41873,\"token\":\"abc\"}'\nexec cat >/dev/null\n"_s);

		auto started = await(MockBackend::start(script)).value();

		QVERIFY(started.has_value());
		QCOMPARE((*started)->endpoint().url.port(), 41873);
		QVERIFY((*started)->isRunning());
	}

	void mockBackendThatExitsEarlyIsAnError()
	{
		QTemporaryDir directory;
		const QString script = writeScript(directory, u"quit"_s, u"exit 3\n"_s);

		const auto started = await(MockBackend::start(script)).value();

		QVERIFY(!started.has_value());
		QVERIFY2(started.error().contains(u"exited before announcing"_s), qPrintable(started.error()));
	}

	void mockBackendThatCannotStartIsAnError()
	{
		const auto started = await(MockBackend::start(u"/nonexistent/ref-server"_s)).value();
		QVERIFY(!started.has_value());
		QVERIFY(started.error().contains(u"/nonexistent/ref-server"_s));
	}

private:
	static QString writeScript(const QTemporaryDir& directory, const QString& name, const QString& body)
	{
		const QString path = directory.filePath(name);
		QFile file(path);
		if(!file.open(QIODevice::WriteOnly))
		{
			qFatal("cannot write %s", qPrintable(path));
		}
		file.write((u"#!/bin/sh\n"_s + body).toUtf8());
		file.setPermissions(file.permissions() | QFileDevice::ExeOwner);
		return path;
	}
#endif
};

QTEST_GUILESS_MAIN(TestLauncher)
#include "tst_launcher.moc"
