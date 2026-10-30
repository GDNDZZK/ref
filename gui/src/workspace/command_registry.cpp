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

// How well a query matches; lower is better. Kept in this order.
enum Rank
{
	ExactSlash,
	SlashPrefix,
	TitleWordPrefix,
	Substring,
	Subsequence,
	NoMatch,
};

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
	const auto builtin = [this](const QString& id, const char* title, QStringList slash, const char* hint, const QString& shortcut, const char* category, const QString& icon, Needs needs) {
		m_entries.append({id, title, std::move(slash), hint, shortcut, category, icon, needs, u"builtin"_s});
	};
	const char* const Session = QT_TRANSLATE_NOOP("Commands", "Session");
	const char* const View = QT_TRANSLATE_NOOP("Commands", "View");
	const char* const Help = QT_TRANSLATE_NOOP("Commands", "Help");
	const Needs Connected = Need::Connected;
	const Needs OnSession = Need::Connected | Need::Session;

	builtin(u"new"_s, QT_TRANSLATE_NOOP("Commands", "New session"), {u"new"_s, u"clear"_s}, "", u"Ctrl+Shift+N"_s, Session, u"chat-plus-16"_s, Connected);
	builtin(u"new-in"_s, QT_TRANSLATE_NOOP("Commands", "New session in another folder…"), {}, "", {}, Session, u"folder-16"_s, Connected);
	builtin(u"sessions"_s, QT_TRANSLATE_NOOP("Commands", "Switch session (live + history)"), {u"sessions"_s, u"resume"_s, u"continue"_s}, "", u"Ctrl+Shift+L"_s, Session, u"chat-list-16"_s, Connected);
	builtin(u"models"_s, QT_TRANSLATE_NOOP("Commands", "Switch model (this session only)"), {u"models"_s}, "", u"Ctrl+Shift+M"_s, Session, u"model-16"_s, OnSession);
	builtin(u"agents"_s, QT_TRANSLATE_NOOP("Commands", "Switch agent"), {u"agents"_s}, "", u"Ctrl+Shift+A"_s, Session, u"agent-16"_s, OnSession);
	builtin(u"variant"_s, QT_TRANSLATE_NOOP("Commands", "Set or cycle model variant"), {u"variant"_s, u"variants"_s, u"effort"_s}, "[name]", u"Ctrl+Shift+T"_s, Session, u"variant-16"_s, OnSession);
	builtin(u"compact"_s, QT_TRANSLATE_NOOP("Commands", "Compact the conversation"), {u"compact"_s, u"summarize"_s}, "", u"Ctrl+Shift+C"_s, Session, u"history-16"_s, OnSession);
	builtin(u"undo"_s, QT_TRANSLATE_NOOP("Commands", "Undo last prompt"), {u"undo"_s}, "", u"Ctrl+Shift+U"_s, Session, u"undo-16"_s, OnSession);
	builtin(u"rename"_s, QT_TRANSLATE_NOOP("Commands", "Rename the session"), {u"rename"_s}, "NAME", {}, Session, u"pencil-16"_s, OnSession);
	builtin(u"cd"_s, QT_TRANSLATE_NOOP("Commands", "Change this session's working directory"), {u"cd"_s, u"cwd"_s}, "[DIR]", {}, Session, u"folder-16"_s, OnSession);
	builtin(u"close"_s, QT_TRANSLATE_NOOP("Commands", "Close this session"), {u"close"_s}, "", u"Ctrl+Shift+K"_s, Session, u"close-16"_s, OnSession);
	builtin(u"close-session"_s, QT_TRANSLATE_NOOP("Commands", "Close live sessions (picker, or by name / id)"), {u"close-session"_s, u"kill"_s}, "[name or id]", {}, Session, u"chat-x-16"_s, Connected | Need::LiveSession);
	builtin(u"peers"_s, QT_TRANSLATE_NOOP("Commands", "List other live sessions you can message"), {u"peers"_s, u"list-agents"_s}, "", {}, Session, u"chat-list-16"_s, OnSession);
	builtin(u"peers-name"_s, QT_TRANSLATE_NOOP("Commands", "Show or set this session's peer name"), {u"peers-name"_s}, "[name]", {}, Session, u"chat-16"_s, OnSession);
	builtin(u"peers-inbox"_s, QT_TRANSLATE_NOOP("Commands", "Messages received from other sessions"), {u"peers-inbox"_s}, "", {}, Session, u"chat-16"_s, OnSession);
	builtin(u"peers-outbox"_s, QT_TRANSLATE_NOOP("Commands", "Messages sent to other sessions and their status"), {u"peers-outbox"_s}, "", {}, Session, u"chat-send-16"_s, OnSession);
	builtin(u"export"_s, QT_TRANSLATE_NOOP("Commands", "Export transcript as markdown"), {u"export"_s}, "[PATH]", u"Ctrl+Shift+X"_s, Session, u"file-text-16"_s, OnSession);
	builtin(u"copy"_s, QT_TRANSLATE_NOOP("Commands", "Copy last assistant message (raw)"), {u"copy"_s}, "", u"Ctrl+Shift+Y"_s, Session, u"copy-16"_s, OnSession);
	builtin(u"thinking"_s, QT_TRANSLATE_NOOP("Commands", "Toggle expanded thinking"), {u"thinking"_s}, "", u"Ctrl+Shift+R"_s, View, u"info-16"_s, OnSession);
	builtin(u"details"_s, QT_TRANSLATE_NOOP("Commands", "Toggle expanded tool details"), {u"details"_s}, "", u"Ctrl+Shift+O"_s, View, u"tool-16"_s, OnSession);
	builtin(u"watch"_s, QT_TRANSLATE_NOOP("Commands", "Split view of background sessions"), {u"watch"_s, u"split"_s}, "", u"Ctrl+Shift+W"_s, View, u"split-20"_s, Connected);
	builtin(u"sidebar"_s, QT_TRANSLATE_NOOP("Commands", "Toggle sidebar"), {u"sidebar"_s}, "", u"Ctrl+Shift+B"_s, View, u"sidebar-20"_s, Need::Nothing);
	builtin(u"zoom"_s, QT_TRANSLATE_NOOP("Commands", "Zoom in on the focused tile"), {}, "", u"Ctrl+Shift+Z"_s, View, u"maximize-20"_s, Connected | Need::SplitView);
	builtin(u"mcp"_s, QT_TRANSLATE_NOOP("Commands", "MCP servers: status, errors and tools"), {u"mcp"_s, u"mcps"_s}, "", {}, Help, u"mcp-16"_s, Connected);
	builtin(u"help"_s, QT_TRANSLATE_NOOP("Commands", "Keybindings and commands"), {u"help"_s}, "", u"Ctrl+Shift+H"_s, Help, u"info-16"_s, Need::Nothing);
	builtin(u"settings"_s, QT_TRANSLATE_NOOP("Commands", "Settings"), {}, "", u"Ctrl+Shift+S"_s, Help, u"sliders-20"_s, Need::Nothing);
	builtin(u"palette"_s, QT_TRANSLATE_NOOP("Commands", "Command palette"), {}, "", u"Ctrl+P"_s, Help, u"command-20"_s, Need::Nothing);
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

