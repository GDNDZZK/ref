#include "workspace/settings.h"

#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QtTest>
#include <limits>

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
	void runningOrderAndUiScaleValidationAndPersistence();
	void readUiScaleOnlyIsReadOnlyAndNormalizes();
	void sidebarSectionHeightsPersistAndValidate();
	void sidebarHistoryFiltersPersistAndValidate();
	void backendSettingsPersistValidateAndUseOwnerOnlyPermissions();
	void floatingBallPositionPersistsAndValidates();
	void floatingBallSizesPersistAndValidate();
	void messageWindowSizePersistsAndValidates();
};

void TestSettings::defaultsWithoutAFile()
{
	QTemporaryDir dir;
	const Settings settings(dir.filePath(u"gui.json"_s));

	QCOMPARE(settings.theme(), Settings::System);
	QCOMPARE(settings.language(), Settings::System);
	QVERIFY(settings.notifyEnabled() && settings.notifyPermission() && settings.notifyTurnDone() && settings.notifyTurnError());
	QVERIFY(settings.sidebarVisible());
	QVERIFY(settings.sidebarTodoExpanded());
	QVERIFY(settings.sidebarRunningExpanded());
	QVERIFY(settings.sidebarHistoryExpanded());
	QCOMPARE(settings.sidebarWidth(), Settings::DefaultSidebarWidth);
	QCOMPARE(settings.sidebarTodoHeight(), 0);
	QCOMPARE(settings.sidebarRunningHeight(), 0);
	QCOMPARE(settings.sidebarHistoryCreatorFilter(), Settings::HistoryCreatorAll);
	QVERIFY(!settings.sidebarHistoryCurrentDir());
	QCOMPARE(settings.windowWidth(), Settings::DefaultWindowWidth);
	QVERIFY(settings.sidebarRunningOrder().isEmpty());
	QCOMPARE(settings.uiScale(), Settings::DefaultUiScale);
	QCOMPARE(settings.minUiScale(), Settings::MinUiScale);
	QCOMPARE(settings.maxUiScale(), Settings::MaxUiScale);
	QCOMPARE(settings.uiScaleStep(), Settings::UiScaleStep);
	QCOMPARE(Settings::MinUiScale, 0.5);
	QCOMPARE(Settings::MaxUiScale, 4.0);
	QCOMPARE(Settings::UiScaleStep, 0.05);
	QCOMPARE(settings.messageWindowSize(), Settings::DefaultMessageWindowSize);
	QCOMPARE(settings.defaultMessageWindowSize(), Settings::DefaultMessageWindowSize);
	QCOMPARE(settings.messageWindowStep(), Settings::MessageWindowStep);
	QCOMPARE(Settings::DefaultMessageWindowSize, 1000);
	QCOMPARE(Settings::MessageWindowStep, 50);
	QCOMPARE(settings.floatingBallSize(), Settings::DefaultFloatingBallSize);
	QCOMPARE(settings.floatingPanelWidth(), Settings::DefaultFloatingPanelWidth);
	QCOMPARE(settings.floatingPanelHeight(), Settings::DefaultFloatingPanelHeight);
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
		settings.setSidebarTodoExpanded(false);
		settings.setSidebarRunningExpanded(false);
		settings.setSidebarHistoryExpanded(false);
		settings.setSidebarWidth(340);
		settings.setSidebarTodoHeight(140);
		settings.setSidebarRunningHeight(220);
		settings.setWindowWidth(1200);
		settings.setWindowHeight(800);
		settings.setSidebarRunningOrder({u"ses_b"_s, u"ses_a"_s, u"ses_b"_s, QString()});
		settings.setUiScale(1.254);
		settings.save();
	}
	const Settings again(path);
	QCOMPARE(again.theme(), Settings::Dark);
	QCOMPARE(again.language(), Settings::ChineseSimplified);
	QVERIFY(!again.notifyTurnDone());
	QVERIFY(again.notifyPermission());
	QVERIFY(!again.sidebarVisible());
	QVERIFY(!again.sidebarTodoExpanded());
	QVERIFY(!again.sidebarRunningExpanded());
	QVERIFY(!again.sidebarHistoryExpanded());
	QCOMPARE(again.sidebarWidth(), 340);
	QCOMPARE(again.sidebarTodoHeight(), 140);
	QCOMPARE(again.sidebarRunningHeight(), 220);
	QCOMPARE(again.windowWidth(), 1200);
	QCOMPARE(again.windowHeight(), 800);
	QCOMPARE(again.sidebarRunningOrder(), (QStringList{u"ses_b"_s, u"ses_a"_s}));
	QCOMPARE(again.uiScale(), 1.25);
	// Clamping works for setter and loaded values.
	Settings clamped(path);
	clamped.setSidebarWidth(100);
	QCOMPARE(clamped.sidebarWidth(), Settings::MinSidebarWidth);
	clamped.setSidebarWidth(900);
	QCOMPARE(clamped.sidebarWidth(), Settings::MaxSidebarWidth);
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
	writeFile(path, R"({"theme":"purple","language":"zh_CN","sidebar":{"visible":"yes","todoExpanded":"no","runningExpanded":1,"historyExpanded":{},"width":"wide","todoHeight":"tall","runningHeight":-5},"window":{"width":5,"height":700}})");
	const Settings settings(path);
	QCOMPARE(settings.theme(), Settings::System);
	QCOMPARE(settings.language(), Settings::ChineseSimplified); // the valid fields still count
	QVERIFY(settings.sidebarVisible());
	QVERIFY(settings.sidebarTodoExpanded());
	QVERIFY(settings.sidebarRunningExpanded());
	QVERIFY(settings.sidebarHistoryExpanded());
	QCOMPARE(settings.sidebarWidth(), Settings::DefaultSidebarWidth);
	QCOMPARE(settings.sidebarTodoHeight(), 0);
	QCOMPARE(settings.sidebarRunningHeight(), 0);
	QCOMPARE(settings.windowWidth(), Settings::DefaultWindowWidth);
	QCOMPARE(settings.windowHeight(), 700);
	const QString problem = settings.loadProblem();
	QVERIFY(problem.contains(u"theme"_s));
	QVERIFY(problem.contains(u"sidebar.visible"_s));
	QVERIFY(problem.contains(u"sidebar.todoExpanded"_s));
	QVERIFY(problem.contains(u"sidebar.runningExpanded"_s));
	QVERIFY(problem.contains(u"sidebar.historyExpanded"_s));
	QVERIFY(problem.contains(u"sidebar.width"_s));
	QVERIFY(problem.contains(u"sidebar.todoHeight: expected a non-negative number in pixels"_s));
	QVERIFY(problem.contains(u"sidebar.runningHeight: expected a non-negative number in pixels"_s));
	QVERIFY(problem.contains(u"window.width"_s));
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

