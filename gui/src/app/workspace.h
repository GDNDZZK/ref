#pragma once

#include "app/chat_session.h"
#include "cli/launch_options.h"
#include "client/backend.h"

#include <QCoroTask>
#include <QHash>
#include <QObject>
#include <QtQmlIntegration>

class QJSEngine;
class QQmlEngine;

// Everything the window shows, above single sessions: the connection to the backend, the
// sessions opened in this window and which one is current. The one instance is created in
// main() and handed to QML as the singleton `Workspace`.
class Workspace : public QObject
{
	Q_OBJECT
	QML_ELEMENT
	QML_SINGLETON
	Q_PROPERTY(ChatSession* current READ current NOTIFY currentChanged)
	Q_PROPERTY(QString currentId READ currentId NOTIFY currentChanged)
	Q_PROPERTY(QString connectionState READ connectionState NOTIFY connectionChanged)
	Q_PROPERTY(QString connectionText READ connectionText NOTIFY connectionChanged)
	Q_PROPERTY(QString failureText READ failureText NOTIFY failureTextChanged)
	Q_PROPERTY(QString cwd READ cwd NOTIFY cwdChanged)
	Q_PROPERTY(QStringList visibleSessionIds READ visibleSessionIds NOTIFY visibleSessionIdsChanged)
	Q_PROPERTY(bool windowActive READ isWindowActive WRITE setWindowActive NOTIFY windowActiveChanged)

public:
	explicit Workspace(refgui::cli::LaunchOptions options, QObject* parent = nullptr);
	~Workspace() override;

	// QML gets the instance given to setInstance().
	static void setInstance(Workspace* instance);
	static Workspace* create(QQmlEngine* qmlEngine, QJSEngine* jsEngine);

	ChatSession* current() const { return m_current; }
	QString currentId() const;
	// "connecting", "starting", "connected" or "disconnected".
	QString connectionState() const;
	// What to tell the user about the connection. Empty while connected.
	QString connectionText() const;
	// Why the session could not be opened. Empty when all is well.
	QString failureText() const { return m_failure; }
	// The directory new sessions start in.
	const QString& cwd() const { return m_cwd; }
	// The sessions the user can see now: the current one, or every tile in split view.
	QStringList visibleSessionIds() const;
	// The window is in the foreground. Set by the window; the notification logic needs it with the above.
	bool isWindowActive() const { return m_windowActive; }
	void setWindowActive(bool active);

signals:
	void currentChanged();
	void connectionChanged();
	void failureTextChanged();
	void cwdChanged();
	void visibleSessionIdsChanged();
	void windowActiveChanged();
	// A message for the user that is not about one session. `level` is info, warn or error.
	void toastRequested(const QString& level, const QString& text);
	// The first session is open.
	void opened(const QString& sessionId);
	// The first session could not be opened.
	void openFailed(const QString& reason);

private:
	QCoro::Task<> openFirstSession();
	QCoro::Task<refgui::client::Result<void>> show(const QString& sessionId);
	ChatSession& chat(const QString& sessionId);
	void setFailure(const QString& text);

	refgui::cli::LaunchOptions m_options;
	QString m_cwd;
	// The chat sessions refer to the backend, so it is declared first.
	refgui::client::Backend m_backend;
	QHash<QString, ChatSession*> m_chats;
	ChatSession* m_current = nullptr;
	QString m_failure;
	bool m_started = false;
	bool m_windowActive = false;
};
