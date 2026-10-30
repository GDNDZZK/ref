// Runs against a private dbus-daemon and a fake notification service in this process, so
// no real notification is ever shown and the user's session bus is not touched.
#include "notify/dbus_notifier.h"

#include <QDBusConnection>
#include <QDBusContext>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QProcess>
#include <QSignalSpy>
#include <QTest>
#include <QVariantMap>

using namespace Qt::StringLiterals;
using refgui::notify::DBusNotifier;
using refgui::notify::Notification;
using Kind = refgui::proto::NotifyFrame::Kind;

namespace
{

const QString ServiceName = u"org.freedesktop.Notifications"_s;
const QString ObjectPath = u"/org/freedesktop/Notifications"_s;
const QString BusService = u"org.freedesktop.DBus"_s;
const QString BusPath = u"/org/freedesktop/DBus"_s;

constexpr int FlushTimeoutMs = 5000;

struct Received
{
	QString summary;
	QString body;
	uint replaces = 0;
	QStringList actions;
	QVariantMap hints;
};

// What a desktop's notification daemon does, as far as the notifier uses it.
class FakeService : public QObject, protected QDBusContext
{
	Q_OBJECT
	Q_CLASSINFO("D-Bus Interface", "org.freedesktop.Notifications")

public:
	QList<Received> received;
	bool failNext = false;
	QStringList capabilities{u"body"_s, u"actions"_s};
	uint lastId = 0;

public slots:
	QString GetServerInformation(QString& vendor, QString& version, QString& specVersion)
	{
		vendor = u"test"_s;
		version = u"1"_s;
		specVersion = u"1.2"_s;
		return u"fake"_s;
	}

	QStringList GetCapabilities()
	{
		return capabilities;
	}

	uint Notify(const QString&, uint replaces, const QString&, const QString& summary, const QString& body, const QStringList& actions,
	            const QVariantMap& hints, int)
	{
		if(failNext)
		{
			failNext = false;
			sendErrorReply(QDBusError::Failed, u"boom"_s);
			return 0;
		}
		received.append({summary, body, replaces, actions, hints});
		lastId = replaces != 0 ? replaces : lastId + 1;
		return lastId;
	}
};

} // namespace

class TestDBusNotifier : public QObject
{
	Q_OBJECT

private slots:
	void initTestCase();
	void cleanupTestCase();
	void unavailableWithoutAService();
	void availableOnceAServiceAppears();
	void showsTitleBodyAndClickAction();
	void aNewerNotificationOfASessionReplacesTheOlder();
	void clickActivatesTheSession();
	void closedNotificationsAreForgotten();
	void failedCallIsReported();
	void unavailableAgainWhenTheServiceLeaves();
	void markupServicesGetTheBodyEscaped();
	void plainServicesGetTheBodyAsItIs();

private:
	void restartService(const QStringList& capabilities);

	void sendSignal(const QString& name, const QList<QVariant>& arguments);
	void flush();

	QProcess m_daemon;
	QString m_address;
	std::unique_ptr<FakeService> m_service;
	std::unique_ptr<DBusNotifier> m_notifier;
};

void TestDBusNotifier::initTestCase()
{
	m_daemon.start(u"dbus-daemon"_s, {u"--session"_s, u"--nofork"_s, u"--print-address"_s});
	if(!m_daemon.waitForStarted() || !m_daemon.waitForReadyRead(5000))
	{
		// A CI machine is set up for this test; a missing daemon there is a mistake that must not pass unseen.
		if(qEnvironmentVariableIsSet("CI"))
		{
			QFAIL("dbus-daemon is not available, and this is CI");
		}
		QSKIP("dbus-daemon is not available");
	}
	m_address = QString::fromUtf8(m_daemon.readLine()).trimmed();
	QVERIFY(!m_address.isEmpty());

	m_notifier = std::make_unique<DBusNotifier>(QDBusConnection::connectToBus(m_address, u"notifier"_s));
}

void TestDBusNotifier::cleanupTestCase()
{
	m_notifier.reset();
	m_service.reset();
	QDBusConnection::disconnectFromBus(u"notifier"_s);
	QDBusConnection::disconnectFromBus(u"service"_s);
	m_daemon.terminate();
	m_daemon.waitForFinished(5000);
}

