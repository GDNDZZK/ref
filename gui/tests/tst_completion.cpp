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
	void shellSegmentsFlagsMidTokenAndWindowsPathsWork();
	void shellWrappersTargetWrappedCommand();
};

void TestCompletion::commandTargetIsTheWholeSingleWord()
{
	const auto target = completionTarget(u"/mod"_s, 4, false);
	QCOMPARE(target.kind, Kind::Command);
	QCOMPARE(target.query, u"mod"_s);
	QCOMPARE(target.start, 0);

	QCOMPARE(completionTarget(u"/"_s, 1, false).kind, Kind::Command);
	QCOMPARE(completionTarget(u"/rename now"_s, 11, false).kind, Kind::None); // /rename has free-form text, no arg list
	auto midWord = completionTarget(u"/mod"_s, 2, false);
	QCOMPARE(midWord.kind, Kind::Command);
	QCOMPARE(midWord.query, u"m"_s);
	QCOMPARE(midWord.start, 0);
	QCOMPARE(midWord.end, 4);
	auto midText = completionTarget(u"text /mod more"_s, 9, false);
	QCOMPARE(midText.kind, Kind::Command);
	QCOMPARE(midText.query, u"mod"_s);
	QCOMPARE(midText.start, 5);
	QCOMPARE(midText.end, 9);
	QCOMPARE(completionTarget(u"src/main.rs"_s, 11, false).kind, Kind::None);
	QCOMPARE(completionTarget(u"https://a.b"_s, 11, false).kind, Kind::None);
	QCOMPARE(completionTarget(u"/etc/hosts"_s, 10, false).kind, Kind::None);
	QCOMPARE(completionTarget(u"hello /mod"_s, 6, false).kind, Kind::None); // cursor before `/`
	QCOMPARE(completionTarget(u"/"_s, 0, false).kind, Kind::None);          // cursor before `/`
	auto afterSlashOnly = completionTarget(u"hello /mod"_s, 7, false);
	QCOMPARE(afterSlashOnly.kind, Kind::Command);
	QCOMPARE(afterSlashOnly.query, QString());
	QCOMPARE(afterSlashOnly.start, 6);
	QCOMPARE(afterSlashOnly.end, 10);
	QCOMPARE(completionTarget(u"check /foo\\bar"_s, 14, false).kind, Kind::None);
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

void TestCompletion::shellSegmentsFlagsMidTokenAndWindowsPathsWork()
{
	// First unpiped command token has atLineStart == true; empty forced also has atLineStart == true.
	QVERIFY(completionTarget(QString(), 0, true, true).atLineStart);
	QVERIFY(completionTarget(u"car"_s, 3, true).atLineStart);

	// Pipelines, logical operators, and semicolons start a fresh command segment with atLineStart == false.
	auto target = completionTarget(u"echo hi | rg"_s, 12, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"rg"_s);
	QCOMPARE(target.start, 10);
	QCOMPARE(target.end, 12);
	QVERIFY(!target.atLineStart);

	QCOMPARE(completionTarget(u"echo hi | "_s, 10, true, false).kind, Kind::None);
	target = completionTarget(u"echo hi | "_s, 10, true, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, QString());
	QCOMPARE(target.start, 10);
	QCOMPARE(target.end, 10);
	QVERIFY(!target.atLineStart);

	target = completionTarget(u"cargo build && ./target/debug/ap"_s, 32, true);
	QCOMPARE(target.kind, Kind::ShellPath);
	QCOMPARE(target.query, u"./target/debug/ap"_s);
	QCOMPARE(target.start, 15);
	QCOMPARE(target.end, 32);
	QVERIFY(!target.atLineStart);

	target = completionTarget(u"false || py"_s, 11, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"py"_s);
	QCOMPARE(target.start, 9);
	QVERIFY(!target.atLineStart);

	target = completionTarget(u"cd src; ls"_s, 10, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"ls"_s);
	QCOMPARE(target.start, 8);
	QVERIFY(!target.atLineStart);

	// Leading NAME=VALUE assignments (including quoted values) precede the command word.
	target = completionTarget(u"FOO=1 BAR=\"a b\" py"_s, 18, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"py"_s);
	QCOMPARE(target.start, 16);
	QCOMPARE(target.end, 18);
	QVERIFY(!target.atLineStart);

	// Option flags starting with `-` suppress path completion noise.
	QCOMPARE(completionTarget(u"ls -l"_s, 5, true).kind, Kind::None);
	QCOMPARE(completionTarget(u"cargo --release"_s, 15, true).kind, Kind::None);

	// Mid-token cursor sets `end` to the full active token end instead of leaving a broken tail.
	target = completionTarget(u"cargo src/main.rs"_s, 3, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"car"_s);
	QCOMPARE(target.start, 0);
	QCOMPARE(target.end, 5);
	QVERIFY(!target.atLineStart);

	target = completionTarget(u"/close-session mysess other"_s, 17, false);
	QCOMPARE(target.kind, Kind::CommandArg);
	QCOMPARE(target.command, u"close-session"_s);
	QCOMPARE(target.query, u"my"_s);
	QCOMPARE(target.start, 15);
	QCOMPARE(target.end, 21);

	// Windows drive letters and backslash paths target ShellPath.
	target = completionTarget(u"C:\\Users\\alice\\do"_s, 17, true);
	QCOMPARE(target.kind, Kind::ShellPath);
	QCOMPARE(target.query, u"C:\\Users\\alice\\do"_s);
	QCOMPARE(completionTarget(u".\\src\\ma"_s, 8, true).kind, Kind::ShellPath);
	target = completionTarget(u"type src\\ma"_s, 11, true);
	QCOMPARE(target.kind, Kind::ShellPath);
	QCOMPARE(target.query, u"src\\ma"_s);
	QCOMPARE(target.start, 5);
	QCOMPARE(target.end, 11);
}
void TestCompletion::shellWrappersTargetWrappedCommand()
{
	auto target = completionTarget(u"sudo doc"_s, 8, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"doc"_s);
	QCOMPARE(target.start, 5);
	QVERIFY(!target.atLineStart);
	target = completionTarget(u"sudo -u root doc"_s, 16, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"doc"_s);
	target = completionTarget(u"sudo -u "_s, 8, true, true);
	QCOMPARE(target.kind, Kind::None); // wrapper flag arg suppresses completion
	QCOMPARE(completionTarget(u"sudo -u ro"_s, 10, true).kind, Kind::None);
	QCOMPARE(completionTarget(u"timeout "_s, 8, true, true).kind, Kind::None);
	QCOMPARE(completionTarget(u"timeout 5"_s, 9, true).kind, Kind::None);
	QCOMPARE(completionTarget(u"timeout 5s"_s, 10, true).kind, Kind::None);
	target = completionTarget(u"time sudo env FOO=1 doc"_s, 23, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"doc"_s);
	target = completionTarget(u"timeout 5s car"_s, 14, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"car"_s);
	target = completionTarget(u"taskset -c 0,1 car"_s, 18, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"car"_s);
	target = completionTarget(u"taskset 0x1 car"_s, 15, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"car"_s);
	target = completionTarget(u"sudo doc src/ma"_s, 15, true);
	QCOMPARE(target.kind, Kind::ShellPath);
	QCOMPARE(target.query, u"src/ma"_s);
	QCOMPARE(completionTarget(u"sudo ./run"_s, 10, true).kind, Kind::ShellPath);
	target = completionTarget(u"sudo "_s, 5, true, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, QString());
	// Path-prefixed wrappers.
	target = completionTarget(u"/usr/bin/sudo car"_s, 17, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"car"_s);
	target = completionTarget(u"/bin/env FOO=1 car"_s, 18, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"car"_s);
	target = completionTarget(u"C:\\tools\\sudo -u root car"_s, 25, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"car"_s);

	// Clustered and attached short flags, plus wrapper flag parity.
	QCOMPARE(completionTarget(u"sudo -Eu "_s, 9, true, true).kind, Kind::None);
	target = completionTarget(u"sudo -Eu root car"_s, 17, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"car"_s);
	target = completionTarget(u"sudo -Euroot car"_s, 16, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"car"_s);
	QCOMPARE(completionTarget(u"timeout -vk "_s, 12, true, true).kind, Kind::None);
	QCOMPARE(completionTarget(u"timeout -vk 5s "_s, 15, true, true).kind, Kind::None);
	target = completionTarget(u"timeout -vk 5s 10s car"_s, 22, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"car"_s);
	target = completionTarget(u"timeout -vk5s 5s car"_s, 20, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"car"_s);
	QCOMPARE(completionTarget(u"taskset -pc "_s, 12, true, true).kind, Kind::None);
	target = completionTarget(u"taskset -p car"_s, 14, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"car"_s);
	target = completionTarget(u"taskset -cp car"_s, 15, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"car"_s);
	target = completionTarget(u"taskset -pc 0,1 car"_s, 19, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"car"_s);
	target = completionTarget(u"watch -d car"_s, 12, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"car"_s);
	target = completionTarget(u"watch -n 2 car"_s, 14, true);
	QCOMPARE(target.kind, Kind::ShellCommand);
	QCOMPARE(target.query, u"car"_s);
}
QTEST_GUILESS_MAIN(TestCompletion)
#include "tst_completion.moc"
