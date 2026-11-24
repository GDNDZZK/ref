#include "app/floating_ball_helper.h"

#include <QtTest>

using namespace Qt::StringLiterals;

class TestFloatingBallHelper : public QObject
{
	Q_OBJECT

private slots:
	void clampBallPositionStaysInsideAvailableGeometry();
	void resolveBallPositionUsesDefaultSavedAndFallbackScreens();
	void computePanelPlacementExpandsInwardFromScreenCorners();
	void clampWindowRectAndPickScreenNameWork();
};

void TestFloatingBallHelper::clampBallPositionStaysInsideAvailableGeometry()
{
	const QRect avail(100, 50, 1200, 800);
	const QSize ball(56, 56);

	QCOMPARE(FloatingBallHelper::clampBallPosition({0, 0}, ball, avail), QPoint(100, 50));
	QCOMPARE(FloatingBallHelper::clampBallPosition({2000, 1500}, ball, avail), QPoint(1244, 794));
	QCOMPARE(FloatingBallHelper::clampBallPosition({300, 400}, ball, avail), QPoint(300, 400));
	QCOMPARE(FloatingBallHelper::clampBallPosition({10, 20}, ball, QRect()), QPoint(10, 20));
}

void TestFloatingBallHelper::resolveBallPositionUsesDefaultSavedAndFallbackScreens()
{
	const QList<FloatingBallHelper::ScreenGeometry> screens{
		{.name = u"DP-1"_s, .availableGeometry = QRect(0, 0, 1920, 1080)},
		{.name = u"HDMI-1"_s, .availableGeometry = QRect(1920, 0, 1280, 720)},
	};
	const QSize ball(56, 56);

	QString resolvedScreen;
	const QPoint defPos = FloatingBallHelper::resolveBallPosition(false, {}, {}, ball, screens, &resolvedScreen);
	QCOMPARE(resolvedScreen, u"DP-1"_s);
	QCOMPARE(defPos, QPoint(1920 - 56 - 24, 1080 - 56 - 24));

	const QPoint savedPos = FloatingBallHelper::resolveBallPosition(true, u"HDMI-1"_s, {3500, 900}, ball, screens, &resolvedScreen);
	QCOMPARE(resolvedScreen, u"HDMI-1"_s);
	QCOMPARE(savedPos, QPoint(1920 + 1280 - 56, 720 - 56));

	// Disappeared screen falls back to primary / best intersection and clamps cleanly.
	const QPoint fallbackPos = FloatingBallHelper::resolveBallPosition(true, u"MISSING-1"_s, {2200, 300}, ball, screens, &resolvedScreen);
	QCOMPARE(resolvedScreen, u"HDMI-1"_s);
	QCOMPARE(fallbackPos, QPoint(2200, 300));
}

void TestFloatingBallHelper::computePanelPlacementExpandsInwardFromScreenCorners()
{
	const QList<FloatingBallHelper::ScreenGeometry> screens{
		{.name = u"DP-1"_s, .availableGeometry = QRect(0, 0, 1920, 1080)},
	};
	const QSize panel(380, 520);

	// Bottom-right corner ball expands up and left.
	const auto br = FloatingBallHelper::computePanelPlacement(QRect(1840, 1000, 56, 56), panel, screens, u"DP-1"_s);
	QCOMPARE(br.screenName, u"DP-1"_s);
	QVERIFY(br.expandLeft);
	QVERIFY(br.expandUp);
	QCOMPARE(br.rect, QRect(1840 + 56 - 380, 1000 + 56 - 520, 380, 520));

	// Top-left corner ball expands down and right.
	const auto tl = FloatingBallHelper::computePanelPlacement(QRect(24, 24, 56, 56), panel, screens, u"DP-1"_s);
	QVERIFY(!tl.expandLeft);
	QVERIFY(!tl.expandUp);
	QCOMPARE(tl.rect, QRect(24, 24, 380, 520));
}

void TestFloatingBallHelper::clampWindowRectAndPickScreenNameWork()
{
	const QList<FloatingBallHelper::ScreenGeometry> screens{
		{.name = u"DP-1"_s, .availableGeometry = QRect(0, 0, 1920, 1080)},
		{.name = u"HDMI-1"_s, .availableGeometry = QRect(1920, 0, 1280, 720)},
	};
	const QRect clamped = FloatingBallHelper::clampWindowRect(QRect(3180, 700, 56, 56), screens);
	QCOMPARE(clamped, QRect(3144, 664, 56, 56));
	QCOMPARE(FloatingBallHelper::pickScreenName(clamped, screens), u"HDMI-1"_s);
}

QTEST_GUILESS_MAIN(TestFloatingBallHelper)
#include "tst_floating_ball_helper.moc"
