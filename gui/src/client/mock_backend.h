#pragma once

#include "client/endpoint.h"

#include <QCoroTask>
#include <QProcess>
#include <QString>

#include <expected>
#include <memory>

namespace refgui::client
{

// A private `ref-server --mock`, for working on the UI without touching the shared backend.
// It is killed with this object, and it also exits when its stdin closes (we hold the
// write end), so it does not outlive a GUI that died.
class MockBackend
{
public:
	// Starts the mock backend and waits for it to announce its port.
	static QCoro::Task<std::expected<std::unique_ptr<MockBackend>, QString>> start(QString program);

	// The one JSON line the backend prints once it listens: {"port":N,"token":"..."}.
	static std::expected<Endpoint, QString> parseAnnouncement(const QByteArray& line);

	~MockBackend();
	MockBackend(const MockBackend&) = delete;
	MockBackend& operator=(const MockBackend&) = delete;

	const Endpoint& endpoint() const { return m_endpoint; }
	bool isRunning() const { return m_process.state() != QProcess::NotRunning; }

private:
	MockBackend() = default;

	QProcess m_process;
	Endpoint m_endpoint;
};

} // namespace refgui::client
