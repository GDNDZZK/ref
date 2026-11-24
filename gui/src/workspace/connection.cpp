#include "workspace/connection.h"

#include "client/endpoint.h"

#include <utility>
#include <variant>

using namespace Qt::StringLiterals;

namespace refgui::workspace
{
namespace
{

// The loopback names under which a backend runs on this machine.
bool isLoopback(const QString& host)
{
	return host == u"localhost"_s || host == client::LoopbackHost || host == u"::1"_s || host == u"[::1]"_s;
}

bool runsHere(const client::Backend::Source& source)
{
	const auto* remote = std::get_if<client::Backend::Mode::Remote>(&source);
	return remote == nullptr || isLoopback(remote->endpoint.url.host());
}

} // namespace

Connection::Connection(client::Backend::Source source, QString cwd, QObject* parent)
	: QObject(parent)
	, m_local(runsHere(source))
	, m_backend(std::move(source), std::move(cwd))
{
	connect(&m_backend, &client::Backend::stateChanged, this, &Connection::stateChanged);
	connect(&m_backend, &client::Backend::connected, this, &Connection::connected);
	connect(&m_backend.client(), &client::ServerClient::frameReceived, this, &Connection::onFrame);
}

void Connection::start()
{
\tm_backend.start();
}

void Connection::switchSource(client::Backend::Source source)
{
\tm_local = runsHere(source);
\tm_backend.switchSource(std::move(source));
\temit sourceChanged();
}

void Connection::shutdown()
{
\tm_backend.shutdown();
}

bool Connection::isProtocolOutdated() const
{
\treturn isConnected() \u0026\u0026 protocol() \u003c proto::ProtocolVersion;
}

QString Connection::backendUrl() const
{
\tif(const auto* remote = std::get_if\u003cclient::Backend::Mode::Remote\u003e(\u0026m_backend.source()))
\t{
\t\treturn remote-\u003eendpoint.url.toString();
\t}
\treturn {};
}

QString Connection::backendToken() const
{
\tif(const auto* remote = std::get_if\u003cclient::Backend::Mode::Remote\u003e(\u0026m_backend.source()))
\t{
\t\treturn remote-\u003eendpoint.token;
\t}
\treturn {};
}

QString Connection::backendMode() const
{
\tif(std::holds_alternative\u003cclient::Backend::Mode::Remote\u003e(m_backend.source()))
\t{
\t\treturn u\"remote\"_s;
\t}
\tif(std::holds_alternative\u003cclient::Backend::Mode::Mock\u003e(m_backend.source()))
\t{
\t\treturn u\"mock\"_s;
\t}
\treturn u\"auto\"_s;
}

QCoro::Task\u003cclient::Result\u003cvoid\u003e\u003e Connection::stopServer()
{
\treturn m_backend.client().shutdown();
}
void Connection::onFrame(const proto::ServerFrame& frame)
{
	if(const auto* toast = std::get_if<proto::ToastFrame>(&frame))
	{
		emit toastReceived(*toast);
	}
	else if(const auto* changed = std::get_if<proto::ConfigChangedFrame>(&frame))
	{
		emit configChangedReceived(changed->warnings);
	}
}

} // namespace refgui::workspace
