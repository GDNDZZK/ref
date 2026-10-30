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
	QCoro::Task<Result<proto::HelloResult>> open(Endpoint endpoint, QString cwd, std::chrono::milliseconds timeout);
	void close();
	bool isOpen() const;

	QCoro::Task<Result<proto::SessionInfo>> createSession(QString cwd, std::optional<QString> agent, std::optional<proto::ModelRef> model);
	// `foreground` counts the attachment as the user's view of the session; a watcher only follows it.
	QCoro::Task<Result<proto::SessionSnapshot>> attach(QString sessionId, bool foreground = true);
	QCoro::Task<Result<void>> detach(QString sessionId);
	QCoro::Task<Result<proto::ServerConfig>> getConfig();
	QCoro::Task<Result<QList<proto::SessionInfo>>> listSessions();
	// Persisted sessions, newest first; of every directory when `cwd` is empty.
	QCoro::Task<Result<QList<proto::SessionInfo>>> listHistory(QString cwd, int limit);
	// Subscribes to the events of every live session (split view). Answers with the live sessions.
	QCoro::Task<Result<QList<proto::SessionInfo>>> watchAll(bool enabled);
	QCoro::Task<Result<proto::PromptResult>> prompt(QString sessionId, QString text);
	QCoro::Task<Result<void>> abort(QString sessionId);
	QCoro::Task<Result<void>> permissionReply(QString sessionId, QString callId, bool allow, bool always);

	// Session management. The ones that change the session answer with its new info.
	QCoro::Task<Result<proto::SessionInfo>> setModel(QString sessionId, proto::ModelRef model);
	QCoro::Task<Result<proto::SessionInfo>> setAgent(QString sessionId, QString agent);
	// No `variant` rotates to the model's next one.
	QCoro::Task<Result<proto::SessionInfo>> setVariant(QString sessionId, std::optional<QString> variant);
	QCoro::Task<Result<proto::SessionInfo>> rename(QString sessionId, QString name);
	QCoro::Task<Result<proto::SessionInfo>> setCwd(QString sessionId, QString path);
	QCoro::Task<Result<void>> compact(QString sessionId);
	QCoro::Task<Result<void>> close(QString sessionId);
	QCoro::Task<Result<void>> deleteSession(QString sessionId);
	// The path the transcript was written to, on the backend's machine.
	QCoro::Task<Result<QString>> exportTranscript(QString sessionId, QString path);
	// `command` is peers, peers-name, peers-inbox or peers-outbox; answers with text to show.
	QCoro::Task<Result<QString>> peerCommand(QString sessionId, QString command, QString args);
	QCoro::Task<Result<QList<proto::McpServerStatus>>> mcpStatus(std::optional<QString> cwd);
	QCoro::Task<Result<proto::DirListing>> listDirs(QString path);
	QCoro::Task<Result<void>> shutdown();

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
