// Adapted from an early version of qutie by LightBlueCube
#pragma once

#include "app/composer_model_type.h"
#include "client/backend.h"
#include "composer/composer_model.h"
#include "session/session_controller.h"

#include <QCoroTask>
#include <QObject>
#include <QSet>
#include <QTimer>
#include <QtQmlIntegration>

#include <chrono>

// One session as the window shows it: the conversation and the actions on it. Workspace
// creates one for each session opened in the window and keeps it until the session is
// closed; while the session is not shown it is detached but keeps its state.
class ChatSession : public QObject
{
	Q_OBJECT
	QML_ELEMENT
	QML_UNCREATABLE("Created by Workspace.")
	Q_PROPERTY(QString sessionId READ sessionId CONSTANT)
	Q_PROPERTY(QAbstractItemModel* blocks READ blocks CONSTANT)
	Q_PROPERTY(bool busy READ isBusy NOTIFY busyChanged)
	Q_PROPERTY(bool ready READ isReady NOTIFY readyChanged)
	Q_PROPERTY(QString statusText READ statusText NOTIFY statusTextChanged)
	Q_PROPERTY(QString title READ title NOTIFY titleChanged)
	Q_PROPERTY(refgui::composer::ComposerModel* composer READ composer CONSTANT)
	// What the session is doing: "idle", "thinking", "running", "waiting_permission", "retrying" or "compacting".
	Q_PROPERTY(QString activity READ activity NOTIFY infoChanged)
	Q_PROPERTY(qint64 busySince READ busySince NOTIFY infoChanged) // epoch ms, 0 while idle
	Q_PROPERTY(int retryAttempt READ retryAttempt NOTIFY infoChanged)
	Q_PROPERTY(qint64 retryAt READ retryAt NOTIFY infoChanged) // epoch ms
	Q_PROPERTY(QString retryError READ retryError NOTIFY infoChanged)
	Q_PROPERTY(qint64 contextUsed READ contextUsed NOTIFY infoChanged)
	Q_PROPERTY(qint64 contextMax READ contextMax NOTIFY infoChanged)
	Q_PROPERTY(qint64 contextRecommend READ contextRecommend NOTIFY infoChanged)
	Q_PROPERTY(qint64 contextAuto READ contextAuto NOTIFY infoChanged)
	Q_PROPERTY(bool moreBefore READ hasMoreBefore NOTIFY moreBeforeChanged)
	Q_PROPERTY(bool loadingOlder READ isLoadingOlder NOTIFY loadingOlderChanged)
	Q_PROPERTY(QString olderError READ olderError NOTIFY olderErrorChanged) // why the last page did not load
	Q_PROPERTY(bool thinkingExpanded READ thinkingExpanded NOTIFY expansionChanged)
	Q_PROPERTY(bool detailsExpanded READ detailsExpanded NOTIFY expansionChanged)
	// The newest tool call waits out its start delay; Esc in the input box cancels it.
	Q_PROPERTY(bool lastToolDelayed READ lastToolDelayed NOTIFY toolsChanged)
	// Tool calls waiting for the user's permission, the oldest first.
	Q_PROPERTY(int permissionCount READ permissionCount NOTIFY toolsChanged)
	// The keys a, s, d, Enter and Esc may answer the oldest request: it has been shown for permissionGuard.
	Q_PROPERTY(bool permissionKeysActive READ permissionKeysActive NOTIFY toolsChanged)

public:
	// How long a request is shown before the keys answer it: the user may still be typing.
	static constexpr std::chrono::milliseconds PermissionGuard{350};

	// `backend` is shared by all sessions of the window and outlives them.
	ChatSession(refgui::client::Backend& backend, QString sessionId, QObject* parent = nullptr);

	const QString& sessionId() const { return m_sessionId; }
	bool isAttached() const { return m_controller.isAttached(); }

