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
		file.write(lineOf(item));
	}
}

HistoryBrowser::HistoryBrowser(const PromptHistory& history)
	: m_history(history)
{
}

std::optional<QString> HistoryBrowser::previous(const QString& current)
{
	const QStringList& items = m_history.items();
	if(items.isEmpty())
	{
		return std::nullopt;
	}
	if(!m_position)
	{
		m_draft = current;
		m_position = items.size() - 1;
	}
	else if(*m_position == 0)
	{
		return std::nullopt;
	}
	else
	{
		--*m_position;
	}
	return items.at(*m_position);
}

std::optional<QString> HistoryBrowser::next()
{
	if(!m_position)
	{
		return std::nullopt;
	}
	if(*m_position + 1 >= m_history.items().size())
	{
		m_position.reset();
		return std::exchange(m_draft, QString());
	}
	++*m_position;
	return m_history.items().at(*m_position);
}

} // namespace refgui::compose
