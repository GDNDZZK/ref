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
	// Messages waiting for the next safe point, in delivery order: the server's word, from the
	// snapshot and Queue events. Prompts of the user can be withdrawn while they are here.
	const QList<proto::Message>& queued() const { return m_queued; }
	// Empty until a session is attached.
	const QString& sessionId() const { return m_sessionId; }
	bool hasSession() const { return !m_sessionId.isEmpty(); }
	// The backend sends this session's events and the snapshot is in the model. False before
	// the first attach and after detach().
	bool isAttached() const { return m_attached; }

	// The session has work going on (an LLM response, tools, a question to the user).
	bool isBusy() const;
	// The user still sees a reply taking shape: busy, or streamed text not shown yet.
	bool isTurnInProgress() const;

	// The backend must be connected for all of these.
	QCoro::Task<Result<void>> create(QString cwd, std::optional<QString> agent, std::optional<proto::ModelRef> model);
	// `foreground` counts the attachment as the user's view of the session; a watcher follows
	// it without that. Attaching the session that is already attached only changes the mode:
	// the model is kept, so a tile can be zoomed without its conversation reloading.
	QCoro::Task<Result<void>> attach(QString sessionId, bool foreground = true);
	// Stops the events (unless a WatchAll is on) but keeps the model and the session id, so
	// the controller can attach again.
	QCoro::Task<Result<void>> detach();
	// Gives up the foreground but keeps following the session: the backend no longer counts
	// this window as attached, and the events come on through a WatchAll. Precondition: attached.
	QCoro::Task<Result<void>> background();
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
	void queueChanged();
	// The backend reported a problem with the session (an Error event).
	void errorReported(const QString& text);
	// Attaching again after a reconnect failed, e.g. because the session is gone.
	void reattachFailed(const QString& reason);

private:
	QCoro::Task<> reattach();
	void adopt(const proto::SessionSnapshot& snapshot);
	void onFrame(const proto::ServerFrame& frame);
	void onEvent(const proto::SessionEvent& event);
	void setInfo(const proto::SessionInfo& info);
	void setQueued(QList<proto::Message> messages);

	client::Backend& m_backend;
	ConversationModel m_model;
	ConversationFeed m_feed;
	QString m_sessionId;
	bool m_attached = false;
	bool m_foreground = true;
	proto::SessionInfo m_info;
	QList<proto::Message> m_queued;
};

} // namespace refgui::session
