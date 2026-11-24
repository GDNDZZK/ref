#pragma once

#include "client/endpoint.h"
#include "client/launcher.h"
#include "client/mock_backend.h"
#include "client/server_client.h"
#include "client/stop_source.h"

#include <QCoroTask>
#include <QObject>
#include <QString>

#include <memory>

namespace refgui::client
{

// The connection to the backend over the whole life of the program: finds or starts the
// backend, connects, and when the connection drops tries again until it is back. Whoever
// uses the connection learns of each (re)connection from `connected()` and takes up where
// it left off, e.g. attaches its session again.
class Backend : public QObject
{
	Q_OBJECT
	Q_PROPERTY(State state READ state NOTIFY stateChanged)

public:
	enum class State
	{
		Connecting,
		StartingBackend, // a backend we started is booting
		Connected,
		Disconnected, // waiting to try again; `reason()` says why
	};
	Q_ENUM(State)

	struct Mode
	{
		struct Shared
		{
		}; // find the running backend or start one
		struct Remote
		{
			Endpoint endpoint;
		}; // this one, never start anything
		struct Mock
		{
		}; // a private ref-server --mock
	};
	using Source = std::variant<Mode::Shared, Mode::Remote, Mode::Mock>;

	// `cwd` is what Hello tells the backend.
	Backend(Source source, QString cwd, QObject* parent = nullptr);
	~Backend() override;

\t// Starts connecting. Call once.
\tvoid start();
\t// Replaces the backend source, cancels any active connection/reconnect loop, and reconnects.
\tvoid switchSource(Source source);
\tconst Source\u0026 source() const { return m_source; }
\t// Closes the connection for good; pending requests fail first. Call before destroying
\t// whatever awaits them. Also ends the connecting loop; the destructor does this too.
\tvoid shutdown();
	// The system notifications this connection wants; Hello tells the backend, so set it before
	// `start()` and again whenever it changes (the new value is used on reconnecting). A running
	// connection is told with ServerClient::setNotify.
	void setNotifyKinds(QList<proto::NotifyFrame::Kind> kinds) { m_notifyKinds = std::move(kinds); }

	ServerClient& client() { return m_client; }
	State state() const { return m_state; }
	const QString& reason() const { return m_reason; }
	const proto::HelloResult& hello() const { return m_hello; }
	// What Hello of the current connection asked for; `setNotifyKinds` may have changed since.
	const QList<proto::NotifyFrame::Kind>& helloNotifyKinds() const { return m_helloNotifyKinds; }

signals:
\tvoid stateChanged();
\t// The connection is up and Hello is done.
\tvoid connected();
\tvoid loopCancelled();

private:
\tQCoro::Task\u003c\u003e run(quint64 generation, std::shared_ptr\u003cStopSource\u003e stop);
\tQCoro::Task\u003cstd::expected\u003cEndpoint, QString\u003e\u003e connectOnce(StopSource\u0026 stop);
\tQCoro::Task\u003clauncher::ConnectAttempt\u003e tryConnect(const Endpoint\u0026 endpoint, std::chrono::milliseconds timeout);
\tQCoro::Task\u003cstd::expected\u003cEndpoint, QString\u003e\u003e startMock(StopSource\u0026 stop);
\tvoid setState(State state, QString reason = {});

\tSource m_source;
\tQString m_cwd;
\tQList\u003cproto::NotifyFrame::Kind\u003e m_notifyKinds;
\tQList\u003cproto::NotifyFrame::Kind\u003e m_helloNotifyKinds;
\tServerClient m_client;
\tstd::unique_ptr\u003cMockBackend\u003e m_mock;
\tproto::HelloResult m_hello;
\tState m_state = State::Connecting;
\tQString m_reason;
\tbool m_started = false;
\tbool m_stopping = false;
\tquint64 m_generation = 0;
\tstd::shared_ptr\u003cStopSource\u003e m_stop; // ends every wait of the connecting loop
};

} // namespace refgui::client
