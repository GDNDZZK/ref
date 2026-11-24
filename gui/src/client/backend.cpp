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
	Backend& backend;
	ServerClient& client;
	QString reason{};
	QMetaObject::Connection clientConn{};
	QMetaObject::Connection cancelConn{};

	bool await_ready() const noexcept { return false; }

	void await_suspend(std::coroutine_handle<> waiter)
	{
		clientConn = QObject::connect(
			&client,
			&ServerClient::closed,
			&client,
			[this, waiter](const QString& why) {
				QObject::disconnect(cancelConn);
				reason = why;
				waiter.resume();
			},
			Qt::SingleShotConnection);
		cancelConn = QObject::connect(
			&backend,
			&Backend::loopCancelled,
			&backend,
			[this, waiter] {
				QObject::disconnect(clientConn);
				reason = ConnectionClosed;
				waiter.resume();
			},
			Qt::SingleShotConnection);
	}

	QString await_resume() { return std::move(reason); }
};

} // namespace

Backend::Backend(Source source, QString cwd, QObject* parent)
	: QObject(parent)
	, m_source(std::move(source))
	, m_cwd(std::move(cwd))
	, m_client(this)
	, m_stop(std::make_shared<StopSource>())
{
}

Backend::~Backend()
{
	shutdown();
}

void Backend::start()
{
	if(m_started || m_stopping)
	{
		return;
	}
	m_started = true;
	run(m_generation, m_stop);
}

void Backend::switchSource(Source source)
{
	++m_generation;
	m_source = std::move(source);
	if(m_stop)
	{
		m_stop->request();
	}
	m_client.close();
	emit loopCancelled();
	m_mock.reset();
	m_hello = {};
	m_stop = std::make_shared<StopSource>();
	if(m_started && !m_stopping)
	{
		setState(State::Connecting);
		run(m_generation, m_stop);
	}
}

void Backend::shutdown()
{
	m_stopping = true;
	++m_generation;
	if(m_stop)
	{
		m_stop->request();
	}
	m_client.close();
	emit loopCancelled();
}

// Connect, wait for the connection to end, pause, repeat.
QCoro::Task<> Backend::run(quint64 generation, std::shared_ptr<StopSource> stop)
{
	while(!m_stopping && generation == m_generation)
	{
		setState(State::Connecting);
		const std::expected<Endpoint, QString> endpoint = co_await connectOnce(generation, *stop);
		if(m_stopping || generation != m_generation)
		{
			co_return;
		}
		if(!endpoint)
		{
			setState(State::Disconnected, endpoint.error());
			co_await stop->sleep(ReconnectPause);
			continue;
		}

		setState(State::Connected);
		emit connected();
		if(m_stopping || generation != m_generation)
		{
			co_return;
		}
		const QString reason = co_await ClosedAwaiter{*this, m_client};
		if(m_stopping || generation != m_generation)
		{
			co_return;
		}
		setState(State::Disconnected, reason);
		co_await stop->sleep(ReconnectPause);
	}
}

QCoro::Task<std::expected<Endpoint, QString>> Backend::connectOnce(quint64 generation, StopSource& stop)
{
	if(m_stopping || stop.stopped() || generation != m_generation)
	{
		co_return std::unexpected(ConnectionClosed);
	}
	if(std::holds_alternative<Mode::Mock>(m_source))
	{
		const std::expected<Endpoint, QString> mock = co_await startMock(generation, stop);
		if(!mock)
		{
			co_return std::unexpected(mock.error());
		}
		if(m_stopping || stop.stopped() || generation != m_generation)
		{
			co_return std::unexpected(ConnectionClosed);
		}
		co_return co_await launcher::connectTarget(launcher::Explicit{*mock},
		                                           [this](const Endpoint& endpoint, std::chrono::milliseconds timeout) { return tryConnect(endpoint, timeout); },
		                                           [] {},
		                                           {},
		                                           stop);
	}

	launcher::Target target = launcher::Auto{};
	if(const auto* remote = std::get_if<Mode::Remote>(&m_source))
	{
		target = launcher::Explicit{remote->endpoint};
	}
	co_return co_await launcher::connectTarget(
		target,
		[this](const Endpoint& endpoint, std::chrono::milliseconds timeout) { return tryConnect(endpoint, timeout); },
		[this] { setState(State::StartingBackend); },
		instancesDirectory(),
		stop);
}

QCoro::Task<launcher::ConnectAttempt> Backend::tryConnect(const Endpoint& endpoint, std::chrono::milliseconds timeout)
{
	m_helloNotifyKinds = m_notifyKinds;
	const Result<proto::HelloResult> hello = co_await m_client.open(endpoint, m_cwd, timeout, m_helloNotifyKinds);
	if(!hello)
	{
		co_return std::unexpected(hello.error());
	}
	m_hello = *hello;
	co_return launcher::ConnectAttempt();
}

// The mock backend is started once and reused by reconnects, unless it died.
QCoro::Task<std::expected<Endpoint, QString>> Backend::startMock(quint64 generation, StopSource& stop)
{
	if(m_mock && m_mock->isRunning())
	{
		co_return m_mock->endpoint();
	}
	m_mock.reset();
	auto mock = co_await MockBackend::start(launcher::serverBinary(), stop);
	if(!mock)
	{
		co_return std::unexpected(mock.error());
	}
	if(m_stopping || stop.stopped() || generation != m_generation)
	{
		co_return std::unexpected(ConnectionClosed);
	}
	m_mock = std::move(*mock);
	co_return m_mock->endpoint();
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
