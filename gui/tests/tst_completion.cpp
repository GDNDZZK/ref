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
	void commandArgTargetCoversBuiltinsAndAliases();
	void fileTargetIsTheWordAfterAtIncludingCustomCommands();
	void shellModeTargetsCommandsAndPaths();
};

void TestCompletion::commandTargetIsTheWholeSingleWord()
{
	const auto target = completionTarget(u"/mod"_s, 4, false);
	QCOMPARE(target.kind, Kind::Command);
	QCOMPARE(target.query, u"mod"_s);
	QCOMPARE(target.start, 0);

	QCOMPARE(completionTarget(u"/"_s, 1, false).kind, Kind::Command);
	QCOMPARE(completionTarget(u"/rename now"_s, 11, false).kind, Kind::None); // /rename has free-form text, no arg list
	QCOMPARE(completionTarget(u"/mod"_s, 2, false).kind, Kind::None);          // cursor not at the end
	QCOMPARE(completionTarget(u"text /mod"_s, 9, false).kind, Kind::None);
}

void TestCompletion::commandArgTargetCoversBuiltinsAndAliases()
{
	auto target = completionTarget(u"/cd src/ap"_s, 10, false);
	QCOMPARE(target.kind, Kind::CommandArg);
	QCOMPARE(target.command, u"cd"_s);
	QCOMPARE(target.query, u"src/ap"_s);
	QCOMPARE(target.start, 4);
	QCOMPARE(target.end, 10);

	target = completionTarget(u"/cwd "_s, 5, false);
	QCOMPARE(target.kind, Kind::CommandArg);
	QCOMPARE(target.command, u"cd"_s);
	QCOMPARE(target.query, QString());

	target = completionTarget(u"/export docs/rep"_s, 16, false);
	QCOMPARE(target.kind, Kind::CommandArg);
	QCOMPARE(target.command, u"export"_s);
	QCOMPARE(target.query, u"docs/rep"_s);

	target = completionTarget(u"/effort hi"_s, 10, false);
	QCOMPARE(target.kind, Kind::CommandArg);
	QCOMPARE(target.command, u"variant"_s);
	QCOMPARE(target.query, u"hi"_s);

	target = completionTarget(u"/MODEL son"_s, 10, false);
	QCOMPARE(target.kind, Kind::CommandArg);
	QCOMPARE(target.command, u"models"_s);
	QCOMPARE(target.query, u"son"_s);

	target = completionTarget(u"/agent pl"_s, 9, false);
	QCOMPARE(target.kind, Kind::CommandArg);
	QCOMPARE(target.command, u"agents"_s);
	QCOMPARE(target.query, u"pl"_s);

	target = completionTarget(u"/resume my"_s, 10, false);
	QCOMPARE(target.kind, Kind::CommandArg);
	QCOMPARE(target.command, u"sessions"_s);
	QCOMPARE(target.query, u"my"_s);

	// Multi-token completion on /close-session (/kill) replaces only the active whitespace token.
	target = completionTarget(u"/close-session alpha be"_s, 23, false);
	QCOMPARE(target.kind, Kind::CommandArg);
	QCOMPARE(target.command, u"close-session"_s);
	QCOMPARE(target.query, u"be"_s);
	QCOMPARE(target.start, 21);
	QCOMPARE(target.end, 23);
}

void TestCompletion::fileTargetIsTheWordAfterAtIncludingCustomCommands()
{
	auto target = completionTarget(u"look at @src/ma"_s, 15, false);
	QCOMPARE(target.kind, Kind::File);
	QCOMPARE(target.query, u"src/ma"_s);
	QCOMPARE(target.start, 8);

	QCOMPARE(completionTarget(u"@"_s, 1, false).kind, Kind::File);
	QCOMPARE(completionTarget(u"@a b"_s, 4, false).kind, Kind::None);        // whitespace after the query
	QCOMPARE(completionTarget(u"mail me@host"_s, 12, false).kind, Kind::None); // not at a word start
	QCOMPARE(completionTarget(u"/cmd @fi"_s, 8, false).kind, Kind::File);       // a mention inside a custom command

	target = completionTarget(u"/review check @src/lib"_s, 22, false);
	QCOMPARE(target.kind, Kind::File);
	QCOMPARE(target.query, u"src/lib"_s);
	QCOMPARE(target.start, 14);

	// Only the text before the cursor counts.
	target = completionTarget(u"@abc def"_s, 2, false);
	QCOMPARE(target.query, u"a"_s);
}

void TestCompletion::shellModeTargetsCommandsAndPaths()
{
	// Empty shell draft is quiet until explicitly triggered (e.g. Tab).
	QCOMPARE(completionTarget(QString(), 0, true, false).kind, Kind::None);
	const auto forced = completionTarget(QString(), 0, true, true);
	QCOMPARE(forced.kind, Kind::ShellCommand);
	QCOMPARE(forced.query, QString());

	// First token without path characters completes commands (history + binaries).
	auto target = completionTarget(u"car"_s, 3, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"car"_s);
	QCOMPARE(target.start, 0);
	QCOMPARE(target.end, 3);

	// First token with `/`, `.` or `~` completes paths.
	QCOMPARE(completionTarget(u"./run"_s, 5, true).kind, Kind::ShellPath);
	QCOMPARE(completionTarget(u"~/bin/x"_s, 7, true).kind, Kind::ShellPath);
	QCOMPARE(completionTarget(u"/usr/bin/e"_s, 10, true).kind, Kind::ShellPath);
	QCOMPARE(completionTarget(u"~"_s, 1, true).kind, Kind::ShellPath);

	// Subsequent tokens in shell mode always complete paths, never @-mentions or /commands.
	target = completionTarget(u"ls "_s, 3, true);
	QCOMPARE(target.kind, Kind::ShellPath);
	QCOMPARE(target.query, QString());
	QCOMPARE(target.start, 3);

	target = completionTarget(u"cat src/ma"_s, 10, true);
	QCOMPARE(target.kind, Kind::ShellPath);
	QCOMPARE(target.query, u"src/ma"_s);
	QCOMPARE(target.start, 4);
	QCOMPARE(target.end, 10);
}

QTEST_GUILESS_MAIN(TestCompletion)
#include "tst_completion.moc"
