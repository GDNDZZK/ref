#include "workspace/toast_text.h"

#include <QtTest>

using namespace Qt::StringLiterals;
using namespace refgui::workspace;

class TestToastText : public QObject
{
	Q_OBJECT

private slots:
	void collapsesWhitespace()
	{
		QCOMPARE(oneLine(u"  first\n\tsecond   third \n"_s), u"first second third"_s);
	}

	void dropsControlCharacters()
	{
		QCOMPARE(oneLine(u"a\u001b[0mb"_s), u"a[0mb"_s);
	}

	void shortTextStaysAsItIs()
	{
		QCOMPARE(truncated(u"hello"_s, 10), u"hello"_s);
		QCOMPARE(truncated(u"hello"_s, 5), u"hello"_s);
	}

	void longTextEndsWithAnEllipsis()
	{
		QCOMPARE(truncated(u"hello world"_s, 6), u"hello\u2026"_s);
	}

	void countsCharactersNotUtf16Units()
	{
		// Each emoji is two UTF-16 units but one character.
		const QString emoji = QString::fromStdU32String(U"\U0001F600\U0001F601\U0001F602\U0001F603");
		QCOMPARE(truncated(emoji, 4), emoji);
		QCOMPARE(truncated(emoji, 3), QString::fromStdU32String(U"\U0001F600\U0001F601") + QChar(0x2026));
	}

	void doesNotCutInsideAGrapheme()
	{
		// A family emoji is five characters joined by zero-width joiners and shows as one.
		const QString family = QString::fromStdU32String(U"\U0001F468\u200D\U0001F469\u200D\U0001F467");
		const QString text = u"ab"_s + family + u"cd"_s;

		QCOMPARE(truncated(text, 5), u"ab\u2026"_s); // the family would need characters 3 to 7
		QCOMPARE(truncated(text, 8), u"ab"_s + family + u"\u2026"_s);

		// A letter with a combining accent is two characters and one grapheme.
		const QString accented = u"abe\u0301cd"_s;
		QCOMPARE(truncated(accented, 4), u"ab\u2026"_s);
	}

	void toastTextIsAtMostTwoHundredCharacters()
	{
		const QString text = toastText(QString(u'x').repeated(500));
		QCOMPARE(text.size(), MaxToastChars);
		QVERIFY(text.endsWith(QChar(0x2026)));
	}
};

QTEST_APPLESS_MAIN(TestToastText)
#include "tst_toast_text.moc"
