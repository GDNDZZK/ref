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

	// Starts connecting. Call once.
	void start();
	// Closes the connection for good; pending requests fail first. Call before destroying
	// whatever awaits them. Also ends the connecting loop; the destructor does this too.
	void shutdown();

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
	void stateChanged();
	// The connection is up and Hello is done.
	void connected();

private:
	QCoro::Task<> run();
	QCoro::Task<std::expected<Endpoint, QString>> connectOnce();
	QCoro::Task<bool> tryConnect(const Endpoint& endpoint, std::chrono::milliseconds timeout);
	QCoro::Task<std::expected<Endpoint, QString>> startMock();
	void setState(State state, QString reason = {});

	Source m_source;
	QString m_cwd;
	QList<proto::NotifyFrame::Kind> m_notifyKinds;
	QList<proto::NotifyFrame::Kind> m_helloNotifyKinds;
	ServerClient m_client;
	std::unique_ptr<MockBackend> m_mock;
	proto::HelloResult m_hello;
	State m_state = State::Connecting;
	QString m_reason;
	bool m_stopping = false;
	StopSource m_stop; // ends every wait of the connecting loop
};

} // namespace refgui::client
