#pragma once

#include "client/backend.h"
#include "proto/types.h"
#include "session/conversation_feed.h"
#include "session/conversation_model.h"

#include <QCoroTask>
#include <QObject>

#include <optional>

namespace refgui::session
{

// One session on the backend: attaches to it, keeps the conversation model in step with
// its events, and sends the user's actions. Creating one per session is fine; nothing is
// shared between controllers.
//
// The conversation is only what the server says. A prompt is not echoed: the user's
// message shows when the server announces it.
class SessionController : public QObject
{
	Q_OBJECT
	Q_PROPERTY(bool busy READ isBusy NOTIFY busyChanged)
	Q_PROPERTY(bool turnInProgress READ isTurnInProgress NOTIFY turnInProgressChanged)

public:
	template<typename T>
	using Result = client::Result<T>;

	explicit SessionController(client::Backend& backend, QObject* parent = nullptr);

	ConversationModel& model() { return m_model; }
	const ConversationModel& model() const { return m_model; }
	const proto::SessionInfo& info() const { return m_info; }
	// Empty until a session is attached.
	const QString& sessionId() const { return m_sessionId; }
	bool hasSession() const { return !m_sessionId.isEmpty(); }

	// The session has work going on (an LLM response, tools, a question to the user).
	bool isBusy() const;
	// The user still sees a reply taking shape: busy, or streamed text not shown yet.
	bool isTurnInProgress() const;

	// The backend must be connected for all of these.
	QCoro::Task<Result<void>> create(QString cwd, std::optional<QString> agent, std::optional<proto::ModelRef> model);
	QCoro::Task<Result<void>> attach(QString sessionId);
	// Attaches the newest session of `cwd`. False when it has none.
	QCoro::Task<Result<bool>> continueLast(QString cwd);

	// Shows streamed text that is still waiting for its turn on screen.
	void showPendingNow() { m_feed.flush(); }

	QCoro::Task<Result<proto::PromptResult>> send(QString text);
	QCoro::Task<Result<void>> abort();
	QCoro::Task<Result<void>> permissionReply(QString callId, bool allow, bool always);

signals:
	void infoChanged();
	void busyChanged();
	void turnInProgressChanged();
	// The backend reported a problem with the session (an Error event).
	void errorReported(const QString& text);
	// A toast the backend wants shown.
	void toastReceived(const refgui::proto::ToastFrame& toast);
	// Attaching again after a reconnect failed, e.g. because the session is gone.
	void reattachFailed(const QString& reason);

private:
	QCoro::Task<> reattach();
	void adopt(const proto::SessionSnapshot& snapshot);
	void onFrame(const proto::ServerFrame& frame);
	void onEvent(const proto::SessionEvent& event);
	void setInfo(const proto::SessionInfo& info);

	client::Backend& m_backend;
	ConversationModel m_model;
	ConversationFeed m_feed;
	QString m_sessionId;
	proto::SessionInfo m_info;
};

} // namespace refgui::session
