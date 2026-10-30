#include "client/server_client.h"

#include "proto/parse.h"
#include "proto/request.h"

#include <QJsonDocument>
#include <QLoggingCategory>
#include <QUrlQuery>

#include <QCoroWebSocket>


using namespace Qt::StringLiterals;

namespace refgui::client
{
namespace
{

Q_LOGGING_CATEGORY(lcClient, "refgui.client")

const QString ClientName = u"ref-gui"_s;
const QString NotConnected = u"not connected"_s;
const QString ConnectionClosed = u"connection closed"_s;

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
}

ServerClient::~ServerClient()
{
	failPending(ConnectionClosed);
}

QCoro::Task<Result<proto::HelloResult>> ServerClient::open(Endpoint endpoint, QString cwd, std::chrono::milliseconds timeout)
{
	m_closeReason.clear();
	const bool connected = co_await qCoro(m_socket).open(withToken(endpoint.url, endpoint.token), timeout);
	if(!connected)
	{
		const QString reason = m_socket.errorString();
		m_socket.abort();
		co_return std::unexpected(reason);
	}

	const Result<QJsonObject> response = co_await request(proto::request::hello(ClientName, cwd, endpoint.token));
	const Result<proto::HelloResult> hello = asResult(response, proto::parseHelloResult);
	if(!hello)
	{
		close();
	}
	co_return hello;
}

void ServerClient::close()
{
	m_closeReason = ConnectionClosed;
	m_socket.close();
	if(m_socket.state() != QAbstractSocket::UnconnectedState)
	{
		m_socket.abort();
	}
	failPending(ConnectionClosed);
}

bool ServerClient::isOpen() const
{
	return m_socket.state() == QAbstractSocket::ConnectedState;
}

QCoro::Task<Result<proto::SessionInfo>> ServerClient::createSession(QString cwd, std::optional<QString> agent, std::optional<proto::ModelRef> model)
{
	co_return asResult(co_await request(proto::request::createSession(cwd, agent, model)), proto::parseSessionResult);
}

QCoro::Task<Result<proto::SessionSnapshot>> ServerClient::attach(QString sessionId)
{
	co_return asResult(co_await request(proto::request::attach(sessionId, true)), proto::parseAttachResult);
}

QCoro::Task<Result<QList<proto::SessionInfo>>> ServerClient::listHistory(QString cwd, int limit)
{
	co_return asResult(co_await request(proto::request::listHistory(cwd, limit)), proto::parseSessionsResult);
}

QCoro::Task<Result<proto::PromptResult>> ServerClient::prompt(QString sessionId, QString text)
{
	co_return asResult(co_await request(proto::request::prompt(sessionId, text)), proto::parsePromptResult);
}

QCoro::Task<Result<void>> ServerClient::abort(QString sessionId)
{
	co_return asVoidResult(co_await request(proto::request::abort(sessionId)));
}

QCoro::Task<Result<void>> ServerClient::permissionReply(QString sessionId, QString callId, bool allow, bool always)
{
	co_return asVoidResult(co_await request(proto::request::permissionReply(sessionId, callId, allow, always)));
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
	const QString reason = m_closeReason.isEmpty() ? (m_socket.closeReason().isEmpty() ? m_socket.errorString() : m_socket.closeReason()) : m_closeReason;
	m_closeReason.clear();
	failPending(ConnectionClosed);
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
