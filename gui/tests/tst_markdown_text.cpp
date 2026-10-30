#include "session/markdown_text.h"

#include <QGuiApplication>
#include <QRandomGenerator>
#include <QTextBlock>
#include <QTextDocument>
#include <QTextFragment>
#include <QtTest>

using namespace Qt::StringLiterals;
using refgui::session::markdownForDisplay;

class TestMarkdownText : public QObject
{
	Q_OBJECT

private slots:
	void turnsImagesIntoLinks();
	void escapesTagsAndComments();
	void leavesCodeSpansAlone();
	void leavesPlainTextAlone();
	void keepsEscapedCharacters();
	void leavesFencedBlocksAlone();
	void leavesUnclosedFencesAlone();
	void leavesIndentedBlocksAlone();
	void leavesFencesInListsAlone();
	void goesBackToTextAfterCode();
	void codeSpansDoNotCrossParagraphs();
	void leavesNoImageForTheRendererWhateverTheText();
};

void TestMarkdownText::turnsImagesIntoLinks()
{
	QCOMPARE(markdownForDisplay(u"![logo](http://example.com/a.png)"_s), u"[logo](http://example.com/a.png)"_s);
	QCOMPARE(markdownForDisplay(u"a ![x][ref] b"_s), u"a [x][ref] b"_s);
	QCOMPARE(markdownForDisplay(u"![](http://example.com/a.png \"t\")"_s), u"[](http://example.com/a.png \"t\")"_s);
}

void TestMarkdownText::escapesTagsAndComments()
{
	QCOMPARE(markdownForDisplay(u"<img src=\"http://example.com/x\">"_s), u"\\<img src=\"http://example.com/x\">"_s);
	QCOMPARE(markdownForDisplay(u"</div><!-- c --><?x?>"_s), u"\\</div>\\<!-- c -->\\<?x?>"_s);
}

void TestMarkdownText::leavesCodeSpansAlone()
{
	QCOMPARE(markdownForDisplay(u"use `<img>` and ``![x](y)`` here"_s), u"use `<img>` and ``![x](y)`` here"_s);
	// An unclosed span is only a run of backticks.
	QCOMPARE(markdownForDisplay(u"`<b>"_s), u"`\\<b>"_s);
	// Backticks the renderer could pair with a later run, in another paragraph, are escaped.
	QCOMPARE(markdownForDisplay(u"`<b>\n\n`"_s), u"\\`\\<b>\n\n`"_s);
}

void TestMarkdownText::leavesPlainTextAlone()
{
	const QString text = u"Wow! 1 < 2 and a<b is not a tag, 3 <= 4.\n\n- item\n- [link](http://example.com)"_s;
	QCOMPARE(markdownForDisplay(text), u"Wow! 1 < 2 and a\\<b is not a tag, 3 <= 4.\n\n- item\n- [link](http://example.com)"_s);
}

void TestMarkdownText::keepsEscapedCharacters()
{
	QCOMPARE(markdownForDisplay(u"\\<b> \\![x](y)"_s), u"\\<b> \\![x](y)"_s);
	QCOMPARE(markdownForDisplay(u"trailing \\"_s), u"trailing \\"_s);
}

void TestMarkdownText::leavesFencedBlocksAlone()
{
	const QString fenced = u"```cpp\nstd::vector<int> x; ![a](b)\n```\n"_s;
	QCOMPARE(markdownForDisplay(fenced), fenced);
	const QString tilde = u"~~~\n<b>x</b>\n~~~\n"_s;
	QCOMPARE(markdownForDisplay(tilde), tilde);
	// A longer fence is closed by a fence at least as long, not by a shorter one.
	const QString longer = u"````\n```\n<b>\n````\n"_s;
	QCOMPARE(markdownForDisplay(longer), longer);
	// The other kind of fence inside does not close it.
	const QString mixed = u"```\n~~~\n<b>\n```\n"_s;
	QCOMPARE(markdownForDisplay(mixed), mixed);
}

void TestMarkdownText::leavesUnclosedFencesAlone()
{
	QCOMPARE(markdownForDisplay(u"```cpp\nstd::vector<int> x"_s), u"```cpp\nstd::vector<int> x"_s);
	QCOMPARE(markdownForDisplay(u"text <b>\n\n~~~\n<i>"_s), u"text \\<b>\n\n~~~\n<i>"_s);
	// Backticks in the info string make it a code span, not a fence.
	QCOMPARE(markdownForDisplay(u"```a``` <b>"_s), u"```a``` \\<b>"_s);
}

