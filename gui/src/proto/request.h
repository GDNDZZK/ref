#pragma once

#include "proto/types.h"

#include <QJsonObject>
#include <QString>

#include <optional>

// Request bodies, `{"op": ..., ...}`. The client adds the correlation id when it sends one.
namespace refgui::proto::request
{

QJsonObject hello(const QString& client, const QString& cwd, const QString& token);
QJsonObject createSession(const QString& cwd, const std::optional<QString>& agent, const std::optional<ModelRef>& model);
QJsonObject attach(const QString& sessionId, bool foreground);
QJsonObject detach(const QString& sessionId);
QJsonObject listSessions();
QJsonObject listHistory(const QString& cwd, int limit);
QJsonObject prompt(const QString& sessionId, const QString& text);
QJsonObject abort(const QString& sessionId);
QJsonObject permissionReply(const QString& sessionId, const QString& callId, bool allow, bool always);

} // namespace refgui::proto::request
