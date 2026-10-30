#pragma once

#include <QJsonDocument>
#include <QJsonObject>
#include <QObject>
#include <QUrl>
#include <QWebSocket>
#include <QWebSocketServer>

#include <functional>

namespace refgui::test
{

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
		respond(request, {{QStringLiteral("server_version"), QStringLiteral("0.0.0-test")}, {QStringLiteral("protocol"), 2}});
	}

	QWebSocketServer m_server;
	QWebSocket* m_socket = nullptr;
	QList<QJsonObject> m_requests;
};

} // namespace refgui::test
