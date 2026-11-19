#pragma once

#include <QString>
#include <QStringList>
#include <QStringView>

#include <optional>
#include <variant>

namespace refgui::session
{

// Splits prose outside fenced code blocks into Markdown prose and GFM pipe tables.
//
// Lines that may open or belong to a table are held until a separator row confirms or
// rejects the table; plain prose without a leading pipe streams immediately.
class TableSplitter
{
public:
	struct TextAppended
	{
		QString text;
	};
	struct TableStarted
	{
		QStringList headers;
		QStringList alignments;
		QString rawMarkdown;
		qsizetype replaceChars = 0; // trailing characters already emitted to prose for a non-leading-pipe header
	};
	struct TableRowAppended
	{
		QStringList cells;
		QString rawLine;
	};
	struct TableClosed
	{
	};
	using Event = std::variant<TextAppended, TableStarted, TableRowAppended, TableClosed>;

	QList<Event> feed(QStringView delta);
	QList<Event> finish();

private:
	enum class State
	{
		Prose,
		CandidateHeader,
		InTable,
	};

	static std::optional<QStringList> splitTableRow(QStringView line);
	static std::optional<QStringList> parseSeparatorRow(QStringView line, qsizetype expectedCols);

	void appendToLine(QStringView text, QList<Event>& events);
	void endLine(QList<Event>& events);
	void rejectCandidateHeader(QList<Event>& events);
	void emitText(QStringView text, QList<Event>& events);
	void closeTable(QList<Event>& events);
	void appendProseLine(QStringView line, QList<Event>& events);

	State m_state = State::Prose;
	QString m_heldLine;
	QString m_proseLine;
	bool m_lineReleased = false;
	qsizetype m_emittedLineChars = 0;
	qsizetype m_pendingLineBreaks = 0;
	bool m_atBlockStart = true;

	QString m_headerRaw;
	QStringList m_headerCells;
	bool m_headerFromProse = false;
	qsizetype m_headerReplaceChars = 0;
	qsizetype m_tableCols = 0;
};

} // namespace refgui::session