QVariantMap CommandRegistry::toMap(const Entry& entry) const
{
	return {{u"id"_s, entry.id},
	        {u"title"_s, titleOf(entry)},
	        {u"slash"_s, entry.slash.isEmpty() ? QString() : entry.slash.first()},
	        {u"aliases"_s, entry.slash.mid(1)},
	        {u"hint"_s, QString::fromUtf8(entry.hint)},
	        {u"shortcut"_s, entry.shortcut},
	        {u"keys"_s, keyCaps(entry.shortcut)},
	        {u"category"_s, QCoreApplication::translate(TranslationContext, entry.category)},
	        {u"icon"_s, entry.icon},
	        {u"enabled"_s, entry.enabled},
	        {u"source"_s, entry.source}};
}

QVariantList CommandRegistry::match(const QString& query, bool slashOnly) const
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
		if(slashOnly && (entry.slash.isEmpty() || !entry.enabled))
		{
			continue;
		}
		Rank rank = ExactSlash;
		if(!needle.isEmpty())
		{
			const QString title = titleOf(entry);
			const auto prefixed = [&](const QString& name) { return name.startsWith(needle, Qt::CaseInsensitive); };
			rank = NoMatch;
			if(entry.slash.contains(needle, Qt::CaseInsensitive))
			{
				rank = ExactSlash;
			}
			else if(std::any_of(entry.slash.begin(), entry.slash.end(), prefixed))
			{
				rank = SlashPrefix;
			}
			else if(startsAWord(title, needle))
			{
				rank = TitleWordPrefix;
			}
			else if(title.contains(needle, Qt::CaseInsensitive))
			{
				rank = Substring;
			}
			else if(isSubsequence(title, needle))
			{
				rank = Subsequence;
			}
		}
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
	const Entry& entry = m_entries.at(index.row());
	const QVariantMap map = toMap(entry);
	switch(role)
	{
	case IdRole:
		return map.value(u"id"_s);
	case TitleRole:
		return map.value(u"title"_s);
	case SlashRole:
		return map.value(u"slash"_s);
	case AliasesRole:
		return map.value(u"aliases"_s);
	case HintRole:
		return map.value(u"hint"_s);
	case ShortcutRole:
		return map.value(u"shortcut"_s);
	case KeysRole:
		return map.value(u"keys"_s);
	case CategoryRole:
		return map.value(u"category"_s);
	case IconRole:
		return map.value(u"icon"_s);
	case EnabledRole:
		return map.value(u"enabled"_s);
	case SourceRole:
		return map.value(u"source"_s);
	default:
		return {};
	}
}

QHash<int, QByteArray> CommandRegistry::roleNames() const
{
	return {{IdRole, "id"},
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
