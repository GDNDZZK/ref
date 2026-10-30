#include "compose/command_ranking.h"
#include "compose/completion_target.h"
#include "compose/fuzzy.h"

#include <QTest>

using namespace Qt::StringLiterals;
using namespace refgui::compose;
using Kind = CompletionTarget::Kind;

class TestCompletion : public QObject
{
	Q_OBJECT

private slots:
	void fuzzyNeedsASubsequence();
	void fuzzyPrefersPrefixesAndRuns();
	void fuzzyIgnoresCaseAndSpaces();
	void commandTargetIsTheWholeSingleWord();
	void fileTargetIsTheWordAfterAt();
	void shellModeHasNoTarget();
	void rankingPutsBestFirstAndShowsAliases();
	void rankingKeepsBuiltinsAboveCustomOnATie();
	void rankingStopsAtTheLimit();
};

void TestCompletion::fuzzyNeedsASubsequence()
{
	QVERIFY(fuzzyScore(u"models"_s, u"mdl"_s).has_value());
	QVERIFY(!fuzzyScore(u"models"_s, u"mx"_s).has_value());
	QVERIFY(!fuzzyScore(u"models"_s, u"sm"_s).has_value()); // order matters
	QCOMPARE(fuzzyScore(u"anything"_s, QString()), 0);
}

void TestCompletion::fuzzyPrefersPrefixesAndRuns()
{
	QVERIFY(*fuzzyScore(u"compact"_s, u"com"_s) > *fuzzyScore(u"recompact"_s, u"com"_s));
	QVERIFY(*fuzzyScore(u"src/main.rs"_s, u"main"_s) > *fuzzyScore(u"src/mxaxixn.rs"_s, u"main"_s));
	QVERIFY(*fuzzyScore(u"a-b"_s, u"b"_s) > *fuzzyScore(u"ab"_s, u"b"_s)); // after a separator
}

void TestCompletion::fuzzyIgnoresCaseAndSpaces()
{
	QVERIFY(fuzzyScore(u"README.md"_s, u"readme"_s).has_value());
	QVERIFY(fuzzyScore(u"new session"_s, u"n s"_s).has_value()); // spaces in the query are skipped
}

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

void TestCompletion::rankingPutsBestFirstAndShowsAliases()
{
	const QList<CommandEntry> commands = {
		{.name = u"new"_s, .aliases = {u"clear"_s}},
		{.name = u"models"_s},
		{.name = u"compact"_s, .aliases = {u"summarize"_s}},
	};

	auto matches = rankCommands(commands, u"mod"_s, 10);
	QCOMPARE(matches.size(), 1);
	QCOMPARE(matches[0].label, u"/models"_s);

	matches = rankCommands(commands, u"summ"_s, 10);
	QCOMPARE(matches.size(), 1);
	QCOMPARE(matches[0].entry, 2);
	QCOMPARE(matches[0].label, u"/summarize \u2192 /compact"_s);

	matches = rankCommands(commands, u""_s, 10);
	QCOMPARE(matches.size(), 3); // an empty query lists everything in order
	QCOMPARE(matches[0].entry, 0);
}

void TestCompletion::rankingKeepsBuiltinsAboveCustomOnATie()
{
	const QList<CommandEntry> commands = {
		{.name = u"review"_s, .builtin = false},
		{.name = u"review"_s},
	};
	const auto matches = rankCommands(commands, u"rev"_s, 10);
	QCOMPARE(matches[0].entry, 1);
}

void TestCompletion::rankingStopsAtTheLimit()
{
	QList<CommandEntry> commands;
	for(int i = 0; i < 10; ++i)
	{
		commands.append({.name = u"cmd%1"_s.arg(i)});
	}
	QCOMPARE(rankCommands(commands, u"cmd"_s, 4).size(), 4);
}

QTEST_GUILESS_MAIN(TestCompletion)
#include "tst_completion.moc"
