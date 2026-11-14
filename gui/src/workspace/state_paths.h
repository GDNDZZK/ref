#pragma once

#include <QString>

namespace refgui::workspace
{

// The path of the file `name` in the directory where ref keeps its state.
QString stateFilePath(const QString& name);

} // namespace refgui::workspace
