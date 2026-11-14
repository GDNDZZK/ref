#include "client/launcher.h"

#include "client/instance_files.h"
#include "client/spawn_lock.h"

#include <QCoreApplication>
#include <QDir>
#include <QLoggingCategory>
#include <QProcess>
#include <QRandomGenerator>
#include <QTcpServer>
#include <QtSystemDetection>

#ifdef Q_OS_WIN
#include <windows.h> // CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW
#endif

using namespace Qt::StringLiterals;
using namespace std::chrono_literals;

namespace refgui::client::launcher
{
namespace
{

Q_LOGGING_CATEGORY(lcLauncher, "refgui.client.launcher")

constexpr auto DiscoveredConnectTimeout = 1500ms; // an instance found through its file
constexpr auto ExplicitConnectTimeout = 3s;
constexpr auto SpawnLockTimeout = 15s; // how long to wait for a frontend that is starting the backend
constexpr auto SpawnLockPoll = 50ms;
constexpr auto BackendStartTimeout = 10s; // for the started backend to publish its instance file
constexpr auto StartingTick = 300ms;
constexpr auto InstancePoll = 50ms;
constexpr int NewBackendConnectAttempts = 20; // it may not accept connections yet
constexpr auto NewBackendConnectPause = 100ms;
constexpr int PortAttempts = 200;
constexpr quint16 FirstPort = 10000;
constexpr quint16 LastPort = 65535;

using Connected = std::expected<Endpoint, QString>;

Connected stoppedResult()
{
	return std::unexpected(u"stopped"_s);
}

// Connects to the newest live instance that accepts us. Empty when none did and no stop was requested.
QCoro::Task<std::optional<Connected>> connectExisting(const QString& directory, const TryConnect& tryConnect, const StopSource& stop)
{
	for(const proto::InstanceInfo& instance : liveInstances(directory))
	{
		const Endpoint endpoint = endpointFor(instance);
		const ConnectAttempt attempt = co_await tryConnect(endpoint, DiscoveredConnectTimeout);
		if(attempt)
		{
			co_return endpoint;
		}
		// Normal for an instance that is going away: the next one, or a new backend, is tried.
		qCDebug(lcLauncher) << "instance" << instance.pid << "on port" << instance.port << "refused:" << attempt.error();
	}
	if(stop.stopped())
	{
		co_return stoppedResult();
	}
	co_return std::nullopt;
}

// Waits for the lock, polling. Empty when it timed out or locking is impossible, in which
// case the caller carries on unlocked rather than hanging.
QCoro::Task<std::unique_ptr<SpawnLock>> acquireSpawnLock(const QString& directory, StopSource& stop)
{
	std::unique_ptr<SpawnLock> acquired;
	const auto lockedOrUnavailable = [&] {
		auto lock = SpawnLock::tryAcquire(directory);
		if(lock)
		{
			acquired = std::move(*lock);
			return true;
		}
		return lock.error() == SpawnLock::Failure::Unavailable;
	};
	co_await stop.pollUntil(SpawnLockTimeout, SpawnLockPoll, lockedOrUnavailable);
	co_return acquired;
}

QString startFailure(const QProcess& process)
{
	return u"failed to start %1: %2"_s.arg(serverBinary(), process.errorString());
}

// Starts a backend that outlives us, on `port`, and exits when its last client leaves.
std::expected<void, QString> spawnServer(quint16 port)
{
	QProcess process;
	process.setProgram(serverBinary());
	process.setArguments({u"--port"_s, QString::number(port), u"--idle-timeout"_s, u"0"_s});
	process.setStandardInputFile(QProcess::nullDevice());
	process.setStandardOutputFile(QProcess::nullDevice());
	process.setStandardErrorFile(QProcess::nullDevice());
#ifdef Q_OS_WIN
	// A new process group and no console window, like the TUI's launcher.
	process.setCreateProcessArgumentsModifier([](QProcess::CreateProcessArguments* arguments) {
		arguments->flags |= CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
	});
#endif
	if(!process.startDetached())
	{
		return std::unexpected(startFailure(process));
	}
	return {};
}

QCoro::Task<std::optional<proto::InstanceInfo>> waitForInstance(const QString& directory, quint16 port, std::chrono::milliseconds timeout, StopSource& stop)
{
	std::optional<proto::InstanceInfo> found;
	const auto published = [&] {
		for(const proto::InstanceInfo& instance : liveInstances(directory))
		{
			if(instance.port == port)
			{
				found = instance;
				return true;
			}
		}
		return false;
	};
	co_await stop.pollUntil(timeout, InstancePoll, published);
	co_return found;
}

// Waits for the backend we started on `port` to publish its instance file, then connects to it.
QCoro::Task<Connected> connectToStarted(const QString& directory, quint16 port, const TryConnect& tryConnect, const std::function<void()>& onStartingBackend, StopSource& stop)
{
	std::chrono::milliseconds waited = 0ms;
	std::optional<proto::InstanceInfo> instance = co_await waitForInstance(directory, port, StartingTick, stop);
	while(!instance)
	{
		if(stop.stopped())
		{
			co_return stoppedResult();
		}
		onStartingBackend();
		waited += StartingTick;
		if(waited > BackendStartTimeout)
		{
			co_return std::unexpected(u"backend did not start within %1s"_s.arg(std::chrono::duration_cast<std::chrono::seconds>(BackendStartTimeout).count()));
		}
		instance = co_await waitForInstance(directory, port, StartingTick, stop);
	}

	const Endpoint endpoint = endpointFor(*instance);
	QString lastReason;
	for(int attempt = 0; attempt < NewBackendConnectAttempts; ++attempt)
	{
		const ConnectAttempt connected = co_await tryConnect(endpoint, DiscoveredConnectTimeout);
		if(connected)
		{
			co_return endpoint;
		}
		lastReason = connected.error();
		if(stop.stopped())
		{
			co_return stoppedResult();
		}
		co_await stop.sleep(NewBackendConnectPause);
	}
	co_return std::unexpected(u"could not connect to the new backend: %1"_s.arg(lastReason));
}

// Starts a backend and connects to it. Only one frontend starts it; the others wait for
// the lock and then find its instance.
QCoro::Task<Connected> discover(QString directory, TryConnect tryConnect, std::function<void()> onStartingBackend, StopSource& stop)
{
	if(const std::optional<Connected> running = co_await connectExisting(directory, tryConnect, stop))
	{
		co_return *running;
	}

	const std::unique_ptr<SpawnLock> lock = co_await acquireSpawnLock(directory, stop);
	if(stop.stopped())
	{
		co_return stoppedResult();
	}
	if(const std::optional<Connected> startedMeanwhile = co_await connectExisting(directory, tryConnect, stop))
	{
		co_return *startedMeanwhile;
	}

	const quint16 port = pickFreePort();
	if(port == 0)
	{
		co_return std::unexpected(u"no free port"_s);
	}
	if(const std::expected<void, QString> spawned = spawnServer(port); !spawned)
	{
		co_return std::unexpected(spawned.error());
	}
	co_return co_await connectToStarted(directory, port, tryConnect, onStartingBackend, stop);
}

} // namespace

QCoro::Task<std::expected<Endpoint, QString>> connectTarget(Target target,
                                                            TryConnect tryConnect,
                                                            std::function<void()> onStartingBackend,
                                                            QString instancesDirectory,
                                                            StopSource& stop)
{
	if(const auto* explicitTarget = std::get_if<Explicit>(&target))
	{
		const ConnectAttempt attempt = co_await tryConnect(explicitTarget->endpoint, ExplicitConnectTimeout);
		if(attempt)
		{
			co_return explicitTarget->endpoint;
		}
		co_return std::unexpected(u"cannot connect to %1: %2"_s.arg(explicitTarget->endpoint.url.toString(), attempt.error()));
	}
	co_return co_await discover(std::move(instancesDirectory), std::move(tryConnect), std::move(onStartingBackend), stop);
}

QString serverBinary()
{
#ifdef Q_OS_WIN
	const QString fileName = u"ref-server.exe"_s;
#else
	const QString fileName = u"ref-server"_s;
#endif
	const QString sibling = QDir(QCoreApplication::applicationDirPath()).filePath(fileName);
	return QFile::exists(sibling) ? sibling : fileName;
}

quint16 pickFreePort()
{
	for(int attempt = 0; attempt < PortAttempts; ++attempt)
	{
		const auto port = static_cast<quint16>(QRandomGenerator::global()->bounded(quint32{FirstPort}, quint32{LastPort} + 1));
		QTcpServer probe;
		if(probe.listen(QHostAddress::LocalHost, port))
		{
			return port;
		}
	}
	return 0;
}

} // namespace refgui::client::launcher
