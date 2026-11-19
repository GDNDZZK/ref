#pragma once

#include "proto/types.h"

#include <QList>
#include <QString>
#include <QStringView>

#include <optional>
#include <utility>

namespace refgui::compose
{

// What Enter sends: the text, with `[Image N]` tokens, and the images those tokens name.
struct Submission
{
	QString text;
	QList<proto::ImageData> images;
};

// The prompt being written: text, cursor, and the images attached to it.
//
// Like the TUI, an attached image puts a token `[Image N]` into the text. The text that is
// sent carries the tokens, so a prompt written here can be taken back in the TUI or the
// WebUI and the other way round. An image whose token has left the text is gone too.
// Positions are QString (UTF-16) indexes, which is what a QML TextArea uses.
class PromptDraft
{
public:
	struct Attachment
	{
		QString token;
		proto::NamedImage image;
	};

	const QString& text() const { return m_text; }
	qsizetype cursor() const { return m_cursor; }
	const QList<Attachment>& attachments() const { return m_attachments; }
	bool shellMode() const { return m_shellMode; }
	bool isEmpty() const { return m_text.trimmed().isEmpty() && m_attachments.isEmpty(); }
	bool canUndo() const { return !m_undo.isEmpty(); }
	bool canRedo() const { return !m_redo.isEmpty(); }

	// The user edited the text. Attachments whose token is no longer in it are dropped.
	void setText(const QString& text, qsizetype cursor);
	void setCursor(qsizetype cursor);
	void setShellMode(bool shellMode);
	// Replaces text, cursor, and shellMode as a single undo step.
	void replace(const QString& text, qsizetype cursor, bool shellMode);

	// Puts `image` in the text at the cursor as a new token, with a space before it when it
	// follows other text and one after it.
	void attach(proto::NamedImage image);
	// Inserts `path` at the cursor with surrounding spacing like a mention.
	void insertPath(const QString& path);
	// Removes the attachment and its token.
	void removeAttachment(qsizetype index);

	// Puts a prompt that was taken back (undo, withdrawing a queued prompt) into the draft:
	// it fills an empty draft and goes after a blank line otherwise. The tokens in `text` are
	// bound to `images` in order of their numbers and renumbered after the draft's own;
	// images without a token get one at the end. The cursor ends up at the end.
	void restore(const QString& text, const QList<proto::NamedImage>& images, bool shellMode = false);

	// The prompt as it is sent, and the draft emptied (and undo/redo history cleared).
	Submission take();
	// Undo/redo across text, cursor, attachments, image numbering, and shell mode.
	bool undo();
	bool redo();
	qsizetype snapCursor(qsizetype cursor, qsizetype previousCursor) const;
	std::pair<qsizetype, qsizetype> expandSelection(qsizetype start, qsizetype end) const;
	bool deletePlaceholderAt(qsizetype cursor, bool backward);
	// Empties the draft.
	void clear();
private:
	static constexpr qsizetype MaxUndoSnapshots = 200;

	struct Snapshot
	{
		QString text;
		qsizetype cursor = 0;
		QList<Attachment> attachments;
		int lastImageNumber = 0;
		bool shellMode = false;
	};
	struct TokenSpan
	{
		qsizetype start = 0;
		qsizetype end = 0;
	};
	Snapshot currentSnapshot() const;
	void applySnapshot(Snapshot snapshot);
	void pushUndoSnapshot(Snapshot snapshot);
	void resetState();
	void attachImpl(proto::NamedImage image);
	QList<TokenSpan> tokenSpans() const;
	QString nextToken(QStringView extraText = {});
	void dropOrphans();
	QString m_text;
	qsizetype m_cursor = 0;
	QList<Attachment> m_attachments;
	int m_lastImageNumber = 0; // resets to 0 whenever the draft has no attachments
	bool m_shellMode = false;
	QList<Snapshot> m_undo;
	QList<Snapshot> m_redo;
	bool m_coalescing = false;
	qsizetype m_coalesceCursor = 0;
};

} // namespace refgui::compose
