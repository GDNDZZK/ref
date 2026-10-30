#pragma once

#include "client/endpoint.h"
#include "client/launcher.h"
#include "client/mock_backend.h"
#include "client/server_client.h"

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
	// whatever awaits them.
	void shutdown();

	ServerClient& client() { return m_client; }
	State state() const { return m_state; }
	const QString& reason() const { return m_reason; }
	const proto::HelloResult& hello() const { return m_hello; }

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
	ServerClient m_client;
	std::unique_ptr<MockBackend> m_mock;
	proto::HelloResult m_hello;
	State m_state = State::Connecting;
	QString m_reason;
	bool m_stopping = false;
};

} // namespace refgui::client
