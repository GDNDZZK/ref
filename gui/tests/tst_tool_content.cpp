#include "session/tool_content.h"

#include <QtTest>

using namespace Qt::StringLiterals;
using namespace refgui::session::tool_content;

namespace
{

QVariantMap map(std::initializer_list<std::pair<QString, QVariant>> entries)
{
	QVariantMap result;
	for(const auto& [key, value] : entries)
		result.insert(key, value);
	return result;
}

} // namespace

class TestToolContent : public QObject
{
	Q_OBJECT

private slots:
	void iconsFollowTheTool();
	void parsesObjectsOnly();
	void todosSkipEmptyItemsAndUnknownStatuses();
	void detailsTellCallsApart();
	void detailsFollowTheTerminalClient();
	void toolsWithoutARuleListTheirFirstArguments();
	void listingReadsHeadersRowsAndNotes();
	void listingWithoutHeaderStillGroups();
	void listingIgnoresOtherText();
	void diffRowsMarkKinds();
	void splitsPaths();
	void sizesReadInTheRightUnit();
	void picksTextFromInput();
};

void TestToolContent::iconsFollowTheTool()
{
	QCOMPARE(iconName(u"bash"_s), u"terminal"_s);
	QCOMPARE(iconName(u"mcp_files_list"_s), u"mcp"_s);
	QCOMPARE(iconName(u"something"_s), u"tool"_s);
}

void TestToolContent::parsesObjectsOnly()
{
	QCOMPARE(parseInput(u"{\"path\":\"a.rs\",\"edits\":[1,2]}"_s).value(u"path"_s).toString(), u"a.rs"_s);
	QCOMPARE(parseInput(u"{\"path\":\"a.rs\",\"edits\":[1,2]}"_s).value(u"edits"_s).toList().size(), 2);
	QVERIFY(parseInput({}).isEmpty());
	QVERIFY(parseInput(u"{\"path\":\"a"_s).isEmpty()); // still streaming
	QVERIFY(parseInput(u"[1,2]"_s).isEmpty());
	QVERIFY(parseInput(u"7"_s).isEmpty());
}

void TestToolContent::todosSkipEmptyItemsAndUnknownStatuses()
{
	const auto item = [](const QVariant& content, const QVariant& status) { return map({{u"content"_s, content}, {u"status"_s, status}}); };
	const QVariantMap input = map({{u"todos"_s,
		QVariantList{item(u" write tests "_s, u"in_progress"_s), item(u"  "_s, u"completed"_s), item(42, u"completed"_s),
			item(u"ship"_s, u"bogus"_s), item(u"done"_s, u"completed"_s), u"not an object"_s}}});
	const QVariantList todoItems = todos(input);
	QCOMPARE(todoItems.size(), 3);
	QCOMPARE(todoItems.at(0).toMap().value(u"content"_s).toString(), u"write tests"_s);
	QCOMPARE(todoItems.at(0).toMap().value(u"status"_s).toString(), u"in_progress"_s);
	QCOMPARE(todoItems.at(1).toMap().value(u"status"_s).toString(), u"pending"_s);
	QCOMPARE(todoItems.at(2).toMap().value(u"status"_s).toString(), u"completed"_s);
	QVERIFY(todos({}).isEmpty());
	QVERIFY(todos(map({{u"todos"_s, u"no"_s}})).isEmpty());
}

