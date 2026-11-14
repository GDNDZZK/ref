#include "link_colorizer.h"

#include <QScopedValueRollback>
#include <QTextBlock>
#include <QTextCharFormat>
#include <QTextDocument>
#include <QTextFragment>
#include <QTimer>

LinkHighlighter::LinkHighlighter(QTextDocument* document, const QColor& color) : QSyntaxHighlighter(document), m_color(color)
{
	// setMarkdown() reports only the clear before the import, so the highlighter's own pass sees an
	// empty document. One full pass after the change is done covers the imported text.
	connect(document, &QTextDocument::contentsChanged, this, &LinkHighlighter::scheduleRehighlight);
}

void LinkHighlighter::scheduleRehighlight()
{
	if(m_pending || m_highlighting)
	{
		return;
	}
	m_pending = true;
	QTimer::singleShot(0, this, [this] {
		m_pending = false;
		const QScopedValueRollback guard(m_highlighting, true); // the new formats report a change too
		rehighlight();
	});
}

void LinkHighlighter::setColor(const QColor& color)
{
	if(color == m_color)
	{
		return;
	}
	m_color = color;
	const QScopedValueRollback guard(m_highlighting, true);
	rehighlight();
}

void LinkHighlighter::highlightBlock(const QString&)
{
	const QTextBlock current = currentBlock();
	QTextCharFormat format;
	format.setForeground(m_color);
	for(auto it = current.begin(); !it.atEnd(); ++it)
	{
		const QTextFragment fragment = it.fragment();
		if(fragment.charFormat().isAnchor())
		{
			setFormat(fragment.position() - current.position(), fragment.length(), format);
		}
	}
}

LinkColorizer::LinkColorizer(QObject* parent) : QObject(parent)
{
}

void LinkColorizer::setDocument(QQuickTextDocument* document)
{
	if(m_document == document)
	{
		return;
	}
	delete m_highlighter;
	m_document = document;
	if(m_document)
	{
		m_highlighter = new LinkHighlighter(m_document->textDocument(), m_color);
	}
	emit documentChanged();
}

void LinkColorizer::setColor(const QColor& color)
{
	if(m_color == color)
	{
		return;
	}
	m_color = color;
	if(m_highlighter)
	{
		m_highlighter->setColor(color);
	}
	emit colorChanged();
}
