#pragma once

#include "client/endpoint.h"
#include "proto/types.h"

#include <QCoroTask>
#include <QHash>
#include <QJsonObject>
#include <QElapsedTimer>
#include <QObject>
#include <QTimer>
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

	// A peer that vanishes without closing (a suspended or crashed machine, a dropped network)
	// leaves the connection open and its requests unanswered. So a connection that stays silent
	// for `pingAfter` gets a Ping, and one that stays silent for `deadAfter` is dropped, which
	// is reported as `closed` like any other end. `tick` is how often the silence is looked at.
	struct Heartbeat
	{
		std::chrono::milliseconds pingAfter{30000};
		std::chrono::milliseconds deadAfter{60000};
		std::chrono::milliseconds tick{5000};
	};
	void setHeartbeat(const Heartbeat& heartbeat);

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
	// A page of messages before `before`, see Request::Messages.
	QCoro::Task<Result<proto::MessagesPage>> messages(QString sessionId, QString before, int limit);
	QCoro::Task<Result<proto::ImageData>> getImage(QString sessionId, QString imageId);
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
	QCoro::Task<> ping();
	void onHeartbeat();
	void stopHeartbeat();
	void onTextMessage(const QString& text);
	void onDisconnected();
	void complete(quint64 id, Result<QJsonObject> result);
	void failPending(const QString& reason);

	QWebSocket m_socket;
	quint64 m_lastId = 0;
	QHash<quint64, ResponseAwaiter*> m_pending;
	QString m_closeReason; // set when we close it ourselves
	Heartbeat m_heartbeat;
	QTimer m_heartbeatTimer;
	QElapsedTimer m_silence; // since the last message of the peer
	bool m_helloDone = false;
	bool m_pingOutstanding = false;
};

} // namespace refgui::client
