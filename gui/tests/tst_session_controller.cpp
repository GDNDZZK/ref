#include "client/backend.h"
#include "coro_test.h"
#include "fake_backend.h"
#include "session/session_controller.h"
#include "session_test_data.h"

#include <QJsonArray>
#include <QSignalSpy>
#include <QtTest>

using namespace Qt::StringLiterals;
using namespace refgui;
using namespace refgui::test;
using refgui::client::Backend;
using refgui::session::ConversationModel;
using refgui::session::SessionController;

namespace
{

constexpr int ReconnectTimeoutMs = 8000; // the backend retries after a second

QJsonObject infoJson(const QString& id, const QString& state)
{
	return {{u"id"_s, id},
	        {u"name"_s, u"one"_s},
	        {u"title"_s, u"A title"_s},
	        {u"agent"_s, u"build"_s},
	        {u"model"_s, QJsonObject{{u"provider"_s, u"p"_s}, {u"model"_s, u"m"_s}}},
	        {u"cwd"_s, u"/work"_s},
	        {u"created_at"_s, 1},
	        {u"updated_at"_s, 1},
	        {u"status"_s, QJsonObject{{u"state"_s, state}}},
	        {u"attached"_s, 1},
	        {u"live"_s, true}};
}

QJsonObject messageJson(const QString& id, const QString& role, const QString& text, bool ended)
{
	return {{u"id"_s, id},
	        {u"role"_s, role},
	        {u"time"_s, 1},
	        {u"time_end"_s, ended ? QJsonValue(2) : QJsonValue()},
	        {u"source"_s, QJsonObject{{u"kind"_s, u"human"_s}}},
	        {u"parts"_s, text.isEmpty() ? QJsonArray() : QJsonArray{QJsonObject{{u"type"_s, u"text"_s}, {u"text"_s, text}, {u"time"_s, 1}}}}};
}

QJsonObject snapshotJson(const QString& id, const QString& state, const QJsonArray& messages)
{
	return {{u"snapshot"_s, QJsonObject{{u"info"_s, infoJson(id, state)}, {u"messages"_s, messages}, {u"more_before"_s, false}}}};
}

QJsonObject eventFrame(const QString& sessionId, const QJsonObject& event)
{
	return {{u"kind"_s, u"event"_s}, {u"session_id"_s, sessionId}, {u"event"_s, event}};
}

QJsonObject deltaFrame(const QString& sessionId, const QString& messageId, qsizetype index, const QString& text)
{
	return eventFrame(sessionId,
	                  {{u"type"_s, u"delta"_s},
	                   {u"message_id"_s, messageId},
	                   {u"index"_s, static_cast<int>(index)},
	                   {u"delta"_s, QJsonObject{{u"type"_s, u"text"_s}, {u"text"_s, text}}}});
}

QJsonObject infoFrame(const QString& sessionId, const QString& state)
{
	return eventFrame(sessionId, {{u"type"_s, u"info"_s}, {u"info"_s, infoJson(sessionId, state)}});
}

QJsonObject messageFrame(const QString& sessionId, const QJsonObject& message)
{
	return eventFrame(sessionId, {{u"type"_s, u"message"_s}, {u"message"_s, message}});
}

// A backend that knows one session, "ses_1", with a user message and a reply.
struct Fixture
{
	Fixture()
		: backend(Backend::Source(Backend::Mode::Remote{{fake.url(), u"token"_s}}), u"/work"_s)
		, controller(backend)
	{
		snapshot = snapshotJson(u"ses_1"_s,
		                        u"idle"_s,
		                        {messageJson(u"u1"_s, u"user"_s, u"Hello"_s, true), messageJson(u"a1"_s, u"assistant"_s, u"Hi there"_s, true)});
		fake.onRequest = [this](const QJsonObject& request) { answer(request); };
		backend.start();
		QTRY_COMPARE(backend.state(), Backend::State::Connected);
	}

	~Fixture()
	{
		backend.shutdown();
	}

	void answer(const QJsonObject& request)
	{
		const QString op = request.value(u"op"_s).toString();
		if(op == u"create_session")
		{
			fake.respond(request, {{u"session"_s, infoJson(u"ses_1"_s, u"idle"_s)}});
		}
		else if(op == u"attach")
		{
			fake.respond(request, snapshot);
		}
		else if(op == u"list_history")
		{
			fake.respond(request, {{u"sessions"_s, history}});
		}
		else if(op == u"prompt")
		{
			fake.respond(request, {{u"queued"_s, false}, {u"message_id"_s, u"u2"_s}});
		}
		else if(op == u"messages")
		{
			fake.respond(request, {{u"messages"_s, olderPage}, {u"more_before"_s, olderHasMore}});
		}
		else if(op == u"abort" || op == u"detach")
		{
			fake.respond(request, {});
		}
	}

