#include "client/launcher.h"

#include "client/instance_files.h"
#include "client/spawn_lock.h"

#include <QCoreApplication>
#include <QDir>
#include <QProcess>
#include <QRandomGenerator>
#include <QTcpServer>
#include <QtSystemDetection>

#include <QCoroTimer>

using namespace Qt::StringLiterals;
using namespace std::chrono_literals;

namespace refgui::client::launcher
{
namespace
{

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

// Connects to the newest live instance that accepts us.
QCoro::Task<std::optional<Endpoint>> connectExisting(const QString& directory, const TryConnect& tryConnect)
{
	for(const proto::InstanceInfo& instance : liveInstances(directory))
	{
		const Endpoint endpoint = endpointFor(instance);
		if(co_await tryConnect(endpoint, DiscoveredConnectTimeout))
		{
			co_return endpoint;
		}
	}
	co_return std::nullopt;
}

// Waits for the lock, polling. Empty when it timed out or locking is impossible, in which
// case the caller carries on unlocked rather than hanging.
QCoro::Task<std::unique_ptr<SpawnLock>> acquireSpawnLock(const QString& directory)
{
	std::chrono::milliseconds waited = 0ms;
	while(true)
	{
		auto lock = SpawnLock::tryAcquire(directory);
		if(lock)
		{
			co_return std::move(*lock);
		}
		if(lock.error() == SpawnLock::Failure::Unavailable || waited > SpawnLockTimeout)
		{
			co_return nullptr;
		}
		co_await QCoro::sleepFor(SpawnLockPoll);
		waited += SpawnLockPoll;
	}
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

QCoro::Task<std::optional<proto::InstanceInfo>> waitForInstance(const QString& directory, quint16 port, std::chrono::milliseconds timeout)
{
	std::chrono::milliseconds waited = 0ms;
	while(waited < timeout)
	{
		for(const proto::InstanceInfo& instance : liveInstances(directory))
		{
			if(instance.port == port)
			{
				co_return instance;
			}
		}
		co_await QCoro::sleepFor(InstancePoll);
		waited += InstancePoll;
	}
	co_return std::nullopt;
}

// Starts a backend and connects to it. Only one frontend starts it; the others wait for
// the lock and then find its instance.
QCoro::Task<Connected> discover(QString directory, TryConnect tryConnect, std::function<void()> onStartingBackend)
{
	if(const std::optional<Endpoint> running = co_await connectExisting(directory, tryConnect))
	{
		co_return *running;
	}

	const std::unique_ptr<SpawnLock> lock = co_await acquireSpawnLock(directory);
	if(const std::optional<Endpoint> startedMeanwhile = co_await connectExisting(directory, tryConnect))
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

	std::chrono::milliseconds waited = 0ms;
	std::optional<proto::InstanceInfo> instance;
	while(!(instance = co_await waitForInstance(directory, port, StartingTick)))
	{
		onStartingBackend();
		waited += StartingTick;
		if(waited > BackendStartTimeout)
		{
			co_return std::unexpected(u"backend did not start within %1s"_s.arg(std::chrono::duration_cast<std::chrono::seconds>(BackendStartTimeout).count()));
		}
	}

	const Endpoint endpoint = endpointFor(*instance);
	for(int attempt = 0; attempt < NewBackendConnectAttempts; ++attempt)
	{
		if(co_await tryConnect(endpoint, DiscoveredConnectTimeout))
		{
			co_return endpoint;
		}
		co_await QCoro::sleepFor(NewBackendConnectPause);
	}
	co_return std::unexpected(u"could not connect to the new backend"_s);
}

} // namespace

QCoro::Task<std::expected<Endpoint, QString>> connectTarget(Target target,
                                                            TryConnect tryConnect,
                                                            std::function<void()> onStartingBackend,
                                                            QString instancesDirectory)
{
	if(const auto* explicitTarget = std::get_if<Explicit>(&target))
	{
		if(co_await tryConnect(explicitTarget->endpoint, ExplicitConnectTimeout))
		{
			co_return explicitTarget->endpoint;
		}
		co_return std::unexpected(u"cannot connect to %1"_s.arg(explicitTarget->endpoint.url.toString()));
	}
	co_return co_await discover(std::move(instancesDirectory), std::move(tryConnect), std::move(onStartingBackend));
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
