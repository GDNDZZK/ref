// The requests of Workspace that manage a session: rename, model, agent, export and so on.
#include "app/workspace.h"

#include "client/result.h"

#include <QJSEngine>
#include <QPointer>
#include <QUrl>

#include <type_traits>

using refgui::client::Result;
using refgui::proto::ModelRef;
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
		report(reply, false, result.error().message);
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
	perform(m_directory.rename(sessionId, name), wrap(cb), noPayload);
}

void Workspace::setSessionCwd(const QString& sessionId, const QString& path, const QJSValue& cb)
{
	perform(m_directory.setCwd(sessionId, path), wrap(cb), noPayload);
}

void Workspace::setModel(const QString& sessionId, const QString& provider, const QString& model, const QString& variant)
{
	ModelRef ref{provider, model, variant.isEmpty() ? std::nullopt : std::optional<QString>(variant)};
	perform(m_directory.setModel(sessionId, std::move(ref)), {}, noPayload);
}

void Workspace::setAgent(const QString& sessionId, const QString& agent)
{
	perform(m_directory.setAgent(sessionId, agent), {}, noPayload);
}

void Workspace::setVariant(const QString& sessionId, const QString& variant)
{
	const std::optional<QString> wanted = variant.isEmpty() ? std::nullopt : std::optional<QString>(variant);
	perform(m_directory.setVariant(sessionId, wanted), {}, noPayload);
}

void Workspace::clearVariant(const QString& sessionId)
{
	perform(m_directory.setVariant(sessionId, QString()), {}, noPayload);
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
	perform(m_directory.compact(sessionId), {}, noPayload);
}

void Workspace::exportTranscript(const QString& sessionId, const QString& path, const QJSValue& cb)
{
	exportTo(sessionId, path, wrap(cb));
}

void Workspace::exportTo(const QString& sessionId, const QString& path, Reply reply)
{
	perform(m_directory.exportTranscript(sessionId, path), std::move(reply), [](const QString& written) { return QVariant(written); });
}

void Workspace::peerCommand(const QString& sessionId, const QString& command, const QString& args, const QJSValue& cb)
{
	peerText(sessionId, command, args, wrap(cb));
}

void Workspace::peerText(const QString& sessionId, const QString& command, const QString& args, Reply reply)
{
	perform(m_directory.peerCommand(sessionId, command, args), std::move(reply), [](const QString& text) { return QVariant(text); });
}

void Workspace::listDirs(const QString& path, const QJSValue& cb, const QString& cwd, bool includeFiles)
{
	const std::optional<QString> optCwd = cwd.isEmpty() ? std::nullopt : std::optional<QString>(cwd);
	[](QPointer<Workspace> target, QString targetPath, std::optional<QString> targetCwd, bool wantFiles, QJSValue callback) -> QCoro::Task<> {
		const Result<refgui::proto::DirListing> result = co_await target->m_directory.listDirs(std::move(targetPath), std::move(targetCwd), wantFiles);
		if(!target || !callback.isCallable() || target->m_engine == nullptr)
		{
			co_return;
		}
		QJSValueList args;
		if(result)
		{
			const QVariantMap payload{{u"path"_s, result->path}, {u"dirs"_s, result->dirs}, {u"files"_s, result->files}};
			args = {true, target->m_engine->toScriptValue(QVariant(payload)), QString()};
		}
		else
		{
			args = {false, result.error().message, result.error().code};
		}
		const QJSValue callResult = callback.call(args);
		if(callResult.isError())
		{
			qWarning().noquote() << u"A reply callback failed: %1 (%2:%3)"_s.arg(callResult.property(u"message"_s).toString(), callResult.property(u"fileName"_s).toString(), callResult.property(u"lineNumber"_s).toString());
		}
	}(QPointer<Workspace>(this), path, optCwd, includeFiles, cb);
}

void Workspace::reloadConfig(const QJSValue& cb)
{
	const Reply reply = wrap(cb);
	const QString cwd = effectiveCwd();
	const std::optional<QString> optCwd = cwd.isEmpty() ? std::nullopt : std::optional<QString>(cwd);
	[](QPointer<Workspace> target, std::optional<QString> targetCwd, Reply callback) -> QCoro::Task<> {
		if(!target)
		{
			co_return;
		}
		const Result<refgui::proto::ReloadConfigResult> result = co_await target->m_config.reload(std::move(targetCwd));
		if(!target)
		{
			co_return;
		}
		if(!result)
		{
			target->report(callback, false, result.error().message);
			co_return;
		}
		target->toast(refgui::workspace::ToastLevel::Info, tr("Configuration reloaded"));
		target->toastWarnings(result->warnings);
		if(callback)
		{
			callback(true, QVariant(result->warnings));
		}
	}(QPointer<Workspace>(this), optCwd, reply);
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
	perform(m_directory.remove(sessionId), reloaded, noPayload);
}