	QStringList ops() const
	{
		QStringList result;
		for(const QJsonObject& request : fake.requests())
		{
			result.append(request.value(u"op"_s).toString());
		}
		return result;
	}

	FakeBackend fake;
	Backend backend;
	SessionController controller;
	QJsonObject snapshot;
	QJsonArray history;
	QJsonArray olderPage;
	bool olderHasMore = false;
};

} // namespace

class TestSessionController : public QObject
{
	Q_OBJECT

private slots:
	void createShowsTheSnapshot();
	void attachFailureIsReported();
	void streamsAReplyAndTracksBusy();
	void ignoresEventsOfOtherSessions();
	void sendDoesNotEchoThePrompt();
	void abortAsksTheBackend();
	void continueLastAttachesTheNewestSession();
	void continueLastWithoutHistory();
	void attachesAgainAfterReconnect();
	void passesOnErrors();
	void watcherAttachDoesNotTakeTheForeground();
	void upgradingAWatcherKeepsTheConversation();
	void detachedControllerIgnoresEventsAndAttachesAgain();
	void loadsOlderMessagesInFront();
	void doesNotAskWhenNothingIsOlder();
	void showsSessionErrorsAtTheEndUntilTheNextPrompt();
};

void TestSessionController::createShowsTheSnapshot()
{
	Fixture f;

	const auto result = await(f.controller.create(u"/work"_s, u"build"_s, std::nullopt));

	QVERIFY(result && result->has_value());
	QCOMPARE(f.ops(), (QStringList{u"hello"_s, u"create_session"_s, u"attach"_s}));
	QCOMPARE(f.controller.sessionId(), u"ses_1"_s);
	QCOMPARE(f.controller.info().title, u"A title"_s);
	QCOMPARE(render(f.controller.model()), (QStringList{u"user:Hello"_s, u"markdown:Hi there"_s}));
	QVERIFY(!f.controller.isBusy());
}

void TestSessionController::attachFailureIsReported()
{
	Fixture f;
	f.fake.onRequest = [&f](const QJsonObject& request) { f.fake.fail(request, u"no such session"_s); };

	const auto result = await(f.controller.attach(u"ses_x"_s));

	QVERIFY(result);
	QVERIFY(!result->has_value());
	QCOMPARE(result->error(), u"no such session"_s);
	QVERIFY(!f.controller.hasSession());
}

void TestSessionController::streamsAReplyAndTracksBusy()
{
	Fixture f;
	QVERIFY(await(f.controller.attach(u"ses_1"_s)));
	QSignalSpy busy(&f.controller, &SessionController::busyChanged);

	f.fake.send(messageFrame(u"ses_1"_s, messageJson(u"u2"_s, u"user"_s, u"Again"_s, true)));
	f.fake.send(infoFrame(u"ses_1"_s, u"busy"_s));
	f.fake.send(messageFrame(u"ses_1"_s, messageJson(u"a2"_s, u"assistant"_s, QString(), false)));
	QTRY_VERIFY(f.controller.isBusy());
	f.fake.send(deltaFrame(u"ses_1"_s, u"a2"_s, 0, u"Working on "_s));
	f.fake.send(deltaFrame(u"ses_1"_s, u"a2"_s, 0, u"it."_s));
	f.fake.send(messageFrame(u"ses_1"_s, messageJson(u"a2"_s, u"assistant"_s, u"Working on it."_s, true)));
	f.fake.send(infoFrame(u"ses_1"_s, u"idle"_s));

	QTRY_VERIFY(!f.controller.isBusy());
	QVERIFY(f.controller.isTurnInProgress() || render(f.controller.model()).size() == 4);
	QTRY_VERIFY(!f.controller.isTurnInProgress());
	QCOMPARE(render(f.controller.model()),
	         (QStringList{u"user:Hello"_s, u"markdown:Hi there"_s, u"user:Again"_s, u"markdown:Working on it."_s}));
	QCOMPARE(busy.count(), 2);
}