void TestSettings::runningOrderAndUiScaleValidationAndPersistence()
{
	{
		QTemporaryDir dir;
		const QString path = dir.filePath(u"gui.json"_s);
		Settings settings(path);
		QSignalSpy orderChanged(&settings, &Settings::sidebarRunningOrderChanged);
		QSignalSpy scaleChanged(&settings, &Settings::uiScaleChanged);

		settings.setSidebarRunningOrder({u"s1"_s, QString(), u"s2"_s, u"s1"_s});
		QCOMPARE(settings.sidebarRunningOrder(), (QStringList{u"s1"_s, u"s2"_s}));
		settings.setSidebarRunningOrder({u"s1"_s, u"s2"_s, u"s2"_s});
		QCOMPARE(orderChanged.count(), 1);

		settings.setUiScale(0.2);
		QCOMPARE(settings.uiScale(), Settings::MinUiScale);
		settings.setUiScale(5.0);
		QCOMPARE(settings.uiScale(), Settings::MaxUiScale);
		settings.setUiScale(0.0);
		settings.setUiScale(-1.0);
		settings.setUiScale(std::numeric_limits<double>::quiet_NaN());
		settings.setUiScale(std::numeric_limits<double>::infinity());
		QCOMPARE(settings.uiScale(), Settings::MaxUiScale);
		QCOMPARE(scaleChanged.count(), 2);
		settings.save();
		QCOMPARE(readJson(path).value(u"uiScale"_s).toDouble(), Settings::MaxUiScale);
		QCOMPARE(readJson(path).value(u"sidebar"_s).toObject().value(u"runningOrder"_s).toArray().toVariantList(),
		         (QVariantList{u"s1"_s, u"s2"_s}));
	}
	{
		QTemporaryDir dir;
		const QString path = dir.filePath(u"gui.json"_s);
		writeFile(path, R"({"sidebar":{"runningOrder":"bad"},"uiScale":"big"})");
		const Settings settings(path);
		QVERIFY(settings.sidebarRunningOrder().isEmpty());
		QCOMPARE(settings.uiScale(), Settings::DefaultUiScale);
		QVERIFY(settings.loadProblem().contains(u"sidebar.runningOrder: expected an array of session ids"_s));
		QVERIFY(settings.loadProblem().contains(u"uiScale: expected a positive number"_s));
	}
	{
		QTemporaryDir dir;
		const QString path = dir.filePath(u"gui.json"_s);
		writeFile(path, R"({"sidebar":{"runningOrder":["ses_1",""]},"uiScale":-0.5})");
		const Settings settings(path);
		QVERIFY(settings.sidebarRunningOrder().isEmpty());
		QCOMPARE(settings.uiScale(), Settings::DefaultUiScale);
		QVERIFY(settings.loadProblem().contains(u"sidebar.runningOrder: expected session ids"_s));
		QVERIFY(settings.loadProblem().contains(u"uiScale: expected a positive number"_s));
	}
	{
		QTemporaryDir dir;
		const QString path = dir.filePath(u"gui.json"_s);
		writeFile(path, R"({"sidebar":{"runningOrder":["ses_2","ses_1","ses_2"]},"uiScale":1.236})");
		const Settings settings(path);
		QVERIFY(settings.loadProblem().isEmpty());
		QCOMPARE(settings.sidebarRunningOrder(), (QStringList{u"ses_2"_s, u"ses_1"_s}));
		QCOMPARE(settings.uiScale(), 1.24);
	}
}

