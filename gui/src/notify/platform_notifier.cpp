#include "notify/notifier.h"

#include <QtSystemDetection>

#if defined(Q_OS_WIN)
#include "notify/toast_notifier.h"
#elif defined(Q_OS_MACOS)
#include "notify/user_notifications_notifier.h"
#elif defined(Q_OS_UNIX)
#include "notify/dbus_notifier.h"
#endif

using namespace Qt::StringLiterals;

namespace refgui::notify
{
namespace
{

#if !defined(Q_OS_WIN) && !defined(Q_OS_MACOS) && !defined(Q_OS_UNIX)
// A platform without a notification service we know of.
class NoNotifier : public Notifier
{
public:
	using Notifier::Notifier;

	bool isAvailable() const override { return false; }
	QString unavailableReason() const override { return tr("This system has no notification service ref can use."); }
	void show(const Notification&) override {}
};
#endif

} // namespace

std::unique_ptr<Notifier> createPlatformNotifier(QObject* parent)
{
#if defined(Q_OS_WIN)
	return std::make_unique<ToastNotifier>(parent);
#elif defined(Q_OS_MACOS)
	return std::make_unique<UserNotificationsNotifier>(parent);
#elif defined(Q_OS_UNIX)
	return std::make_unique<DBusNotifier>(QDBusConnection::sessionBus(), parent);
#else
	return std::make_unique<NoNotifier>(parent);
#endif
}

} // namespace refgui::notify
