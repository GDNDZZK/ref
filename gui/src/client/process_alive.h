#pragma once

#include <QtTypes>

namespace refgui::client
{

// Whether a process with this id exists, even one we may not signal.
bool processIsAlive(quint32 pid);

} // namespace refgui::client
