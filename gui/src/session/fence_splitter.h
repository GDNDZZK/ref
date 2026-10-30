// Adapted from an early version of qutie by LightBlueCube
#pragma once

#include <QChar>
#include <QList>
#include <QString>
#include <QStringView>

#include <optional>
#include <variant>

namespace refgui::session
{

// Splits streamed Markdown into prose and fenced code blocks.
//
// Follows the CommonMark fenced code block rules: a fence is a run of at least three
// ``` or ~~~, indented by at most three spaces; the first word of the info string is
// the language; the closing fence uses the same marker and is at least as long.
//
// Text is released as soon as it is unambiguous. Only a line that may still turn into
// a fence is held back until it ends. Line breaks are released lazily, so a block never
// ends with the line breaks that separate it from the next one.
class FenceSplitter
{
public:
	struct CodeOpened
	{
		QString language; // empty when the fence has no info string
	};
	struct CodeClosed
	{
	};
	struct TextAppended
	{
		QString text; // belongs to the open code block, or to prose outside one
	};
	using Event = std::variant<CodeOpened, CodeClosed, TextAppended>;

	QList<Event> feed(QStringView delta);

	// Releases the held-back last line. Feeding afterwards is not supported.
	QList<Event> finish();

private:
	struct Fence
	{
		QChar marker{};
		qsizetype length = 0;
		qsizetype indent = 0;
		QString language{};
	};

	static std::optional<Fence> parseOpeningFence(QStringView line);
	static bool closes(const Fence& fence, QStringView line);

	void appendToLine(QStringView text, QList<Event>& events);
	void endLine(QList<Event>& events);
	bool mayBecomeFence(QStringView partialLine) const;
	QStringView stripCodeIndent(QStringView line) const;
	void emitText(QStringView text, QList<Event>& events);

	std::optional<Fence> m_openFence;
	QString m_heldLine;         // start of the current line while it may still be a fence
	bool m_lineReleased = false; // the current line is plain content and already emitted in part
	qsizetype m_pendingLineBreaks = 0;
	bool m_atBlockStart = true; // nothing emitted since the last fence or the start
};

} // namespace refgui::session
