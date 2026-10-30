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
	m_text = text;
	m_cursor = std::clamp<qsizetype>(cursor, 0, text.size());
	dropOrphans();
}

void PromptDraft::setCursor(qsizetype cursor)
{
	m_cursor = std::clamp<qsizetype>(cursor, 0, m_text.size());
}

QString PromptDraft::nextToken()
{
	return u"[Image %1]"_s.arg(++m_lastImageNumber);
}

void PromptDraft::attach(proto::ImageData image, std::optional<QString> name)
{
	const QString token = nextToken();
	m_attachments.append({token, std::move(image), std::move(name)});

	const bool followsText = m_cursor > 0 && m_text[m_cursor - 1] != u' ' && m_text[m_cursor - 1] != u'\n';
	const QString inserted = (followsText ? u" "_s : QString()) + token + u" "_s;
	m_text.insert(m_cursor, inserted);
	m_cursor += inserted.size();
}

void PromptDraft::removeAttachment(qsizetype index)
{
	const QString token = m_attachments.at(index).token;
	m_attachments.removeAt(index);

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

void PromptDraft::restore(const QString& text, const QList<NamedImage>& images)
{
	if(text.trimmed().isEmpty() && images.isEmpty())
	{
		return;
	}

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
		const QString token = nextToken();
		restored.replace(placeholder(i), token);
		m_attachments.append({token, images[i].image, images[i].name});
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
		attach(images[i].image, images[i].name);
	}
}

Submission PromptDraft::take()
{
	Submission submission{.text = m_text, .images = {}};
	for(const Attachment& attachment : m_attachments)
	{
		submission.images.append(attachment.image);
	}
	clear();
	return submission;
}

void PromptDraft::clear()
{
	m_text.clear();
	m_cursor = 0;
	m_attachments.clear();
}

void PromptDraft::dropOrphans()
{
	m_attachments.removeIf([this](const Attachment& attachment) { return !m_text.contains(attachment.token); });
}

} // namespace refgui::compose
