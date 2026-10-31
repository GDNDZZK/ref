#include "workspace/settings.h"

#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QtTest>

using namespace Qt::StringLiterals;
using refgui::workspace::Settings;

namespace
{

QJsonObject readJson(const QString& path)
{
	QFile file(path);
	[&] { QVERIFY2(file.open(QIODevice::ReadOnly), qPrintable(path)); }();
	return QJsonDocument::fromJson(file.readAll()).object();
}

void writeFile(const QString& path, const QByteArray& content)
{
	QFile file(path);
	[&] { QVERIFY(file.open(QIODevice::WriteOnly)); }();
	file.write(content);
}

} // namespace

class TestSettings : public QObject
{
	Q_OBJECT

private slots:
	void defaultsWithoutAFile();
	void savedChangesComeBack();
	void changesInABurstMakeOneWrite();
	void pendingChangesAreWrittenOnDestruction();
	void unknownFieldsSurvive();
	void brokenFileIsKeptAndDefaultsUsed();
	void unknownValuesAreIgnoredAndReported();
	void notifyKindsFollowTheSwitches();
	void unwritableLocationIsReported();
};

void TestSettings::defaultsWithoutAFile()
{
	QTemporaryDir dir;
	const Settings settings(dir.filePath(u"gui.json"_s));

	QCOMPARE(settings.theme(), Settings::System);
	QCOMPARE(settings.language(), Settings::System);
	QVERIFY(settings.notifyEnabled() && settings.notifyPermission() && settings.notifyTurnDone() && settings.notifyTurnError());
	QVERIFY(settings.sidebarVisible());
	QCOMPARE(settings.windowWidth(), Settings::DefaultWindowWidth);
	QCOMPARE(settings.windowHeight(), Settings::DefaultWindowHeight);
	QVERIFY(settings.loadProblem().isEmpty());
	QVERIFY(!QFile::exists(dir.filePath(u"gui.json"_s))); // nothing is written until something changes
}

void TestSettings::savedChangesComeBack()
{
	QTemporaryDir dir;
	const QString path = dir.filePath(u"state/gui.json"_s); // the directory does not exist yet
	{
		Settings settings(path);
		settings.setTheme(Settings::Dark);
		settings.setLanguage(Settings::ChineseSimplified);
		settings.setNotifyTurnDone(false);
		settings.setSidebarVisible(false);
		settings.setWindowWidth(1200);
		settings.setWindowHeight(800);
		settings.save();
	}

	const Settings again(path);
	QCOMPARE(again.theme(), Settings::Dark);
	QCOMPARE(again.language(), Settings::ChineseSimplified);
	QVERIFY(!again.notifyTurnDone());
	QVERIFY(again.notifyPermission());
	QVERIFY(!again.sidebarVisible());
	QCOMPARE(again.windowWidth(), 1200);
	QCOMPARE(again.windowHeight(), 800);
}

void TestSettings::changesInABurstMakeOneWrite()
{
	QTemporaryDir dir;
	const QString path = dir.filePath(u"gui.json"_s);
	Settings settings(path);

	for(int width = 900; width < 910; ++width)
	{
		settings.setWindowWidth(width);
	}
	QVERIFY(!QFile::exists(path)); // not yet
	QTRY_VERIFY(QFile::exists(path));
	QCOMPARE(readJson(path).value(u"window"_s).toObject().value(u"width"_s).toInt(), 909);
}

void TestSettings::pendingChangesAreWrittenOnDestruction()
{
	QTemporaryDir dir;
	const QString path = dir.filePath(u"gui.json"_s);
	{
		Settings settings(path);
		settings.setTheme(Settings::Light);
	}
	QCOMPARE(readJson(path).value(u"theme"_s).toString(), u"light"_s);
}

void TestSettings::unknownFieldsSurvive()
{
	QTemporaryDir dir;
	const QString path = dir.filePath(u"gui.json"_s);
	writeFile(path, R"({"theme":"dark","future":{"a":1},"notifications":{"enabled":false,"sound":true}})");
	{
		Settings settings(path);
		QVERIFY(!settings.notifyEnabled());
		settings.setLanguage(Settings::English);
	}

	const QJsonObject saved = readJson(path);
	QCOMPARE(saved.value(u"future"_s).toObject().value(u"a"_s).toInt(), 1);
	QCOMPARE(saved.value(u"notifications"_s).toObject().value(u"sound"_s).toBool(), true);
	QCOMPARE(saved.value(u"notifications"_s).toObject().value(u"enabled"_s).toBool(), false);
	QCOMPARE(saved.value(u"theme"_s).toString(), u"dark"_s);
	QCOMPARE(saved.value(u"language"_s).toString(), u"en"_s);
}

void TestSettings::brokenFileIsKeptAndDefaultsUsed()
{
	QTemporaryDir dir;
	const QString path = dir.filePath(u"gui.json"_s);
	writeFile(path, "{ not json");

	const Settings settings(path);

	QCOMPARE(settings.theme(), Settings::System);
	QVERIFY(settings.loadProblem().contains(u"gui.json.bad"_s));
	QVERIFY(QFile::exists(path + u".bad"_s));
	QVERIFY(!QFile::exists(path));
}

void TestSettings::unknownValuesAreIgnoredAndReported()
{
	QTemporaryDir dir;
	const QString path = dir.filePath(u"gui.json"_s);
	writeFile(path, R"({"theme":"purple","language":"zh_CN","sidebar":{"visible":"yes"},"window":{"width":5,"height":700}})");

	const Settings settings(path);

	QCOMPARE(settings.theme(), Settings::System);
	QCOMPARE(settings.language(), Settings::ChineseSimplified); // the valid fields still count
	QVERIFY(settings.sidebarVisible());
	QCOMPARE(settings.windowWidth(), Settings::DefaultWindowWidth);
	QCOMPARE(settings.windowHeight(), 700);
	const QString problem = settings.loadProblem();
	QVERIFY(problem.contains(u"theme"_s));
	QVERIFY(problem.contains(u"sidebar"_s));
	QVERIFY(problem.contains(u"window.width"_s));
	QVERIFY(QFile::exists(path)); // a file with some bad values is not thrown away
}

void TestSettings::notifyKindsFollowTheSwitches()
{
	QTemporaryDir dir;
	Settings settings(dir.filePath(u"gui.json"_s));
	QSignalSpy changed(&settings, &Settings::notifyKindsChanged);

	QCOMPARE(settings.notifyKinds(), (QStringList{u"permission"_s, u"question"_s, u"turn_done"_s, u"turn_error"_s}));
	settings.setNotifyTurnDone(false);
	QCOMPARE(settings.notifyKinds(), (QStringList{u"permission"_s, u"question"_s, u"turn_error"_s}));
	settings.setNotifyEnabled(false);
	QVERIFY(settings.notifyKinds().isEmpty());
	settings.setNotifyEnabled(false); // no change, no signal
	QCOMPARE(changed.count(), 2);
}

void TestSettings::unwritableLocationIsReported()
{
	QTemporaryDir dir;
	writeFile(dir.filePath(u"blocker"_s), "a file where a directory should be");
	Settings settings(dir.filePath(u"blocker/gui.json"_s));
	QSignalSpy failed(&settings, &Settings::saveFailed);

	settings.setTheme(Settings::Dark);
	settings.save();

	QCOMPARE(failed.count(), 1);
}

QTEST_GUILESS_MAIN(TestSettings)
#include "tst_settings.moc"
