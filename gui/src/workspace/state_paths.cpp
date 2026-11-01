#include "workspace/state_paths.h"

#include "client/instance_files.h"

namespace refgui::workspace
{

QString stateFilePath(const QString& name)
{
	return client::stateDirectory() + u'/' + name;
}

} // namespace refgui::workspace