void TestSettings::readUiScaleOnlyIsReadOnlyAndNormalizes()
{
	QTemporaryDir dir;
	const QString missingPath = dir.filePath(u"missing.json"_s);
	QCOMPARE(Settings::readUiScaleOnly(missingPath), Settings::DefaultUiScale);
	QVERIFY(!QFile::exists(missingPath));

	const QString brokenPath = dir.filePath(u"broken.json"_s);
	writeFile(brokenPath, "{ not json");
	QCOMPARE(Settings::readUiScaleOnly(brokenPath), Settings::DefaultUiScale);
	QVERIFY(QFile::exists(brokenPath));
	QVERIFY(!QFile::exists(brokenPath + u".bad"_s));

	const QString arrayPath = dir.filePath(u"array.json"_s);
	writeFile(arrayPath, "[1, 2]");
	QCOMPARE(Settings::readUiScaleOnly(arrayPath), Settings::DefaultUiScale);
	QVERIFY(!QFile::exists(arrayPath + u".bad"_s));

	const QString invalidScalePath = dir.filePath(u"invalid.json"_s);
	writeFile(invalidScalePath, R"({"uiScale":-2.0})");
	QCOMPARE(Settings::readUiScaleOnly(invalidScalePath), Settings::DefaultUiScale);

	const QString lowScalePath = dir.filePath(u"low.json"_s);
	writeFile(lowScalePath, R"({"uiScale":0.2})");
	QCOMPARE(Settings::readUiScaleOnly(lowScalePath), Settings::MinUiScale);

	const QString highScalePath = dir.filePath(u"high.json"_s);
	writeFile(highScalePath, R"({"uiScale":5.0})");
	QCOMPARE(Settings::readUiScaleOnly(highScalePath), Settings::MaxUiScale);

	const QString preciseScalePath = dir.filePath(u"precise.json"_s);
	writeFile(preciseScalePath, R"({"uiScale":1.554})");
	QCOMPARE(Settings::readUiScaleOnly(preciseScalePath), 1.55);
}

