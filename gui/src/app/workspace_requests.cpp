// The requests of Workspace that manage a session: rename, model, agent, export and so on.
#include "app/workspace.h"

#include <QJSEngine>

#include <type_traits>

using refgui::client::Result;
using refgui::proto::ModelRef;
using refgui::proto::SessionInfo;
using namespace Qt::StringLiterals;

Workspace::Reply Workspace::wrap(const QJSValue& cb) const
{
	if(!cb.isCallable() || m_engine == nullptr)
	{
		return {};
	}
	return [this, cb](bool ok, const QVariant& payload) {
		QJSValue callback = cb;
		callback.call({ok, m_engine->toScriptValue(payload)});
	};
}

// Hands the outcome to the caller; a failure nobody asked about becomes a toast.
void Workspace::report(const Reply& reply, bool ok, const QVariant& payload)
{
	if(reply)
	{
		reply(ok, payload);
		return;
	}
	if(!ok)
	{
		toast(u"error"_s, payload.toString());
	}
}

// Waits for `request`, runs `onOk` with its answer (it returns what the callback gets) and reports.
template<typename T, typename OnOk>
QCoro::Task<> Workspace::perform(QCoro::Task<Result<T>> request, Reply reply, OnOk onOk)
{
	const Result<T> result = co_await std::move(request);
	if(!result)
	{
		report(reply, false, result.error());
		co_return;
	}
	if constexpr(std::is_void_v<T>)
	{
		report(reply, true, onOk());
	}
	else
	{
		report(reply, true, onOk(*result));
	}
}

void Workspace::renameSession(const QString& sessionId, const QString& name, const QJSValue& cb)
{
	perform(m_backend.client().rename(sessionId, name), wrap(cb), [this](const SessionInfo& info) {
		m_directory.update(info);
		return QVariant();
	});
}

void Workspace::setSessionCwd(const QString& sessionId, const QString& path, const QJSValue& cb)
{
	perform(m_backend.client().setCwd(sessionId, path), wrap(cb), [this](const SessionInfo& info) {
		m_directory.update(info);
		return QVariant();
	});
}

void Workspace::setModel(const QString& sessionId, const QString& provider, const QString& model, const QString& variant)
{
	ModelRef ref{provider, model, variant.isEmpty() ? std::nullopt : std::optional<QString>(variant)};
	perform(m_backend.client().setModel(sessionId, std::move(ref)), {}, [this](const SessionInfo& info) {
		m_directory.update(info);
		return QVariant();
	});
}

void Workspace::setAgent(const QString& sessionId, const QString& agent)
{
	perform(m_backend.client().setAgent(sessionId, agent), {}, [this](const SessionInfo& info) {
		m_directory.update(info);
		return QVariant();
	});
}

void Workspace::setVariant(const QString& sessionId, const QString& variant)
{
	const std::optional<QString> wanted = variant.isEmpty() ? std::nullopt : std::optional<QString>(variant);
	perform(m_backend.client().setVariant(sessionId, wanted), {}, [this](const SessionInfo& info) {
		m_directory.update(info);
		return QVariant();
	});
}

void Workspace::cycleAgent(const QString& sessionId, int direction)
{
	const QVariantList agents = m_config.agents();
	const refgui::proto::SessionInfo* info = m_directory.find(sessionId);
	if(agents.isEmpty() || info == nullptr)
	{
		return;
	}
	qsizetype index = -1;
	for(qsizetype i = 0; i < agents.size(); ++i)
	{
		if(agents[i].toMap().value(u"name"_s).toString() == info->agent)
		{
			index = i;
			break;
		}
	}
	const qsizetype count = agents.size();
	const qsizetype next = ((index + direction) % count + count) % count;
	setAgent(sessionId, agents[next].toMap().value(u"name"_s).toString());
}

void Workspace::compact(const QString& sessionId)
{
	perform(m_backend.client().compact(sessionId), {}, [] { return QVariant(); });
}

void Workspace::exportTranscript(const QString& sessionId, const QString& path, const QJSValue& cb)
{
	exportTo(sessionId, path, wrap(cb));
}

void Workspace::exportTo(const QString& sessionId, const QString& path, Reply reply)
{
	perform(m_backend.client().exportTranscript(sessionId, path), std::move(reply), [](const QString& written) { return QVariant(written); });
}

void Workspace::peerCommand(const QString& sessionId, const QString& command, const QString& args, const QJSValue& cb)
{
	peerText(sessionId, command, args, wrap(cb));
}

void Workspace::peerText(const QString& sessionId, const QString& command, const QString& args, Reply reply)
{
	perform(m_backend.client().peerCommand(sessionId, command, args), std::move(reply), [](const QString& text) { return QVariant(text); });
}

void Workspace::listDirs(const QString& path, const QJSValue& cb)
{
	perform(m_backend.client().listDirs(path), wrap(cb), [](const refgui::proto::DirListing& listing) {
		return QVariant(QVariantMap{{u"path"_s, listing.path}, {u"dirs"_s, listing.dirs}});
	});
}

void Workspace::deleteHistory(const QString& sessionId)
{
	m_directory.forgetPersisted(sessionId);
	perform(m_backend.client().deleteSession(sessionId), {}, [this] {
		m_directory.refreshHistory();
		return QVariant();
	});
}

// ---- Backend --------------------------------------------------------------------------

void Workspace::restartBackend()
{
	if(!canRestartBackend())
	{
		return;
	}
	// The connection drops with the backend and Backend starts a new one; the request then
	// fails with "connection closed", which is expected.
	perform(m_backend.client().shutdown(), [](bool, const QVariant&) {}, [] { return QVariant(); });
}
