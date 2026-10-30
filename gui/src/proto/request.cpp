#include "proto/request.h"

#include <QJsonArray>

using namespace Qt::StringLiterals;

namespace refgui::proto::request
{
namespace
{

QJsonObject op(const QString& name)
{
	return {{u"op"_s, name}};
}

QJsonObject toJson(const ModelRef& model)
{
	QJsonObject json{{u"provider"_s, model.provider}, {u"model"_s, model.model}};
	if(model.variant)
	{
		json.insert(u"variant"_s, *model.variant);
	}
	return json;
}

QString toJson(NotifyFrame::Kind kind)
{
	switch(kind)
	{
	case NotifyFrame::Kind::Permission:
		return u"permission"_s;
	case NotifyFrame::Kind::TurnDone:
		return u"turn_done"_s;
	case NotifyFrame::Kind::TurnError:
		return u"turn_error"_s;
	}
	Q_UNREACHABLE_RETURN(QString());
}

QJsonArray toJson(const QList<NotifyFrame::Kind>& kinds)
{
	QJsonArray json;
	for(const NotifyFrame::Kind kind : kinds)
	{
		json.append(toJson(kind));
	}
	return json;
}

} // namespace

QJsonObject hello(const QString& client, const QString& cwd, const QString& token, const QList<NotifyFrame::Kind>& notify)
{
	QJsonObject json = op(u"hello"_s);
	json.insert(u"client"_s, client);
	json.insert(u"version"_s, ProtocolVersion);
	json.insert(u"cwd"_s, cwd);
	json.insert(u"token"_s, token);
	json.insert(u"notify"_s, toJson(notify));
	return json;
}

QJsonObject createSession(const QString& cwd, const std::optional<QString>& agent, const std::optional<ModelRef>& model)
{
	QJsonObject json = op(u"create_session"_s);
	json.insert(u"cwd"_s, cwd);
	if(agent)
	{
		json.insert(u"agent"_s, *agent);
	}
	if(model)
	{
		json.insert(u"model"_s, toJson(*model));
	}
	return json;
}

QJsonObject attach(const QString& sessionId, bool foreground)
{
	QJsonObject json = op(u"attach"_s);
	json.insert(u"session_id"_s, sessionId);
	json.insert(u"foreground"_s, foreground);
	return json;
}

QJsonObject detach(const QString& sessionId)
{
	QJsonObject json = op(u"detach"_s);
	json.insert(u"session_id"_s, sessionId);
	return json;
}

QJsonObject listSessions()
{
	return op(u"list_sessions"_s);
}

QJsonObject listHistory(const QString& cwd, int limit)
{
	QJsonObject json = op(u"list_history"_s);
	json.insert(u"cwd"_s, cwd);
	json.insert(u"limit"_s, limit);
	return json;
}

QJsonObject prompt(const QString& sessionId, const QString& text, const QList<ImageData>& images)
{
	QJsonArray imagesJson;
	for(const ImageData& image : images)
	{
		imagesJson.append(QJsonObject{{u"mime"_s, image.mime}, {u"data"_s, image.data}});
	}
	QJsonObject json = op(u"prompt"_s);
	json.insert(u"session_id"_s, sessionId);
	json.insert(u"text"_s, text);
	json.insert(u"images"_s, imagesJson);
	return json;
}

QJsonObject removeQueued(const QString& sessionId, const QString& messageId)
{
	QJsonObject json = op(u"remove_queued"_s);
	json.insert(u"session_id"_s, sessionId);
	json.insert(u"message_id"_s, messageId);
	return json;
}

QJsonObject undo(const QString& sessionId, const std::optional<QString>& messageId)
{
	QJsonObject json = op(u"undo"_s);
	json.insert(u"session_id"_s, sessionId);
	if(messageId)
	{
		json.insert(u"message_id"_s, *messageId);
	}
	return json;
}

QJsonObject shell(const QString& sessionId, const QString& command)
{
	QJsonObject json = op(u"shell"_s);
	json.insert(u"session_id"_s, sessionId);
	json.insert(u"command"_s, command);
	return json;
}

QJsonObject listFiles(const QString& cwd, const QString& query, int limit)
{
	QJsonObject json = op(u"list_files"_s);
	json.insert(u"cwd"_s, cwd);
	json.insert(u"query"_s, query);
	json.insert(u"limit"_s, limit);
	return json;
}

QJsonObject expandCommand(const QString& name, const QString& args, const QString& cwd)
{
	QJsonObject json = op(u"expand_command"_s);
	json.insert(u"name"_s, name);
	json.insert(u"args"_s, args);
	json.insert(u"cwd"_s, cwd);
	return json;
}

QJsonObject setNotify(const QList<NotifyFrame::Kind>& kinds)
{
	QJsonObject json = op(u"set_notify"_s);
	json.insert(u"kinds"_s, toJson(kinds));
	return json;
}

QJsonObject setFocus(bool focused, const QStringList& sessions)
{
	QJsonObject json = op(u"set_focus"_s);
	json.insert(u"focused"_s, focused);
	json.insert(u"sessions"_s, QJsonArray::fromStringList(sessions));
	return json;
}

QJsonObject abort(const QString& sessionId)
{
	QJsonObject json = op(u"abort"_s);
	json.insert(u"session_id"_s, sessionId);
	return json;
}

QJsonObject permissionReply(const QString& sessionId, const QString& callId, bool allow, bool always)
{
	QJsonObject json = op(u"permission_reply"_s);
	json.insert(u"session_id"_s, sessionId);
	json.insert(u"call_id"_s, callId);
	json.insert(u"allow"_s, allow);
	json.insert(u"always"_s, always);
	return json;
}

QJsonObject getConfig()
{
	return op(u"get_config"_s);
}

QJsonObject watchAll(bool enabled)
{
	QJsonObject json = op(u"watch_all"_s);
	json.insert(u"enabled"_s, enabled);
	return json;
}

QJsonObject setModel(const QString& sessionId, const ModelRef& model)
{
	QJsonObject json = op(u"set_model"_s);
	json.insert(u"session_id"_s, sessionId);
	json.insert(u"model"_s, toJson(model));
	return json;
}

QJsonObject setAgent(const QString& sessionId, const QString& agent)
{
	QJsonObject json = op(u"set_agent"_s);
	json.insert(u"session_id"_s, sessionId);
	json.insert(u"agent"_s, agent);
	return json;
}

QJsonObject setVariant(const QString& sessionId, const std::optional<QString>& variant)
{
	QJsonObject json = op(u"set_variant"_s);
	json.insert(u"session_id"_s, sessionId);
	if(variant)
	{
		json.insert(u"variant"_s, *variant);
	}
	return json;
}

QJsonObject rename(const QString& sessionId, const QString& name)
{
	QJsonObject json = op(u"rename"_s);
	json.insert(u"session_id"_s, sessionId);
	json.insert(u"name"_s, name);
	return json;
}

QJsonObject setCwd(const QString& sessionId, const QString& path)
{
	QJsonObject json = op(u"set_cwd"_s);
	json.insert(u"session_id"_s, sessionId);
	json.insert(u"path"_s, path);
	return json;
}

QJsonObject compact(const QString& sessionId)
{
	QJsonObject json = op(u"compact"_s);
	json.insert(u"session_id"_s, sessionId);
	return json;
}

QJsonObject close(const QString& sessionId)
{
	QJsonObject json = op(u"close"_s);
	json.insert(u"session_id"_s, sessionId);
	return json;
}

QJsonObject deleteSession(const QString& sessionId)
{
	QJsonObject json = op(u"delete"_s);
	json.insert(u"session_id"_s, sessionId);
	return json;
}

QJsonObject exportTranscript(const QString& sessionId, const QString& path)
{
	QJsonObject json = op(u"export"_s);
	json.insert(u"session_id"_s, sessionId);
	json.insert(u"path"_s, path);
	return json;
}

QJsonObject peerCommand(const QString& sessionId, const QString& command, const QString& args)
{
	QJsonObject json = op(u"peer_command"_s);
	json.insert(u"session_id"_s, sessionId);
	json.insert(u"command"_s, command);
	json.insert(u"args"_s, args);
	return json;
}

QJsonObject mcpStatus(const std::optional<QString>& cwd)
{
	QJsonObject json = op(u"mcp_status"_s);
	if(cwd)
	{
		json.insert(u"cwd"_s, *cwd);
	}
	return json;
}

QJsonObject listDirs(const QString& path)
{
	QJsonObject json = op(u"list_dirs"_s);
	json.insert(u"path"_s, path);
	return json;
}

QJsonObject messages(const QString& sessionId, const QString& before, int limit)
{
	QJsonObject json = op(u"messages"_s);
	json.insert(u"session_id"_s, sessionId);
	json.insert(u"before"_s, before);
	json.insert(u"limit"_s, limit);
	return json;
}

QJsonObject getImage(const QString& sessionId, const QString& imageId)
{
	QJsonObject json = op(u"get_image"_s);
	json.insert(u"session_id"_s, sessionId);
	json.insert(u"image_id"_s, imageId);
	return json;
}

QJsonObject shutdown()
{
	return op(u"shutdown"_s);
}

QJsonObject ping()
{
	return op(u"ping"_s);
}

} // namespace refgui::proto::request