void TestSettings::sidebarSectionHeightsPersistAndValidate()
{
	QTemporaryDir dir;
	const QString path = dir.filePath(u"gui.json"_s);
	Settings settings(path);
	QSignalSpy todoSpy(&settings, &Settings::sidebarTodoHeightChanged);
	QSignalSpy runningSpy(&settings, &Settings::sidebarRunningHeightChanged);

	settings.setSidebarTodoHeight(-25);
	settings.setSidebarRunningHeight(-1);
	QCOMPARE(settings.sidebarTodoHeight(), 0);
	QCOMPARE(settings.sidebarRunningHeight(), 0);
	QCOMPARE(todoSpy.count(), 0);
	QCOMPARE(runningSpy.count(), 0);

	settings.setSidebarTodoHeight(160);
	settings.setSidebarRunningHeight(240);
	QCOMPARE(todoSpy.count(), 1);
	QCOMPARE(runningSpy.count(), 1);
	settings.save();

	const QJsonObject sidebar = readJson(path).value(u"sidebar"_s).toObject();
	QCOMPARE(sidebar.value(u"todoHeight"_s).toInt(), 160);
	QCOMPARE(sidebar.value(u"runningHeight"_s).toInt(), 240);

	settings.setSidebarTodoHeight(-10);
	QCOMPARE(settings.sidebarTodoHeight(), 0);
	QCOMPARE(todoSpy.count(), 2);
}

void TestSettings::sidebarHistoryFiltersPersistAndValidate()
{
	QTemporaryDir dir;
	const QString path = dir.filePath(u"gui.json"_s);
	{
		Settings settings(path);
		QSignalSpy creatorSpy(&settings, &Settings::sidebarHistoryCreatorFilterChanged);
		QSignalSpy dirSpy(&settings, &Settings::sidebarHistoryCurrentDirChanged);
		settings.setSidebarHistoryCreatorFilter(Settings::HistoryCreatorUser);
		settings.setSidebarHistoryCurrentDir(true);
		QCOMPARE(creatorSpy.count(), 1);
		QCOMPARE(dirSpy.count(), 1);
		settings.save();
	}

	const Settings reloaded(path);
	QVERIFY(reloaded.loadProblem().isEmpty());
	QCOMPARE(reloaded.sidebarHistoryCreatorFilter(), Settings::HistoryCreatorUser);
	QVERIFY(reloaded.sidebarHistoryCurrentDir());

	const QString badPath = dir.filePath(u"bad_filter.json"_s);
	writeFile(badPath, R"({"sidebar":{"historyCreatorFilter":"creator","historyCurrentDir":"yes"}})");
	const Settings bad(badPath);
	QCOMPARE(bad.sidebarHistoryCreatorFilter(), Settings::HistoryCreatorAll);
	QVERIFY(!bad.sidebarHistoryCurrentDir());
	QVERIFY(bad.loadProblem().contains(u"sidebar.historyCreatorFilter"_s));
	QVERIFY(bad.loadProblem().contains(u"sidebar.historyCurrentDir"_s));
}

