#include "session/markdown_text.h"

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

QTEST_APPLESS_MAIN(TestMarkdownText)
#include "tst_markdown_text.moc"
