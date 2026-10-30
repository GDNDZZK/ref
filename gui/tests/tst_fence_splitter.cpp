// Adapted from an early version of qutie by LightBlueCube
#include "session/fence_splitter.h"

#include <QTest>

using namespace Qt::StringLiterals;
using refgui::session::FenceSplitter;

namespace
{

// Flattens events into one string: fences become markers, text is concatenated.
// Chunking only changes how text is split across events, so equal renderings mean
// equal block contents.
QString render(const QList<FenceSplitter::Event>& events)
{
	QString rendered;
	for(const FenceSplitter::Event& event : events)
	{
		if(const auto* opened = std::get_if<FenceSplitter::CodeOpened>(&event))
		{
			rendered += u"["_s + opened->language + u"]"_s;
		}
		else if(std::holds_alternative<FenceSplitter::CodeClosed>(event))
		{
			rendered += u"[/]"_s;
		}
		else
		{
			rendered += std::get<FenceSplitter::TextAppended>(event).text;
		}
	}
	return rendered;
}

QString splitWhole(const QString& input)
{
	FenceSplitter splitter;
	// Two statements: the operands of + are unsequenced, and finish() must run last.
	const QString fed = render(splitter.feed(input));
	return fed + render(splitter.finish());
}

QString splitPerCharacter(const QString& input)
{
	FenceSplitter splitter;
	QString rendered;
	for(const QChar c : input)
	{
		rendered += render(splitter.feed(QStringView(&c, 1)));
	}
	return rendered + render(splitter.finish());
}

} // namespace

class TestFenceSplitter : public QObject
{
	Q_OBJECT

private slots:
	void splits_data();
	void splits();
	void splitsPerCharacter_data();
	void splitsPerCharacter();

	void releasesProseBeforeLineEnds();
	void holdsPotentialFence();
};

void TestFenceSplitter::splits_data()
{
	QTest::addColumn<QString>("input");
	QTest::addColumn<QString>("expected");

	QTest::newRow("plain") << u"Hello\nworld"_s << u"Hello\nworld"_s;
	QTest::newRow("fence") << u"a\n```cpp\nint x;\n```\nb"_s << u"a[cpp]int x;[/]b"_s;
	QTest::newRow("tilde fence") << u"~~~\nx\n~~~"_s << u"[]x[/]"_s;
	QTest::newRow("unclosed fence") << u"```\nx"_s << u"[]x"_s;
	QTest::newRow("info string first word") << u"``` rust ignore\nx\n```"_s << u"[rust]x[/]"_s;

	QTest::newRow("blank lines around fence dropped") << u"a\n\n```\nx\n```\n\nb"_s << u"a[]x[/]b"_s;
	QTest::newRow("blank lines inside code kept") << u"```\n\nx\n\ny\n```"_s << u"[]\nx\n\ny[/]"_s;

	QTest::newRow("other marker is content") << u"~~~\n```\n~~~"_s << u"[]```[/]"_s;
	QTest::newRow("shorter closing is content") << u"````\n```\n`````"_s << u"[]```[/]"_s;
	QTest::newRow("closing with text is content") << u"```\n``` x\n```"_s << u"[]``` x[/]"_s;
	QTest::newRow("crlf") << u"```cpp\r\nx\r\n```\r\n"_s << u"[cpp]x\r[/]"_s;

	QTest::newRow("indented fence strips indent") << u"  ```py\n  x\n    y\n  ```"_s << u"[py]x\n  y[/]"_s;
	QTest::newRow("four spaces is not a fence") << u"    ```\nx"_s << u"    ```\nx"_s;
	QTest::newRow("inline code is not a fence") << u"```a``` b"_s << u"```a``` b"_s;
	QTest::newRow("short run is not a fence") << u"``x`` y"_s << u"``x`` y"_s;
}

void TestFenceSplitter::splits()
{
	QFETCH(QString, input);
	QFETCH(QString, expected);

	QCOMPARE(splitWhole(input), expected);
}

void TestFenceSplitter::splitsPerCharacter_data()
{
	splits_data();
}

void TestFenceSplitter::splitsPerCharacter()
{
	QFETCH(QString, input);
	QFETCH(QString, expected);

	QCOMPARE(splitPerCharacter(input), expected);
}

void TestFenceSplitter::releasesProseBeforeLineEnds()
{
	FenceSplitter splitter;

	QCOMPARE(render(splitter.feed(u"Hello wor")), u"Hello wor"_s);
	QCOMPARE(render(splitter.feed(u"ld\nnext")), u"ld\nnext"_s);
}

void TestFenceSplitter::holdsPotentialFence()
{
	FenceSplitter splitter;

	QCOMPARE(render(splitter.feed(u"``")), QString());
	QCOMPARE(render(splitter.feed(u"`cp")), QString());
	QCOMPARE(render(splitter.feed(u"p\nint")), u"[cpp]int"_s);
	QCOMPARE(render(splitter.feed(u"\n`")), QString());
	QCOMPARE(render(splitter.feed(u"`x")), u"\n``x"_s);
}

QTEST_GUILESS_MAIN(TestFenceSplitter)
#include "tst_fence_splitter.moc"
