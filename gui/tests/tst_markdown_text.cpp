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
	void leavesFencedBlocksAlone();
	void leavesUnclosedFencesAlone();
	void leavesIndentedBlocksAlone();
	void leavesFencesInListsAlone();
	void goesBackToTextAfterCode();
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

QTEST_APPLESS_MAIN(TestMarkdownText)
#include "tst_markdown_text.moc"
