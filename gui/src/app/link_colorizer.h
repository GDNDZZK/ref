#pragma once

#include <QColor>
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
	LinkHighlighter(QTextDocument* document, const QColor& color);
	~LinkHighlighter() override;
	void setColor(const QColor& color);

protected:
	void highlightBlock(const QString& text) override;

private:
	void scheduleRehighlight();

	QColor m_color;
	bool m_pending = false;
	bool m_highlighting = false;
};

// LinkHighlighter for a TextEdit: `LinkColorizer { document: edit.textDocument; color: ... }`.
class LinkColorizer : public QObject
{
	Q_OBJECT
	QML_ELEMENT
	Q_PROPERTY(QQuickTextDocument* document READ document WRITE setDocument NOTIFY documentChanged FINAL)
	Q_PROPERTY(QColor color READ color WRITE setColor NOTIFY colorChanged FINAL)

public:
	explicit LinkColorizer(QObject* parent = nullptr);

	QQuickTextDocument* document() const { return m_document; }
	void setDocument(QQuickTextDocument* document);
	QColor color() const { return m_color; }
	void setColor(const QColor& color);

signals:
	void documentChanged();
	void colorChanged();

private:
	QPointer<QQuickTextDocument> m_document;
	QColor m_color;
	QPointer<LinkHighlighter> m_highlighter; // a child of the text document
};
