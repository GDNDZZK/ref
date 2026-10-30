#pragma once

#include <QString>
#include <QStringList>

#include <optional>

namespace refgui::compose
{

// The prompts sent before, shared by all sessions and with the TUI: one JSON object
// `{"text": …}` per line in `<state dir>/prompt-history.jsonl`. Shell lines are kept with a
// leading `!`. The newest 500 are kept.
class PromptHistory
{
public:
	static constexpr qsizetype MaxItems = 500;

	// Memory only when `path` is empty.
	explicit PromptHistory(QString path = {});

	const QStringList& items() const { return m_items; }

	// Remembers `text` (trailing whitespace dropped). Blank text and a repeat of the newest
	// entry are ignored. Failing to write the file is logged; the entry stays in memory.
	void push(const QString& text);

private:
	void appendToFile(const QString& text);
	void rewriteFile();

	QString m_path;
	QStringList m_items;
};

// Where the user is in the history while pressing Up and Down, for one input box. The text
// being written before the first Up is kept and comes back after the last Down.
class HistoryBrowser
{
public:
	explicit HistoryBrowser(const PromptHistory& history);

	bool browsing() const { return m_position.has_value(); }

	// The older entry, or nullopt when there is none (empty history, or at the oldest).
	std::optional<QString> previous(const QString& current);
	// The newer entry, the saved text after the newest, or nullopt when not browsing.
	std::optional<QString> next();
	// Typing or sending ends the browsing.
	void reset() { m_position.reset(); }

private:
	const PromptHistory& m_history;
	std::optional<qsizetype> m_position;
	QString m_draft;
};

} // namespace refgui::compose