void TestDBusNotifier::sendSignal(const QString& name, const QList<QVariant>& arguments)
{
	QDBusMessage signal = QDBusMessage::createSignal(ObjectPath, ServiceName, name);
	signal.setArguments(arguments);
	QDBusConnection bus = QDBusConnection::connectToBus(m_address, u"service"_s);
	bus.send(signal);
	// The daemon has the signal once it answers a later call of the same connection.
	bus.call(QDBusMessage::createMethodCall(BusService, BusPath, BusService, u"GetId"_s));
}

// Returns when the notifier has handled every reply and signal the daemon sent it before now: the
// daemon hands them over in order, so the answer to a later call comes after them.
void TestDBusNotifier::flush()
{
	QDBusConnection bus = QDBusConnection::connectToBus(m_address, u"notifier"_s);
	QDBusPendingCallWatcher watcher(bus.asyncCall(QDBusMessage::createMethodCall(BusService, BusPath, BusService, u"GetId"_s)));
	QSignalSpy finished(&watcher, &QDBusPendingCallWatcher::finished);
	QVERIFY(finished.wait(FlushTimeoutMs));
}

void TestDBusNotifier::unavailableWithoutAService()
{
	QTRY_VERIFY(!m_notifier->unavailableReason().isEmpty() && !m_notifier->unavailableReason().contains(u"Looking"_s));
	QVERIFY(!m_notifier->isAvailable());

	QSignalSpy shown(m_notifier.get(), &DBusNotifier::showFailed);
	m_notifier->show({.sessionId = u"s1"_s, .kind = Kind::TurnDone, .title = u"t"_s, .body = u"b"_s});
	flush();
	QCOMPARE(shown.count(), 0); // nothing was attempted
}

void TestDBusNotifier::availableOnceAServiceAppears()
{
	QDBusConnection bus = QDBusConnection::connectToBus(m_address, u"service"_s);
	m_service = std::make_unique<FakeService>();
	QVERIFY(bus.registerObject(ObjectPath, m_service.get(), QDBusConnection::ExportAllSlots));
	QVERIFY(bus.registerService(ServiceName));

	QTRY_VERIFY(m_notifier->isAvailable());
	QVERIFY(m_notifier->unavailableReason().isEmpty());
}

void TestDBusNotifier::showsTitleBodyAndClickAction()
{
	m_notifier->show({.sessionId = u"s1"_s, .kind = Kind::Permission, .title = u"Fix build"_s, .body = u"Needs your permission: bash"_s});
	QTRY_COMPARE(m_service->received.size(), 1);

	const Received& received = m_service->received.last();
	QCOMPARE(received.summary, u"Fix build"_s);
	QCOMPARE(received.body, u"Needs your permission: bash"_s);
	QCOMPARE(received.replaces, 0u);
	QVERIFY(received.actions.contains(u"default"_s));
	QCOMPARE(received.hints.value(u"urgency"_s).value<uchar>(), uchar(2)); // asks for attention
}

void TestDBusNotifier::aNewerNotificationOfASessionReplacesTheOlder()
{
	flush(); // the reply to the first one arrives, so its id is known
	const uint first = m_service->lastId;
	QVERIFY(first != 0);

	m_notifier->show({.sessionId = u"s1"_s, .kind = Kind::Permission, .title = u"Fix build"_s, .body = u"Needs your permission"_s});
	QTRY_COMPARE(m_service->received.size(), 2);
	QCOMPARE(m_service->received.last().replaces, first);
	QCOMPARE(m_service->received.last().hints.value(u"urgency"_s).value<uchar>(), uchar(2));
	flush();

	m_notifier->show({.sessionId = u"s2"_s, .kind = Kind::Permission, .title = u"Other"_s, .body = u"Needs your permission"_s});
	QTRY_COMPARE(m_service->received.size(), 3);
	QCOMPARE(m_service->received.last().replaces, 0u);
	flush();

	// Another kind of the same session is a notification of its own, as in the Web interface.
	m_notifier->show({.sessionId = u"s1"_s, .kind = Kind::TurnDone, .title = u"Fix build"_s, .body = u"Finished"_s});
	QTRY_COMPARE(m_service->received.size(), 4);
	QCOMPARE(m_service->received.last().replaces, 0u);
	QCOMPARE(m_service->received.last().hints.value(u"urgency"_s).value<uchar>(), uchar(1));
	flush();
}