void TestToolContent::detailsTellCallsApart()
{
	QCOMPARE(detail(u"bash"_s, map({{u"command"_s, u"ls -l\nwc"_s}})), u"$ ls -l"_s);
	QCOMPARE(detail(u"bash"_s, {}), QString());
	QCOMPARE(detail(u"read"_s, map({{u"path"_s, u"src/a.rs"_s}})), u"src/a.rs"_s);
	QCOMPARE(detail(u"edit"_s, map({{u"path"_s, u"a.rs"_s}, {u"edits"_s, QVariantList{1}}})), u"a.rs"_s);
	QCOMPARE(detail(u"edit"_s, map({{u"path"_s, u"a.rs"_s}, {u"edits"_s, QVariantList{1, 2, 3}}})), u"a.rs (3 edits)"_s);
	QCOMPARE(detail(u"grep"_s, map({{u"pattern"_s, u"fn main"_s}})), u"\"fn main\""_s);
	QCOMPARE(detail(u"grep"_s, map({{u"pattern"_s, u"x"_s}, {u"path"_s, u"src"_s}})), u"\"x\" in src"_s);
	QCOMPARE(detail(u"find"_s, map({{u"path"_s, u"src"_s}})), u"src"_s);
	QCOMPARE(detail(u"find"_s, map({{u"pattern"_s, u"*.rs"_s}, {u"path"_s, u"src"_s}})), u"*.rs"_s);
	QCOMPARE(detail(u"webfetch"_s, map({{u"url"_s, u"https://example.com"_s}})), u"https://example.com"_s);
	QCOMPARE(detail(u"send_message"_s, map({{u"to"_s, u"worker"_s}})), u"\u2192 worker"_s);
	QCOMPARE(detail(u"send_message"_s, {}), QString());
	QCOMPARE(detail(u"create_session"_s, map({{u"name"_s, u"worker"_s}, {u"agent"_s, u"build"_s}})), u"worker (build)"_s);
	QCOMPARE(detail(u"create_session"_s, map({{u"name"_s, u"worker"_s}})), u"worker ()"_s);
	QCOMPARE(detail(u"webfetch"_s, map({{u"url"_s, u"https://e.com"_s}, {u"format"_s, u"text"_s}})), u"https://e.com (text)"_s);
	QCOMPARE(detail(u"webfetch"_s, map({{u"url"_s, u"https://e.com"_s}, {u"format"_s, u"markdown"_s}})), u"https://e.com"_s);
	QCOMPARE(detail(u"close_session"_s, map({{u"session"_s, u"worker"_s}})), u"worker"_s);
	QCOMPARE(detail(u"context_window"_s, map({{u"session"_s, u"worker"_s}})), u"worker"_s);
	const auto todo = [](const QString& status) { return map({{u"content"_s, u"x"_s}, {u"status"_s, status}}); };
	QCOMPARE(detail(u"todo"_s, map({{u"todos"_s, QVariantList{todo(u"completed"_s), todo(u"pending"_s)}}})), u"1/2 done"_s);
	QCOMPARE(detail(u"todo"_s, {}), QString());
}

void TestToolContent::detailsFollowTheTerminalClient()
{
	// Text that is present counts even when empty; a missing key falls back to the generic detail.
	QCOMPARE(detail(u"bash"_s, map({{u"command"_s, u""_s}})), u"$"_s);
	QCOMPARE(detail(u"find"_s, map({{u"pattern"_s, u""_s}, {u"path"_s, u"src"_s}})), QString());
	QCOMPARE(detail(u"read"_s, map({{u"file"_s, u"a.rs"_s}})), u"file=a.rs"_s);
	// One line: white space runs collapse, control characters go, the ends are trimmed.
	QCOMPARE(detail(u"bash"_s, map({{u"command"_s, u"a \t b\u0001c"_s}})), u"$ a bc"_s);
	QCOMPARE(detail(u"read"_s, map({{u"path"_s, u"  a\u00a0 b "_s}})), u"a b"_s);
}

