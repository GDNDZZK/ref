#pragma once

#include <QColor>
#include <QTimer>
#include <QObject>
#include <QPointer>
#include <QQuickTextDocument>
#include <QSyntaxHighlighter>
#include <QtQmlIntegration>

// Colours the links of a document. Qt's Markdown reader gives each link the application palette's
// link colour (pure blue) when it reads the text; a highlighter's colour wins over it and can change.
class LinkHighlighter : public QSyntaxHighlighter
{
public:
	LinkHighlighter(QTextDocument* document, const QColor& color, bool streaming);
	~LinkHighlighter() override;
	void setColor(const QColor& color);
	void setStreaming(bool streaming);

protected:
	void highlightBlock(const QString& text) override;

private:
	static constexpr int StreamingThrottleMs = 180;

	void scheduleRehighlight();
	void runRehighlight();

	QTimer m_timer;
	QColor m_color;
	bool m_streaming = false;
	bool m_highlighting = false;
};

// LinkHighlighter for a TextEdit: `LinkColorizer { document: edit.textDocument; color: ... }`.
class LinkColorizer : public QObject
{
	Q_OBJECT
	QML_ELEMENT
	Q_PROPERTY(QQuickTextDocument* document READ document WRITE setDocument NOTIFY documentChanged FINAL)
	Q_PROPERTY(QColor color READ color WRITE setColor NOTIFY colorChanged FINAL)
	Q_PROPERTY(bool streaming READ isStreaming WRITE setStreaming NOTIFY streamingChanged FINAL)

public:
	explicit LinkColorizer(QObject* parent = nullptr);

	QQuickTextDocument* document() const { return m_document; }
	void setDocument(QQuickTextDocument* document);
	QColor color() const { return m_color; }
	void setColor(const QColor& color);
	bool isStreaming() const { return m_streaming; }
	void setStreaming(bool streaming);

signals:
	void documentChanged();
	void colorChanged();
	void streamingChanged();

private:
	QPointer<QQuickTextDocument> m_document;
	QColor m_color;
	QPointer<LinkHighlighter> m_highlighter; // a child of the text document
	bool m_streaming = false;
};