	// Shows the session's conversation: it replaces the model with the backend's snapshot.
	// A tile watches (`foreground` false); the session in the window's main view is in the foreground.
	QCoro::Task<refgui::client::Result<void>> attach(bool foreground);
	QCoro::Task<refgui::client::Result<void>> detach();
	// The bytes of a picture of this session, e.g. an attachment or a tool's image.
	QCoro::Task<refgui::client::Result<QByteArray>> fetchImage(QString imageId);
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
	// The input box of this session.
	refgui::composer::ComposerModel* composer() { return &m_composer; }
	QString activity() const;
	qint64 busySince() const;
	int retryAttempt() const;
	qint64 retryAt() const;
	QString retryError() const;
	qint64 contextUsed() const;
	qint64 contextMax() const;
	qint64 contextRecommend() const;
	qint64 contextAuto() const;
	bool hasMoreBefore() const { return m_controller.hasMoreBefore(); }
	bool isLoadingOlder() const { return m_controller.isLoadingOlder(); }
	const QString& olderError() const { return m_olderError; }
	bool thinkingExpanded() const;
	bool detailsExpanded() const;
	bool lastToolDelayed() const;
	int permissionCount() const;
	bool permissionKeysActive() const { return m_permissionKeysActive; }

	// Precondition: !busy and `text` is not blank.
	Q_INVOKABLE void send(const QString& text);
	// Precondition: busy.
	Q_INVOKABLE void stop();
	// Answers a tool call that waits for permission.
	Q_INVOKABLE void permissionReply(const QString& callId, bool allow, bool always);
	// Answers the oldest request by the keys. Does nothing unless permissionKeysActive.
	Q_INVOKABLE void answerOldestPermission(bool allow, bool always);
	// Offers a key press to the pending request, as the TUI and the WebUI do: a, Enter, s, d and Esc
	// answer it, and any other typing is the request's until it is answered. True when the key is used.
	// Always false while the keys are not active, so the input box takes the typing meanwhile.
	Q_INVOKABLE bool permissionKey(int key, const QString& text, int modifiers, bool autoRepeat);
	// The same as the Cancel button on a delayed tool call.
	Q_INVOKABLE void cancelDelayedTool();
	// Takes back a message of the user and everything after it; the text and pictures return to
	// the input box (draftRestored).
	Q_INVOKABLE void undo(const QString& messageId);
	// The same for the last message of the user.
	Q_INVOKABLE void undoLast();
	// Adds the page of messages before the oldest one.
	Q_INVOKABLE void loadOlder();
	// Puts the text of a message on the clipboard.
	Q_INVOKABLE void copyMessage(const QString& messageId);
	// For the commands copy, thinking and details.
	Q_INVOKABLE void copyLastReply();
	Q_INVOKABLE void toggleReasoning();
	Q_INVOKABLE void toggleDetails();

signals:
	void busyChanged();
	void readyChanged();
	void statusTextChanged();
	void titleChanged();
	void infoChanged();
	void moreBeforeChanged();
	void loadingOlderChanged();
	void olderErrorChanged();
	void expansionChanged();
	void toolsChanged();
	// The backend reported a problem with the session (an Error event).
	void errorReported(const QString& text);
	// Undo took a prompt back: its text and pictures, for the input box.
	void draftRestored(const QString& text, const QList<refgui::proto::NamedImage>& images);

private:
	QCoro::Task<> sendPrompt(QString text);
	QCoro::Task<> sendAbort();
	QCoro::Task<> fetchOlder();
	QCoro::Task<> sendUndo(std::optional<QString> messageId);
	QCoro::Task<> sendPermission(QString callId, bool allow, bool always);
	void setFailure(const QString& text);
	void trackOldestPermission();

	refgui::client::Backend& m_backend;
	QString m_sessionId;
	refgui::session::SessionController m_controller;
	refgui::composer::ComposerModel m_composer;
	QString m_failure; // the last thing that went wrong, until the next action
	QString m_olderError;
	QSet<QString> m_repliedCalls; // permission requests that have an answer on its way
	QString m_guardedCallId; // the request the guard timer runs for
	QTimer m_permissionGuard;
	bool m_permissionKeysActive = false;
};
