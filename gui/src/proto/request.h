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
QJsonObject getConfig();
QJsonObject watchAll(bool enabled);
QJsonObject setModel(const QString& sessionId, const ModelRef& model);
QJsonObject setAgent(const QString& sessionId, const QString& agent);
// An empty `variant` rotates to the model's next one.
QJsonObject setVariant(const QString& sessionId, const std::optional<QString>& variant);
QJsonObject rename(const QString& sessionId, const QString& name);
QJsonObject setCwd(const QString& sessionId, const QString& path);
QJsonObject compact(const QString& sessionId);
QJsonObject close(const QString& sessionId);
QJsonObject deleteSession(const QString& sessionId);
QJsonObject exportTranscript(const QString& sessionId, const QString& path);
QJsonObject peerCommand(const QString& sessionId, const QString& command, const QString& args);
QJsonObject mcpStatus(const std::optional<QString>& cwd);
QJsonObject listDirs(const QString& path);
QJsonObject shutdown();

} // namespace refgui::proto::request
