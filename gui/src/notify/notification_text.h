#pragma once

#include "notify/notifier.h"

namespace refgui::notify
{

// The notification for a Notify frame. The backend sends the facts (kind, session title,
// tool name or error text); the words are made here, in the interface language.
Notification makeNotification(const proto::NotifyFrame& frame);

} // namespace refgui::notify
