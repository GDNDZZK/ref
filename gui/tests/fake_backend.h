#pragma once

#include "proto/types.h"

#include <QJsonDocument>
#include <QJsonObject>
#include <QObject>
#include <QUrl>
#include <QWebSocket>
#include <QWebSocketServer>

#include <functional>

namespace refgui::test
{

// What the fake answers to Hello.
inline const QString FakeServerVersion = QStringLiteral("0.0.0-test");

// The fields of a session's info JSON the tests vary; the rest is fixed.
struct SessionJson
{
	QString name{};
	QString title{};
	QString agent = QStringLiteral("build");
	QJsonObject model{{QStringLiteral("provider"), QStringLiteral("p")}, {QStringLiteral("model"), QStringLiteral("m")}};
	QString cwd = QStringLiteral("/work");
	qint64 createdAt = 1; // also the update time
	QString state = QStringLiteral("idle");
	int attached = 0;
	bool live = true;
};

// A `SessionInfo` as the backend sends it.
inline QJsonObject sessionInfoJson(const QString& id, const SessionJson& fields = {})
{
	return {{QStringLiteral("id"), id},
	        {QStringLiteral("name"), fields.name},
	        {QStringLiteral("title"), fields.title},
	        {QStringLiteral("agent"), fields.agent},
	        {QStringLiteral("model"), fields.model},
	        {QStringLiteral("cwd"), fields.cwd},
	        {QStringLiteral("created_at"), fields.createdAt},
	        {QStringLiteral("updated_at"), fields.createdAt},
	        {QStringLiteral("status"), QJsonObject{{QStringLiteral("state"), fields.state}}},
	        {QStringLiteral("attached"), fields.attached},
	        {QStringLiteral("live"), fields.live}};
}

// A ref-server stand-in on a loopback WebSocket. It records the request frames it gets and
// leaves answering them to `onRequest`; Hello is answered by default.
class FakeBackend : public QObject
{
	Q_OBJECT

public:
	explicit FakeBackend(QObject* parent = nullptr)
		: QObject(parent)
		, m_server(QStringLiteral("fake-backend"), QWebSocketServer::NonSecureMode)
	{
		const bool listening = m_server.listen(QHostAddress::LocalHost, 0);
		Q_ASSERT(listening);
		connect(&m_server, &QWebSocketServer::newConnection, this, [this] {
			m_socket = m_server.nextPendingConnection();
			m_socket->setParent(this); // freed with the fake, also when a client reconnects
			connect(m_socket, &QWebSocket::textMessageReceived, this, &FakeBackend::onText);
			emit accepted();
		});
	}

	QUrl url() const { return QUrl(QStringLiteral("ws://127.0.0.1:%1/").arg(m_server.serverPort())); }
	QWebSocket* socket() const { return m_socket; }
	const QList<QJsonObject>& requests() const { return m_requests; }

	// Called for every request after Hello.
	std::function<void(const QJsonObject& request)> onRequest;
	// Replaces the default answer to Hello.
	std::function<void(const QJsonObject& hello)> onHello;

	void send(const QJsonObject& frame) { m_socket->sendTextMessage(QString::fromUtf8(QJsonDocument(frame).toJson(QJsonDocument::Compact))); }

	void respond(const QJsonObject& request, const QJsonObject& result)
	{
		send({{QStringLiteral("kind"), QStringLiteral("response")},
		      {QStringLiteral("id"), request.value(QStringLiteral("id"))},
		      {QStringLiteral("ok"), true},
		      {QStringLiteral("result"), result}});
	}

	void respondHello(const QJsonObject& hello, int protocol = proto::ProtocolVersion)
	{
		respond(hello, {{QStringLiteral("server_version"), FakeServerVersion}, {QStringLiteral("protocol"), protocol}});
	}

	void fail(const QJsonObject& request, const QString& error)
	{
		send({{QStringLiteral("kind"), QStringLiteral("response")},
		      {QStringLiteral("id"), request.value(QStringLiteral("id"))},
		      {QStringLiteral("ok"), false},
		      {QStringLiteral("result"), QJsonValue::Null},
		      {QStringLiteral("error"), error}});
	}

signals:
	void accepted();

private:
	void onText(const QString& text)
	{
		const QJsonObject request = QJsonDocument::fromJson(text.toUtf8()).object();
		m_requests.append(request);
		if(request.value(QStringLiteral("op")).toString() != QStringLiteral("hello"))
		{
			if(onRequest)
			{
				onRequest(request);
			}
			return;
		}
		if(onHello)
		{
			onHello(request);
			return;
		}
		respondHello(request);
	}

	QWebSocketServer m_server;
	QWebSocket* m_socket = nullptr;
	QList<QJsonObject> m_requests;
};

} // namespace refgui::test
