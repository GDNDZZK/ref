#include "client/mock_backend.h"

#include <QJsonDocument>
#include <QJsonObject>

using namespace Qt::StringLiterals;
using namespace std::chrono_literals;

namespace refgui::client
{
namespace
{

constexpr auto AnnounceTimeout = 10s;
constexpr auto AnnouncePoll = 20ms;
// Time the backend gets to leave and remove its workspace once its stdin is closed, the same as
// the Rust client's MockBackend::shutdown; then it is ended.
constexpr auto ExitTimeout = 3s;
constexpr auto KillTimeout = 2s;

} // namespace

QCoro::Task<std::expected<std::unique_ptr<MockBackend>, QString>> MockBackend::start(QString program, StopSource& stop)
{
	std::unique_ptr<MockBackend> backend(new MockBackend);
	QProcess& process = backend->m_process;
	process.setProgram(program);
	process.setArguments({u"--mock"_s});
	process.setStandardErrorFile(QProcess::nullDevice());
	process.start(); // stdin stays a pipe we keep open

	const auto failure = [&](const QString& reason) {
		return std::unexpected(u"cannot start the mock backend: %1"_s.arg(reason));
	};

	// Polled: neither the process starting nor its exit is announced by anything that
	// still fires when it happens before we start waiting.
	std::chrono::milliseconds waited = 0ms;
	const auto announcementPending = [&] {
		return !process.canReadLine() && process.state() != QProcess::NotRunning && waited < AnnounceTimeout && !stop.stopped();
	};
	while(announcementPending())
	{
		co_await stop.sleep(AnnouncePoll);
		waited += AnnouncePoll;
	}
	if(stop.stopped())
	{
		co_return failure(u"stopped"_s);
	}
	if(process.error() == QProcess::FailedToStart)
	{
		co_return failure(u"%1: %2"_s.arg(program, process.errorString()));
	}
	const QByteArray line = process.readLine();
	if(line.isEmpty())
	{
		const bool exited = process.state() == QProcess::NotRunning;
		co_return failure(exited ? u"exited before announcing its port"_s : u"no port announced within 10s"_s);
	}
	const std::expected<Endpoint, QString> endpoint = parseAnnouncement(line);
	if(!endpoint)
	{
		co_return failure(endpoint.error());
	}
	backend->m_endpoint = *endpoint;
	co_return backend;
}

std::expected<Endpoint, QString> MockBackend::parseAnnouncement(const QByteArray& line)
{
	const QJsonObject announcement = QJsonDocument::fromJson(line).object();
	const int port = announcement.value(u"port"_s).toInt();
	const QString token = announcement.value(u"token"_s).toString();
	if(port <= 0 || token.isEmpty())
	{
		return std::unexpected(u"unexpected first line %1"_s.arg(QString::fromUtf8(line.trimmed())));
	}
	return Endpoint{.url = QUrl(u"ws://127.0.0.1:%1/"_s.arg(port)), .token = token};
}

MockBackend::~MockBackend()
{
	if(!isRunning())
	{
		return;
	}
	const auto wait = [this](std::chrono::milliseconds timeout) { return m_process.waitForFinished(static_cast<int>(timeout.count())); };
	m_process.closeWriteChannel(); // the backend exits and removes its workspace when its stdin ends
	if(wait(ExitTimeout))
	{
		return;
	}
	m_process.terminate();
	if(!wait(KillTimeout))
	{
		m_process.kill();
		wait(KillTimeout);
	}
}

} // namespace refgui::client
