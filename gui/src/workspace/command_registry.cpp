#include "workspace/command_registry.h"

#include <QCoreApplication>
#include <QRegularExpression>

#include <algorithm>

using namespace Qt::StringLiterals;

namespace refgui::workspace
{
namespace
{

constexpr const char* TranslationContext = "Commands";

// The words of the title start with the query: "var" finds "Set or cycle model variant".
bool startsAWord(const QString& text, const QString& query)
{
	const QStringList words = text.split(u' ', Qt::SkipEmptyParts);
	return std::any_of(words.begin(), words.end(), [&](const QString& word) { return word.startsWith(query, Qt::CaseInsensitive); });
}

// The query's letters appear in `text` in order, e.g. "ssn" in "Switch session".
bool isSubsequence(const QString& text, const QString& query)
{
	qsizetype from = 0;
	for(const QChar letter : query)
	{
		from = text.indexOf(letter, from, Qt::CaseInsensitive);
		if(from < 0)
		{
			return false;
		}
		++from;
	}
	return true;
}

} // namespace

CommandRegistry::CommandRegistry(QObject* parent)
	: QAbstractListModel(parent)
{
	// Titles are the TUI's descriptions where the behavior is the same. Keys: Ctrl+Shift+letter
	// (Ctrl is Cmd on macOS), the same letters as the WebUI's Alt+letter.
	const char* const Session = QT_TRANSLATE_NOOP("Commands", "Session");
	const char* const View = QT_TRANSLATE_NOOP("Commands", "View");
	const char* const Help = QT_TRANSLATE_NOOP("Commands", "Help");
	const Needs Connected = Need::Connected;
	const Needs OnSession = Need::Connected | Need::Session;

	const QList<Entry> builtins{
		{.id = u"new"_s, .title = QT_TRANSLATE_NOOP("Commands", "New session"), .slash = {u"new"_s, u"clear"_s}, .shortcut = u"Ctrl+Shift+N"_s, .category = Session, .icon = u"chat-plus-16"_s, .needs = Connected},
		{.id = u"new-in"_s, .title = QT_TRANSLATE_NOOP("Commands", "New session in another folder…"), .category = Session, .icon = u"folder-16"_s, .needs = Connected},
		{.id = u"sessions"_s, .title = QT_TRANSLATE_NOOP("Commands", "Switch session (live + history)"), .slash = {u"sessions"_s, u"resume"_s, u"continue"_s}, .shortcut = u"Ctrl+Shift+L"_s, .category = Session, .icon = u"chat-list-16"_s, .needs = Connected},
		{.id = u"models"_s, .title = QT_TRANSLATE_NOOP("Commands", "Switch model (this session only)"), .slash = {u"models"_s}, .shortcut = u"Ctrl+Shift+M"_s, .category = Session, .icon = u"model-16"_s, .needs = OnSession},
		{.id = u"agents"_s, .title = QT_TRANSLATE_NOOP("Commands", "Switch agent"), .slash = {u"agents"_s}, .shortcut = u"Ctrl+Shift+A"_s, .category = Session, .icon = u"agent-16"_s, .needs = OnSession},
		{.id = u"variant"_s, .title = QT_TRANSLATE_NOOP("Commands", "Set or cycle model variant"), .slash = {u"variant"_s, u"variants"_s, u"effort"_s}, .hint = "[name]", .shortcut = u"Ctrl+Shift+T"_s, .category = Session, .icon = u"variant-16"_s, .needs = OnSession},
		{.id = u"compact"_s, .title = QT_TRANSLATE_NOOP("Commands", "Compact the conversation"), .slash = {u"compact"_s, u"summarize"_s}, .shortcut = u"Ctrl+Shift+C"_s, .category = Session, .icon = u"history-16"_s, .needs = OnSession},
		{.id = u"undo"_s, .title = QT_TRANSLATE_NOOP("Commands", "Undo last prompt"), .slash = {u"undo"_s}, .shortcut = u"Ctrl+Shift+U"_s, .category = Session, .icon = u"undo-16"_s, .needs = OnSession},
		{.id = u"rename"_s, .title = QT_TRANSLATE_NOOP("Commands", "Rename the session"), .slash = {u"rename"_s}, .hint = "NAME", .category = Session, .icon = u"pencil-16"_s, .needs = OnSession},
		{.id = u"cd"_s, .title = QT_TRANSLATE_NOOP("Commands", "Change this session's working directory"), .slash = {u"cd"_s, u"cwd"_s}, .hint = "[DIR]", .category = Session, .icon = u"folder-16"_s, .needs = OnSession},
		{.id = u"close"_s, .title = QT_TRANSLATE_NOOP("Commands", "Close this session"), .slash = {u"close"_s}, .shortcut = u"Ctrl+Shift+K"_s, .category = Session, .icon = u"close-16"_s, .needs = OnSession},
		{.id = u"close-session"_s, .title = QT_TRANSLATE_NOOP("Commands", "Close live sessions (picker, or by name / id)"), .slash = {u"close-session"_s, u"kill"_s}, .hint = "[name or id]", .category = Session, .icon = u"chat-x-16"_s, .needs = Connected | Need::LiveSession},
		{.id = u"peers"_s, .title = QT_TRANSLATE_NOOP("Commands", "List other live sessions you can message"), .slash = {u"peers"_s, u"list-agents"_s}, .category = Session, .icon = u"chat-list-16"_s, .needs = OnSession},
		{.id = u"peers-name"_s, .title = QT_TRANSLATE_NOOP("Commands", "Show or set this session's peer name"), .slash = {u"peers-name"_s}, .hint = "[name]", .category = Session, .icon = u"chat-16"_s, .needs = OnSession},
		{.id = u"peers-inbox"_s, .title = QT_TRANSLATE_NOOP("Commands", "Messages received from other sessions"), .slash = {u"peers-inbox"_s}, .category = Session, .icon = u"chat-16"_s, .needs = OnSession},
		{.id = u"peers-outbox"_s, .title = QT_TRANSLATE_NOOP("Commands", "Messages sent to other sessions and their status"), .slash = {u"peers-outbox"_s}, .category = Session, .icon = u"chat-send-16"_s, .needs = OnSession},
		{.id = u"export"_s, .title = QT_TRANSLATE_NOOP("Commands", "Export transcript as markdown"), .slash = {u"export"_s}, .hint = "[PATH]", .shortcut = u"Ctrl+Shift+X"_s, .category = Session, .icon = u"file-text-16"_s, .needs = OnSession},
		{.id = u"copy"_s, .title = QT_TRANSLATE_NOOP("Commands", "Copy last assistant message (raw)"), .slash = {u"copy"_s}, .shortcut = u"Ctrl+Shift+Y"_s, .category = Session, .icon = u"copy-16"_s, .needs = OnSession},
		{.id = u"thinking"_s, .title = QT_TRANSLATE_NOOP("Commands", "Toggle expanded thinking"), .slash = {u"thinking"_s}, .shortcut = u"Ctrl+Shift+R"_s, .category = View, .icon = u"info-16"_s, .needs = OnSession},
		{.id = u"details"_s, .title = QT_TRANSLATE_NOOP("Commands", "Toggle expanded tool details"), .slash = {u"details"_s}, .shortcut = u"Ctrl+Shift+O"_s, .category = View, .icon = u"tool-16"_s, .needs = OnSession},
		{.id = u"watch"_s, .title = QT_TRANSLATE_NOOP("Commands", "Split view of background sessions"), .slash = {u"watch"_s, u"split"_s}, .shortcut = u"Ctrl+Shift+W"_s, .category = View, .icon = u"split-20"_s, .needs = Connected},
		{.id = u"sidebar"_s, .title = QT_TRANSLATE_NOOP("Commands", "Toggle sidebar"), .slash = {u"sidebar"_s}, .shortcut = u"Ctrl+Shift+B"_s, .category = View, .icon = u"sidebar-20"_s, .needs = Need::Nothing},
		{.id = u"zoom"_s, .title = QT_TRANSLATE_NOOP("Commands", "Zoom in on the focused tile"), .shortcut = u"Ctrl+Shift+Z"_s, .category = View, .icon = u"maximize-20"_s, .needs = Connected | Need::SplitView},
		{.id = u"mcp"_s, .title = QT_TRANSLATE_NOOP("Commands", "MCP servers: status, errors and tools"), .slash = {u"mcp"_s, u"mcps"_s}, .category = Help, .icon = u"mcp-16"_s, .needs = Connected},
		{.id = u"help"_s, .title = QT_TRANSLATE_NOOP("Commands", "Keybindings and commands"), .slash = {u"help"_s}, .shortcut = u"Ctrl+Shift+H"_s, .category = Help, .icon = u"info-16"_s, .needs = Need::Nothing},
		{.id = u"settings"_s, .title = QT_TRANSLATE_NOOP("Commands", "Settings"), .shortcut = u"Ctrl+Shift+S"_s, .category = Help, .icon = u"sliders-20"_s, .needs = Need::Nothing},
		{.id = u"palette"_s, .title = QT_TRANSLATE_NOOP("Commands", "Command palette"), .shortcut = u"Ctrl+P"_s, .category = Help, .icon = u"command-20"_s, .needs = Need::Nothing},
	};
	for(Entry entry : builtins)
	{
		entry.source = u"builtin"_s;
		m_entries.append(std::move(entry));
	}
	m_builtinCount = m_entries.size();
}

void CommandRegistry::setHandler(const QString& id, Handler handler)
{
	for(qsizetype row = 0; row < m_builtinCount; ++row)
	{
		if(m_entries.at(row).id == id)
		{
			m_entries[row].handler = std::move(handler);
			refreshEnabled();
			return;
		}
	}
	Q_ASSERT_X(false, "CommandRegistry::setHandler", qPrintable(u"no built-in command "_s + id));
}

void CommandRegistry::setCustomRunner(CustomRunner runner)
{
	m_customRunner = std::move(runner);
	refreshEnabled();
}

void CommandRegistry::setCustomCommands(const QList<proto::CommandInfo>& commands)
{
	beginResetModel();
	m_entries.resize(m_builtinCount);
	for(const proto::CommandInfo& command : commands)
	{
		if(findSlash(command.name) != nullptr)
		{
			continue;
		}
		const QString name = command.name;
		Entry entry{.id = u"custom:"_s + name,
		            .title = "",
		            .slash = {name},
		            .hint = "[args]",
		            .shortcut = {},
		            .category = QT_TRANSLATE_NOOP("Commands", "Custom"),
		            .icon = u"slash-16"_s,
		            .needs = Need::Connected | Need::Session,
		            .source = command.source,
		            .customTitle = command.description};
		entry.handler = [this, name](const QString& args, const QString& sessionId) {
			Q_ASSERT(m_customRunner);
			m_customRunner(name, args, sessionId);
		};
		m_entries.append(std::move(entry));
	}
	endResetModel();
	refreshEnabled();
}

void CommandRegistry::setContext(const CommandContext& context)
{
	m_context = context;
	refreshEnabled();
}

bool CommandRegistry::isAvailable(const Entry& entry) const
{
	if(!entry.handler)
	{
		return false;
	}
	if(entry.needs.testFlag(Need::Connected) && !m_context.connected)
	{
		return false;
	}
	if(entry.needs.testFlag(Need::Session) && !m_context.hasSession)
	{
		return false;
	}
	if(entry.needs.testFlag(Need::LiveSession) && m_context.liveSessions == 0)
	{
		return false;
	}
	return !entry.needs.testFlag(Need::SplitView) || m_context.splitView;
}

void CommandRegistry::refreshEnabled()
{
	for(qsizetype row = 0; row < m_entries.size(); ++row)
	{
		Entry& entry = m_entries[row];
		const bool available = isAvailable(entry);
		if(available == entry.enabled)
		{
			continue;
		}
		entry.enabled = available;
		emit dataChanged(index(static_cast<int>(row)), index(static_cast<int>(row)), {EnabledRole});
	}
}

const CommandRegistry::Entry* CommandRegistry::find(const QString& id) const
{
	const auto found = std::find_if(m_entries.begin(), m_entries.end(), [&](const Entry& entry) { return entry.id == id; });
	return found == m_entries.end() ? nullptr : &*found;
}

const CommandRegistry::Entry* CommandRegistry::findSlash(const QString& name) const
{
	const auto found = std::find_if(m_entries.begin(), m_entries.end(), [&](const Entry& entry) { return entry.slash.contains(name); });
	return found == m_entries.end() ? nullptr : &*found;
}

bool CommandRegistry::run(const QString& id, const QString& args, const QString& sessionId)
{
	const Entry* entry = find(id);
	if(entry == nullptr || !entry->enabled)
	{
		return false;
	}
	entry->handler(args.trimmed(), sessionId);
	return true;
}

bool CommandRegistry::runSlash(const QString& line, const QString& sessionId)
{
	const QString text = line.trimmed();
	if(!text.startsWith(u'/'))
	{
		return false;
	}
	const qsizetype space = text.indexOf(QRegularExpression(u"\\s"_s));
	const QString name = text.mid(1, space < 0 ? -1 : space - 1);
	const QString args = space < 0 ? QString() : text.mid(space + 1).trimmed();

	const Entry* entry = findSlash(name);
	if(entry == nullptr)
	{
		return false;
	}
	if(!entry->enabled)
	{
		emit unavailable(entry->id);
		return true;
	}
	entry->handler(args, sessionId);
	return true;
}

QString CommandRegistry::titleOf(const Entry& entry) const
{
	if(!entry.customTitle.isEmpty())
	{
		return entry.customTitle;
	}
	return QCoreApplication::translate(TranslationContext, entry.title);
}

QVariant CommandRegistry::valueOf(const Entry& entry, int role) const
{
	switch(role)
	{
	case IdRole:
		return entry.id;
	case TitleRole:
		return titleOf(entry);
	case SlashRole:
		return entry.slash.isEmpty() ? QString() : entry.slash.first();
	case AliasesRole:
		return entry.slash.mid(1);
	case HintRole:
		return QString::fromUtf8(entry.hint);
	case ShortcutRole:
		return entry.shortcut;
	case KeysRole:
		return keyCaps(entry.shortcut);
	case CategoryRole:
		return QCoreApplication::translate(TranslationContext, entry.category);
	case IconRole:
		return entry.icon;
	case EnabledRole:
		return entry.enabled;
	case SourceRole:
		return entry.source;
	default:
		return {};
	}
}

QVariantMap CommandRegistry::toMap(const Entry& entry) const
{
	return {{u"id"_s, valueOf(entry, IdRole)},
	        {u"title"_s, valueOf(entry, TitleRole)},
	        {u"slash"_s, valueOf(entry, SlashRole)},
	        {u"aliases"_s, valueOf(entry, AliasesRole)},
	        {u"hint"_s, valueOf(entry, HintRole)},
	        {u"shortcut"_s, valueOf(entry, ShortcutRole)},
	        {u"keys"_s, valueOf(entry, KeysRole)},
	        {u"category"_s, valueOf(entry, CategoryRole)},
	        {u"icon"_s, valueOf(entry, IconRole)},
	        {u"enabled"_s, valueOf(entry, EnabledRole)},
	        {u"source"_s, valueOf(entry, SourceRole)}};
}

CommandRegistry::Rank CommandRegistry::rankOf(const Entry& entry, const QString& needle) const
{
	if(needle.isEmpty())
	{
		return ExactSlash;
	}
	if(entry.slash.contains(needle, Qt::CaseInsensitive))
	{
		return ExactSlash;
	}
	const auto prefixed = [&](const QString& name) { return name.startsWith(needle, Qt::CaseInsensitive); };
	if(std::any_of(entry.slash.begin(), entry.slash.end(), prefixed))
	{
		return SlashPrefix;
	}
	const QString title = titleOf(entry);
	if(startsAWord(title, needle))
	{
		return TitleWordPrefix;
	}
	if(title.contains(needle, Qt::CaseInsensitive))
	{
		return Substring;
	}
	if(isSubsequence(title, needle))
	{
		return Subsequence;
	}
	return NoMatch;
}

QVariantList CommandRegistry::matching(const QString& query, bool slashOnly) const
{
	const QString needle = query.trimmed().startsWith(u'/') ? query.trimmed().mid(1) : query.trimmed();

	struct Hit
	{
		Rank rank;
		const Entry* entry;
	};
	QList<Hit> hits;
	for(const Entry& entry : m_entries)
	{
		const bool hiddenFromSlashList = slashOnly && (entry.slash.isEmpty() || !entry.enabled);
		if(hiddenFromSlashList)
		{
			continue;
		}
		const Rank rank = rankOf(entry, needle);
		if(rank != NoMatch)
		{
			hits.append({rank, &entry});
		}
	}
	std::stable_sort(hits.begin(), hits.end(), [](const Hit& a, const Hit& b) { return a.rank < b.rank; });

	QVariantList result;
	for(const Hit& hit : hits)
	{
		result.append(toMap(*hit.entry));
	}
	return result;
}

void CommandRegistry::retranslate()
{
	if(!m_entries.isEmpty())
	{
		emit dataChanged(index(0), index(static_cast<int>(m_entries.size()) - 1), {TitleRole, HintRole, CategoryRole});
	}
}

QStringList CommandRegistry::keyCaps(const QString& shortcut)
{
	if(shortcut.isEmpty())
	{
		return {};
	}
	QStringList caps = shortcut.split(u'+');
#ifdef Q_OS_MACOS
	for(QString& cap : caps)
	{
		if(cap == u"Ctrl")
		{
			cap = u"⌘"_s;
		}
		else if(cap == u"Shift")
		{
			cap = u"⇧"_s;
		}
		else if(cap == u"Alt")
		{
			cap = u"⌥"_s;
		}
	}
#endif
	return caps;
}

int CommandRegistry::rowCount(const QModelIndex& parent) const
{
	return parent.isValid() ? 0 : static_cast<int>(m_entries.size());
}

QVariant CommandRegistry::data(const QModelIndex& index, int role) const
{
	if(!checkIndex(index, CheckIndexOption::IndexIsValid | CheckIndexOption::ParentIsInvalid))
	{
		return {};
	}
	return valueOf(m_entries.at(index.row()), role);
}

QHash<int, QByteArray> CommandRegistry::roleNames() const
{
	return {{IdRole, "commandId"}, // not "id": that is a QML keyword
	        {TitleRole, "title"},
	        {SlashRole, "slash"},
	        {AliasesRole, "aliases"},
	        {HintRole, "hint"},
	        {ShortcutRole, "shortcut"},
	        {KeysRole, "keys"},
	        {CategoryRole, "category"},
	        {IconRole, "icon"},
	        {EnabledRole, "enabled"},
	        {SourceRole, "source"}};
}

} // namespace refgui::workspace
