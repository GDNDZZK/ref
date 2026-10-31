#include "client/server_client.h"

#include "proto/parse.h"
#include "proto/request.h"

#include <QJsonDocument>
#include <QLoggingCategory>
#include <QUrlQuery>

#include <utility>


using namespace Qt::StringLiterals;

namespace refgui::client
{
namespace
{

Q_LOGGING_CATEGORY(lcClient, "refgui.client")

const QString ClientName = u"ref-gui"_s;
const QString NotConnected = u"not connected"_s;
const QString ConnectionClosed = u"connection closed"_s;
const QString BackendSilent = u"the backend stopped answering"_s;

// The token goes in the URL as well as in Hello, the way ref-server's other clients send it.
QUrl withToken(QUrl url, const QString& token)
{
	if(token.isEmpty())
	{
		return url;
	}
	QUrlQuery query(url);
	query.addQueryItem(u"token"_s, token);
	url.setQuery(query);
	return url;
}

template<typename T>
Result<T> asResult(const Result<QJsonObject>& response, proto::Parsed<T> (*parse)(const QJsonObject&))
{
	if(!response)
	{
		return std::unexpected(response.error());
	}
	proto::Parsed<T> parsed = parse(*response);
	if(!parsed)
	{
		return std::unexpected(u"unexpected response: %1"_s.arg(parsed.error().message));
	}
	return std::move(*parsed);
}

Result<void> asVoidResult(const Result<QJsonObject>& response)
{
	if(!response)
	{
		return std::unexpected(response.error());
	}
	return {};
}

} // namespace

ServerClient::ServerClient(QObject* parent)
	: QObject(parent)
{
	// Frames are bounded by the protocol, not by Qt: a snapshot can be large.
	m_socket.setMaxAllowedIncomingFrameSize(QWebSocket::maxIncomingFrameSize());
	m_socket.setMaxAllowedIncomingMessageSize(QWebSocket::maxIncomingMessageSize());

	connect(&m_socket, &QWebSocket::textMessageReceived, this, &ServerClient::onTextMessage);
	connect(&m_socket, &QWebSocket::disconnected, this, &ServerClient::onDisconnected);
	connect(&m_heartbeatTimer, &QTimer::timeout, this, &ServerClient::onHeartbeat);

	m_connectTimer.setSingleShot(true);
	connect(&m_connectTimer, &QTimer::timeout, this, [this] { finishConnecting(false); });
	connect(&m_socket, &QWebSocket::connected, this, [this] { finishConnecting(true); });
	connect(&m_socket, &QWebSocket::errorOccurred, this, [this] { finishConnecting(false); });
}

ServerClient::~ServerClient()
{
	failPending(ConnectionClosed);
}

bool ServerClient::ConnectAwaiter::await_ready()
{
	client.m_connectOutcome = false;
	client.m_socket.open(url);
	// A URL the socket refuses ends here, before anything could be awaited.
	return client.m_socket.state() == QAbstractSocket::UnconnectedState;
}

void ServerClient::ConnectAwaiter::await_suspend(std::coroutine_handle<> handle)
{
	client.m_connectWaiter = handle;
	if(timeout.count() > 0)
	{
		client.m_connectTimer.start(timeout);
	}
}

void ServerClient::finishConnecting(bool connected)
{
	const std::coroutine_handle<> waiter = std::exchange(m_connectWaiter, {});
	if(!waiter)
	{
		return;
	}
	m_connectTimer.stop();
	m_connectOutcome = connected;
	waiter.resume();
}

QCoro::Task<Result<proto::HelloResult>> ServerClient::open(Endpoint endpoint,
                                                           QString cwd,
                                                           std::chrono::milliseconds timeout,
                                                           QList<proto::NotifyFrame::Kind> notify)
{
	m_closeReason.clear();
	const bool connected = co_await ConnectAwaiter{*this, withToken(endpoint.url, endpoint.token), timeout};
	if(!connected)
	{
		const QString reason = m_socket.errorString();
		m_socket.abort();
		co_return std::unexpected(reason);
	}

	// Also a backend that never answers Hello is given up on.
	m_silence.start();
	m_heartbeatTimer.start(m_heartbeat.tick);
	const Result<QJsonObject> response = co_await request(proto::request::hello(ClientName, cwd, endpoint.token, notify));
	const Result<proto::HelloResult> hello = asResult(response, proto::parseHelloResult);
	if(!hello)
	{
		close();
		co_return hello;
	}
	m_helloDone = true;
	co_return hello;
}

void ServerClient::setHeartbeat(const Heartbeat& heartbeat)
{
	m_heartbeat = heartbeat;
	if(m_heartbeatTimer.isActive())
	{
		m_heartbeatTimer.start(m_heartbeat.tick);
	}
}

void ServerClient::onHeartbeat()
{
	const auto silence = std::chrono::milliseconds(m_silence.elapsed());
	if(silence >= m_heartbeat.deadAfter)
	{
		qCWarning(lcClient) << "no word from the backend for" << silence.count() << "ms; dropping the connection";
		m_closeReason = BackendSilent;
		m_socket.abort();
		return;
	}
	// Before Hello is answered the backend does not take requests.
	if(silence >= m_heartbeat.pingAfter && m_helloDone && !m_pingOutstanding)
	{
		ping();
	}
}

QCoro::Task<> ServerClient::ping()
{
	m_pingOutstanding = true;
	static_cast<void>(co_await request(proto::request::ping()));
	m_pingOutstanding = false;
}

void ServerClient::stopHeartbeat()
{
	m_heartbeatTimer.stop();
	m_helloDone = false;
	m_pingOutstanding = false;
}

void ServerClient::close()
{
	stopHeartbeat();
	m_closeReason = ConnectionClosed;
	m_socket.close();
	if(m_socket.state() != QAbstractSocket::UnconnectedState)
	{
		m_socket.abort();
	}
	failPending(ConnectionClosed);
	finishConnecting(false);
}

bool ServerClient::isOpen() const
{
	return m_socket.state() == QAbstractSocket::ConnectedState;
}

QCoro::Task<Result<proto::SessionInfo>> ServerClient::createSession(QString cwd, std::optional<QString> agent, std::optional<proto::ModelRef> model)
{
	co_return asResult(co_await request(proto::request::createSession(cwd, agent, model)), proto::parseSessionResult);
}

QCoro::Task<Result<proto::SessionSnapshot>> ServerClient::attach(QString sessionId, bool foreground)
{
	co_return asResult(co_await request(proto::request::attach(sessionId, foreground)), proto::parseAttachResult);
}

QCoro::Task<Result<void>> ServerClient::detach(QString sessionId)
{
	co_return asVoidResult(co_await request(proto::request::detach(sessionId)));
}

QCoro::Task<Result<proto::ServerConfig>> ServerClient::getConfig()
{
	co_return asResult(co_await request(proto::request::getConfig()), proto::parseConfigResult);
}

QCoro::Task<Result<QList<proto::SessionInfo>>> ServerClient::listSessions()
{
	co_return asResult(co_await request(proto::request::listSessions()), proto::parseSessionsResult);
}

QCoro::Task<Result<QList<proto::SessionInfo>>> ServerClient::watchAll(bool enabled)
{
	co_return asResult(co_await request(proto::request::watchAll(enabled)), proto::parseSessionsResult);
}

QCoro::Task<Result<QList<proto::SessionInfo>>> ServerClient::listHistory(QString cwd, int limit)
{
	co_return asResult(co_await request(proto::request::listHistory(cwd, limit)), proto::parseSessionsResult);
}

QCoro::Task<Result<proto::PromptResult>> ServerClient::prompt(QString sessionId, QString text, QList<proto::ImageData> images)
{
	co_return asResult(co_await request(proto::request::prompt(sessionId, text, images)), proto::parsePromptResult);
}

QCoro::Task<Result<proto::RemovedPrompt>> ServerClient::undo(QString sessionId, std::optional<QString> messageId)
{
	co_return asResult(co_await request(proto::request::undo(sessionId, messageId)), proto::parseUndoResult);
}

QCoro::Task<Result<proto::RemovedPrompt>> ServerClient::removeQueued(QString sessionId, QString messageId)
{
	co_return asResult(co_await request(proto::request::removeQueued(sessionId, messageId)), proto::parseRemovedPromptResult);
}

QCoro::Task<Result<void>> ServerClient::shell(QString sessionId, QString command)
{
	co_return asVoidResult(co_await request(proto::request::shell(sessionId, command)));
}

QCoro::Task<Result<QStringList>> ServerClient::listFiles(QString cwd, QString query, int limit)
{
	co_return asResult(co_await request(proto::request::listFiles(cwd, query, limit)), proto::parseFilesResult);
}

QCoro::Task<Result<QString>> ServerClient::expandCommand(QString name, QString args, QString cwd)
{
	co_return asResult(co_await request(proto::request::expandCommand(name, args, cwd)), proto::parseTextResult);
}

QCoro::Task<Result<void>> ServerClient::setNotify(QList<proto::NotifyFrame::Kind> kinds)
{
	co_return asVoidResult(co_await request(proto::request::setNotify(kinds)));
}

QCoro::Task<Result<void>> ServerClient::setFocus(bool focused, QStringList sessions)
{
	co_return asVoidResult(co_await request(proto::request::setFocus(focused, sessions)));
}

QCoro::Task<Result<void>> ServerClient::abort(QString sessionId)
{
	co_return asVoidResult(co_await request(proto::request::abort(sessionId)));
}

QCoro::Task<Result<void>> ServerClient::permissionReply(QString sessionId, QString callId, bool allow, bool always)
{
	co_return asVoidResult(co_await request(proto::request::permissionReply(sessionId, callId, allow, always)));
}

QCoro::Task<Result<proto::SessionInfo>> ServerClient::setModel(QString sessionId, proto::ModelRef model)
{
	co_return asResult(co_await request(proto::request::setModel(sessionId, model)), proto::parseSessionResult);
}

QCoro::Task<Result<proto::SessionInfo>> ServerClient::setAgent(QString sessionId, QString agent)
{
	co_return asResult(co_await request(proto::request::setAgent(sessionId, agent)), proto::parseSessionResult);
}

QCoro::Task<Result<proto::SessionInfo>> ServerClient::setVariant(QString sessionId, std::optional<QString> variant)
{
	co_return asResult(co_await request(proto::request::setVariant(sessionId, variant)), proto::parseSessionResult);
}

QCoro::Task<Result<proto::SessionInfo>> ServerClient::rename(QString sessionId, QString name)
{
	co_return asResult(co_await request(proto::request::rename(sessionId, name)), proto::parseSessionResult);
}

QCoro::Task<Result<proto::SessionInfo>> ServerClient::setCwd(QString sessionId, QString path)
{
	co_return asResult(co_await request(proto::request::setCwd(sessionId, path)), proto::parseSessionResult);
}

QCoro::Task<Result<void>> ServerClient::compact(QString sessionId)
{
	co_return asVoidResult(co_await request(proto::request::compact(sessionId)));
}

QCoro::Task<Result<void>> ServerClient::close(QString sessionId)
{
	co_return asVoidResult(co_await request(proto::request::close(sessionId)));
}

QCoro::Task<Result<void>> ServerClient::deleteSession(QString sessionId)
{
	co_return asVoidResult(co_await request(proto::request::deleteSession(sessionId)));
}

QCoro::Task<Result<QString>> ServerClient::exportTranscript(QString sessionId, QString path)
{
	co_return asResult(co_await request(proto::request::exportTranscript(sessionId, path)), proto::parsePathResult);
}

QCoro::Task<Result<QString>> ServerClient::peerCommand(QString sessionId, QString command, QString args)
{
	co_return asResult(co_await request(proto::request::peerCommand(sessionId, command, args)), proto::parseTextResult);
}

QCoro::Task<Result<QList<proto::McpServerStatus>>> ServerClient::mcpStatus(std::optional<QString> cwd)
{
	co_return asResult(co_await request(proto::request::mcpStatus(cwd)), proto::parseMcpResult);
}

QCoro::Task<Result<proto::DirListing>> ServerClient::listDirs(QString path)
{
	co_return asResult(co_await request(proto::request::listDirs(path)), proto::parseDirsResult);
}

QCoro::Task<Result<proto::MessagesPage>> ServerClient::messages(QString sessionId, QString before, int limit)
{
	co_return asResult(co_await request(proto::request::messages(sessionId, before, limit)), proto::parseMessagesResult);
}

QCoro::Task<Result<proto::ImageData>> ServerClient::getImage(QString sessionId, QString imageId)
{
	co_return asResult(co_await request(proto::request::getImage(sessionId, imageId)), proto::parseImageResult);
}

QCoro::Task<Result<void>> ServerClient::shutdown()
{
	co_return asVoidResult(co_await request(proto::request::shutdown()));
}

void ServerClient::ResponseAwaiter::await_suspend(std::coroutine_handle<> handle)
{
	waiter = handle;
	client.m_pending.insert(id, this);
}

QCoro::Task<Result<QJsonObject>> ServerClient::request(QJsonObject body)
{
	if(m_socket.state() != QAbstractSocket::ConnectedState)
	{
		co_return std::unexpected(NotConnected);
	}
	const quint64 id = ++m_lastId;
	body.insert(u"id"_s, static_cast<qint64>(id));
	m_socket.sendTextMessage(QString::fromUtf8(QJsonDocument(body).toJson(QJsonDocument::Compact)));
	co_return co_await ResponseAwaiter{.client = *this, .id = id};
}

void ServerClient::onTextMessage(const QString& text)
{
	m_silence.restart();
	QJsonParseError parseError;
	const QJsonDocument document = QJsonDocument::fromJson(text.toUtf8(), &parseError);
	if(!document.isObject())
	{
		qCWarning(lcClient) << "frame is not a JSON object:" << parseError.errorString();
		return;
	}

	const proto::Parsed<proto::ServerFrame> frame = proto::parseServerFrame(document.object());
	if(!frame)
	{
		// A newer backend may send frames this client does not know; they are not an error.
		qCWarning(lcClient) << (frame.error().unknown ? "ignoring frame:" : "malformed frame:") << frame.error().message;
		return;
	}

	if(const auto* response = std::get_if<proto::ResponseFrame>(&*frame))
	{
		Result<QJsonObject> result = response->ok ? Result<QJsonObject>(response->result) : std::unexpected(response->error);
		complete(response->id, std::move(result));
		return;
	}
	emit frameReceived(*frame);
}

void ServerClient::onDisconnected()
{
	stopHeartbeat();
	const QString reason = m_closeReason.isEmpty() ? (m_socket.closeReason().isEmpty() ? m_socket.errorString() : m_socket.closeReason()) : m_closeReason;
	m_closeReason.clear();
	failPending(ConnectionClosed);
	finishConnecting(false);
	emit closed(reason.isEmpty() ? ConnectionClosed : reason);
}

void ServerClient::complete(quint64 id, Result<QJsonObject> result)
{
	ResponseAwaiter* awaiter = m_pending.take(id);
	if(awaiter == nullptr)
	{
		qCDebug(lcClient) << "response to no request" << id;
		return;
	}
	awaiter->result = std::move(result);
	awaiter->waiter.resume();
}

void ServerClient::failPending(const QString& reason)
{
	// Resuming a coroutine may send a new request, so take the waiting ones out first.
	const QList<quint64> ids = m_pending.keys();
	for(const quint64 id : ids)
	{
		if(m_pending.contains(id))
		{
			complete(id, std::unexpected(reason));
		}
	}
}

} // namespace refgui::client