void TestSettings::backendSettingsPersistValidateAndUseOwnerOnlyPermissions()
{
	QTemporaryDir dir;
	const QString path = dir.filePath(u"gui.json"_s);
	{
		Settings settings(path);
		QVERIFY(settings.backendUrl().isEmpty());
		QVERIFY(settings.backendToken().isEmpty());

		QSignalSpy spy(&settings, &Settings::backendChanged);
		settings.setBackend(u"wss://example.com:8443/ws"_s, u"secret_tok"_s);
		QCOMPARE(spy.count(), 1);
		QCOMPARE(settings.backendUrl(), u"wss://example.com:8443/ws"_s);
		QCOMPARE(settings.backendToken(), u"secret_tok"_s);
		QVERIFY(QFile::exists(path)); // setBackend writes immediately
	}
#ifndef Q_OS_WIN
	const QFileDevice::Permissions perms = QFileInfo(path).permissions();
	QVERIFY(perms.testFlag(QFileDevice::ReadOwner));
	QVERIFY(perms.testFlag(QFileDevice::WriteOwner));
	QVERIFY(!perms.testFlag(QFileDevice::ReadGroup));
	QVERIFY(!perms.testFlag(QFileDevice::WriteGroup));
	QVERIFY(!perms.testFlag(QFileDevice::ReadOther));
	QVERIFY(!perms.testFlag(QFileDevice::WriteOther));
#endif
	{
		Settings reloaded(path);
		QVERIFY(reloaded.loadProblem().isEmpty());
		QCOMPARE(reloaded.backendUrl(), u"wss://example.com:8443/ws"_s);
		QCOMPARE(reloaded.backendToken(), u"secret_tok"_s);
		reloaded.setBackend({}, {});
	}
	const QJsonObject cleared = readJson(path);
	QVERIFY(!cleared.contains(u"backendUrl"_s));
	QVERIFY(!cleared.contains(u"backendToken"_s));

	const QString badPath = dir.filePath(u"bad_backend.json"_s);
	writeFile(badPath, R"({"backendUrl":"http://not-ws.example","backendToken":"orphan"})");
	const Settings bad(badPath);
	QVERIFY(bad.backendUrl().isEmpty());
	QVERIFY(bad.backendToken().isEmpty());
	QVERIFY(bad.loadProblem().contains(u"backendUrl"_s));
}

void TestSettings::floatingBallPositionPersistsAndValidates()
{
	QTemporaryDir dir;
	const QString path = dir.filePath(u"gui.json"_s);
	{
		Settings settings(path);
		QVERIFY(!settings.hasFloatingBallPosition());
		QSignalSpy spy(&settings, &Settings::floatingBallChanged);
		settings.setFloatingBallPosition(u"DP-1"_s, 140, 260);
		QCOMPARE(spy.count(), 1);
		QVERIFY(settings.hasFloatingBallPosition());
		QCOMPARE(settings.floatingBallScreen(), u"DP-1"_s);
		QCOMPARE(settings.floatingBallX(), 140);
		QCOMPARE(settings.floatingBallY(), 260);
		settings.save();
	}
	const Settings reloaded(path);
	QVERIFY(reloaded.loadProblem().isEmpty());
	QVERIFY(reloaded.hasFloatingBallPosition());
	QCOMPARE(reloaded.floatingBallScreen(), u"DP-1"_s);
	QCOMPARE(reloaded.floatingBallX(), 140);
	QCOMPARE(reloaded.floatingBallY(), 260);

	const QString badPath = dir.filePath(u"bad_ball.json"_s);
	writeFile(badPath, R"({"floatingBall":{"x":"bad","y":10}})");
	const Settings bad(badPath);
	QVERIFY(!bad.hasFloatingBallPosition());
	QVERIFY(bad.loadProblem().contains(u"floatingBall"_s));
}

void TestSettings::floatingBallSizesPersistAndValidate()
{
	QTemporaryDir dir;
	const QString path = dir.filePath(u"gui.json"_s);
	{
		Settings settings(path);
		QCOMPARE(settings.floatingBallSize(), Settings::DefaultFloatingBallSize);
		QCOMPARE(settings.defaultFloatingBallSize(), Settings::DefaultFloatingBallSize);
		QCOMPARE(settings.minFloatingBallSize(), Settings::MinFloatingBallSize);
		QCOMPARE(settings.maxFloatingBallSize(), Settings::MaxFloatingBallSize);
		QCOMPARE(settings.floatingBallSizeStep(), Settings::FloatingBallSizeStep);
		QCOMPARE(settings.floatingPanelWidth(), Settings::DefaultFloatingPanelWidth);
		QCOMPARE(settings.defaultFloatingPanelWidth(), Settings::DefaultFloatingPanelWidth);
		QCOMPARE(settings.minFloatingPanelWidth(), Settings::MinFloatingPanelWidth);
		QCOMPARE(settings.maxFloatingPanelWidth(), Settings::MaxFloatingPanelWidth);
		QCOMPARE(settings.floatingPanelHeight(), Settings::DefaultFloatingPanelHeight);
		QCOMPARE(settings.defaultFloatingPanelHeight(), Settings::DefaultFloatingPanelHeight);
		QCOMPARE(settings.minFloatingPanelHeight(), Settings::MinFloatingPanelHeight);
		QCOMPARE(settings.maxFloatingPanelHeight(), Settings::MaxFloatingPanelHeight);
		QCOMPARE(settings.floatingPanelSizeStep(), Settings::FloatingPanelSizeStep);

		QSignalSpy spy(&settings, &Settings::floatingBallChanged);
		settings.setFloatingBallSize(10);
		QCOMPARE(settings.floatingBallSize(), Settings::MinFloatingBallSize);
		settings.setFloatingBallSize(200);
		QCOMPARE(settings.floatingBallSize(), Settings::MaxFloatingBallSize);
		settings.setFloatingBallSize(200); // unchanged
		settings.setFloatingBallSize(72);

		settings.setFloatingPanelWidth(100);
		QCOMPARE(settings.floatingPanelWidth(), Settings::MinFloatingPanelWidth);
		settings.setFloatingPanelWidth(2000);
		QCOMPARE(settings.floatingPanelWidth(), Settings::MaxFloatingPanelWidth);
		settings.setFloatingPanelHeight(100);
		QCOMPARE(settings.floatingPanelHeight(), Settings::MinFloatingPanelHeight);
		settings.setFloatingPanelHeight(2000);
		QCOMPARE(settings.floatingPanelHeight(), Settings::MaxFloatingPanelHeight);

		settings.setFloatingPanelSize(460, 640);
		QCOMPARE(settings.floatingPanelWidth(), 460);
		QCOMPARE(settings.floatingPanelHeight(), 640);
		settings.setFloatingPanelSize(460, 640); // unchanged, no signal
		QCOMPARE(spy.count(), 8);

		settings.setFloatingPanelSize(50, 5000);
		QCOMPARE(settings.floatingPanelWidth(), Settings::MinFloatingPanelWidth);
		QCOMPARE(settings.floatingPanelHeight(), Settings::MaxFloatingPanelHeight);
		QCOMPARE(spy.count(), 9);

		settings.setFloatingPanelSize(480, 660);
		QCOMPARE(spy.count(), 10);
		settings.save();
	}

	const QJsonObject savedBall = readJson(path).value(u"floatingBall"_s).toObject();
	QCOMPARE(savedBall.value(u"ballSize"_s).toInt(), 72);
	QCOMPARE(savedBall.value(u"panelWidth"_s).toInt(), 480);
	QCOMPARE(savedBall.value(u"panelHeight"_s).toInt(), 660);

	const Settings reloaded(path);
	QVERIFY(reloaded.loadProblem().isEmpty());
	QVERIFY(!reloaded.hasFloatingBallPosition());
	QCOMPARE(reloaded.floatingBallSize(), 72);
	QCOMPARE(reloaded.floatingPanelWidth(), 480);
	QCOMPARE(reloaded.floatingPanelHeight(), 660);

	// Out-of-range numeric values are clamped on load; unknown keys survive when reset to default.
	const QString clampedPath = dir.filePath(u"clamped_ball.json"_s);
	writeFile(clampedPath, R"({"floatingBall":{"ballSize":12,"panelWidth":999,"panelHeight":120,"extra":"keep"}})");
	{
		Settings clamped(clampedPath);
		QVERIFY(clamped.loadProblem().isEmpty());
		QCOMPARE(clamped.floatingBallSize(), Settings::MinFloatingBallSize);
		QCOMPARE(clamped.floatingPanelWidth(), Settings::MaxFloatingPanelWidth);
		QCOMPARE(clamped.floatingPanelHeight(), Settings::MinFloatingPanelHeight);

		clamped.setFloatingBallSize(Settings::DefaultFloatingBallSize);
		clamped.setFloatingPanelSize(Settings::DefaultFloatingPanelWidth, Settings::DefaultFloatingPanelHeight);
		clamped.save();
	}
	const QJsonObject resetBall = readJson(clampedPath).value(u"floatingBall"_s).toObject();
	QVERIFY(!resetBall.contains(u"ballSize"_s));
	QVERIFY(!resetBall.contains(u"panelWidth"_s));
	QVERIFY(!resetBall.contains(u"panelHeight"_s));
	QCOMPARE(resetBall.value(u"extra"_s).toString(), u"keep"_s);

	// Non-numeric values are reported and fall back to defaults.
	const QString badPath = dir.filePath(u"bad_ball_sizes.json"_s);
	writeFile(badPath, R"({"floatingBall":{"ballSize":"big","panelWidth":null,"panelHeight":{}}})");
	const Settings bad(badPath);
	QCOMPARE(bad.floatingBallSize(), Settings::DefaultFloatingBallSize);
	QCOMPARE(bad.floatingPanelWidth(), Settings::DefaultFloatingPanelWidth);
	QCOMPARE(bad.floatingPanelHeight(), Settings::DefaultFloatingPanelHeight);
	QVERIFY(bad.loadProblem().contains(u"floatingBall.ballSize"_s));
	QVERIFY(bad.loadProblem().contains(u"floatingBall.panelWidth"_s));
	QVERIFY(bad.loadProblem().contains(u"floatingBall.panelHeight"_s));
}

void TestSettings::messageWindowSizePersistsAndValidates()
{
	QTemporaryDir dir;
	const QString zeroPath = dir.filePath(u"zero.json"_s);
	{
		Settings settings(zeroPath);
		QSignalSpy spy(&settings, &Settings::messageWindowSizeChanged);
		settings.setMessageWindowSize(-50); // clamps to 0 (unlimited)
		QCOMPARE(settings.messageWindowSize(), 0);
		QCOMPARE(spy.count(), 1);
		settings.setMessageWindowSize(0); // no change, no signal
		QCOMPARE(spy.count(), 1);
		settings.save();
	}
	{
		const Settings reloaded(zeroPath);
		QVERIFY(reloaded.loadProblem().isEmpty());
		QCOMPARE(reloaded.messageWindowSize(), 0);
		QCOMPARE(readJson(zeroPath).value(u"messageWindowSize"_s).toInt(-1), 0);
	}

	const QString largePath = dir.filePath(u"large.json"_s);
	{
		Settings settings(largePath);
		settings.setMessageWindowSize(25000);
		QCOMPARE(settings.messageWindowSize(), 25000);
		settings.save();
	}
	{
		const Settings reloaded(largePath);
		QVERIFY(reloaded.loadProblem().isEmpty());
		QCOMPARE(reloaded.messageWindowSize(), 25000);
		QCOMPARE(readJson(largePath).value(u"messageWindowSize"_s).toInt(), 25000);
	}

	const QString negPath = dir.filePath(u"neg.json"_s);
	writeFile(negPath, R"({"messageWindowSize":-1})");
	{
		const Settings bad(negPath);
		QCOMPARE(bad.messageWindowSize(), Settings::DefaultMessageWindowSize);
		QVERIFY(bad.loadProblem().contains(u"messageWindowSize"_s));
	}

	const QString strPath = dir.filePath(u"str.json"_s);
	writeFile(strPath, R"({"messageWindowSize":"1000"})");
	{
		const Settings bad(strPath);
		QCOMPARE(bad.messageWindowSize(), Settings::DefaultMessageWindowSize);
		QVERIFY(bad.loadProblem().contains(u"messageWindowSize"_s));
	}

	const QString fracPath = dir.filePath(u"frac.json"_s);
	writeFile(fracPath, R"({"messageWindowSize":12.5})");
	{
		const Settings bad(fracPath);
		QCOMPARE(bad.messageWindowSize(), Settings::DefaultMessageWindowSize);
		QVERIFY(bad.loadProblem().contains(u"messageWindowSize"_s));
	}
}

QTEST_GUILESS_MAIN(TestSettings)
#include "tst_settings.moc"
