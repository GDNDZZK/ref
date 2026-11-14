// The window-wide commands: what each one of the CommandRegistry does.
#include "app/workspace.h"

using namespace Qt::StringLiterals;

void Workspace::updateCommandContext()
{
	m_commands.setContext({.connected = m_connection.isConnected(),
	                       .hasSession = m_current != nullptr,
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
	on(u"sessions"_s, openDialogCommand(u"sessions"_s));
	on(u"models"_s, dialogFor(u"models"_s));
	on(u"agents"_s, dialogFor(u"agents"_s));
	on(u"variant"_s, [this](const QString& args, const QString& sessionId) { setVariant(targetId(sessionId), args.trimmed()); });
	on(u"compact"_s, [this](const QString&, const QString& sessionId) { compact(targetId(sessionId)); });
	on(u"undo"_s, [this](const QString&, const QString& sessionId) { runOnSession(sessionId, &ChatSession::undoLast); });
	on(u"copy"_s, [this](const QString&, const QString& sessionId) { runOnSession(sessionId, &ChatSession::copyLastReply); });
	on(u"thinking"_s, [this](const QString&, const QString& sessionId) { runOnSession(sessionId, &ChatSession::toggleReasoning); });
	on(u"details"_s, [this](const QString&, const QString& sessionId) { runOnSession(sessionId, &ChatSession::toggleDetails); });

	on(u"rename"_s, argumentOrDialog(u"rename"_s, [this](const QString& id, const QString& name) { renameSession(id, name); }));
	on(u"cd"_s, argumentOrDialog(u"cd"_s, [this](const QString& id, const QString& path) { setSessionCwd(id, path); }));
	on(u"close"_s, [this](const QString&, const QString& sessionId) {
		const QString id = targetId(sessionId);
		const refgui::proto::SessionInfo* info = m_directory.find(id);
		const bool working = info != nullptr && info->status.state != refgui::proto::SessionStatus::State::Idle;
		if(working)
		{
			openDialog(u"close"_s, {{u"sessionId"_s, id}});
			return;
		}
		closeSession(id);
	});
	on(u"close-session"_s, [this](const QString& args, const QString&) {
		const QString wanted = args.trimmed();
		if(wanted.isEmpty())
		{
			openDialog(u"sessions"_s, {{u"mode"_s, u"close"_s}});
			return;
		}
		for(const refgui::proto::SessionInfo& info : m_directory.liveSessions())
		{
			if(info.id == wanted || info.name.compare(wanted, Qt::CaseInsensitive) == 0)
			{
				closeSession(info.id);
				return;
			}
		}
		toast(refgui::workspace::ToastLevel::Warn, tr("No live session \"%1\".").arg(wanted));
	});

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
	on(u"mcp"_s, [this](const QString&, const QString&) {
		refreshMcp();
		openDialog(u"mcp"_s);
	});
	on(u"help"_s, openDialogCommand(u"help"_s));
	on(u"settings"_s, openDialogCommand(u"settings"_s));
	on(u"palette"_s, openDialogCommand(u"palette"_s));

	m_commands.setCustomRunner([this](const QString& name, const QString& args, const QString& sessionId) { runCustomCommand(name, args, sessionId); });
	updateCommandContext();
}
