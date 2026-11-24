// The window-wide commands: what each one of the CommandRegistry does.
#include "app/workspace.h"

#include <algorithm>
#include <optional>

using namespace Qt::StringLiterals;

namespace
{

QString unquote(const QString& text)
{
	const QString trimmed = text.trimmed();
	if(trimmed.size() >= 2 && ((trimmed.startsWith(u'"') && trimmed.endsWith(u'"')) || (trimmed.startsWith(u'\'') && trimmed.endsWith(u'\''))))
	{
		return trimmed.mid(1, trimmed.size() - 2);
	}
	return trimmed;
}

QStringList splitArguments(const QString& text)
{
	QStringList tokens;
	qsizetype i = 0;
	const qsizetype n = text.size();
	while(i < n)
	{
		while(i < n && text[i].isSpace())
		{
			++i;
		}
		if(i >= n)
		{
			break;
		}
		if(text[i] == u'"' || text[i] == u'\'')
		{
			const QChar quote = text[i++];
			const qsizetype start = i;
			while(i < n && text[i] != quote)
			{
				++i;
			}
			const QString token = text.mid(start, i - start).trimmed();
			if(!token.isEmpty())
			{
				tokens.append(token);
			}
			if(i < n)
			{
				++i;
			}
			continue;
		}
		const qsizetype start = i;
		while(i < n && !text[i].isSpace())
		{
			++i;
		}
		tokens.append(text.mid(start, i - start));
	}
	return tokens;
}

const refgui::proto::SessionInfo* findSessionIn(const QList<refgui::proto::SessionInfo>& sessions, const QString& raw)
{
	const QString wanted = unquote(raw);
	if(wanted.isEmpty())
	{
		return nullptr;
	}
	for(const refgui::proto::SessionInfo& info : sessions)
	{
		if(info.id == wanted || info.name == wanted)
		{
			return &info;
		}
	}
	for(const refgui::proto::SessionInfo& info : sessions)
	{
		if(info.id.compare(wanted, Qt::CaseInsensitive) == 0 || info.name.compare(wanted, Qt::CaseInsensitive) == 0)
		{
			return &info;
		}
	}
	for(const refgui::proto::SessionInfo& info : sessions)
	{
		if(info.title == wanted)
		{
			return &info;
		}
	}
	for(const refgui::proto::SessionInfo& info : sessions)
	{
		if(info.title.compare(wanted, Qt::CaseInsensitive) == 0)
		{
			return &info;
		}
	}
	return nullptr;
}

const refgui::proto::ModelInfo* findModelIn(const QList<refgui::proto::ModelInfo>& models, const QString& spec, const QString& preferredProvider)
{
	for(const refgui::proto::ModelInfo& model : models)
	{
		if(model.provider + u"/"_s + model.id == spec)
		{
			return &model;
		}
	}
	for(const refgui::proto::ModelInfo& model : models)
	{
		if((model.provider + u"/"_s + model.id).compare(spec, Qt::CaseInsensitive) == 0)
		{
			return &model;
		}
	}
	const auto pickByField = [&](auto matches) -> const refgui::proto::ModelInfo* {
		const refgui::proto::ModelInfo* fallback = nullptr;
		for(const refgui::proto::ModelInfo& model : models)
		{
			if(!matches(model))
			{
				continue;
			}
			if(model.provider == preferredProvider)
			{
				return &model;
			}
			if(fallback == nullptr)
			{
				fallback = &model;
			}
		}
		return fallback;
	};
	if(const auto* exactId = pickByField([&](const refgui::proto::ModelInfo& m) { return m.id == spec; }))
	{
		return exactId;
	}
	if(const auto* ciId = pickByField([&](const refgui::proto::ModelInfo& m) { return m.id.compare(spec, Qt::CaseInsensitive) == 0; }))
	{
		return ciId;
	}
	if(const auto* exactName = pickByField([&](const refgui::proto::ModelInfo& m) { return m.name == spec; }))
	{
		return exactName;
	}
	return pickByField([&](const refgui::proto::ModelInfo& m) { return m.name.compare(spec, Qt::CaseInsensitive) == 0; });
}

} // namespace

