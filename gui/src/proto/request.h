#pragma once

#include "proto/types.h"

#include <QJsonObject>
#include <QList>
#include <QString>
#include <QStringList>

#include <optional>

// Request bodies, `{"op": ..., ...}`. The client adds the correlation id when it sends one.
namespace refgui::proto::request
{

// `notify`: the kinds of system notification this connection wants (none by default).
QJsonObject hello(const QString& client, const QString& cwd, const QString& token, const QList<NotifyFrame::Kind>& notify = {});
QJsonObject createSession(const QString& cwd, const std::optional<QString>& agent, const std::optional<ModelRef>& model);
QJsonObject attach(const QString& sessionId, bool foreground);
QJsonObject detach(const QString& sessionId);
QJsonObject listSessions();
QJsonObject listHistory(const QString& cwd, int limit);
QJsonObject prompt(const QString& sessionId, const QString& text, const QList<ImageData>& images = {});
QJsonObject removeQueued(const QString& sessionId, const QString& messageId);
QJsonObject shell(const QString& sessionId, const QString& command);
QJsonObject listFiles(const QString& cwd, const QString& query, int limit);
QJsonObject expandCommand(const QString& name, const QString& args, const QString& cwd);
QJsonObject setNotify(const QList<NotifyFrame::Kind>& kinds);
QJsonObject setFocus(bool focused, const QStringList& sessions);
QJsonObject abort(const QString& sessionId);
QJsonObject permissionReply(const QString& sessionId, const QString& callId, bool allow, bool always);

} // namespace refgui::proto::request
