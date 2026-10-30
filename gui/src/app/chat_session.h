// Adapted from an early version of qutie by LightBlueCube
#pragma once

#include "client/backend.h"
#include "session/session_controller.h"

#include <QCoroTask>
#include <QObject>
#include <QtQmlIntegration>

// One session as the window shows it: the conversation and the actions on it. Workspace
// creates one for each session opened in the window and keeps it until the session is
// closed; while the session is not shown it is detached but keeps its state.
class ChatSession : public QObject
{
	Q_OBJECT
	QML_ELEMENT
	QML_UNCREATABLE("Created by Workspace.")
	Q_PROPERTY(QAbstractItemModel* blocks READ blocks CONSTANT)
	Q_PROPERTY(bool busy READ isBusy NOTIFY busyChanged)
	Q_PROPERTY(bool ready READ isReady NOTIFY readyChanged)
	Q_PROPERTY(QString statusText READ statusText NOTIFY statusTextChanged)
	Q_PROPERTY(QString title READ title NOTIFY titleChanged)

public:
	// `backend` is shared by all sessions of the window and outlives them.
	ChatSession(refgui::client::Backend& backend, QString sessionId, QObject* parent = nullptr);

	const QString& sessionId() const { return m_sessionId; }
	bool isAttached() const { return m_controller.isAttached(); }

	// Shows the session's conversation: it replaces the model with the backend's snapshot.
	// A tile watches (`foreground` false); the session in the window's main view is in the foreground.
	QCoro::Task<refgui::client::Result<void>> attach(bool foreground);
	QCoro::Task<refgui::client::Result<void>> detach();
	// Stays shown but no longer in the foreground (split view). Precondition: attached.
	QCoro::Task<refgui::client::Result<void>> background();

	QAbstractItemModel* blocks();
	// A reply is being produced or still being shown.
	bool isBusy() const;
	// The backend is reachable and a session is open: prompts can be sent.
	bool isReady() const;
	// What the user should be told about this session: the last failure or a remark. Empty when all is well.
	QString statusText() const;
	QString title() const;

	// Precondition: !busy and `text` is not blank.
	Q_INVOKABLE void send(const QString& text);
	// Precondition: busy.
	Q_INVOKABLE void stop();
	// Answers a tool call that waits for permission.
	Q_INVOKABLE void permissionReply(const QString& callId, bool allow, bool always);

signals:
	void busyChanged();
	void readyChanged();
	void statusTextChanged();
	void titleChanged();
	// The backend reported a problem with the session (an Error event).
	void errorReported(const QString& text);

private:
	QCoro::Task<> sendPrompt(QString text);
	QCoro::Task<> sendAbort();
	QCoro::Task<> sendPermission(QString callId, bool allow, bool always);
	void setFailure(const QString& text);

	refgui::client::Backend& m_backend;
	QString m_sessionId;
	refgui::session::SessionController m_controller;
	QString m_failure; // the last thing that went wrong, until the next action
};
