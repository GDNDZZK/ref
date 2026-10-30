#pragma once

#include "client/endpoint.h"
#include "proto/types.h"

#include <QCoroTask>
#include <QHash>
#include <QJsonObject>
#include <QObject>
#include <QWebSocket>

#include <chrono>
#include <coroutine>
#include <expected>
#include <optional>

namespace refgui::client
{

// The result of a request: the answer, or the reason it failed (the backend's error text,
// or "connection closed").
template<typename T>
using Result = std::expected<T, QString>;

// One WebSocket connection to ref-server, speaking the ref protocol.
//
// Requests are coroutines: they send a frame with a fresh id and resume, in the receive
// handler, with the response carrying that id. Everything else the backend sends is passed
// on in the order it arrived. A response is handled before the frame that follows it, so
// the snapshot an Attach returns is in place before the events that continue it.
//
// Closing the connection, or destroying the client, fails every request still waiting, and
// their coroutines run on right there. Close the client before destroying whatever those
// coroutines touch.
class ServerClient : public QObject
{
	Q_OBJECT

public:
	explicit ServerClient(QObject* parent = nullptr);
	~ServerClient() override;

	// Connects and says Hello. Gives up on the connection after `timeout`.
	// `notify` is what Hello tells the backend about the system notifications this connection shows.
	QCoro::Task<Result<proto::HelloResult>> open(Endpoint endpoint,
	                                             QString cwd,
	                                             std::chrono::milliseconds timeout,
	                                             QList<proto::NotifyFrame::Kind> notify = {});
	void close();
	bool isOpen() const;

	QCoro::Task<Result<proto::SessionInfo>> createSession(QString cwd, std::optional<QString> agent, std::optional<proto::ModelRef> model);
	QCoro::Task<Result<proto::SessionSnapshot>> attach(QString sessionId);
	QCoro::Task<Result<QList<proto::SessionInfo>>> listHistory(QString cwd, int limit);
	QCoro::Task<Result<proto::PromptResult>> prompt(QString sessionId, QString text, QList<proto::ImageData> images = {});
	QCoro::Task<Result<proto::RemovedPrompt>> removeQueued(QString sessionId, QString messageId);
	QCoro::Task<Result<void>> shell(QString sessionId, QString command);
	// Paths below `cwd` matching `query` (best first); directories end with "/".
	QCoro::Task<Result<QStringList>> listFiles(QString cwd, QString query, int limit);
	// The prompt text a user-defined slash command stands for.
	QCoro::Task<Result<QString>> expandCommand(QString name, QString args, QString cwd);
	QCoro::Task<Result<void>> setNotify(QList<proto::NotifyFrame::Kind> kinds);
	QCoro::Task<Result<void>> setFocus(bool focused, QStringList sessions);
	QCoro::Task<Result<void>> abort(QString sessionId);
	QCoro::Task<Result<void>> permissionReply(QString sessionId, QString callId, bool allow, bool always);

signals:
	// Every frame but responses, in arrival order.
	void frameReceived(const refgui::proto::ServerFrame& frame);
	// The connection ended; `reason` is for the user.
	void closed(const QString& reason);

private:
	// Suspends the coroutine that awaits it until the response with its id arrives.
	struct ResponseAwaiter
	{
		ServerClient& client;
		quint64 id;
		std::coroutine_handle<> waiter{};
		std::optional<Result<QJsonObject>> result{};

		bool await_ready() const noexcept { return false; }
		void await_suspend(std::coroutine_handle<> handle);
		Result<QJsonObject> await_resume() { return std::move(*result); }
	};

	QCoro::Task<Result<QJsonObject>> request(QJsonObject body);
	void onTextMessage(const QString& text);
	void onDisconnected();
	void complete(quint64 id, Result<QJsonObject> result);
	void failPending(const QString& reason);

	QWebSocket m_socket;
	quint64 m_lastId = 0;
	QHash<quint64, ResponseAwaiter*> m_pending;
	QString m_closeReason; // set when we close it ourselves
};

} // namespace refgui::client
