#pragma once

#include "proto/types.h"

#include <QAbstractListModel>
#include <QFlags>
#include <QList>
#include <QString>
#include <QStringList>
#include <QVariantList>

#include <functional>

namespace refgui::workspace
{

// What a command needs before it can run.
enum class Need
{
	Nothing = 0,
	Connected = 1 << 0, // the backend is reachable
	Session = 1 << 1,   // there is a session to act on
	LiveSession = 1 << 2, // at least one live session exists
	SplitView = 1 << 3, // split view is on
};
Q_DECLARE_FLAGS(Needs, Need)
Q_DECLARE_OPERATORS_FOR_FLAGS(Needs)

// The facts the commands' availability depends on.
struct CommandContext
{
	bool connected = false;
	bool hasSession = false;
	int liveSessions = 0;
	bool splitView = false;
};

// Every command of the window: what the command palette lists, what the `/` list in the
// input box offers and what the shortcuts run. Ids are the TUI's slash names (`new`,
// `sessions`, ...) so that all frontends talk about the same commands.
//
// The table of built-in commands lives here; whoever can carry a command out registers its
// handler with setHandler(). A command without a handler is not available.
class CommandRegistry : public QAbstractListModel
{
	Q_OBJECT

public:
	enum Role
	{
		IdRole = Qt::UserRole + 1,
		TitleRole,
		SlashRole,   // the canonical slash name, empty for palette-only commands
		AliasesRole, // the other slash names
		HintRole,    // the arguments, e.g. "[DIR]"; empty for a command without
		ShortcutRole, // portable text, e.g. "Ctrl+Shift+N"
		KeysRole,    // the same as key caps for display: ["Ctrl", "Shift", "N"] (⌘ ⇧ on macOS)
		CategoryRole,
		IconRole,
		EnabledRole,
		SourceRole, // builtin, config, skill or mcp
	};

	// Runs a command: its arguments (the text after the command) and the session to act on
	// (empty: the focused one).
	using Handler = std::function<void(const QString& args, const QString& sessionId)>;
	// Runs a user-defined command by name, with the same arguments.
	using CustomRunner = std::function<void(const QString& name, const QString& args, const QString& sessionId)>;

	explicit CommandRegistry(QObject* parent = nullptr);

	// Precondition: `id` is a built-in command.
	void setHandler(const QString& id, Handler handler);
	void setCustomRunner(CustomRunner runner);
	// Replaces the user-defined commands. Names that are built-in commands are skipped.
	void setCustomCommands(const QList<proto::CommandInfo>& commands);
	void setContext(const CommandContext& context);

	// Runs the command and returns true; false when there is no such command or it is not
	// available now.
	Q_INVOKABLE bool run(const QString& id, const QString& args = {}, const QString& sessionId = {});
	// Runs "/name args" if `name` is a command or an alias. Returns false when the line is no
	// command at all, so that the caller sends it as a prompt. A known command that is not
	// available now returns true and emits unavailable().
	Q_INVOKABLE bool runSlash(const QString& line, const QString& sessionId = {});

	// The commands matching `query`, best first, as maps with the role names as keys (id,
	// title, slash, aliases, hint, shortcut, keys, category, icon, enabled, source). With an
	// empty query: all, in the table's order. `slashOnly` leaves out palette-only and
	// unavailable commands, for the `/` list.
	Q_INVOKABLE QVariantList match(const QString& query, bool slashOnly) const;

	// Tells the views the titles changed (after the language did).
	void retranslate();

	// The keys of `shortcut` as caps to draw.
	static QStringList keyCaps(const QString& shortcut);

	int rowCount(const QModelIndex& parent = {}) const override;
	QVariant data(const QModelIndex& index, int role) const override;
	QHash<int, QByteArray> roleNames() const override;

signals:
	// A command that exists was asked for while it cannot run.
	void unavailable(const QString& id);

private:
	struct Entry
	{
		QString id;
		const char* title; // English source text, translated when shown
		QStringList slash;
		const char* hint; // arguments for display, not translated
		QString shortcut;
		const char* category;
		QString icon;
		Needs needs;
		QString source;
		QString customTitle{}; // user-defined: the description, as the backend gave it
		Handler handler{};
		bool enabled = false;
	};

	bool isAvailable(const Entry& entry) const;
	void refreshEnabled();
	const Entry* find(const QString& id) const;
	const Entry* findSlash(const QString& name) const;
	QVariantMap toMap(const Entry& entry) const;
	QString titleOf(const Entry& entry) const;

	QList<Entry> m_entries;
	qsizetype m_builtinCount = 0;
	CommandContext m_context;
	CustomRunner m_customRunner;
};

} // namespace refgui::workspace
