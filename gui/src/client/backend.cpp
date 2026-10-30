#include "client/backend.h"

#include "client/instance_files.h"

#include <QCoroSignal>
#include <QCoroTimer>

using namespace Qt::StringLiterals;
using namespace std::chrono_literals;

namespace refgui::client
{
namespace
{

constexpr auto ReconnectPause = 1s;

} // namespace

Backend::Backend(Source source, QString cwd, QObject* parent)
	: QObject(parent)
	, m_source(std::move(source))
	, m_cwd(std::move(cwd))
	, m_client(this)
{
}

Backend::~Backend() = default;

void Backend::start()
{
	run();
}

void Backend::shutdown()
{
	m_stopping = true;
	m_client.close();
}

// Connect, wait for the connection to end, pause, repeat.
QCoro::Task<> Backend::run()
{
	while(!m_stopping)
	{
		setState(State::Connecting);
		const std::expected<Endpoint, QString> endpoint = co_await connectOnce();
		if(m_stopping)
		{
			co_return;
		}
		if(!endpoint)
		{
			setState(State::Disconnected, endpoint.error());
			co_await QCoro::sleepFor(ReconnectPause);
			continue;
		}

		setState(State::Connected);
		emit connected();
		const QString reason = co_await qCoro(&m_client, &ServerClient::closed);
		if(m_stopping)
		{
			co_return;
		}
		setState(State::Disconnected, reason);
		co_await QCoro::sleepFor(ReconnectPause);
	}
}

QCoro::Task<std::expected<Endpoint, QString>> Backend::connectOnce()
{
	if(std::holds_alternative<Mode::Mock>(m_source))
	{
		const std::expected<Endpoint, QString> mock = co_await startMock();
		if(!mock)
		{
			co_return std::unexpected(mock.error());
		}
		co_return co_await launcher::connectTarget(launcher::Explicit{*mock},
		                                           [this](const Endpoint& endpoint, std::chrono::milliseconds timeout) { return tryConnect(endpoint, timeout); },
		                                           [] {},
		                                           {});
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
		instancesDirectory());
}

QCoro::Task<bool> Backend::tryConnect(const Endpoint& endpoint, std::chrono::milliseconds timeout)
{
	const Result<proto::HelloResult> hello = co_await m_client.open(endpoint, m_cwd, timeout);
	if(hello)
	{
		m_hello = *hello;
	}
	co_return hello.has_value();
}

// The mock backend is started once and reused by reconnects, unless it died.
QCoro::Task<std::expected<Endpoint, QString>> Backend::startMock()
{
	if(m_mock && m_mock->isRunning())
	{
		co_return m_mock->endpoint();
	}
	m_mock.reset();
	auto mock = co_await MockBackend::start(launcher::serverBinary());
	if(!mock)
	{
		co_return std::unexpected(mock.error());
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