// ---- Backend --------------------------------------------------------------------------

void Workspace::restartBackend()
{
	if(!canRestartBackend())
	{
		return;
	}
	// The connection drops with the backend and Backend starts a new one; the request then
	// fails with "connection closed", which is expected. Any other failure is reported.
	const Reply reportRefusal = [this](bool ok, const QVariant& payload) {
		if(!ok && !refgui::client::isConnectionLost(payload.toString()))
		{
			toast(refgui::workspace::ToastLevel::Error, payload.toString());
		}
	};
	perform(m_connection.stopServer(), reportRefusal, noPayload);
}

bool Workspace::isValidBackendUrl(const QString& url) const
{
	const QUrl parsed(url.trimmed());
	return parsed.isValid() && (parsed.scheme() == u"ws"_s || parsed.scheme() == u"wss"_s) && !parsed.host().isEmpty();
}

bool Workspace::switchBackend(const QString& url, const QString& token, bool saveAsDefault, bool keepExistingToken)
{
	const QString trimmedUrl = url.trimmed();
	const QUrl parsed(trimmedUrl);
	if(!parsed.isValid() || (parsed.scheme() != u"ws"_s && parsed.scheme() != u"wss"_s) || parsed.host().isEmpty())
	{
		return false;
	}
	QString effectiveToken = token.trimmed();
	if(effectiveToken.isEmpty() && keepExistingToken)
	{
		effectiveToken = currentEditableToken();
	}
	if(saveAsDefault)
	{
		m_temporaryBackend.reset();
		m_cliRemoteSource = false;
		m_cliMockSource = false;
		m_settings.setBackend(parsed.toString(), effectiveToken);
	}
	else
	{
		m_temporaryBackend = refgui::client::Endpoint{parsed, effectiveToken};
	}
	applyBackendSwitch(refgui::client::Backend::Mode::Remote{{parsed, effectiveToken}});
	return true;
}

void Workspace::restoreDefaultBackend()
{
	m_temporaryBackend.reset();
	m_cliRemoteSource = false;
	m_cliMockSource = false;
	m_settings.setBackend({}, {});
	applyBackendSwitch(refgui::client::Backend::Mode::Shared{});
}

void Workspace::applyBackendSwitch(refgui::client::Backend::Source source)
{
	++m_showSerial;
	m_options.start = {};
	m_options.watch = false;
	m_connection.switchSource(std::move(source));

	const bool wasSplit = m_split;
	const bool wasZoomed = !m_zoomed.isEmpty();
	m_split = false;
	m_zoomed.clear();
	if(!m_focusedTile.isEmpty())
	{
		setFocusedTile({});
	}
	m_tiles.setTiles({});
	m_directory.live().setTileIds({});
	if(wasZoomed)
	{
		emit zoomedChanged();
	}
	if(wasSplit)
	{
		emit splitViewChanged();
	}
	if(m_splitFilter != u"all"_s)
	{
		m_splitFilter = u"all"_s;
		emit splitFilterChanged();
	}
	if(m_current != nullptr)
	{
		setCurrent(nullptr);
	}
	else
	{
		updateHighlight();
	}

	const QStringList chatIds = m_chats.keys();
	for(const QString& chatId : chatIds)
	{
		if(ChatSession* gone = m_chats.take(chatId))
		{
			emit sessionDropped(gone);
			m_stage.deleteWhenLeft(gone);
		}
	}
	m_seenLive.clear();
	m_leaving.clear();
	m_tileAttaching.clear();
	m_unzooming.clear();
	m_localMutationsInFlight = 0;
	if(!m_failure.isEmpty())
	{
		m_failure.clear();
		emit failureTextChanged();
	}

	m_directory.reset();
	m_config.reset();
	m_configEditor.reset();
	m_stash.reset();
	m_started = false;
	syncConfigCwd();
	emit clearImageCacheRequested();
	emit visibleSessionIdsChanged();
	emit splitCreatorsChanged();
	updateCommandContext();
	emit backendChanged();
}
