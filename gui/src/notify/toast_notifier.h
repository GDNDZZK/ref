#pragma once

#include "notify/notifier.h"

#include <memory>

namespace refgui::notify
{

// Windows toast notifications. A desktop program that is not installed through a package
// needs an AppUserModelID registered under HKCU to be allowed to show toasts; the notifier
// registers it on first use (no administrator rights needed). A click on a toast
// activates its session while ref is running.
class ToastNotifier : public Notifier
{
	Q_OBJECT

public:
	explicit ToastNotifier(QObject* parent = nullptr);
	~ToastNotifier() override;

	bool isAvailable() const override;
	QString unavailableReason() const override;
	void show(const Notification& notification) override;

private:
	struct State;
	std::unique_ptr<State> m_state;
};

} // namespace refgui::notify
