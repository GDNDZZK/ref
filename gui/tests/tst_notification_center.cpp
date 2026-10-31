// What the window tells the backend about notifications and what it does with the ones that come back.
#include "client/backend.h"
#include "fake_backend.h"
#include "notify/notification_center.h"

#include <QJsonArray>
#include <QSignalSpy>
#include <QWindow>
#include <QtTest>

using namespace Qt::StringLiterals;
using namespace refgui;
using refgui::client::Backend;
using refgui::notify::Notification;
using refgui::notify::NotificationCenter;
using refgui::notify::Notifier;
using refgui::test::FakeBackend;

namespace
{

class RecordingNotifier : public Notifier
{
	Q_OBJECT

public:
	bool isAvailable() const override { return available; }
	QString unavailableReason() const override { return available ? QString() : u"no service"_s; }
	void show(const Notification& notification) override { shown.append(notification); }

	bool available = true;
	QList<Notification> shown;
};

QJsonObject notifyFrame(const QString& kind)
{
	return {{u"kind"_s, u"notify"_s},
	        {u"session_id"_s, u"ses_2"_s},
	        {u"notify_kind"_s, kind},
	        {u"session_title"_s, u"Fix build"_s},
	        {u"detail"_s, u"bash"_s}};
}

struct Fixture
{
	Fixture()
		: backend(Backend::Source(Backend::Mode::Remote{{fake.url(), u"token"_s}}), u"/work"_s)
		, notifier(new RecordingNotifier)
		, center(backend, std::unique_ptr<Notifier>(notifier))
	{
		fake.onRequest = [this](const QJsonObject& request) { fake.respond(request, {}); };
		fake.onHello = [this](const QJsonObject& hello)
		{
			helloKinds = hello.value(u"notify"_s).toArray();
			fake.respond(hello, {{u"server_version"_s, u"0.0.0-test"_s}, {u"protocol"_s, 2}});
		};
	}

	~Fixture()
	{
		backend.shutdown();
	}

	void connect()
	{
		backend.start();
		QTRY_COMPARE(backend.state(), Backend::State::Connected);
	}

	QList<QJsonObject> requestsOf(const QString& op) const
	{
		QList<QJsonObject> result;
		for(const QJsonObject& request : fake.requests())
		{
			if(request.value(u"op"_s).toString() == op)
			{
				result.append(request);
			}
		}
		return result;
	}

	FakeBackend fake;
	Backend backend;
	RecordingNotifier* notifier; // owned by `center`
	NotificationCenter center;
	QJsonArray helloKinds;
};

} // namespace

class TestNotificationCenter : public QObject
{
	Q_OBJECT

private slots:
	void kindsSetBeforeConnectingGoInHello();
	void connectingDoesNotSendTheKindsAgain();
	void kindsChangedWhileHelloWasOnItsWayAreSent();
	void kindsChangedLaterAreSent();
	void unknownKindsAreIgnored();
	void focusIsSentOnChangeOnly();
	void focusIsSentAgainAfterConnecting();
	void notificationsAreShown();
	void questionsAreShown();
	void unavailableNotifierFlashesTheWindow();
	void clickingARequestsTheSession();
};

void TestNotificationCenter::kindsSetBeforeConnectingGoInHello()
{
	Fixture f;
	f.center.setKinds({u"permission"_s, u"turn_error"_s});
	f.connect();
	QCOMPARE(f.helloKinds, (QJsonArray{u"permission"_s, u"turn_error"_s}));
}

void TestNotificationCenter::connectingDoesNotSendTheKindsAgain()
{
	Fixture f;
	f.center.setKinds({u"permission"_s});
	f.center.setFocus(true, {u"ses_1"_s});
	f.connect();

	QTRY_COMPARE(f.requestsOf(u"set_focus"_s).size(), 1); // the focus is not in Hello, the kinds are
	QVERIFY(f.requestsOf(u"set_notify"_s).isEmpty());
}

void TestNotificationCenter::kindsChangedWhileHelloWasOnItsWayAreSent()
{
	Fixture f;
	QJsonObject pendingHello;
	f.fake.onHello = [&](const QJsonObject& hello) { pendingHello = hello; };
	f.center.setKinds({u"permission"_s});
	f.backend.start();
	QTRY_VERIFY(!pendingHello.isEmpty());

	f.center.setKinds({u"turn_done"_s}); // not connected yet, so nothing is sent
	f.fake.respond(pendingHello, {{u"server_version"_s, u"0.0.0-test"_s}, {u"protocol"_s, 2}});

	QTRY_COMPARE(f.backend.state(), Backend::State::Connected);
	QTRY_COMPARE(f.requestsOf(u"set_notify"_s).size(), 1);
	QCOMPARE(f.requestsOf(u"set_notify"_s).last().value(u"kinds"_s).toArray(), (QJsonArray{u"turn_done"_s}));
}