void TestToolContent::toolsWithoutARuleListTheirFirstArguments()
{
	const QVariantMap input = map({{u"a"_s, u"one  two"_s}, {u"b"_s, 7}, {u"c"_s, true}, {u"d"_s, u"never"_s}, {u"list"_s, QVariantList{1}}});
	QCOMPARE(detail(u"mcp_files_list"_s, input), u"a=one two b=7 c=true"_s);
	// Lists and objects are skipped and do not count.
	QCOMPARE(detail(u"x"_s, map({{u"a"_s, QVariantList{1}}, {u"b"_s, 1.5}})), u"b=1.5"_s);
	QCOMPARE(detail(u"x"_s, {}), QString());

	// Values are cut at 40 terminal columns, wide characters count twice.
	QCOMPARE(detail(u"x"_s, map({{u"a"_s, QString(45, u'z')}})), u"a="_s + QString(39, u'z') + u"\u2026"_s);
	QCOMPARE(detail(u"x"_s, map({{u"a"_s, QString(20, u'\u4e2d')}})), u"a="_s + QString(20, u'\u4e2d'));
	QCOMPARE(detail(u"x"_s, map({{u"a"_s, QString(21, u'\u4e2d')}})), u"a="_s + QString(19, u'\u4e2d') + u"\u2026"_s);
	// Transport emoji (U+1F680) are wide; a cluster counts at most two columns, combining marks none.
	const QString rocket = QString::fromUcs4(U"\x1F680");
	QCOMPARE(detail(u"x"_s, map({{u"a"_s, rocket.repeated(21)}})), u"a="_s + rocket.repeated(19) + u"\u2026"_s);
	const QString family = QString::fromUcs4(U"\x1F468\x200D\x1F469\x200D\x1F467");
	QCOMPARE(detail(u"x"_s, map({{u"a"_s, family.repeated(20)}})), u"a="_s + family.repeated(20));
	const QString accented = QString::fromUcs4(U"e\x0301");
	QCOMPARE(detail(u"x"_s, map({{u"a"_s, accented.repeated(40)}})), u"a="_s + accented.repeated(40));
}

void TestToolContent::listingReadsHeadersRowsAndNotes()
{
	const QString output = u"[src/a.rs#AB12]\n 1:fn a() {}\n*2:fn b() {}\n...\n10:}\n\n[src/b.rs#cd34]\n*1:x\n[output cut]"_s;
	const QVariantList groups = listing(output);
	QCOMPARE(groups.size(), 2);

	const QVariantMap first = groups.at(0).toMap();
	QCOMPARE(first.value(u"path"_s).toString(), u"src/a.rs"_s);
	QCOMPARE(first.value(u"tag"_s).toString(), u"AB12"_s);
	const QVariantList rows = first.value(u"rows"_s).toList();
	QCOMPARE(rows.size(), 4);
	QCOMPARE(rows.at(0).toMap().value(u"kind"_s).toString(), u"context"_s);
	QCOMPARE(rows.at(0).toMap().value(u"number"_s).toString(), u"1"_s);
	QCOMPARE(rows.at(0).toMap().value(u"text"_s).toString(), u"fn a() {}"_s);
	QCOMPARE(rows.at(1).toMap().value(u"kind"_s).toString(), u"match"_s);
	QCOMPARE(rows.at(2).toMap().value(u"kind"_s).toString(), u"gap"_s);
	QCOMPARE(rows.at(3).toMap().value(u"kind"_s).toString(), u"line"_s);
	QCOMPARE(rows.at(3).toMap().value(u"number"_s).toString(), u"10"_s);
	QVERIFY(first.value(u"notes"_s).toList().isEmpty());

	const QVariantMap second = groups.at(1).toMap();
	QCOMPARE(second.value(u"path"_s).toString(), u"src/b.rs"_s);
	QCOMPARE(second.value(u"rows"_s).toList().size(), 1);
	QCOMPARE(second.value(u"notes"_s).toList(), QVariantList{u"[output cut]"_s});
}

void TestToolContent::listingWithoutHeaderStillGroups()
{
	const QVariantList groups = listing(u"1:a\n2:b"_s);
	QCOMPARE(groups.size(), 1);
	QCOMPARE(groups.at(0).toMap().value(u"path"_s).toString(), QString());
	QCOMPARE(groups.at(0).toMap().value(u"rows"_s).toList().size(), 2);
}

