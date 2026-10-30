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

} // namespace

QJsonObject hello(const QString& client, const QString& cwd, const QString& token)
{
	QJsonObject json = op(u"hello"_s);
	json.insert(u"client"_s, client);
	json.insert(u"version"_s, ProtocolVersion);
	json.insert(u"cwd"_s, cwd);
	json.insert(u"token"_s, token);
	json.insert(u"notify"_s, QJsonArray()); // system notifications are not wired up yet
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

QJsonObject prompt(const QString& sessionId, const QString& text)
{
	QJsonObject json = op(u"prompt"_s);
	json.insert(u"session_id"_s, sessionId);
	json.insert(u"text"_s, text);
	json.insert(u"images"_s, QJsonArray());
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

} // namespace refgui::proto::request
