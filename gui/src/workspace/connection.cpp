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
	m_backend.start();
}

void Connection::shutdown()
{
	m_backend.shutdown();
}

bool Connection::isProtocolOutdated() const
{
	return isConnected() && protocol() < proto::ProtocolVersion;
}

QCoro::Task<client::Result<void>> Connection::stopServer()
{
	return m_backend.client().shutdown();
}

void Connection::onFrame(const proto::ServerFrame& frame)
{
	if(const auto* toast = std::get_if<proto::ToastFrame>(&frame))
	{
		emit toastReceived(*toast);
	}
}

} // namespace refgui::workspace
