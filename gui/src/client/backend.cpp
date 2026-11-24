#include "client/backend.h"

#include "client/instance_files.h"

#include <utility>

using namespace Qt::StringLiterals;
using namespace std::chrono_literals;

namespace refgui::client
{
namespace
{

constexpr auto ReconnectPause = 1s;

// Waits for the client's `closed` signal and resumes the coroutine inside the emission.
// QCoro's signal awaiter resumes through the event loop instead, and that event could
// outlive a Backend destroyed in between.
struct ClosedAwaiter
{
\tBackend\u0026 backend;
\tServerClient\u0026 client;
\tQString reason{};
\tQMetaObject::Connection clientConn{};
\tQMetaObject::Connection cancelConn{};

\tbool await_ready() const noexcept { return false; }

\tvoid await_suspend(std::coroutine_handle\u003c\u003e waiter)
\t{
\t\tclientConn = QObject::connect(
\t\t\t\u0026client,
\t\t\t\u0026ServerClient::closed,
\t\t\t\u0026client,
\t\t\t[this, waiter](const QString\u0026 why) {
\t\t\t\tQObject::disconnect(cancelConn);
\t\t\t\treason = why;
\t\t\t\twaiter.resume();
\t\t\t},
\t\t\tQt::SingleShotConnection);
\t\tcancelConn = QObject::connect(
\t\t\t\u0026backend,
\t\t\t\u0026Backend::loopCancelled,
\t\t\t\u0026backend,
\t\t\t[this, waiter] {
\t\t\t\tQObject::disconnect(clientConn);
\t\t\t\treason = ConnectionClosed;
\t\t\t\twaiter.resume();
\t\t\t},
\t\t\tQt::SingleShotConnection);
\t}

\tQString await_resume() { return std::move(reason); }
};

} // namespace

Backend::Backend(Source source, QString cwd, QObject* parent)
\t: QObject(parent)
\t, m_source(std::move(source))
\t, m_cwd(std::move(cwd))
\t, m_client(this)
\t, m_stop(std::make_shared\u003cStopSource\u003e())
{
}

Backend::~Backend()
{
\tshutdown();
}

void Backend::start()
{
\tif(m_started || m_stopping)
\t{
\t\treturn;
\t}
\tm_started = true;
\trun(m_generation, m_stop);
}

void Backend::switchSource(Source source)
{
\t++m_generation;
\tm_source = std::move(source);
\tif(m_stop)
\t{
\t\tm_stop-\u003erequest();
\t}
\tm_client.close();
\temit loopCancelled();
\tm_mock.reset();
\tm_hello = {};
\tm_stop = std::make_shared\u003cStopSource\u003e();
\tif(m_started \u0026\u0026 !m_stopping)
\t{
\t\tsetState(State::Connecting);
\t\trun(m_generation, m_stop);
\t}
}

void Backend::shutdown()
{
\tm_stopping = true;
\t++m_generation;
\tif(m_stop)
\t{
\t\tm_stop-\u003erequest();
\t}
\tm_client.close();
\temit loopCancelled();
}

// Connect, wait for the connection to end, pause, repeat.
QCoro::Task\u003c\u003e Backend::run(quint64 generation, std::shared_ptr\u003cStopSource\u003e stop)
{
\twhile(!m_stopping \u0026\u0026 generation == m_generation)
\t{
\t\tsetState(State::Connecting);
\t\tconst std::expected\u003cEndpoint, QString\u003e endpoint = co_await connectOnce(*stop);
\t\tif(m_stopping || generation != m_generation)
\t\t{
\t\t\tco_return;
\t\t}
\t\tif(!endpoint)
\t\t{
\t\t\tsetState(State::Disconnected, endpoint.error());
\t\t\tco_await stop-\u003esleep(ReconnectPause);
\t\t\tcontinue;
\t\t}

\t\tsetState(State::Connected);
\t\temit connected();
\t\tif(m_stopping || generation != m_generation)
\t\t{
\t\t\tco_return;
\t\t}
\t\tconst QString reason = co_await ClosedAwaiter{*this, m_client};
\t\tif(m_stopping || generation != m_generation)
\t\t{
\t\t\tco_return;
\t\t}
\t\tsetState(State::Disconnected, reason);
\t\tco_await stop-\u003esleep(ReconnectPause);
\t}
}

QCoro::Task\u003cstd::expected\u003cEndpoint, QString\u003e\u003e Backend::connectOnce(StopSource\u0026 stop)
{
\tif(std::holds_alternative\u003cMode::Mock\u003e(m_source))
\t{
\t\tconst std::expected\u003cEndpoint, QString\u003e mock = co_await startMock(stop);
\t\tif(!mock)
\t\t{
\t\t\tco_return std::unexpected(mock.error());
\t\t}
\t\tco_return co_await launcher::connectTarget(launcher::Explicit{*mock},
\t\t                                           [this](const Endpoint\u0026 endpoint, std::chrono::milliseconds timeout) { return tryConnect(endpoint, timeout); },
\t\t                                           [] {},
\t\t                                           {},
\t\t                                           stop);
\t}

\tlauncher::Target target = launcher::Auto{};
\tif(const auto* remote = std::get_if\u003cMode::Remote\u003e(\u0026m_source))
\t{
\t\ttarget = launcher::Explicit{remote-\u003eendpoint};
\t}
\tco_return co_await launcher::connectTarget(
\t\ttarget,
\t\t[this](const Endpoint\u0026 endpoint, std::chrono::milliseconds timeout) { return tryConnect(endpoint, timeout); },
\t\t[this] { setState(State::StartingBackend); },
\t\tinstancesDirectory(),
\t\tstop);
}

QCoro::Task\u003clauncher::ConnectAttempt\u003e Backend::tryConnect(const Endpoint\u0026 endpoint, std::chrono::milliseconds timeout)
{
\tm_helloNotifyKinds = m_notifyKinds;
\tconst Result\u003cproto::HelloResult\u003e hello = co_await m_client.open(endpoint, m_cwd, timeout, m_helloNotifyKinds);
\tif(!hello)
\t{
\t\tco_return std::unexpected(hello.error());
\t}
\tm_hello = *hello;
\tco_return launcher::ConnectAttempt();
}

// The mock backend is started once and reused by reconnects, unless it died.
QCoro::Task\u003cstd::expected\u003cEndpoint, QString\u003e\u003e Backend::startMock(StopSource\u0026 stop)
{
\tif(m_mock \u0026\u0026 m_mock-\u003eisRunning())
\t{
\t\tco_return m_mock-\u003eendpoint();
\t}
\tm_mock.reset();
\tauto mock = co_await MockBackend::start(launcher::serverBinary(), stop);
\tif(!mock)
\t{
\t\tco_return std::unexpected(mock.error());
\t}
\tm_mock = std::move(*mock);
\tco_return m_mock-\u003eendpoint();
}
void Backend::setState(State state, QString reason)
{
	const bool unchanged = state == m_state && reason == m_reason;
	m_state = state;
	m_reason = std::move(reason);
	if(!unchanged)
	{
		emit stateChanged();
	}
}

} // namespace refgui::client