void TestToolContent::listingIgnoresOtherText()
{
	QVERIFY(listing({}).isEmpty());
	const QVariantList groups = listing(u"No matches found"_s);
	QCOMPARE(groups.size(), 1);
	QVERIFY(groups.at(0).toMap().value(u"rows"_s).toList().isEmpty());
	QCOMPARE(groups.at(0).toMap().value(u"notes"_s).toList(), QVariantList{u"No matches found"_s});
}

void TestToolContent::diffRowsMarkKinds()
{
	const QVariantList rows = diffRows(u"--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n keep\n-old\n+new\n"_s);
	QCOMPARE(rows.size(), 6);
	const auto kind = [&rows](int i) { return rows.at(i).toMap().value(u"kind"_s).toString(); };
	QCOMPARE(kind(0), u"file"_s);
	QCOMPARE(kind(1), u"file"_s);
	QCOMPARE(kind(2), u"hunk"_s);
	QCOMPARE(kind(3), u"context"_s);
	QCOMPARE(kind(4), u"removed"_s);
	QCOMPARE(kind(5), u"added"_s);
	QCOMPARE(rows.at(4).toMap().value(u"sign"_s).toString(), u"-"_s);
	QCOMPARE(rows.at(4).toMap().value(u"text"_s).toString(), u"old"_s);
	QCOMPARE(rows.at(5).toMap().value(u"sign"_s).toString(), u"+"_s);
	QCOMPARE(rows.at(2).toMap().value(u"sign"_s).toString(), QString());
	QCOMPARE(rows.at(2).toMap().value(u"text"_s).toString(), u"@@ -1,2 +1,2 @@"_s);
	QCOMPARE(rows.at(3).toMap().value(u"text"_s).toString(), u" keep"_s);
	QVERIFY(diffRows({}).isEmpty());
}

void TestToolContent::splitsPaths()
{
	const QVariantMap file = splitPath(u"src/parser.rs"_s);
	QCOMPARE(file.value(u"directory"_s).toString(), u"src/"_s);
	QCOMPARE(file.value(u"name"_s).toString(), u"parser.rs"_s);
	QVERIFY(!file.value(u"isDirectory"_s).toBool());

	const QVariantMap directory = splitPath(u"src/session/"_s);
	QCOMPARE(directory.value(u"directory"_s).toString(), u"src/"_s);
	QCOMPARE(directory.value(u"name"_s).toString(), u"session/"_s);
	QVERIFY(directory.value(u"isDirectory"_s).toBool());

	const QVariantMap bare = splitPath(u"Cargo.toml"_s);
	QCOMPARE(bare.value(u"directory"_s).toString(), QString());
	QCOMPARE(bare.value(u"name"_s).toString(), u"Cargo.toml"_s);
}

void TestToolContent::sizesReadInTheRightUnit()
{
	QCOMPARE(sizeText(0), u"0 B"_s);
	QCOMPARE(sizeText(1023), u"1023 B"_s);
	QCOMPARE(sizeText(1536), u"1.5 KiB"_s);
	QCOMPARE(sizeText(2 * 1024 * 1024), u"2.0 MiB"_s);
}

void TestToolContent::picksTextFromInput()
{
	const QVariantMap input = map({{u"to"_s, u""_s}, {u"name"_s, u"worker"_s}, {u"session"_s, u"other"_s}, {u"n"_s, 3}});
	QCOMPARE(firstText(input, {u"to"_s, u"name"_s, u"session"_s}), u"worker"_s);
	QCOMPARE(firstText(input, {u"n"_s, u"missing"_s}), QString());
	QCOMPARE(firstLine(u"a\nb"_s), u"a"_s);
	QCOMPARE(firstLine(u"ab"_s), u"ab"_s);
	QCOMPARE(firstLine({}), QString());
}

QTEST_APPLESS_MAIN(TestToolContent)
#include "tst_tool_content.moc"
