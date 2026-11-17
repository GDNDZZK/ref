#include "app/link_colorizer.h"

#include <QGuiApplication>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QQuickItem>
#include <QQuickTextDocument>
#include <QTextBlock>
#include <QTextDocument>
#include <QTextLayout>
#include <QtTest>

#include <memory>

using namespace Qt::StringLiterals;

class TestLinkColorizer : public QObject
{
	Q_OBJECT

private slots:
	void coloursOnlyTheLink();
	void followsTheColour();
	void coloursTextAddedLater();
	void settlesAfterItsOwnPass();
	void followsTheBoundColourInQml();
	void throttlesWhileStreamingAndFlushesOnEnd();
};

namespace
{

// The colour the highlighter gave the text at `position` of the first block, invalid for none.
QColor colourAt(const QTextDocument& document, int position)
{
	const QTextBlock block = document.firstBlock();
	for(const QTextLayout::FormatRange& range : block.layout()->formats())
	{
		if(position >= range.start && position < range.start + range.length && range.format.hasProperty(QTextFormat::ForegroundBrush))
		{
			return range.format.foreground().color();
		}
	}
	return {};
}

} // namespace

void TestLinkColorizer::coloursOnlyTheLink()
{
	QTextDocument document;
	document.setMarkdown(u"see [the docs](https://example.com) here"_s);
	LinkHighlighter highlighter(&document, QColor(u"#356889"_s));
	highlighter.rehighlight();

	const int link = static_cast<int>(document.toPlainText().indexOf(u"the docs"_s));
	QCOMPARE(colourAt(document, link + 7), QColor(u"#356889"_s));
	QVERIFY(!colourAt(document, 0).isValid());
	QVERIFY(!colourAt(document, link + 9).isValid());
}

void TestLinkColorizer::followsTheColour()
{
	QTextDocument document;
	document.setMarkdown(u"[link](https://example.com)"_s);
	LinkHighlighter highlighter(&document, QColor(u"#356889"_s));
	highlighter.rehighlight();
	highlighter.setColor(QColor(u"#7aa2f7"_s));
	QCOMPARE(colourAt(document, 0), QColor(u"#7aa2f7"_s));
}

void TestLinkColorizer::coloursTextAddedLater()
{
	QTextDocument document;
	LinkHighlighter highlighter(&document, QColor(u"#356889"_s));
	QCoreApplication::processEvents();
	document.setMarkdown(u"streamed"_s);
	QCoreApplication::processEvents();
	document.setMarkdown(u"streamed [link](https://example.com)"_s);
	QCoreApplication::processEvents();
	QCOMPARE(colourAt(document, static_cast<int>(document.toPlainText().indexOf(u"link"_s))), QColor(u"#356889"_s));
}

// Its own formats change the document too; that must not start another pass.
void TestLinkColorizer::settlesAfterItsOwnPass()
{
	QTextDocument document;
	LinkHighlighter highlighter(&document, QColor(u"#356889"_s));
	document.setMarkdown(u"[link](https://example.com)"_s);
	QCoreApplication::processEvents();
	QCoreApplication::processEvents();
	QSignalSpy changes(&document, &QTextDocument::contentsChanged);
	QCoreApplication::processEvents();
	QCoreApplication::processEvents();
	QCOMPARE(changes.count(), 0);
	QCOMPARE(colourAt(document, 0), QColor(u"#356889"_s));
}

// What ProseText does: the colour is a binding to the theme, so a theme switch changes a property.
void TestLinkColorizer::followsTheBoundColourInQml()
{
	qmlRegisterType<LinkColorizer>("LinkTest", 1, 0, "LinkColorizer");
	QQmlEngine engine;
	QQmlComponent component(&engine);
	component.setData(R"qml(
		import QtQuick
		import LinkTest
		Item {
			id: root
			property color tone: "#356889"
			TextEdit {
				id: edit
				objectName: "edit"
				textFormat: TextEdit.MarkdownText
				text: "[link](https://example.com)"
			}
			LinkColorizer { document: edit.textDocument; color: root.tone }
		})qml", QUrl());
	const std::unique_ptr<QObject> root(component.create());
	QVERIFY2(root, qPrintable(component.errorString()));
	auto* edit = root->findChild<QQuickItem*>(u"edit"_s);
	QVERIFY(edit);
	const auto* docWrapper = edit->property("textDocument").value<QQuickTextDocument*>();
	QVERIFY(docWrapper);
	const QTextDocument* document = docWrapper->textDocument();
	QVERIFY(document);

	QCoreApplication::processEvents();
	QCOMPARE(colourAt(*document, 0), QColor(u"#356889"_s));

	root->setProperty("tone", QColor(u"#7aa2f7"_s));
	QCOMPARE(colourAt(*document, 0), QColor(u"#7aa2f7"_s));
}

void TestLinkColorizer::throttlesWhileStreamingAndFlushesOnEnd()
{
	QTextDocument document;
	LinkHighlighter highlighter(&document, QColor(u"#356889"_s), true);
	QCoreApplication::processEvents();
	document.setMarkdown(u"streamed [link](https://example.com)"_s);
	QCoreApplication::processEvents();
	// Throttled while streaming: immediate event loop turn has not rehighlighted yet.
	const int pos = static_cast<int>(document.toPlainText().indexOf(u"link"_s));
	QVERIFY(!colourAt(document, pos).isValid());
	// Ending streaming flushes the pending rehighlight immediately.
	highlighter.setStreaming(false);
	QCOMPARE(colourAt(document, pos), QColor(u"#356889"_s));
}

QTEST_MAIN(TestLinkColorizer)
#include "tst_link_colorizer.moc"
