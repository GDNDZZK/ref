#pragma once

#include "client/endpoint.h"
#include "proto/types.h"

#include <QList>
#include <QString>

// The instance files ref-server writes, which is how frontends find a running backend.
namespace refgui::client
{

// $REF_STATE_DIR, else $XDG_STATE_HOME/ref, else ~/.local/state/ref (on every platform,
// like crates/proto).
QString stateDirectory();
QString instancesDirectory();

// The instances in `directory` whose process is alive, newest first. Files of dead
// processes are deleted; files that are not instance files are left alone.
QList<proto::InstanceInfo> liveInstances(const QString& directory);

// The address to connect to: a wildcard bind address becomes loopback.
Endpoint endpointFor(const proto::InstanceInfo& instance);

} // namespace refgui::client
