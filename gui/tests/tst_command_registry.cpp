#include "workspace/command_registry.h"

#include <QSignalSpy>
#include <QtTest>

using namespace Qt::StringLiterals;
using namespace refgui::workspace;

namespace
{

constexpr CommandContext Idle{.connected = true, .hasSession = true, .liveSessions = 2, .splitView = false};

QStringList ids(const QVariantList& matches)
{
	QStringList result;
	for(const QVariant& match : matches)
	{
		result.append(match.toMap().value(u"id"_s).toString());
	}
	return result;
}

// Adds `value` and tells whether it was new.
bool addNew(QSet<QString>& set, const QString& value)
{
	const qsizetype before = set.size();
	set.insert(value);
	return set.size() > before;
}

// A registry whose every built-in command records that it ran.
struct Fixture
{
	Fixture()
	{
		for(int row = 0; row < registry.rowCount(); ++row)
		{
			const QString id = registry.index(row).data(CommandRegistry::IdRole).toString();
			if(id.startsWith(u"custom:"))
			{
				continue;
			}
			registry.setHandler(id, [this, id](const QString& args, const QString& sessionId) { ran << u"%1|%2|%3"_s.arg(id, args, sessionId); });
		}
		registry.setContext(Idle);
	}

	CommandRegistry registry;
	QStringList ran;
};

} // namespace

class TestCommandRegistry : public QObject
{
	Q_OBJECT

private slots:
	void idsShortcutsAndSlashNamesAreUnique();
	void commandsWithoutAHandlerAreUnavailable();
	void availabilityFollowsTheContext();
	void runPassesArgumentsAndSession();
	void runSlashKnowsAliases();
	void runSlashLeavesOrdinaryTextAlone();
	void unavailableSlashCommandIsHandledAndReported();
	void matchRanksSlashNamesBeforeTitles();
	void slashListHidesUnavailableAndPaletteOnlyCommands();
	void customCommandsRunThroughTheRunner();
	void keyCapsSplitTheShortcut();
};

void TestCommandRegistry::idsShortcutsAndSlashNamesAreUnique()
{
	Fixture f;
	QSet<QString> idsSeen;
	QSet<QString> shortcuts;
	QSet<QString> slashNames;
	for(int row = 0; row < f.registry.rowCount(); ++row)
	{
		const QModelIndex index = f.registry.index(row);
		QVERIFY(addNew(idsSeen, index.data(CommandRegistry::IdRole).toString()));
		const QString shortcut = index.data(CommandRegistry::ShortcutRole).toString();
		QVERIFY2(shortcut.isEmpty() || addNew(shortcuts, shortcut), qPrintable(shortcut));
		QStringList names = index.data(CommandRegistry::AliasesRole).toStringList();
		names.prepend(index.data(CommandRegistry::SlashRole).toString());
		for(const QString& name : names)
		{
			QVERIFY2(name.isEmpty() || addNew(slashNames, name), qPrintable(name));
		}
	}
	QVERIFY(idsSeen.contains(u"new"_s) && idsSeen.contains(u"palette"_s));
}

void TestCommandRegistry::commandsWithoutAHandlerAreUnavailable()
{
	CommandRegistry registry;
	registry.setContext(Idle);

	QVERIFY(!registry.run(u"new"_s));
	registry.setHandler(u"new"_s, [](const QString&, const QString&) {});
	QVERIFY(registry.run(u"new"_s));
}

void TestCommandRegistry::availabilityFollowsTheContext()
{
	Fixture f;
	QSignalSpy changed(&f.registry, &QAbstractItemModel::dataChanged);

	f.registry.setContext({.connected = false});
	QVERIFY(!f.registry.run(u"new"_s));
	QVERIFY(f.registry.run(u"help"_s)); // needs nothing
	QVERIFY(changed.count() > 0);

	f.registry.setContext({.connected = true, .hasSession = false});
	QVERIFY(f.registry.run(u"new"_s));
	QVERIFY(!f.registry.run(u"rename"_s, u"x"_s));
	QVERIFY(!f.registry.run(u"close-session"_s));

	f.registry.setContext(Idle);
	QVERIFY(f.registry.run(u"close-session"_s));
	QVERIFY(!f.registry.run(u"zoom"_s)); // only in split view
	f.registry.setContext({.connected = true, .hasSession = true, .liveSessions = 2, .splitView = true});
	QVERIFY(f.registry.run(u"zoom"_s));
}

void TestCommandRegistry::runPassesArgumentsAndSession()
{
	Fixture f;

	QVERIFY(f.registry.run(u"rename"_s, u"  parser fix "_s, u"ses_2"_s));

	QCOMPARE(f.ran, (QStringList{u"rename|parser fix|ses_2"_s}));
	QVERIFY(!f.registry.run(u"no-such-command"_s));
}

