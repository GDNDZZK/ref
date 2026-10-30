#pragma once

#include "notify/notifier.h"

#include <memory>

namespace refgui::notify
{

// macOS notifications through the UserNotifications framework. Works for an app bundle
// (ref.app); asks the user for permission on first use. Unavailable when run outside a
// bundle, when the user has refused, or when macOS rejects the app (for instance an unsigned
// one).
class UserNotificationsNotifier : public Notifier
{
	Q_OBJECT

public:
	explicit UserNotificationsNotifier(QObject* parent = nullptr);
	~UserNotificationsNotifier() override;

	bool isAvailable() const override;
	QString unavailableReason() const override;
	void show(const Notification& notification) override;

private:
	struct State;
	std::unique_ptr<State> m_state;
};

} // namespace refgui::notify
