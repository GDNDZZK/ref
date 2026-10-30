#pragma once

#include "client/backend.h"
#include "notify/notifier.h"

#include <QCoroTask>
#include <QObject>
#include <QPointer>
#include <QStringList>
#include <QWindow>

#include <memory>

namespace refgui::notify
{

// Tells the backend what the window wants to hear about and where the user is looking, and
// shows what the backend sends back. The backend decides who gets a notification (not the
// window with the user's eyes on the session); this shows it as a system notification, or,
// where those are not available, makes the window ask for attention (the taskbar flashes).
class NotificationCenter : public QObject
{
	Q_OBJECT
	Q_PROPERTY(QStringList kinds READ kinds WRITE setKinds NOTIFY kindsChanged)
	Q_PROPERTY(bool systemNotificationsAvailable READ systemNotificationsAvailable NOTIFY availabilityChanged)
	Q_PROPERTY(QString unavailableReason READ unavailableReason NOTIFY availabilityChanged)

public:
	// The backend outlives the center.
	NotificationCenter(client::Backend& backend, std::unique_ptr<Notifier> notifier, QObject* parent = nullptr);

	// The wire names of the kinds the user wants: permission, turn_done, turn_error. Unknown
	// names are ignored.
	const QStringList& kinds() const { return m_kindNames; }
	void setKinds(const QStringList& kinds);

	// Where the user is looking: whether the window has their attention, and the sessions
	// it shows. The backend does not notify about those.
	Q_INVOKABLE void setFocus(bool focused, const QStringList& sessions);

	// The window to raise on a click and to flash when there is no system notification.
	void setWindow(QWindow* window) { m_window = window; }

	bool systemNotificationsAvailable() const { return m_notifier->isAvailable(); }
	QString unavailableReason() const { return m_notifier->unavailableReason(); }

signals:
	void kindsChanged();
	void availabilityChanged();
	// The user clicked the notification of this session; the window has been raised.
	void sessionRequested(const QString& sessionId);

private:
	void onFrame(const proto::ServerFrame& frame);
	void getAttention();
	void raiseWindow();
	QCoro::Task<> sendKinds();
	QCoro::Task<> sendFocus();

	client::Backend& m_backend;
	std::unique_ptr<Notifier> m_notifier;
	QPointer<QWindow> m_window;
	QStringList m_kindNames;
	QList<proto::NotifyFrame::Kind> m_kinds;
	bool m_focused = false;
	QStringList m_sessions;
	bool m_focusKnown = false; // setFocus() was called, so the backend has something to be told
};

} // namespace refgui::notify
