#include "compose/prompt_history.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLoggingCategory>

using namespace Qt::StringLiterals;

Q_LOGGING_CATEGORY(lcHistory, "refgui.history")

namespace refgui::compose
{
namespace
{

const QString TextKey = u"text"_s;

QByteArray lineOf(const QString& text)
{
	return QJsonDocument(QJsonObject{{TextKey, text}}).toJson(QJsonDocument::Compact) + '\n';
}

} // namespace

PromptHistory::PromptHistory(QString path)
	: m_path(std::move(path))
{
	QFile file(m_path);
	if(m_path.isEmpty() || !file.open(QIODevice::ReadOnly))
	{
		return;
	}
	while(!file.atEnd())
	{
		const QJsonObject line = QJsonDocument::fromJson(file.readLine()).object();
		if(line.contains(TextKey))
		{
			m_items.append(line.value(TextKey).toString());
		}
	}
	if(m_items.size() > MaxItems)
	{
		m_items.remove(0, m_items.size() - MaxItems);
	}
}

void PromptHistory::push(const QString& text)
{
	QString trimmed = text;
	while(!trimmed.isEmpty() && trimmed.back().isSpace())
	{
		trimmed.removeLast();
	}
	if(trimmed.isEmpty() || (!m_items.isEmpty() && m_items.last() == trimmed))
	{
		return;
	}

	m_items.append(trimmed);
	if(m_items.size() > MaxItems)
	{
		m_items.remove(0, m_items.size() - MaxItems);
		rewriteFile();
		return;
	}
	appendToFile(trimmed);
}

void PromptHistory::appendToFile(const QString& text)
{
	if(m_path.isEmpty())
	{
		return;
	}
	QDir().mkpath(QFileInfo(m_path).absolutePath());
	QFile file(m_path);
	if(!file.open(QIODevice::Append) || file.write(lineOf(text)) < 0)
	{
		qCWarning(lcHistory) << "cannot write" << m_path << file.errorString();
	}
}

void PromptHistory::rewriteFile()
{
	if(m_path.isEmpty())
	{
		return;
	}
	QFile file(m_path);
	if(!file.open(QIODevice::WriteOnly | QIODevice::Truncate))
	{
		qCWarning(lcHistory) << "cannot write" << m_path << file.errorString();
		return;
	}
	for(const QString& item : m_items)
	{
		if(file.write(lineOf(item)) < 0)
		{
			qCWarning(lcHistory) << "cannot write" << m_path << file.errorString();
			return;
		}
	}
}

HistoryBrowser::HistoryBrowser(const PromptHistory& history)
	: m_history(history)
{
}

std::optional<QString> HistoryBrowser::previous(const QString& current, bool shellOnly)
{
	const QStringList& items = m_history.items();
	if(items.isEmpty())
	{
		return std::nullopt;
	}
	const bool wantShell = m_position ? m_shellOnly : shellOnly;
	const auto matches = [&](const QString& item) { return !wantShell || item.startsWith(u'!'); };
	const qsizetype startFrom = m_position ? *m_position - 1 : items.size() - 1;
	for(qsizetype idx = startFrom; idx >= 0; --idx)
	{
		if(matches(items.at(idx)))
		{
			if(!m_position)
			{
				m_draft = current;
				m_shellOnly = shellOnly;
			}
			m_position = idx;
			return items.at(idx);
		}
	}
	return std::nullopt;
}

std::optional<QString> HistoryBrowser::next()
{
	if(!m_position)
	{
		return std::nullopt;
	}
	const QStringList& items = m_history.items();
	const auto matches = [&](const QString& item) { return !m_shellOnly || item.startsWith(u'!'); };
	for(qsizetype idx = *m_position + 1; idx < items.size(); ++idx)
	{
		if(matches(items.at(idx)))
		{
			m_position = idx;
			return items.at(idx);
		}
	}
	m_position.reset();
	m_shellOnly = false;
	return std::exchange(m_draft, QString());
}

} // namespace refgui::compose
