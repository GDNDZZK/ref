#pragma once

#include "proto/json_reader.h"
#include "proto/types.h"

// Readers shared by the parse functions; they throw json::Failure.
namespace refgui::proto::json
{

ToolState readToolState(const QString& path, const QJsonValue& value);
ModelRef readModelRef(const Fields& fields);
SessionInfo readSessionInfo(const Fields& fields);
Part readPart(const Fields& fields);
Message readMessage(const Fields& fields);
QList<Message> readMessages(const Fields& fields, QLatin1StringView key);
ImageData readImageData(const Fields& fields);
PermissionRequest readPermissionRequest(const Fields& fields);
NamedImage readNamedImage(const Fields& fields);

} // namespace refgui::proto::json
