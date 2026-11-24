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

\tvoid start();
\tvoid switchSource(client::Backend::Source source);
\t// Closes the connection for good; requests that are out fail.
\tvoid shutdown();

\tState state() const { return m_backend.state(); }
\tbool isConnected() const { return state() == State::Connected; }
\t// Why the connection is down, while it is.
\tconst QString\u0026 reason() const { return m_backend.reason(); }
\t// What the backend said in Hello.
\tconst QString\u0026 serverVersion() const { return m_backend.hello().serverVersion; }
\tint protocol() const { return m_backend.hello().protocol; }
\t// Connected to a backend that speaks an older protocol than this frontend.
\tbool isProtocolOutdated() const;
\t// The backend runs on this machine (started here, or a remote one on a loopback address).
\tbool isLocal() const { return m_local; }
\tQString backendUrl() const;
\tQString backendToken() const;
\tQString backendMode() const;

\t// Asks the backend to exit. The connection drops with it, so the request ends with
\t// client::ConnectionClosed when it worked.
\tQCoro::Task\u003cclient::Result\u003cvoid\u003e\u003e stopServer();

signals:
\tvoid stateChanged();
\tvoid sourceChanged();
\t// Hello was answered, after the first connection and after each reconnect.
\tvoid connected();
\t// The backend has a message for the user.
\tvoid toastReceived(const proto::ToastFrame\u0026 toast);
\t// Configuration was reloaded or modified on the backend.
\tvoid configChangedReceived(const QStringList\u0026 warnings);
private:
	void onFrame(const proto::ServerFrame& frame);

	bool m_local = true; // before m_backend, which the source is moved into
	client::Backend m_backend;
};

} // namespace refgui::workspace
