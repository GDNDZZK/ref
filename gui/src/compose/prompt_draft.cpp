#include "compose/prompt_draft.h"

#include <QRegularExpression>

#include <algorithm>

using namespace Qt::StringLiterals;

namespace refgui::compose
{
namespace
{

// "[Image 3]" -> 3. Numbers in order of appearance.
QList<int> tokenNumbers(const QString& text)
{
	static const QRegularExpression token(uR"(\[Image (\d+)\])"_s);
	QList<int> numbers;
	for(const QRegularExpressionMatch& match : token.globalMatch(text))
	{
		numbers.append(match.captured(1).toInt());
	}
	return numbers;
}

// Stands in for a token while the tokens are renumbered. U+0001 does not occur in prompts.
QString placeholder(qsizetype index)
{
	return u"\x01%1\x01"_s.arg(index);
}

} // namespace

void PromptDraft::setText(const QString& text, qsizetype cursor)
{
	if(text == m_text)
	{
		setCursor(cursor);
		return;
	}

	const qsizetype clampedCursor = std::clamp<qsizetype>(cursor, 0, text.size());
	const bool singleCharInsert = text.size() == m_text.size() + 1 && clampedCursor > 0
	                              && QStringView(text).left(clampedCursor - 1) == QStringView(m_text).left(clampedCursor - 1)
	                              && QStringView(text).mid(clampedCursor) == QStringView(m_text).mid(clampedCursor - 1);
	const bool isCoalescibleChar = singleCharInsert && !text[clampedCursor - 1].isSpace();
	const bool canCoalesce = m_coalescing && isCoalescibleChar && (clampedCursor - 1) == m_coalesceCursor;
	if(!canCoalesce)
	{
		pushUndoSnapshot(currentSnapshot());
	}
	else
	{
		m_redo.clear();
	}

	m_text = text;
	m_cursor = clampedCursor;
	dropOrphans();

	if(isCoalescibleChar)
	{
		m_coalescing = true;
		m_coalesceCursor = m_cursor;
	}
	else
	{
		m_coalescing = false;
	}
}

void PromptDraft::setCursor(qsizetype cursor)
{
	const qsizetype clamped = std::clamp<qsizetype>(cursor, 0, m_text.size());
	if(clamped == m_cursor)
	{
		return;
	}
	m_cursor = clamped;
	m_coalescing = false;
}
void PromptDraft::setShellMode(bool shellMode)
{
	if(shellMode == m_shellMode)
	{
		return;
	}
	pushUndoSnapshot(currentSnapshot());
	m_coalescing = false;
	m_shellMode = shellMode;
}

void PromptDraft::replace(const QString& text, qsizetype cursor, bool shellMode)
{
	if(text == m_text && shellMode == m_shellMode)
	{
		setCursor(cursor);
		return;
	}
	pushUndoSnapshot(currentSnapshot());
	m_coalescing = false;
	m_shellMode = shellMode;
	m_text = text;
	m_cursor = std::clamp<qsizetype>(cursor, 0, text.size());
	dropOrphans();
}

PromptDraft::Snapshot PromptDraft::currentSnapshot() const
{
	return {.text = m_text,
	        .cursor = m_cursor,
	        .attachments = m_attachments,
	        .lastImageNumber = m_lastImageNumber,
	        .shellMode = m_shellMode};
}

void PromptDraft::applySnapshot(Snapshot snapshot)
{
	m_text = std::move(snapshot.text);
	m_cursor = std::clamp<qsizetype>(snapshot.cursor, 0, m_text.size());
	m_attachments = std::move(snapshot.attachments);
	m_lastImageNumber = snapshot.lastImageNumber;
	m_shellMode = snapshot.shellMode;
}

void PromptDraft::pushUndoSnapshot(Snapshot snapshot)
{
	m_undo.append(std::move(snapshot));
	if(m_undo.size() > MaxUndoSnapshots)
	{
		m_undo.removeFirst();
	}
	m_redo.clear();
}

void PromptDraft::resetState()
{
	m_text.clear();
	m_cursor = 0;
	m_attachments.clear();
	m_lastImageNumber = 0;
	m_shellMode = false;
}

QString PromptDraft::nextToken(QStringView extraText)
{
	while(true)
	{
		const QString candidate = u"[Image %1]"_s.arg(++m_lastImageNumber);
		if(!m_text.contains(candidate) && !extraText.contains(candidate))
		{
			return candidate;
		}
	}
}

void PromptDraft::attach(proto::NamedImage image)
{
	pushUndoSnapshot(currentSnapshot());
	m_coalescing = false;
	attachImpl(std::move(image));
}

void PromptDraft::attachImpl(proto::NamedImage image)
{
	const QString token = nextToken();
	m_attachments.append({token, std::move(image)});

	const bool followsText = m_cursor > 0 && m_text[m_cursor - 1] != u' ' && m_text[m_cursor - 1] != u'\n';
	const QString inserted = (followsText ? u" "_s : QString()) + token + u" "_s;
	m_text.insert(m_cursor, inserted);
	m_cursor += inserted.size();
}

void PromptDraft::insertPath(const QString& path)
{
	pushUndoSnapshot(currentSnapshot());
	m_coalescing = false;
	const bool followsText = m_cursor > 0 && m_text[m_cursor - 1] != u' ' && m_text[m_cursor - 1] != u'\n';
	const bool precedesText = m_cursor < m_text.size() && m_text[m_cursor] != u' ' && m_text[m_cursor] != u'\n';
	const QString inserted = (followsText ? u" "_s : QString()) + path + (precedesText || m_cursor == m_text.size() ? u" "_s : QString());
	m_text.insert(m_cursor, inserted);
	m_cursor += inserted.size();
}

void PromptDraft::removeAttachment(qsizetype index)
{
	if(index < 0 || index >= m_attachments.size())
	{
		return;
	}
	pushUndoSnapshot(currentSnapshot());
	m_coalescing = false;

	const QString token = m_attachments.at(index).token;
	m_attachments.removeAt(index);
	if(m_attachments.isEmpty())
	{
		m_lastImageNumber = 0;
	}

	const qsizetype at = m_text.indexOf(token);
	if(at < 0)
	{
		return;
	}
	// Take one of the two spaces around the token with it, so removing it does not leave a gap.
	qsizetype end = at + token.size();
	qsizetype start = at;
	if(end < m_text.size() && m_text[end] == u' ')
	{
		++end;
	}
	else if(start > 0 && m_text[start - 1] == u' ')
	{
		--start;
	}
	m_text.remove(start, end - start);
	if(m_cursor >= end)
	{
		m_cursor -= end - start;
	}
	else if(m_cursor > start)
	{
		m_cursor = start;
	}
}

void PromptDraft::restore(const QString& text, const QList<proto::NamedImage>& images, bool shellMode)
{
	if(text.trimmed().isEmpty() && images.isEmpty())
	{
		return;
	}

	pushUndoSnapshot(currentSnapshot());
	m_coalescing = false;
	m_shellMode = shellMode;

	QList<int> numbers = tokenNumbers(text);
	std::ranges::sort(numbers);
	numbers.erase(std::ranges::unique(numbers).begin(), numbers.end());

	// Two passes, through placeholders, so a renumbered token is never renumbered again.
	QString restored = text;
	qsizetype bound = 0;
	for(const int number : numbers)
	{
		if(bound == images.size())
		{
			break;
		}
		restored.replace(u"[Image %1]"_s.arg(number), placeholder(bound));
		++bound;
	}
	for(qsizetype i = 0; i < bound; ++i)
	{
		const QString token = nextToken(restored);
		restored.replace(placeholder(i), token);
		m_attachments.append({token, images[i]});
	}

	qsizetype keepEnd = m_text.size();
	while(keepEnd > 0 && m_text[keepEnd - 1].isSpace())
	{
		--keepEnd;
	}
	m_text = keepEnd == 0 ? restored : m_text.left(keepEnd) + u"\n\n"_s + restored;
	m_cursor = m_text.size();

	for(qsizetype i = bound; i < images.size(); ++i)
	{
		attachImpl(images[i]);
	}
}

Submission PromptDraft::take()
{
	Submission submission{.text = m_text, .images = {}};
	for(const Attachment& attachment : m_attachments)
	{
		submission.images.append({.mime = attachment.image.mime, .data = attachment.image.data});
	}
	resetState();
	m_undo.clear();
	m_redo.clear();
	m_coalescing = false;
	return submission;
}

bool PromptDraft::undo()
{
	if(m_undo.isEmpty())
	{
		return false;
	}
	m_redo.append(currentSnapshot());
	applySnapshot(m_undo.takeLast());
	m_coalescing = false;
	return true;
}

bool PromptDraft::redo()
{
	if(m_redo.isEmpty())
	{
		return false;
	}
	m_undo.append(currentSnapshot());
	applySnapshot(m_redo.takeLast());
	m_coalescing = false;
	return true;
}

void PromptDraft::clear()
{
	if(!m_text.isEmpty() || !m_attachments.isEmpty() || m_shellMode)
	{
		pushUndoSnapshot(currentSnapshot());
	}
	m_coalescing = false;
	resetState();
}

void PromptDraft::dropOrphans()
{
	m_attachments.removeIf([this](const Attachment& attachment) { return !m_text.contains(attachment.token); });
	if(m_attachments.isEmpty())
	{
		m_lastImageNumber = 0;
	}
}

QList<PromptDraft::TokenSpan> PromptDraft::tokenSpans() const
{
	QList<TokenSpan> spans;
	spans.reserve(m_attachments.size());
	for(qsizetype i = 0; i < m_attachments.size(); ++i)
	{
		const QString& tok = m_attachments.at(i).token;
		qsizetype from = 0;
		while(from < m_text.size())
		{
			const qsizetype at = m_text.indexOf(tok, from);
			if(at < 0)
			{
				break;
			}
			spans.append({.start = at, .end = at + tok.size()});
			from = at + tok.size();
		}
	}
	std::ranges::sort(spans, {}, &TokenSpan::start);
	return spans;
}

qsizetype PromptDraft::snapCursor(qsizetype cursor, qsizetype previousCursor) const
{
	for(const TokenSpan& span : tokenSpans())
	{
		if(cursor > span.start && cursor < span.end)
		{
			if(previousCursor <= span.start)
			{
				return span.end;
			}
			if(previousCursor >= span.end)
			{
				return span.start;
			}
			return (cursor - span.start) < (span.end - cursor) ? span.start : span.end;
		}
	}
	return cursor;
}

std::pair<qsizetype, qsizetype> PromptDraft::expandSelection(qsizetype start, qsizetype end) const
{
	qsizetype lo = std::clamp<qsizetype>(std::min(start, end), 0, m_text.size());
	qsizetype hi = std::clamp<qsizetype>(std::max(start, end), 0, m_text.size());
	if(lo == hi)
	{
		return {lo, hi};
	}
	for(const TokenSpan& span : tokenSpans())
	{
		if(lo < span.end && hi > span.start)
		{
			lo = std::min(lo, span.start);
			hi = std::max(hi, span.end);
		}
	}
	return {lo, hi};
}

bool PromptDraft::deletePlaceholderAt(qsizetype cursor, bool backward)
{
	const qsizetype clamped = std::clamp<qsizetype>(cursor, 0, m_text.size());
	for(const TokenSpan& span : tokenSpans())
	{
		const bool hitBackward = backward && (clamped == span.end || (clamped > span.start && clamped < span.end));
		const bool hitForward = !backward && (clamped == span.start || (clamped > span.start && clamped < span.end));
		if(!hitBackward && !hitForward)
		{
			continue;
		}
		pushUndoSnapshot(currentSnapshot());
		m_coalescing = false;
		qsizetype delStart = span.start;
		qsizetype delEnd = span.end;
		if(delEnd < m_text.size() && m_text[delEnd] == u' ')
		{
			++delEnd;
		}
		else if(delStart > 0 && m_text[delStart - 1] == u' ')
		{
			--delStart;
		}
		m_text.remove(delStart, delEnd - delStart);
		m_cursor = delStart;
		dropOrphans();
		return true;
	}
	return false;
}

} // namespace refgui::compose