void Workspace::updateCommandContext()
{
	m_commands.setContext({.connected = m_connection.isConnected(),
	                       .hasSession = !targetId({}).isEmpty(),
	                       .canDetach = m_current != nullptr || !m_zoomed.isEmpty(),
	                       .liveSessions = int(m_directory.liveSessions().size()),
	                       .splitView = m_split});
}

// Calls a method of the session the command acts on.
void Workspace::runOnSession(const QString& sessionId, void (ChatSession::*method)())
{
	ChatSession* target = session(targetId(sessionId));
	if(target == nullptr)
	{
		toastCommandUnavailable();
		return;
	}
	(target->*method)();
}

void Workspace::runCustomCommand(const QString& name, const QString& args, const QString& sessionId)
{
	ChatSession* target = session(targetId(sessionId));
	if(target == nullptr)
	{
		toastCommandUnavailable();
		return;
	}
	target->composer()->runCustomCommand(name, args);
}

void Workspace::registerCommands()
{
	using Handler = refgui::workspace::CommandRegistry::Handler;
	const auto on = [this](const QString& id, Handler handler) { m_commands.setHandler(id, std::move(handler)); };
	// A command that opens a dialog for the session it acts on.
	const auto dialogFor = [this](const QString& dialog) {
		return [this, dialog](const QString&, const QString& sessionId) { openDialog(dialog, {{u"sessionId"_s, targetId(sessionId)}}); };
	};
	// A command that opens a dialog of the window.
	const auto openDialogCommand = [this](const QString& id) {
		return [this, id](const QString&, const QString&) { openDialog(id); };
	};
	// A command that acts on its argument, or asks for it in a dialog when there is none.
	const auto argumentOrDialog = [this, dialogFor](const QString& dialogId, auto action) {
		return [this, dialog = dialogFor(dialogId), action](const QString& args, const QString& sessionId) {
			const QString argument = args.trimmed();
			if(argument.isEmpty())
			{
				dialog(args, sessionId);
				return;
			}
			action(targetId(sessionId), argument);
		};
	};

	on(u"new"_s, [this](const QString&, const QString&) { newSession(); });
	on(u"new-in"_s, [this](const QString&, const QString&) {
		if(isBackendLocal())
		{
			emit newSessionFolderRequested();
			return;
		}
		openDialog(u"new-in"_s);
	});
	on(u"sessions"_s, [this](const QString& args, const QString&) {
		const QString wanted = args.trimmed();
		if(wanted.isEmpty())
		{
			openDialog(u"sessions"_s);
			return;
		}
		const QList<refgui::proto::SessionInfo> all = m_directory.allSessions();
		if(const auto* found = findSessionIn(all, wanted))
		{
			openSession(found->id);
			return;
		}
		toast(refgui::workspace::ToastLevel::Warn, tr("No session \"%1\".").arg(unquote(wanted)));
	});
	on(u"models"_s, argumentOrDialog(u"models"_s, [this](const QString& id, const QString& wanted) {
		const qsizetype hash = wanted.lastIndexOf(u'#');
		const QString spec = (hash >= 0 ? wanted.left(hash) : wanted).trimmed();
		const std::optional<QString> explicitVariant = hash >= 0 ? std::optional<QString>(wanted.mid(hash + 1).trimmed()) : std::nullopt;
		const auto* sessionInfo = m_directory.find(id);
		const QString preferredProvider = sessionInfo ? sessionInfo->model.provider : QString();
		const auto* matched = findModelIn(m_config.config().models, spec, preferredProvider);
		if(matched == nullptr)
		{
			toast(refgui::workspace::ToastLevel::Warn, tr("Unknown model \"%1\".").arg(wanted));
			return;
		}
		QString variant;
		if(explicitVariant)
		{
			if(!explicitVariant->isEmpty() && explicitVariant->compare(u"default"_s, Qt::CaseInsensitive) != 0)
			{
				const auto it = std::ranges::find_if(matched->variants, [&](const QString& v) { return v.compare(*explicitVariant, Qt::CaseInsensitive) == 0; });
				if(it == matched->variants.end())
				{
					toast(refgui::workspace::ToastLevel::Warn, tr("Model %1/%2 has no variant \"%3\".").arg(matched->provider, matched->id, *explicitVariant));
					return;
				}
				variant = *it;
			}
		}
		else if(sessionInfo && sessionInfo->model.variant && matched->variants.contains(*sessionInfo->model.variant))
		{
			variant = *sessionInfo->model.variant;
		}
		setModel(id, matched->provider, matched->id, variant);
	}));
	on(u"agents"_s, argumentOrDialog(u"agents"_s, [this](const QString& id, const QString& wanted) {
		const auto& agents = m_config.config().agents;
		auto it = std::ranges::find(agents, wanted, &refgui::proto::AgentInfo::name);
		if(it == agents.end())
		{
			it = std::ranges::find_if(agents, [&](const refgui::proto::AgentInfo& a) { return a.name.compare(wanted, Qt::CaseInsensitive) == 0; });
		}
		if(it == agents.end())
		{
			toast(refgui::workspace::ToastLevel::Warn, tr("Unknown agent \"%1\".").arg(wanted));
			return;
		}
		setAgent(id, it->name);
	}));
	on(u"variant"_s, argumentOrDialog(u"variants"_s, [this](const QString& id, const QString& wanted) {
		if(wanted.compare(u"default"_s, Qt::CaseInsensitive) == 0)
		{
			clearVariant(id);
			return;
		}
		QString resolved = wanted;
		if(const auto* sessionInfo = m_directory.find(id))
		{
			const QStringList variants = m_config.variantsOf(sessionInfo->model.provider, sessionInfo->model.model);
			const auto it = std::ranges::find_if(variants, [&](const QString& v) { return v.compare(wanted, Qt::CaseInsensitive) == 0; });
			if(it != variants.end())
			{
				resolved = *it;
			}
		}
		setVariant(id, resolved);
	}));
	on(u"compact"_s, [this](const QString&, const QString& sessionId) { compact(targetId(sessionId)); });
	on(u"undo"_s, [this](const QString&, const QString& sessionId) { runOnSession(sessionId, &ChatSession::undoLast); });
	on(u"copy"_s, [this](const QString&, const QString& sessionId) { runOnSession(sessionId, &ChatSession::copyLastReply); });
	on(u"thinking"_s, [this](const QString&, const QString& sessionId) { runOnSession(sessionId, &ChatSession::toggleReasoning); });
	on(u"details"_s, [this](const QString&, const QString& sessionId) { runOnSession(sessionId, &ChatSession::toggleDetails); });

	on(u"rename"_s, argumentOrDialog(u"rename"_s, [this](const QString& id, const QString& name) { renameSession(id, name); }));
	on(u"cd"_s, argumentOrDialog(u"cd"_s, [this](const QString& id, const QString& path) { setSessionCwd(id, path); }));
	on(u"detach"_s, [this](const QString&, const QString&) { detachCurrent(); });
	on(u"close"_s, [this](const QString&, const QString& sessionId) { requestCloseSession(targetId(sessionId)); });
	on(u"close-session"_s, [this](const QString& args, const QString&) {
		const QString wanted = args.trimmed();
		if(wanted.isEmpty())
		{
			openDialog(u"sessions"_s, {{u"mode"_s, u"close"_s}});
			return;
		}
		const QList<refgui::proto::SessionInfo> live = m_directory.liveSessions();
		if(const auto* single = findSessionIn(live, wanted))
		{
			closeSession(single->id);
			return;
		}
		QSet<QString> closedIds;
		for(const QString& token : splitArguments(wanted))
		{
			if(const auto* found = findSessionIn(live, token))
			{
				if(!closedIds.contains(found->id))
				{
					closedIds.insert(found->id);
					closeSession(found->id);
				}
			}
			else
			{
				toast(refgui::workspace::ToastLevel::Warn, tr("No live session \"%1\".").arg(token));
			}
		}
	});
	on(u"stash"_s, [this](const QString& args, const QString&) { m_stash.stashAll(args); });
	on(u"stashed"_s, openDialogCommand(u"stash"_s));

	const auto peers = [this](const QString& command, const QString& tab) {
		return [this, command, tab](const QString& args, const QString& sessionId) {
			const QString id = targetId(sessionId);
			if(command == u"peers-name"_s && !args.trimmed().isEmpty())
			{
				peerText(id, command, args.trimmed(), [this](bool ok, const QVariant& text) {
					toast(ok ? refgui::workspace::ToastLevel::Info : refgui::workspace::ToastLevel::Error, text.toString());
				});
				return;
			}
			openDialog(u"peers"_s, {{u"sessionId"_s, id}, {u"tab"_s, tab}});
		};
	};
	on(u"peers"_s, peers(u"peers"_s, u"peers"_s));
	on(u"peers-name"_s, peers(u"peers-name"_s, u"name"_s));
	on(u"peers-inbox"_s, peers(u"peers-inbox"_s, u"inbox"_s));
	on(u"peers-outbox"_s, peers(u"peers-outbox"_s, u"outbox"_s));
	const auto exportAndReport = [this](const QString& id, const QString& path) {
		exportTo(id, path, [this](bool ok, const QVariant& payload) {
			toast(ok ? refgui::workspace::ToastLevel::Info : refgui::workspace::ToastLevel::Error, ok ? tr("Exported to %1").arg(payload.toString()) : payload.toString());
		});
	};
	on(u"export"_s, argumentOrDialog(u"export"_s, exportAndReport));

	on(u"watch"_s, [this](const QString&, const QString&) { toggleSplit(); });
	on(u"sidebar"_s, [this](const QString&, const QString&) { m_settings.setSidebarVisible(!m_settings.sidebarVisible()); });
	on(u"zoom"_s, [this](const QString&, const QString&) {
		if(m_zoomed.isEmpty())
		{
			zoomTile(m_focusedTile);
			return;
		}
		unzoom();
	});
	on(u"ui-zoom-in"_s, [this](const QString&, const QString&) { m_settings.setUiScale(m_settings.uiScale() + refgui::workspace::Settings::UiScaleStep); });
	on(u"ui-zoom-out"_s, [this](const QString&, const QString&) { m_settings.setUiScale(m_settings.uiScale() - refgui::workspace::Settings::UiScaleStep); });
	on(u"ui-zoom-reset"_s, [this](const QString&, const QString&) { m_settings.setUiScale(refgui::workspace::Settings::DefaultUiScale); });
	on(u"floating"_s, [this](const QString&, const QString&) { setFloatingMode(!m_floatingMode); });
	on(u"mcp"_s, [this](const QString&, const QString&) {
		refreshMcp();
		openDialog(u"mcp"_s);
	});
	on(u"reload"_s, [this](const QString&, const QString&) { reloadConfig(); });
	on(u"config"_s, openDialogCommand(u"config"_s));
	on(u"help"_s, openDialogCommand(u"help"_s));
	on(u"settings"_s, openDialogCommand(u"settings"_s));
	on(u"backend"_s, [this](const QString& args, const QString&) {
		const QString trimmed = args.trimmed();
		if(trimmed.isEmpty())
		{
			openDialog(u"backend"_s);
			return;
		}
		const QStringList parts = splitArguments(trimmed);
		if(parts.isEmpty())
		{
			openDialog(u"backend"_s);
			return;
		}
		if(parts.first().compare(u"auto"_s, Qt::CaseInsensitive) == 0
		   || parts.first().compare(u"default"_s, Qt::CaseInsensitive) == 0
		   || parts.first().compare(u"local"_s, Qt::CaseInsensitive) == 0)
		{
			restoreDefaultBackend();
			return;
		}
		const QString url = parts.first();
		const QString token = parts.size() >= 2 ? parts.at(1) : QString();
		if(!switchBackend(url, token, false))
		{
			toast(refgui::workspace::ToastLevel::Warn, tr("Invalid backend URL \"%1\". Expected ws:// or wss://.").arg(url));
		}
	});
	on(u"palette"_s, openDialogCommand(u"palette"_s));

	m_commands.setCustomRunner([this](const QString& name, const QString& args, const QString& sessionId) { runCustomCommand(name, args, sessionId); });
	updateCommandContext();
}
