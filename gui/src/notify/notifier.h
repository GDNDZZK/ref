#pragma once

#include "proto/types.h"

#include <QObject>
#include <QString>

#include <memory>

namespace refgui::notify
{

// A system notification to show.
struct Notification
{
	QString sessionId; // what a click on it opens
	proto::NotifyFrame::Kind kind = proto::NotifyFrame::Kind::TurnDone;
	QString title;
	QString body;
};

// Shows system notifications on one platform. When `isAvailable()` is false, `show()` does
// nothing and the caller falls back to getting the user's attention another way.
class Notifier : public QObject
{
	Q_OBJECT

public:
	using QObject::QObject;

	virtual bool isAvailable() const = 0;
	// Why not, for the settings panel; empty while available.
	virtual QString unavailableReason() const = 0;
	virtual void show(const Notification& notification) = 0;

signals:
	void availabilityChanged();
	// The user clicked the notification of this session.
	void activated(const QString& sessionId);
	// The notification could not be shown although the notifier was available.
	void showFailed(const refgui::notify::Notification& notification, const QString& reason);
};

// The notifier of this platform: D-Bus on Linux and other Unix systems, toasts on Windows,
// UserNotifications on macOS.
std::unique_ptr<Notifier> createPlatformNotifier(QObject* parent = nullptr);

} // namespace refgui::notify