void TestMarkdownText::leavesIndentedBlocksAlone()
{
	QCOMPARE(markdownForDisplay(u"Example:\n\n    indented <int>\n    more <b>\n"_s), u"Example:\n\n    indented <int>\n    more <b>\n"_s);
	QCOMPARE(markdownForDisplay(u"\tx <int>"_s), u"\tx <int>"_s);
	// Four spaces under a paragraph continue it.
	QCOMPARE(markdownForDisplay(u"para\n    <b>"_s), u"para\n    \\<b>"_s);
	// A blank line inside the block does not end it.
	QCOMPARE(markdownForDisplay(u"\n    <a>\n\n    <b>\n"_s), u"\n    <a>\n\n    <b>\n"_s);
}

void TestMarkdownText::leavesFencesInListsAlone()
{
	const QString fenced = u"- item\n\n  ```\n  <b>\n  ```\n- next <b>"_s;
	QCOMPARE(markdownForDisplay(fenced), u"- item\n\n  ```\n  <b>\n  ```\n- next \\<b>"_s);
	const QString deeper = u"1. item\n\n       ```\n       <b>"_s;
	QCOMPARE(markdownForDisplay(deeper), deeper);
	// Four spaces inside a list item that are only its content are not code.
	QCOMPARE(markdownForDisplay(u"- item\n\n    <b>"_s), u"- item\n\n    \\<b>"_s);
}

void TestMarkdownText::goesBackToTextAfterCode()
{
	QCOMPARE(markdownForDisplay(u"```\n<a>\n```\n<b> and `<c>`\n\n    <d>\n\n<e>"_s),
	         u"```\n<a>\n```\n\\<b> and `<c>`\n\n    <d>\n\n\\<e>"_s);
}

namespace
{

// The images the Markdown renderer makes of `markdown`.
int imagesRendered(const QString& markdown)
{
	QTextDocument document;
	document.setMarkdown(markdown);
	int images = 0;
	for(QTextBlock block = document.begin(); block.isValid(); block = block.next())
	{
		for(QTextBlock::iterator it = block.begin(); !it.atEnd(); ++it)
		{
			images += it.fragment().charFormat().isImageFormat() ? 1 : 0;
		}
	}
	return images;
}

} // namespace

// A code span ends in its own paragraph. Backticks that look like its end further down are not its end,
// so the image between them is no code.
void TestMarkdownText::codeSpansDoNotCrossParagraphs()
{
	const QStringList texts{
		u"``![x](http://example.com/a.png)\n\n``"_s,
		u"`![x](http://example.com/a.png)\n\n`"_s,
		u"- `![x](http://example.com/a.png)\n- `"_s,
		u"> `![x](http://example.com/a.png)\n\n> `"_s,
		u"`![x](http://example.com/a.png)\n# heading `"_s,
	};
	for(const QString& text : texts)
	{
		QVERIFY2(imagesRendered(markdownForDisplay(text)) == 0, qPrintable(text));
	}
	// The same line is still a span.
	QCOMPARE(markdownForDisplay(u"`![x](y)` and ``<b>``"_s), u"`![x](y)` and ``<b>``"_s);
}

// Compares what the renderer makes of random mixes of the Markdown pieces that matter: whatever the
// order, no picture comes out of text that went through markdownForDisplay.
void TestMarkdownText::leavesNoImageForTheRendererWhateverTheText()
{
	const QStringList pieces{u"`"_s, u"``"_s, u"```"_s, u"~~~"_s, u"![x](http://example.com/a.png)"_s, u"![x][r]"_s, u"\n"_s, u"\n\n"_s, u"- "_s,
	                         u"1. "_s, u"> "_s, u"    "_s, u"\t"_s, u"# "_s, u"<b>"_s, u"<img src=u>"_s, u"x"_s, u" "_s, u"\\"_s, u"["_s, u"]"_s, u"(u)"_s, u"*"_s, u"!"_s};
	constexpr quint32 Seed = 20240131;
	constexpr int Iterations = 4000;
	constexpr int MaxPieces = 14;
	QRandomGenerator random(Seed);
	for(int i = 0; i < Iterations; ++i)
	{
		QString text;
		const int count = 1 + random.bounded(MaxPieces);
		for(int piece = 0; piece < count; ++piece)
		{
			text += pieces.at(random.bounded(static_cast<int>(pieces.size())));
		}
		QVERIFY2(imagesRendered(markdownForDisplay(text)) == 0, qPrintable(u"%1: %2"_s.arg(i).arg(QString(text).replace(u'\n', u"\\n"_s))));
	}
}

QTEST_MAIN(TestMarkdownText)
#include "tst_markdown_text.moc"
