#pragma once

#include "proto/types.h"

#include <QString>
#include <QStringView>

#include <optional>

namespace refgui::proto
{

// The wire name of a notification kind, as `notify_kind` and the Hello `notify` list spell it.
QString notifyKindName(NotifyFrame::Kind kind);

// The kind a wire name stands for; none for a name this client does not know.
std::optional<NotifyFrame::Kind> notifyKindFromName(QStringView name);

} // namespace refgui::proto