void TestNotificationCenter::kindsChangedLaterAreSent()
{
	Fixture f;
	f.connect();
	QSignalSpy changed(&f.center, &NotificationCenter::kindsChanged);

	f.center.setKinds({u"turn_done"_s});
	f.center.setKinds({u"turn_done"_s}); // no change

	QCOMPARE(changed.count(), 1);
	QTRY_VERIFY(!f.requestsOf(u"set_notify"_s).isEmpty());
	QTRY_COMPARE(f.requestsOf(u"set_notify"_s).last().value(u"kinds"_s).toArray(), (QJsonArray{u"turn_done"_s}));
}

void TestNotificationCenter::unknownKindsAreIgnored()
{
	Fixture f;
	f.center.setKinds({u"nonsense"_s, u"turn_done"_s});
	QCOMPARE(f.center.kinds(), (QStringList{u"turn_done"_s}));
}

void TestNotificationCenter::focusIsSentOnChangeOnly()
{
	Fixture f;
	f.connect();

	f.center.setFocus(true, {u"ses_1"_s});
	f.center.setFocus(true, {u"ses_1"_s});
	f.center.setFocus(false, {u"ses_1"_s});

	QTRY_COMPARE(f.requestsOf(u"set_focus"_s).size(), 2);
	const QJsonObject first = f.requestsOf(u"set_focus"_s).first();
	QVERIFY(first.value(u"focused"_s).toBool());
	QCOMPARE(first.value(u"sessions"_s).toArray(), (QJsonArray{u"ses_1"_s}));
	QVERIFY(!f.requestsOf(u"set_focus"_s).last().value(u"focused"_s).toBool());
}

void TestNotificationCenter::focusIsSentAgainAfterConnecting()
{
	Fixture f;
	f.center.setFocus(true, {u"ses_1"_s}); // before there is a connection
	QVERIFY(f.requestsOf(u"set_focus"_s).isEmpty());

	f.connect();

	QTRY_COMPARE(f.requestsOf(u"set_focus"_s).size(), 1);
}

void TestNotificationCenter::notificationsAreShown()
{
	Fixture f;
	f.connect();

	f.fake.send(notifyFrame(u"permission"_s));

	QTRY_COMPARE(f.notifier->shown.size(), 1);
	QCOMPARE(f.notifier->shown.first().sessionId, u"ses_2"_s);
	QCOMPARE(f.notifier->shown.first().title, u"Fix build"_s);
	QCOMPARE(f.notifier->shown.first().body, u"Waiting for permission to use bash"_s);
}

void TestNotificationCenter::questionsAreShown()
{
	Fixture f;
	f.connect();

	f.fake.send(notifyFrame(u"question"_s));

	QTRY_COMPARE(f.notifier->shown.size(), 1);
	QCOMPARE(f.notifier->shown.first().sessionId, u"ses_2"_s);
	QCOMPARE(f.notifier->shown.first().title, u"Fix build"_s);
	QCOMPARE(f.notifier->shown.first().body, u"Asks: bash"_s);
}

void TestNotificationCenter::unavailableNotifierFlashesTheWindow()
{
	Fixture f;
	f.notifier->available = false;
	QVERIFY(!f.center.systemNotificationsAvailable());
	QCOMPARE(f.center.unavailableReason(), u"no service"_s);
	f.connect();

	f.fake.send(notifyFrame(u"turn_done"_s));
	QTest::qWait(100);

	QVERIFY(f.notifier->shown.isEmpty());
}

void TestNotificationCenter::clickingARequestsTheSession()
{
	Fixture f;
	QSignalSpy requested(&f.center, &NotificationCenter::sessionRequested);

	emit f.notifier->activated(u"ses_2"_s);

	QCOMPARE(requested.count(), 1);
	QCOMPARE(requested.first().first().toString(), u"ses_2"_s);
}

QTEST_GUILESS_MAIN(TestNotificationCenter)
#include "tst_notification_center.moc"
