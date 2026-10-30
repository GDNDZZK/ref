// Adapted from an early version of qutie by LightBlueCube
#pragma once

#include "cli/launch_options.h"
#include "client/backend.h"
#include "session/session_controller.h"

#include <QCoroTask>
#include <QObject>
#include <QtQmlIntegration>

// The one object QML talks to: the conversation and the actions on it.
// Created in C++ and handed to the window, which cannot create one itself.
class ChatSession : public QObject
{
	Q_OBJECT
	QML_ELEMENT
	QML_UNCREATABLE("Created in C++ and passed to Main as `session`.")
	Q_PROPERTY(QAbstractItemModel* blocks READ blocks CONSTANT)
	Q_PROPERTY(bool busy READ isBusy NOTIFY busyChanged)
	Q_PROPERTY(bool ready READ isReady NOTIFY readyChanged)
	Q_PROPERTY(QString connectionState READ connectionState NOTIFY connectionChanged)
	Q_PROPERTY(QString statusText READ statusText NOTIFY statusTextChanged)
	Q_PROPERTY(QString title READ title NOTIFY titleChanged)

public:
	ChatSession(refgui::client::Backend::Source source, QString cwd, refgui::cli::SessionStart start, QObject* parent = nullptr);
	~ChatSession() override;

	QAbstractItemModel* blocks();
	// A reply is being produced or still being shown.
	bool isBusy() const;
	// The backend is reachable and a session is open: prompts can be sent.
	bool isReady() const;
	// "connecting", "starting", "connected" or "disconnected".
	QString connectionState() const;
	// What the user should be told: the connection, or the last failure. Empty when all is well.
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
	void connectionChanged();
	void statusTextChanged();
	void titleChanged();
	// The session is open (first time only).
	void opened(const QString& sessionId);
	// The session could not be opened.
	void openFailed(const QString& reason);

private:
	QCoro::Task<> open();
	QCoro::Task<> sendPrompt(QString text);
	QCoro::Task<> sendAbort();
	QCoro::Task<> sendPermission(QString callId, bool allow, bool always);
	void setFailure(const QString& text);
	void setNotice(const QString& text);

	QString m_cwd;
	refgui::cli::SessionStart m_start;
	// The controller refers to the backend, so it is declared after it.
	refgui::client::Backend m_backend;
	refgui::session::SessionController m_controller;
	QString m_failure; // the last thing that went wrong, until the next action
	QString m_notice;  // a one-off remark about how the session was opened
	bool m_opening = false;
};