void TestDBusNotifier::clickActivatesTheSession()
{
	QSignalSpy activated(m_notifier.get(), &DBusNotifier::activated);
	const uint idOfS1 = 1; // the fake hands out 1 for s1 and reuses it when replacing
	sendSignal(u"ActionInvoked"_s, {idOfS1, u"other"_s});
	flush();
	QCOMPARE(activated.count(), 0);

	sendSignal(u"ActionInvoked"_s, {idOfS1, u"default"_s});
	QTRY_COMPARE(activated.count(), 1);
	QCOMPARE(activated.first().first().toString(), u"s1"_s);
}

void TestDBusNotifier::closedNotificationsAreForgotten()
{
	QSignalSpy activated(m_notifier.get(), &DBusNotifier::activated);
	const uint idOfS2 = 2;
	sendSignal(u"NotificationClosed"_s, {idOfS2, 1u});
	sendSignal(u"ActionInvoked"_s, {idOfS2, u"default"_s});
	flush();
	QCOMPARE(activated.count(), 0);
}

void TestDBusNotifier::failedCallIsReported()
{
	QSignalSpy failed(m_notifier.get(), &DBusNotifier::showFailed);
	m_service->failNext = true;
	m_notifier->show({.sessionId = u"s3"_s, .kind = Kind::TurnError, .title = u"t"_s, .body = u"Failed"_s});
	QTRY_COMPARE(failed.count(), 1);
	QVERIFY(failed.first().at(1).toString().contains(u"boom"_s));
}

void TestDBusNotifier::unavailableAgainWhenTheServiceLeaves()
{
	QSignalSpy changed(m_notifier.get(), &DBusNotifier::availabilityChanged);
	QVERIFY(QDBusConnection::connectToBus(m_address, u"service"_s).unregisterService(ServiceName));
	QTRY_VERIFY(!m_notifier->isAvailable());
	QVERIFY(changed.count() >= 1);
}

// The service goes and comes back saying `capabilities`.
void TestDBusNotifier::restartService(const QStringList& capabilities)
{
	QDBusConnection bus = QDBusConnection::connectToBus(m_address, u"service"_s);
	m_service->capabilities = capabilities;
	m_service->received.clear();
	QVERIFY(bus.registerService(ServiceName));
	QTRY_VERIFY(m_notifier->isAvailable());
}

void TestDBusNotifier::markupServicesGetTheBodyEscaped()
{
	restartService({u"body"_s, u"body-markup"_s});

	m_notifier->show({.sessionId = u"m1"_s, .kind = Kind::TurnError, .title = u"<b>t</b>"_s, .body = u"<img src=\"http://x/y.png\"> & more"_s});

	QTRY_COMPARE(m_service->received.size(), 1);
	QCOMPARE(m_service->received.last().body, u"&lt;img src=&quot;http://x/y.png&quot;&gt; &amp; more"_s);
	QCOMPARE(m_service->received.last().summary, u"<b>t</b>"_s); // the summary is never markup
}

void TestDBusNotifier::plainServicesGetTheBodyAsItIs()
{
	QVERIFY(QDBusConnection::connectToBus(m_address, u"service"_s).unregisterService(ServiceName));
	QTRY_VERIFY(!m_notifier->isAvailable());
	restartService({u"body"_s});

	m_notifier->show({.sessionId = u"p1"_s, .kind = Kind::TurnError, .title = u"t"_s, .body = u"a < b & c"_s});

	QTRY_COMPARE(m_service->received.size(), 1);
	QCOMPARE(m_service->received.last().body, u"a < b & c"_s);
}

QTEST_GUILESS_MAIN(TestDBusNotifier)
#include "tst_dbus_notifier.moc"