void TestSessionController::ignoresEventsOfOtherSessions()
{
	Fixture f;
	QVERIFY(await(f.controller.attach(u"ses_1"_s)));

	f.fake.send(messageFrame(u"ses_other"_s, messageJson(u"u9"_s, u"user"_s, u"not for us"_s, true)));
	f.fake.send(infoFrame(u"ses_1"_s, u"busy"_s)); // a marker: handled after the frame above
	QTRY_VERIFY(f.controller.isBusy());

	QCOMPARE(contentRowCount(f.controller.model()), 2);
}

void TestSessionController::sendDoesNotEchoThePrompt()
{
	Fixture f;
	QVERIFY(await(f.controller.attach(u"ses_1"_s)));

	const auto result = await(f.controller.send(u"Do it"_s));

	QVERIFY(result && result->has_value());
	QCOMPARE(f.fake.requests().last().value(u"op"_s).toString(), u"prompt"_s);
	QCOMPARE(f.fake.requests().last().value(u"session_id"_s).toString(), u"ses_1"_s);
	QCOMPARE(f.fake.requests().last().value(u"text"_s).toString(), u"Do it"_s);
	QCOMPARE(contentRowCount(f.controller.model()), 2);
}

void TestSessionController::abortAsksTheBackend()
{
	Fixture f;
	QVERIFY(await(f.controller.attach(u"ses_1"_s)));

	const auto result = await(f.controller.abort());

	QVERIFY(result && result->has_value());
	QCOMPARE(f.fake.requests().last().value(u"op"_s).toString(), u"abort"_s);
	QCOMPARE(f.fake.requests().last().value(u"session_id"_s).toString(), u"ses_1"_s);
}

void TestSessionController::continueLastAttachesTheNewestSession()
{
	Fixture f;
	f.history = {infoJson(u"ses_1"_s, u"idle"_s)};

	const auto result = await(f.controller.continueLast(u"/work"_s));

	QVERIFY(result && result->has_value());
	QVERIFY(**result);
	QCOMPARE(f.controller.sessionId(), u"ses_1"_s);
	QCOMPARE(f.fake.requests().at(1).value(u"limit"_s).toInt(), 1);
	QCOMPARE(f.fake.requests().at(1).value(u"cwd"_s).toString(), u"/work"_s);
}

void TestSessionController::continueLastWithoutHistory()
{
	Fixture f;

	const auto result = await(f.controller.continueLast(u"/work"_s));

	QVERIFY(result && result->has_value());
	QVERIFY(!**result);
	QVERIFY(!f.controller.hasSession());
	QCOMPARE(f.ops(), (QStringList{u"hello"_s, u"list_history"_s}));
}

void TestSessionController::attachesAgainAfterReconnect()
{
	Fixture f;
	QVERIFY(await(f.controller.attach(u"ses_1"_s)));
	f.snapshot = snapshotJson(u"ses_1"_s, u"idle"_s, {messageJson(u"u1"_s, u"user"_s, u"Hello"_s, true), messageJson(u"a1"_s, u"assistant"_s, u"Hi there, and more"_s, true)});

	f.fake.socket()->close();

	QTRY_COMPARE_WITH_TIMEOUT(f.ops(), (QStringList{u"hello"_s, u"attach"_s, u"hello"_s, u"attach"_s}), ReconnectTimeoutMs);
	QTRY_COMPARE(render(f.controller.model()).last(), u"markdown:Hi there, and more"_s);
	QCOMPARE(f.fake.requests().last().value(u"session_id"_s).toString(), u"ses_1"_s);
}

void TestSessionController::passesOnErrors()
{
	Fixture f;
	QVERIFY(await(f.controller.attach(u"ses_1"_s)));
	QSignalSpy errors(&f.controller, &SessionController::errorReported);

	f.fake.send(eventFrame(u"ses_1"_s, {{u"type"_s, u"error"_s}, {u"text"_s, u"it broke"_s}}));

	QTRY_COMPARE(errors.count(), 1);
	QCOMPARE(errors.first().first().toString(), u"it broke"_s);
}

void TestSessionController::watcherAttachDoesNotTakeTheForeground()
{
	Fixture f;
	QVERIFY(await(f.controller.attach(u"ses_1"_s, false)));

	QCOMPARE(f.fake.requests().last().value(u"op"_s).toString(), u"attach"_s);
	QCOMPARE(f.fake.requests().last().value(u"foreground"_s).toBool(), false);
	QVERIFY(f.controller.isAttached());
}