void TestCommandRegistry::runSlashKnowsAliases()
{
	Fixture f;

	QVERIFY(f.registry.runSlash(u"/clear"_s));
	QVERIFY(f.registry.runSlash(u"  /effort   high  "_s, u"ses_3"_s));
	QVERIFY(f.registry.runSlash(u"/cwd\t~/src"_s));

	QCOMPARE(f.ran, (QStringList{u"new||"_s, u"variant|high|ses_3"_s, u"cd|~/src|"_s}));
}

void TestCommandRegistry::runSlashLeavesOrdinaryTextAlone()
{
	Fixture f;

	QVERIFY(!f.registry.runSlash(u"explain this"_s));
	QVERIFY(!f.registry.runSlash(u"/unknown thing"_s));
	QVERIFY(!f.registry.runSlash(u"/"_s));
	QVERIFY(f.ran.isEmpty());
}

void TestCommandRegistry::unavailableSlashCommandIsHandledAndReported()
{
	Fixture f;
	f.registry.setContext({.connected = false});
	QSignalSpy unavailable(&f.registry, &CommandRegistry::unavailable);

	QVERIFY(f.registry.runSlash(u"/compact"_s)); // not sent to the model as a prompt

	QVERIFY(f.ran.isEmpty());
	QCOMPARE(unavailable.count(), 1);
	QCOMPARE(unavailable.first().first().toString(), u"compact"_s);
}

void TestCommandRegistry::matchRanksSlashNamesBeforeTitles()
{
	Fixture f;

	QCOMPARE(ids(f.registry.matching(u"new"_s, false)).first(), u"new"_s); // exact name first
	QCOMPARE(ids(f.registry.matching(u"/ses"_s, false)).first(), u"sessions"_s); // prefix of a name; a leading slash is ignored
	const QStringList byWord = ids(f.registry.matching(u"split"_s, false));
	QVERIFY(byWord.contains(u"watch"_s)); // alias
	QCOMPARE(ids(f.registry.matching(u"swse"_s, false)).first(), u"sessions"_s); // letters in order in "Switch session" only
	QVERIFY(ids(f.registry.matching(u"qqqq"_s, false)).isEmpty());
	QCOMPARE(f.registry.matching({}, false).size(), f.registry.rowCount());
}

void TestCommandRegistry::slashListHidesUnavailableAndPaletteOnlyCommands()
{
	Fixture f;
	f.registry.setContext({.connected = true, .hasSession = false});

	const QStringList slash = ids(f.registry.matching({}, true));

	QVERIFY(slash.contains(u"new"_s));
	QVERIFY(!slash.contains(u"rename"_s)); // needs a session
	QVERIFY(!slash.contains(u"new-in"_s));  // palette only
	QVERIFY(!slash.contains(u"palette"_s));
	QVERIFY(ids(f.registry.matching({}, false)).contains(u"rename"_s)); // the palette lists it, greyed out
}

void TestCommandRegistry::customCommandsRunThroughTheRunner()
{
	Fixture f;
	QStringList custom;
	f.registry.setCustomRunner([&](const QString& name, const QString& args, const QString& sessionId) { custom << u"%1|%2|%3"_s.arg(name, args, sessionId); });
	f.registry.setCustomCommands({{.name = u"review"_s, .description = u"Review the changes"_s, .source = u"config"_s},
	                              {.name = u"new"_s, .description = u"shadows a built-in"_s, .source = u"config"_s}});

	QVERIFY(f.registry.runSlash(u"/review the parser"_s, u"ses_1"_s));
	QVERIFY(f.registry.runSlash(u"/new"_s)); // still the built-in

	QCOMPARE(custom, (QStringList{u"review|the parser|ses_1"_s}));
	QCOMPARE(f.ran, (QStringList{u"new||"_s}));
	const QVariantMap entry = f.registry.matching(u"review"_s, true).first().toMap();
	QCOMPARE(entry.value(u"title"_s).toString(), u"Review the changes"_s);
	QCOMPARE(entry.value(u"source"_s).toString(), u"config"_s);

	f.registry.setCustomCommands({});
	QVERIFY(!f.registry.runSlash(u"/review"_s));
}

void TestCommandRegistry::keyCapsSplitTheShortcut()
{
	QVERIFY(CommandRegistry::keyCaps({}).isEmpty());
#ifndef Q_OS_MACOS
	QCOMPARE(CommandRegistry::keyCaps(u"Ctrl+Shift+N"_s), (QStringList{u"Ctrl"_s, u"Shift"_s, u"N"_s}));
#endif
}

QTEST_GUILESS_MAIN(TestCommandRegistry)
#include "tst_command_registry.moc"
