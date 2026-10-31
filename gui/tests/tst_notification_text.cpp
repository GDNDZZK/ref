#include "notify/notification_text.h"

#include <QTest>

using namespace Qt::StringLiterals;
using refgui::notify::makeNotification;
using Kind = refgui::proto::NotifyFrame::Kind;

namespace
{

refgui::proto::NotifyFrame frame(Kind kind, const QString& title, std::optional<QString> detail)
{
	return {.sessionId = u"s1"_s, .kind = kind, .sessionTitle = title, .detail = std::move(detail)};
}

} // namespace

class TestNotificationText : public QObject
{
	Q_OBJECT

private slots:
	void permissionNamesTheTool();
	void questionQuotesTheFirstQuestion();
	void turnDoneIsShort();
	void turnErrorShowsTheFirstLineOnly();
	void longErrorsAreCut();
	void titleIsTheSessionOrTheAppName();
};

void TestNotificationText::permissionNamesTheTool()
{
	const auto n = makeNotification(frame(Kind::Permission, u"Fix build"_s, u"bash"_s));
	QCOMPARE(n.sessionId, u"s1"_s);
	QCOMPARE(n.kind, Kind::Permission);
	QCOMPARE(n.title, u"Fix build"_s);
	QCOMPARE(n.body, u"Waiting for permission to use bash"_s);
	QCOMPARE(makeNotification(frame(Kind::Permission, u"t"_s, std::nullopt)).body, u"Waiting for permission"_s);
}

void TestNotificationText::questionQuotesTheFirstQuestion()
{
	const auto n = makeNotification(frame(Kind::Question, u"Fix build"_s, u"Which file first?"_s));
	QCOMPARE(n.kind, Kind::Question);
	QCOMPARE(n.title, u"Fix build"_s);
	QCOMPARE(n.body, u"Asks: Which file first?"_s);
	QCOMPARE(makeNotification(frame(Kind::Question, u"t"_s, std::nullopt)).body, u"Asks a question"_s);
}

void TestNotificationText::turnDoneIsShort()
{
	QCOMPARE(makeNotification(frame(Kind::TurnDone, u"t"_s, std::nullopt)).body, u"Finished"_s);
}

void TestNotificationText::turnErrorShowsTheFirstLineOnly()
{
	const auto n = makeNotification(frame(Kind::TurnError, u"t"_s, u"\n  Rate limited\nretry in 20 s"_s));
	QCOMPARE(n.body, u"Stopped with an error: Rate limited"_s);
	QCOMPARE(makeNotification(frame(Kind::TurnError, u"t"_s, u"  "_s)).body, u"Stopped with an error"_s);
}

void TestNotificationText::longErrorsAreCut()
{
	const auto n = makeNotification(frame(Kind::TurnError, u"t"_s, QString(500, u'x')));
	QCOMPARE(n.body, u"Stopped with an error: "_s + QString(200, u'x') + u"\u2026"_s);
}

void TestNotificationText::titleIsTheSessionOrTheAppName()
{
	QCOMPARE(makeNotification(frame(Kind::TurnDone, u"  "_s, std::nullopt)).title, u"ref"_s);
	QCOMPARE(makeNotification(frame(Kind::TurnDone, QString(), std::nullopt)).title, u"ref"_s);
}

QTEST_GUILESS_MAIN(TestNotificationText)
#include "tst_notification_text.moc"
