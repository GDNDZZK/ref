#include "compose/completion_target.h"

#include <QTest>

using namespace Qt::StringLiterals;
using namespace refgui::compose;
using Kind = CompletionTarget::Kind;

class TestCompletion : public QObject
{
	Q_OBJECT

private slots:
	void commandTargetIsTheWholeSingleWord();
	void fileTargetIsTheWordAfterAt();
	void shellModeHasNoTarget();
};

void TestCompletion::commandTargetIsTheWholeSingleWord()
{
	const auto target = completionTarget(u"/mod"_s, 4, false);
	QCOMPARE(target.kind, Kind::Command);
	QCOMPARE(target.query, u"mod"_s);
	QCOMPARE(target.start, 0);

	QCOMPARE(completionTarget(u"/"_s, 1, false).kind, Kind::Command);
	QCOMPARE(completionTarget(u"/rename now"_s, 11, false).kind, Kind::None); // arguments are typed
	QCOMPARE(completionTarget(u"/mod"_s, 2, false).kind, Kind::None);          // cursor not at the end
	QCOMPARE(completionTarget(u"text /mod"_s, 9, false).kind, Kind::None);
}

void TestCompletion::fileTargetIsTheWordAfterAt()
{
	auto target = completionTarget(u"look at @src/ma"_s, 15, false);
	QCOMPARE(target.kind, Kind::File);
	QCOMPARE(target.query, u"src/ma"_s);
	QCOMPARE(target.start, 8);

	QCOMPARE(completionTarget(u"@"_s, 1, false).kind, Kind::File);
	QCOMPARE(completionTarget(u"@a b"_s, 4, false).kind, Kind::None);        // whitespace after the query
	QCOMPARE(completionTarget(u"mail me@host"_s, 12, false).kind, Kind::None); // not at a word start
	QCOMPARE(completionTarget(u"/cmd @fi"_s, 8, false).kind, Kind::File);       // a mention inside a command line

	// Only the text before the cursor counts.
	target = completionTarget(u"@abc def"_s, 2, false);
	QCOMPARE(target.query, u"a"_s);
}

void TestCompletion::shellModeHasNoTarget()
{
	QCOMPARE(completionTarget(u"ls @fi"_s, 6, true).kind, Kind::None);
	QCOMPARE(completionTarget(u"/x"_s, 2, true).kind, Kind::None);
}

QTEST_GUILESS_MAIN(TestCompletion)
#include "tst_completion.moc"
