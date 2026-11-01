#pragma once

#include "cli/launch_options.h"

#include <QString>
#include <QStringList>

namespace refgui::cli
{

// `arguments` starts with the program name. Relative directories are taken from `currentDirectory`.
ParseResult parseCommandLine(const QStringList& arguments, const QString& currentDirectory, const QString& version);

} // namespace refgui::cli
