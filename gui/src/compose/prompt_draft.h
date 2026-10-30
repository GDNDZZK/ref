#pragma once

#include "proto/types.h"

#include <QList>
#include <QString>

#include <optional>

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
	bool isEmpty() const { return m_text.trimmed().isEmpty() && m_attachments.isEmpty(); }

	// The user edited the text. Attachments whose token is no longer in it are dropped.
	void setText(const QString& text, qsizetype cursor);
	void setCursor(qsizetype cursor);

	// Puts `image` in the text at the cursor as a new token, with a space before it when it
	// follows other text and one after it.
	void attach(proto::NamedImage image);
	// Removes the attachment and its token.
	void removeAttachment(qsizetype index);

	// Puts a prompt that was taken back (undo, withdrawing a queued prompt) into the draft:
	// it fills an empty draft and goes after a blank line otherwise. The tokens in `text` are
	// bound to `images` in order of their numbers and renumbered after the draft's own;
	// images without a token get one at the end. The cursor ends up at the end.
	void restore(const QString& text, const QList<proto::NamedImage>& images);

	// The prompt as it is sent, and the draft emptied.
	Submission take();
	// Empties the draft.
	void clear();

private:
	QString nextToken();
	void dropOrphans();

	QString m_text;
	qsizetype m_cursor = 0;
	QList<Attachment> m_attachments;
	int m_lastImageNumber = 0; // numbers keep rising after the draft is emptied, as in the TUI
};

} // namespace refgui::compose
