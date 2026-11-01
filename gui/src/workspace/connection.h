#pragma once

#include "client/backend.h"
#include "client/result.h"

#include <QCoroTask>
#include <QObject>
#include <QString>

namespace refgui::workspace
{

// The window's link to the backend: owns the client::Backend, starts and stops it, and passes
// on what the window shows of the connection. The app goes through this, not the client layer.
class Connection : public QObject
{
	Q_OBJECT

public:
	using State = client::Backend::State;

	Connection(client::Backend::Source source, QString cwd, QObject* parent = nullptr);

	// For the constructors of the objects that talk to the backend themselves (the session,
	// workspace and notify layers). The app hands it on and calls nothing on it.
	client::Backend& backend() { return m_backend; }

	void start();
	// Closes the connection for good; requests that are out fail.
	void shutdown();

	State state() const { return m_backend.state(); }
	bool isConnected() const { return state() == State::Connected; }
	// Why the connection is down, while it is.
	const QString& reason() const { return m_backend.reason(); }
	// What the backend said in Hello.
	const QString& serverVersion() const { return m_backend.hello().serverVersion; }
	int protocol() const { return m_backend.hello().protocol; }
	// Connected to a backend that speaks an older protocol than this frontend.
	bool isProtocolOutdated() const;
	// The backend runs on this machine (started here, or a remote one on a loopback address).
	bool isLocal() const { return m_local; }

	// Asks the backend to exit. The connection drops with it, so the request ends with
	// client::ConnectionClosed when it worked.
	QCoro::Task<client::Result<void>> stopServer();

signals:
	void stateChanged();
	// Hello was answered, after the first connection and after each reconnect.
	void connected();
	// The backend has a message for the user.
	void toastReceived(const proto::ToastFrame& toast);

private:
	void onFrame(const proto::ServerFrame& frame);

	bool m_local = true; // before m_backend, which the source is moved into
	client::Backend m_backend;
};

} // namespace refgui::workspace
