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
	void highlightsImageTokensForegroundOnly();
	void togglesImageTokensWhileKeepingLinkColor();
	void imageTokensWorkInQml();
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

bool hasBackgroundAt(const QTextDocument& document, int position)
{
	const QTextBlock block = document.firstBlock();
	for(const QTextLayout::FormatRange& range : block.layout()->formats())
	{
		if(position >= range.start && position < range.start + range.length)
		{
			if(range.format.hasProperty(QTextFormat::BackgroundBrush) || range.format.background().style() != Qt::NoBrush)
			{
				return true;
			}
		}
	}
	return false;
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

void TestLinkColorizer::highlightsImageTokensForegroundOnly()
{
	QTextDocument document;
	document.setPlainText(u"look at [Image 1] and [Image 12] plus [Image x]"_s);
	LinkHighlighter highlighter(&document, QColor(u"#356889"_s));
	highlighter.setHighlightImageTokens(true);
	highlighter.rehighlight();
	const QString plain = document.toPlainText();
	const int img1 = static_cast<int>(plain.indexOf(u"[Image 1]"_s));
	const int img12 = static_cast<int>(plain.indexOf(u"[Image 12]"_s));
	const int nonToken = static_cast<int>(plain.indexOf(u"[Image x]"_s));

	QVERIFY(!colourAt(document, 0).isValid());
	QCOMPARE(colourAt(document, img1), QColor(u"#356889"_s));
	QCOMPARE(colourAt(document, img1 + 8), QColor(u"#356889"_s));
	QVERIFY(!hasBackgroundAt(document, img1));
	QVERIFY(!colourAt(document, img1 + 9).isValid());
	QCOMPARE(colourAt(document, img12), QColor(u"#356889"_s));
	QCOMPARE(colourAt(document, img12 + 9), QColor(u"#356889"_s));
	QVERIFY(!hasBackgroundAt(document, img12));
	QVERIFY(!colourAt(document, nonToken).isValid());
}

void TestLinkColorizer::togglesImageTokensWhileKeepingLinkColor()
{
	QTextDocument document;
	document.setMarkdown(u"see [docs](https://example.com) and [Image 2]"_s);
	LinkHighlighter highlighter(&document, QColor(u"#356889"_s));
	highlighter.setHighlightImageTokens(true);
	highlighter.rehighlight();

	const QString plain = document.toPlainText();
	const int linkPos = static_cast<int>(plain.indexOf(u"docs"_s));
	const int imgPos = static_cast<int>(plain.indexOf(u"[Image 2]"_s));
	QCOMPARE(colourAt(document, linkPos), QColor(u"#356889"_s));
	QCOMPARE(colourAt(document, imgPos), QColor(u"#356889"_s));
	QVERIFY(!hasBackgroundAt(document, imgPos));

	highlighter.setHighlightImageTokens(false);
	QVERIFY(!colourAt(document, imgPos).isValid());
	QCOMPARE(colourAt(document, linkPos), QColor(u"#356889"_s));
}

void TestLinkColorizer::imageTokensWorkInQml()
{
	qmlRegisterType<LinkColorizer>("LinkTest", 1, 0, "LinkColorizer");
	QQmlEngine engine;
	QQmlComponent component(&engine);
	component.setData(R"qml(
		import QtQuick
		import LinkTest
		Item {
			id: root
			property bool highlightImages: true
			property color mainTone: "#356889"
			TextEdit {
				id: edit
				objectName: "edit"
				textFormat: TextEdit.PlainText
				text: "hello [Image 3]"
			}
			LinkColorizer {
				id: colorizer
				objectName: "colorizer"
				document: edit.textDocument
				color: root.mainTone
				highlightImageTokens: root.highlightImages
			}
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
	const int pos = static_cast<int>(document->toPlainText().indexOf(u"[Image 3]"_s));
	QCOMPARE(colourAt(*document, pos), QColor(u"#356889"_s));
	QVERIFY(!hasBackgroundAt(*document, pos));
	QVERIFY(!colourAt(*document, 0).isValid());

	root->setProperty("highlightImages", false);
	QVERIFY(!colourAt(*document, pos).isValid());
}

QTEST_MAIN(TestLinkColorizer)
#include "tst_link_colorizer.moc"