void TestSessionController::upgradingAWatcherKeepsTheConversation()
{
	Fixture f;
	f.snapshot = snapshotJson(u"ses_1"_s, u"idle"_s, {messageJson(u"u1"_s, u"user"_s, u"Hello"_s, true)});
	QVERIFY(await(f.controller.attach(u"ses_1"_s, false)));
	QSignalSpy resets(&f.controller.model(), &QAbstractItemModel::modelReset);

	f.snapshot = snapshotJson(u"ses_1"_s, u"idle"_s, {messageJson(u"u1"_s, u"user"_s, u"Changed"_s, true)});
	QVERIFY(await(f.controller.attach(u"ses_1"_s, true)));

	QCOMPARE(f.fake.requests().last().value(u"foreground"_s).toBool(), true);
	QCOMPARE(resets.count(), 0);
	QCOMPARE(render(f.controller.model()), (QStringList{u"user:Hello"_s}));
}

void TestSessionController::detachedControllerIgnoresEventsAndAttachesAgain()
{
	Fixture f;
	f.snapshot = snapshotJson(u"ses_1"_s, u"idle"_s, {messageJson(u"u1"_s, u"user"_s, u"Hello"_s, true)});
	QVERIFY(await(f.controller.attach(u"ses_1"_s)));

	QVERIFY(await(f.controller.detach()));
	QVERIFY(!f.controller.isAttached());
	QCOMPARE(f.controller.sessionId(), u"ses_1"_s);
	QCOMPARE(f.fake.requests().last().value(u"op"_s).toString(), u"detach"_s);

	f.fake.send(eventFrame(u"ses_1"_s, {{u"type"_s, u"message"_s}, {u"message"_s, messageJson(u"u2"_s, u"user"_s, u"Missed"_s, true)}}));
	QTest::qWait(50);
	QCOMPARE(render(f.controller.model()), (QStringList{u"user:Hello"_s}));

	f.snapshot = snapshotJson(u"ses_1"_s, u"idle"_s, {messageJson(u"u1"_s, u"user"_s, u"Hello"_s, true), messageJson(u"u2"_s, u"user"_s, u"Missed"_s, true)});
	QVERIFY(await(f.controller.attach(u"ses_1"_s)));
	QCOMPARE(render(f.controller.model()), (QStringList{u"user:Hello"_s, u"user:Missed"_s}));
}

void TestSessionController::loadsOlderMessagesInFront()
{
	Fixture f;
	f.snapshot[u"snapshot"_s] = [&] {
		QJsonObject snapshot = f.snapshot.value(u"snapshot"_s).toObject();
		snapshot[u"more_before"_s] = true;
		return snapshot;
	}();
	f.olderPage = {messageJson(u"u0"_s, u"user"_s, u"Earlier"_s, true)};
	QVERIFY(await(f.controller.attach(u"ses_1"_s)));
	QVERIFY(f.controller.hasMoreBefore());
	QSignalSpy resets(&f.controller.model(), &QAbstractItemModel::modelReset);

	QVERIFY(await(f.controller.loadOlder()));

	QCOMPARE(render(f.controller.model()).first(), u"user:Earlier"_s);
	QVERIFY(!f.controller.hasMoreBefore());
	QVERIFY(!f.controller.isLoadingOlder());
	QCOMPARE(resets.count(), 0);
	const QJsonObject request = f.fake.requests().last();
	QCOMPARE(request.value(u"op"_s).toString(), u"messages"_s);
	QCOMPARE(request.value(u"before"_s).toString(), u"u1"_s);
}

void TestSessionController::doesNotAskWhenNothingIsOlder()
{
	Fixture f;
	QVERIFY(await(f.controller.attach(u"ses_1"_s)));
	QVERIFY(!f.controller.hasMoreBefore());

	QVERIFY(await(f.controller.loadOlder()));

	QVERIFY(!f.ops().contains(u"messages"_s));
}

void TestSessionController::showsSessionErrorsAtTheEndUntilTheNextPrompt()
{
	Fixture f;
	QVERIFY(await(f.controller.attach(u"ses_1"_s)));

	f.fake.send(eventFrame(u"ses_1"_s, {{u"type"_s, u"error"_s}, {u"text"_s, u"Provider rejected the request"_s}}));
	QTRY_COMPARE(f.controller.model().tailError(), u"Provider rejected the request"_s);
	QCOMPARE(render(f.controller.model()).last(), u"error:Provider rejected the request"_s);

	QVERIFY(await(f.controller.send(u"again"_s)));
	QVERIFY(f.controller.model().tailError().isEmpty());
	QCOMPARE(render(f.controller.model()).size(), 2);
}

QTEST_GUILESS_MAIN(TestSessionController)
#include "tst_session_controller.moc"
