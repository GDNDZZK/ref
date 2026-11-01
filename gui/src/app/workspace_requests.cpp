// The requests of Workspace that manage a session: rename, model, agent, export and so on.
#include "app/workspace.h"

#include <QJSEngine>

#include <functional>
#include <type_traits>

using refgui::client::Result;
using refgui::proto::ModelRef;
using refgui::proto::SessionInfo;
using namespace Qt::StringLiterals;

namespace
{

// The onOk of a request whose answer the caller does not get.
constexpr auto noPayload = [] { return QVariant(); };

} // namespace

Workspace::Reply Workspace::wrap(const QJSValue& cb) const
{
	if(!cb.isCallable() || m_engine == nullptr)
	{
		return {};
	}
	return [this, cb](bool ok, const QVariant& payload) {
		QJSValue callback = cb;
		const QJSValue result = callback.call({ok, m_engine->toScriptValue(payload)});
		if(result.isError())
		{
			qWarning().noquote() << u"A reply callback failed: %1 (%2:%3)"_s.arg(result.property(u"message"_s).toString(), result.property(u"fileName"_s).toString(), result.property(u"lineNumber"_s).toString());
		}
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
		toast(refgui::workspace::ToastLevel::Error, payload.toString());
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

// The onOk of a request that answers with the session's new info.
QVariant Workspace::applyInfo(const SessionInfo& info)
{
	m_directory.update(info);
	return {};
}

void Workspace::renameSession(const QString& sessionId, const QString& name, const QJSValue& cb)
{
	perform(m_backend.client().rename(sessionId, name), wrap(cb), std::bind_front(&Workspace::applyInfo, this));
}

void Workspace::setSessionCwd(const QString& sessionId, const QString& path, const QJSValue& cb)
{
	perform(m_backend.client().setCwd(sessionId, path), wrap(cb), std::bind_front(&Workspace::applyInfo, this));
}

void Workspace::setModel(const QString& sessionId, const QString& provider, const QString& model, const QString& variant)
{
	ModelRef ref{provider, model, variant.isEmpty() ? std::nullopt : std::optional<QString>(variant)};
	perform(m_backend.client().setModel(sessionId, std::move(ref)), {}, std::bind_front(&Workspace::applyInfo, this));
}

void Workspace::setAgent(const QString& sessionId, const QString& agent)
{
	perform(m_backend.client().setAgent(sessionId, agent), {}, std::bind_front(&Workspace::applyInfo, this));
}

void Workspace::setVariant(const QString& sessionId, const QString& variant)
{
	const std::optional<QString> wanted = variant.isEmpty() ? std::nullopt : std::optional<QString>(variant);
	perform(m_backend.client().setVariant(sessionId, wanted), {}, std::bind_front(&Workspace::applyInfo, this));
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
	perform(m_backend.client().compact(sessionId), {}, noPayload);
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
	// The list is reloaded either way: on failure that brings the row back.
	const Reply reloaded = [this](bool ok, const QVariant& payload) {
		m_directory.refreshHistory();
		if(!ok)
		{
			toast(refgui::workspace::ToastLevel::Error, payload.toString());
		}
	};
	perform(m_backend.client().deleteSession(sessionId), reloaded, noPayload);
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
	perform(m_backend.client().shutdown(), [](bool, const QVariant&) {}, noPayload);
}
