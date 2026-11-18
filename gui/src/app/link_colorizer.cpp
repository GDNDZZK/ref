#include "link_colorizer.h"

#include <QScopedValueRollback>
#include <QTextBlock>
#include <QTextCharFormat>
#include <QTextDocument>
#include <QTextFragment>
#include <QTimer>

LinkHighlighter::LinkHighlighter(QTextDocument* document, const QColor& color, bool streaming)
	: QSyntaxHighlighter(document)
	, m_color(color)
	, m_streaming(streaming)
{
	m_timer.setSingleShot(true);
	connect(&m_timer, &QTimer::timeout, this, &LinkHighlighter::runRehighlight);
	// setMarkdown() reports only the clear before the import, so the highlighter's own pass sees an
	// empty document. One full pass after the change is done covers the imported text.
	connect(document, &QTextDocument::contentsChanged, this, &LinkHighlighter::scheduleRehighlight);
}

LinkHighlighter::~LinkHighlighter()
{
	m_timer.stop();
	if(QTextDocument* doc = document())
	{
		disconnect(doc, &QTextDocument::contentsChanged, this, &LinkHighlighter::scheduleRehighlight);
	}
	setDocument(nullptr);
}

void LinkHighlighter::scheduleRehighlight()
{
	if(m_highlighting || m_timer.isActive())
	{
		return;
	}
	m_timer.start(m_streaming ? StreamingThrottleMs : 0);
}

void LinkHighlighter::runRehighlight()
{
	if(m_highlighting)
	{
		return;
	}
	const QScopedValueRollback guard(m_highlighting, true); // the new formats report a change too
	rehighlight();
}

void LinkHighlighter::setColor(const QColor& color)
{
	if(color == m_color)
	{
		return;
	}
	m_color = color;
	m_timer.stop();
	runRehighlight();
}

void LinkHighlighter::setStreaming(bool streaming)
{
	if(streaming == m_streaming)
	{
		return;
	}
	m_streaming = streaming;
	if(!m_streaming)
	{
		m_timer.stop();
		runRehighlight();
	}
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
		m_highlighter = new LinkHighlighter(m_document->textDocument(), m_color, m_streaming);
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

void LinkColorizer::setStreaming(bool streaming)
{
	if(m_streaming == streaming)
	{
		return;
	}
	m_streaming = streaming;
	if(m_highlighter)
	{
		m_highlighter->setStreaming(streaming);
	}
	emit streamingChanged();
}
