#include "client/backend.h"
#include "coro_test.h"
#include "fake_backend.h"
#include "session/session_controller.h"
#include "session_test_data.h"

#include <QJsonArray>
#include <QSignalSpy>
#include <QtTest>

#include <memory>

using namespace Qt::StringLiterals;
using namespace refgui;
using namespace refgui::test;
using refgui::client::Backend;
using refgui::session::ConversationModel;
using refgui::session::SessionController;

namespace
{

constexpr int ReconnectTimeoutMs = 8000; // the backend retries after a second
constexpr int SettleMs = 50;              // time for a frame in flight to be handled

QJsonObject infoJson(const QString& id, const QString& state)
{
	return sessionInfoJson(id, {.name = u"one"_s, .title = u"A title"_s, .state = state, .attached = 1});
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

QJsonObject questionJson(const QString& callId, const QString& text)
{
	const QJsonObject option{{u"label"_s, u"Yes"_s}, {u"description"_s, u"Do it"_s}, {u"default"_s, true}};
	const QJsonObject question{{u"question"_s, text}, {u"header"_s, u"Ask"_s}, {u"options"_s, QJsonArray{option}}, {u"multiple"_s, false}, {u"custom"_s, true}};
	return {{u"session_id"_s, u"ses_1"_s}, {u"message_id"_s, u"a1"_s}, {u"call_id"_s, callId}, {u"questions"_s, QJsonArray{question}}, {u"time"_s, 1}};
}

QJsonObject questionFrame(const QString& callId, const QString& text)
{
	return eventFrame(u"ses_1"_s, {{u"type"_s, u"question"_s}, {u"request"_s, questionJson(callId, text)}});
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
		else if(op == u"prompt")
		{
			fake.respond(request, {{u"queued"_s, false}, {u"message_id"_s, u"u2"_s}});
		}
		else if(op == u"messages")
		{
			fake.respond(request, {{u"messages"_s, olderPage}, {u"more_before"_s, olderHasMore}});
		}
		else if(op == u"abort" || op == u"detach" || op == u"question_reply")
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
	void attachesAgainAfterReconnect();
	void passesOnErrors();
	void watcherAttachDoesNotTakeTheForeground();
	void upgradingAWatcherKeepsTheConversation();
	void detachedControllerIgnoresEventsAndAttachesAgain();
	void tracksTheQueuedPrompts();
	void pendingQuestionsComeWithTheSnapshot();
	void questionEventsAddAndResolvedOnesRemove();
	void questionReplyAsksTheBackend();
	void droppedQuestionIsForgotten();
	void loadsOlderMessagesInFront();
	void doesNotAskWhenNothingIsOlder();
	void showsSessionErrorsAtTheEndUntilTheNextPrompt();
	void attachAnsweredAfterTheControllerIsGone();
	void loadOlderAnsweredAfterTheControllerIsGone();
	void tracksActivelyWorkingAcrossStates();
	void debouncedRangeFetchDiscardsStaleAndEvictsOnDetach();
	void rangeFetchSurfacesErrorAndRetries();
	void retryNowRequestsBackend();
	void unlimitedWindowAutoLoadsAndSwitchingBackTrims();
	void followingDoesNotEvictMaterializedNonTailRange();
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
	QCOMPARE(result->error().message, u"no such session"_s);
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
	QTest::qWait(SettleMs);
	QCOMPARE(render(f.controller.model()), (QStringList{u"user:Hello"_s}));

	f.snapshot = snapshotJson(u"ses_1"_s, u"idle"_s, {messageJson(u"u1"_s, u"user"_s, u"Hello"_s, true), messageJson(u"u2"_s, u"user"_s, u"Missed"_s, true)});
	QVERIFY(await(f.controller.attach(u"ses_1"_s)));
	QCOMPARE(render(f.controller.model()), (QStringList{u"user:Hello"_s, u"user:Missed"_s}));
}

void TestSessionController::tracksTheQueuedPrompts()
{
	Fixture f;
	f.snapshot[u"snapshot"_s] = [&]
	{
		QJsonObject snapshot = f.snapshot.value(u"snapshot"_s).toObject();
		snapshot[u"queued"_s] = QJsonArray{messageJson(u"q1"_s, u"user"_s, u"First"_s, false)};
		return snapshot;
	}();
	QVERIFY(await(f.controller.attach(u"ses_1"_s)));
	QCOMPARE(f.controller.queued().size(), 1);
	QCOMPARE(f.controller.queued().first().id, u"q1"_s);

	QSignalSpy changed(&f.controller, &SessionController::queueChanged);
	f.fake.send(eventFrame(u"ses_1"_s, {{u"type"_s, u"queue"_s}, {u"messages"_s, QJsonArray()}}));
	QTRY_COMPARE(changed.count(), 1);
	QVERIFY(f.controller.queued().isEmpty());
}

void TestSessionController::pendingQuestionsComeWithTheSnapshot()
{
	Fixture f;
	QJsonObject snapshot = f.snapshot.value(u"snapshot"_s).toObject();
	snapshot[u"pending_questions"_s] = QJsonArray{questionJson(u"c1"_s, u"First?"_s)};
	f.snapshot[u"snapshot"_s] = snapshot;

	QVERIFY(await(f.controller.attach(u"ses_1"_s)));

	QCOMPARE(f.controller.pendingQuestions().size(), 1);
	QCOMPARE(f.controller.pendingQuestions().first().callId, u"c1"_s);
	QCOMPARE(f.controller.pendingQuestions().first().questions.first().question, u"First?"_s);
}

void TestSessionController::questionEventsAddAndResolvedOnesRemove()
{
	Fixture f;
	QVERIFY(await(f.controller.attach(u"ses_1"_s)));
	QSignalSpy changed(&f.controller, &SessionController::questionsChanged);

	f.fake.send(questionFrame(u"c1"_s, u"First?"_s));
	QTRY_COMPARE(changed.count(), 1);
	f.fake.send(questionFrame(u"c1"_s, u"First?"_s)); // a repeat changes nothing
	f.fake.send(questionFrame(u"c2"_s, u"Second?"_s));
	QTRY_COMPARE(changed.count(), 2);
	QCOMPARE(f.controller.pendingQuestions().size(), 2);
	QCOMPARE(f.controller.pendingQuestions().first().callId, u"c1"_s); // the oldest first

	f.fake.send(eventFrame(u"ses_1"_s, {{u"type"_s, u"question_resolved"_s}, {u"call_id"_s, u"c1"_s}}));
	QTRY_COMPARE(changed.count(), 3);
	QCOMPARE(f.controller.pendingQuestions().size(), 1);
	QCOMPARE(f.controller.pendingQuestions().first().callId, u"c2"_s);
}

void TestSessionController::questionReplyAsksTheBackend()
{
	Fixture f;
	QVERIFY(await(f.controller.attach(u"ses_1"_s)));

	const auto answered = await(f.controller.questionReply(u"c1"_s, QList<QStringList>{{u"Yes"_s}, {}}));

	QVERIFY(answered && answered->has_value());
	const QJsonObject request = f.fake.requests().last();
	QCOMPARE(request.value(u"op"_s).toString(), u"question_reply"_s);
	QCOMPARE(request.value(u"session_id"_s).toString(), u"ses_1"_s);
	QCOMPARE(request.value(u"call_id"_s).toString(), u"c1"_s);
	QCOMPARE(request.value(u"answers"_s).toArray(), (QJsonArray{QJsonArray{u"Yes"_s}, QJsonArray{}}));

	QVERIFY(await(f.controller.questionReply(u"c1"_s, std::nullopt)));
	QVERIFY(f.fake.requests().last().value(u"answers"_s).isNull()); // dismissed
}

void TestSessionController::droppedQuestionIsForgotten()
{
	Fixture f;
	QVERIFY(await(f.controller.attach(u"ses_1"_s)));
	f.fake.send(questionFrame(u"c1"_s, u"First?"_s));
	QTRY_COMPARE(f.controller.pendingQuestions().size(), 1);
	QSignalSpy changed(&f.controller, &SessionController::questionsChanged);

	f.controller.dropQuestion(u"other"_s);
	QVERIFY(changed.isEmpty());
	f.controller.dropQuestion(u"c1"_s);

	QCOMPARE(changed.count(), 1);
	QVERIFY(f.controller.pendingQuestions().isEmpty());
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

// An answer that arrives after the controller was destroyed must not touch it. The test passes without a
// sanitizer too, but then the waiting caller must at least be told that the request came to nothing.
void TestSessionController::attachAnsweredAfterTheControllerIsGone()
{
	Fixture f;
	std::optional<SessionController::Result<void>> outcome;
	QJsonObject held;
	f.fake.onRequest = [&](const QJsonObject& request) { held = request; };
	auto gone = std::make_unique<SessionController>(f.backend);
	[](SessionController& controller, std::optional<SessionController::Result<void>>& out) -> QCoro::Task<> {
		out = co_await controller.attach(u"ses_1"_s);
	}(*gone, outcome);
	QTRY_VERIFY(!held.isEmpty());

	gone.reset();
	f.fake.respond(held, f.snapshot);

	QTRY_VERIFY(outcome.has_value());
	QVERIFY(!*outcome);
}

void TestSessionController::loadOlderAnsweredAfterTheControllerIsGone()
{
	Fixture f;
	f.snapshot[u"snapshot"_s] = [&] {
		QJsonObject snapshot = f.snapshot.value(u"snapshot"_s).toObject();
		snapshot[u"more_before"_s] = true;
		return snapshot;
	}();
	auto gone = std::make_unique<SessionController>(f.backend);
	QVERIFY(await(gone->attach(u"ses_1"_s)));
	std::optional<SessionController::Result<void>> outcome;
	QJsonObject held;
	f.fake.onRequest = [&](const QJsonObject& request) { held = request; };
	[](SessionController& controller, std::optional<SessionController::Result<void>>& out) -> QCoro::Task<> {
		out = co_await controller.loadOlder();
	}(*gone, outcome);
	QTRY_VERIFY(!held.isEmpty());

	gone.reset();
	f.fake.respond(held, {{u"messages"_s, QJsonArray()}, {u"more_before"_s, false}});

	QTRY_VERIFY(outcome.has_value());
	QVERIFY(!*outcome);
}

void TestSessionController::tracksActivelyWorkingAcrossStates()
{
	Fixture f;
	QVERIFY(await(f.controller.attach(u"ses_1"_s)));
	QVERIFY(!f.controller.isActivelyWorking());
	QVERIFY(!f.controller.isBusy());

	QSignalSpy working(&f.controller, &SessionController::activelyWorkingChanged);

	// Idle -> Busy: actively working.
	f.fake.send(infoFrame(u"ses_1"_s, u"busy"_s));
	QTRY_VERIFY(f.controller.isActivelyWorking());
	QCOMPARE(working.count(), 1);

	// Busy -> WaitingPermission: stops actively working while paused on user, though session stays busy.
	f.fake.send(infoFrame(u"ses_1"_s, u"waiting_permission"_s));
	QTRY_VERIFY(!f.controller.isActivelyWorking());
	QVERIFY(f.controller.isBusy());
	QVERIFY(f.controller.isTurnInProgress());
	QCOMPARE(working.count(), 2);

	// WaitingPermission -> Running: actively working resumes for tool execution.
	f.fake.send(infoFrame(u"ses_1"_s, u"running"_s));
	QTRY_VERIFY(f.controller.isActivelyWorking());
	QCOMPARE(working.count(), 3);

	// Running -> WaitingQuestion -> Retrying -> Compacting -> Idle.
	f.fake.send(infoFrame(u"ses_1"_s, u"waiting_question"_s));
	QTRY_VERIFY(!f.controller.isActivelyWorking());
	QVERIFY(f.controller.isBusy());
	QCOMPARE(working.count(), 4);

	f.fake.send(infoFrame(u"ses_1"_s, u"retrying"_s));
	QTest::qWait(SettleMs);
	QVERIFY(!f.controller.isActivelyWorking());
	QVERIFY(f.controller.isBusy());
	QCOMPARE(working.count(), 4);

	f.fake.send(infoFrame(u"ses_1"_s, u"compacting"_s));
	QTRY_VERIFY(f.controller.isActivelyWorking());
	QCOMPARE(working.count(), 5);

	f.fake.send(infoFrame(u"ses_1"_s, u"idle"_s));
	QTRY_VERIFY(!f.controller.isActivelyWorking());
	QVERIFY(!f.controller.isBusy());
	QCOMPARE(working.count(), 6);

	// Pending paced text keeps activelyWorking true until the stream drains.
	f.fake.send(messageFrame(u"ses_1"_s, messageJson(u"u2"_s, u"user"_s, u"Hi"_s, true)));
	f.fake.send(infoFrame(u"ses_1"_s, u"busy"_s));
	f.fake.send(messageFrame(u"ses_1"_s, messageJson(u"a2"_s, u"assistant"_s, QString(), false)));
	f.fake.send(deltaFrame(u"ses_1"_s, u"a2"_s, 0, QString(200, u'x')));
	f.fake.send(infoFrame(u"ses_1"_s, u"waiting_permission"_s));
	QTRY_VERIFY(f.controller.isBusy());
	QVERIFY(f.controller.isActivelyWorking());
	QTRY_VERIFY(!f.controller.isActivelyWorking());
	QVERIFY(f.controller.isBusy());
}

void TestSessionController::debouncedRangeFetchDiscardsStaleAndEvictsOnDetach()
{
	Fixture f;
	constexpr int Total = 1500;
	constexpr int StartSeq = 500;
	QJsonArray metas;
	for(int i = 0; i < StartSeq; ++i)
	{
		metas.append(QJsonObject{{u"seq"_s, i},
		                         {u"id"_s, u"u_%1"_s.arg(i)},
		                         {u"role"_s, u"user"_s},
		                         {u"source_kind"_s, u"human"_s},
		                         {u"has_error"_s, false},
		                         {u"parts"_s, QJsonArray{QJsonObject{{u"kind"_s, u"text"_s}, {u"chars"_s, 20}, {u"lines"_s, 1}}}}});
	}
	QJsonArray msgs;
	for(int i = StartSeq; i < Total; ++i)
	{
		msgs.append(messageJson(u"u_%1"_s.arg(i), u"user"_s, u"msg"_s, true));
	}
	f.snapshot = {{u"snapshot"_s,
	               QJsonObject{{u"info"_s, infoJson(u"ses_1"_s, u"idle"_s)},
	                           {u"messages"_s, msgs},
	                           {u"more_before"_s, true},
	                           {u"total_messages"_s, Total},
	                           {u"start_seq"_s, StartSeq},
	                           {u"layout_metas"_s, metas}}}};

	QVERIFY(await(f.controller.attach(u"ses_1"_s)));
	QCOMPARE(f.controller.model().slotCount(), Total);
	QCOMPARE(f.controller.model().residentCount(), 1000);
	QVERIFY(!f.controller.hasMoreBefore());

	QList<QJsonObject> heldMessages;
	f.fake.onRequest = [&](const QJsonObject& request) {
		if(request.value(u"op"_s).toString() == u"messages"_s)
		{
			heldMessages.append(request);
			return;
		}
		f.answer(request);
	};

	QSignalSpy evictedSpy(&f.controller, &SessionController::messagesEvicted);
	f.controller.setFollowing(false);
	// Rapidly scrub twice inside 80ms debounce window: only one request fires.
	// setVisibleSeqRange(10, 20) immediately evicts slots outside [0, 520], leaving 500..520 (21 messages).
	f.controller.setVisibleSeqRange(10, 20);
	QCOMPARE(evictedSpy.count(), 1);
	QCOMPARE(f.controller.model().residentCount(), 21);
	QTest::qWait(20);
	f.controller.setVisibleSeqRange(30, 40);
	QTRY_COMPARE(heldMessages.size(), 1);
	const QJsonObject firstReq = heldMessages.takeFirst();

	// Scrub to [0, 5] after first request is in-flight: evicts 506..520 (leaving 500..505 = 6) and increments generation.
	f.controller.setVisibleSeqRange(0, 5);
	QCOMPARE(evictedSpy.count(), 2);
	QCOMPARE(f.controller.model().residentCount(), 6);
	QTRY_COMPARE(heldMessages.size(), 1);
	const QJsonObject secondReq = heldMessages.takeFirst();

	const quint64 firstStart = static_cast<quint64>(firstReq.value(u"start_seq"_s).toInteger());
	QJsonArray stalePayload{messageJson(u"u_stale"_s, u"user"_s, u"stale"_s, true)};
	f.fake.respond(firstReq, {{u"messages"_s, stalePayload}, {u"more_before"_s, false}, {u"start_seq"_s, static_cast<qint64>(firstStart)}, {u"end_seq"_s, static_cast<qint64>(firstStart + 1)}});
	QTest::qWait(SettleMs);
	// Stale response must be ignored: u_stale must not appear.
	QVERIFY(!render(f.controller.model()).contains(u"user:stale"_s));

	// Respond to the current (second) request (0..500) -> materializes 500 messages (+ 6 in 500..505 = 506).
	const quint64 secondStart = static_cast<quint64>(secondReq.value(u"start_seq"_s).toInteger());
	const quint64 secondEnd = static_cast<quint64>(secondReq.value(u"end_seq"_s).toInteger());
	QJsonArray validPayload;
	for(quint64 s = secondStart; s < secondEnd; ++s)
	{
		validPayload.append(messageJson(u"u_%1"_s.arg(s), u"user"_s, u"loaded_%1"_s.arg(s), true));
	}
	f.fake.respond(secondReq, {{u"messages"_s, validPayload}, {u"more_before"_s, false}, {u"start_seq"_s, static_cast<qint64>(secondStart)}, {u"end_seq"_s, static_cast<qint64>(secondEnd)}});
	QTRY_VERIFY(render(f.controller.model()).contains(u"user:loaded_0"_s));
	QCOMPARE(f.controller.model().residentCount(), 506);

	// Detach calls evictOldTail(1000), evicting slots 0..499 (the 500 loaded messages) and emitting messagesEvicted.
	QVERIFY(await(f.controller.detach()));
	QCOMPARE(evictedSpy.count(), 3);
	QCOMPARE(evictedSpy.last().at(0).toStringList().size(), 500);
	QCOMPARE(f.controller.model().residentCount(), 6);
	QCOMPARE(f.controller.model().index(0).data(ConversationModel::KindRole).toString(), u"skeleton"_s);
}

void TestSessionController::rangeFetchSurfacesErrorAndRetries()
{
	Fixture f;
	constexpr int Total = 220;
	constexpr int StartSeq = 120;
	QJsonArray metas;
	for(int i = 0; i < StartSeq; ++i)
	{
		metas.append(QJsonObject{{u"seq"_s, i},
		                         {u"id"_s, u"u_%1"_s.arg(i)},
		                         {u"role"_s, u"user"_s},
		                         {u"source_kind"_s, u"human"_s},
		                         {u"has_error"_s, false},
		                         {u"parts"_s, QJsonArray{QJsonObject{{u"kind"_s, u"text"_s}, {u"chars"_s, 20}, {u"lines"_s, 1}}}}});
	}
	QJsonArray msgs;
	for(int i = StartSeq; i < Total; ++i)
	{
		msgs.append(messageJson(u"u_%1"_s.arg(i), u"user"_s, u"msg"_s, true));
	}
	f.snapshot = {{u"snapshot"_s,
	               QJsonObject{{u"info"_s, infoJson(u"ses_1"_s, u"idle"_s)},
	                           {u"messages"_s, msgs},
	                           {u"more_before"_s, true},
	                           {u"total_messages"_s, Total},
	                           {u"start_seq"_s, StartSeq},
	                           {u"layout_metas"_s, metas}}}};

	QVERIFY(await(f.controller.attach(u"ses_1"_s)));
	QSignalSpy errorSpy(&f.controller, &SessionController::errorReported);
	QSignalSpy rangeErrSpy(&f.controller, &SessionController::rangeErrorChanged);

	int rangeAttempts = 0;
	f.fake.onRequest = [&](const QJsonObject& request) {
		if(request.value(u"op"_s).toString() == u"messages"_s)
		{
			++rangeAttempts;
			if(rangeAttempts == 1)
			{
				f.fake.fail(request, u"range read failed"_s, u"internal"_s);
				return;
			}
			const quint64 s = static_cast<quint64>(request.value(u"start_seq"_s).toInteger());
			const quint64 e = static_cast<quint64>(request.value(u"end_seq"_s).toInteger());
			QJsonArray payload;
			for(quint64 k = s; k < e; ++k)
			{
				payload.append(messageJson(u"u_%1"_s.arg(k), u"user"_s, u"retried_%1"_s.arg(k), true));
			}
			f.fake.respond(request, {{u"messages"_s, payload}, {u"more_before"_s, false}, {u"start_seq"_s, static_cast<qint64>(s)}, {u"end_seq"_s, static_cast<qint64>(e)}});
			return;
		}
		f.answer(request);
	};

	f.controller.setFollowing(false);
	f.controller.setVisibleSeqRange(0, 5);
	QTRY_COMPARE(f.controller.rangeError(), u"range read failed"_s);
	QCOMPARE(errorSpy.count(), 1);
	QCOMPARE(rangeErrSpy.count(), 1);

	f.controller.retryRangeFetch();
	QTRY_VERIFY(f.controller.rangeError().isEmpty());
	QTRY_VERIFY(render(f.controller.model()).contains(u"user:retried_0"_s));
}

void TestSessionController::retryNowRequestsBackend()
{
	Fixture f;
	QVERIFY(await(f.controller.attach(u"ses_1"_s)));
	bool sawRetryNow = false;
	f.fake.onRequest = [&](const QJsonObject& request) {
		if(request.value(u"op"_s).toString() == u"retry_now"_s)
		{
			sawRetryNow = true;
			f.fake.respond(request, {{u"retried"_s, true}});
			return;
		}
		f.answer(request);
	};
	const auto res = await(f.controller.retryNow());
	QVERIFY(res && res->has_value());
	QVERIFY((*res)->retried);
	QVERIFY(sawRetryNow);
}

void TestSessionController::unlimitedWindowAutoLoadsAndSwitchingBackTrims()
{
	Fixture f;
	constexpr int Total = 2500;
	constexpr int StartSeq = 1500;
	QJsonArray metas;
	for(int i = 0; i < StartSeq; ++i)
	{
		metas.append(QJsonObject{{u"seq"_s, i},
		                         {u"id"_s, u"u_%1"_s.arg(i)},
		                         {u"role"_s, u"user"_s},
		                         {u"source_kind"_s, u"human"_s},
		                         {u"has_error"_s, false},
		                         {u"parts"_s, QJsonArray{QJsonObject{{u"kind"_s, u"text"_s}, {u"chars"_s, 20}, {u"lines"_s, 1}}}}});
	}
	QJsonArray msgs;
	for(int i = StartSeq; i < Total; ++i)
	{
		msgs.append(messageJson(u"u_%1"_s.arg(i), u"user"_s, u"msg_%1"_s.arg(i), true));
	}
	f.snapshot = {{u"snapshot"_s,
	               QJsonObject{{u"info"_s, infoJson(u"ses_1"_s, u"idle"_s)},
	                           {u"messages"_s, msgs},
	                           {u"more_before"_s, true},
	                           {u"total_messages"_s, Total},
	                           {u"start_seq"_s, StartSeq},
	                           {u"layout_metas"_s, metas}}}};

	QList<std::pair<quint64, quint64>> rangeRequests;
	bool firstShortPageDone = false;
	f.fake.onRequest = [&](const QJsonObject& request) {
		if(request.value(u"op"_s).toString() == u"messages"_s)
		{
			const quint64 s = static_cast<quint64>(request.value(u"start_seq"_s).toInteger());
			const quint64 e = static_cast<quint64>(request.value(u"end_seq"_s).toInteger());
			rangeRequests.append({s, e});
			// Simulate a short byte-capped server page on the first request [500, 1500) -> only [500, 800).
			const quint64 actualEnd = (!firstShortPageDone && s == 500 && e == 1500) ? 800 : e;
			if(s == 500 && e == 1500)
			{
				firstShortPageDone = true;
			}
			QJsonArray payload;
			for(quint64 k = s; k < actualEnd; ++k)
			{
				payload.append(messageJson(u"u_%1"_s.arg(k), u"user"_s, u"loaded_%1"_s.arg(k), true));
			}
			f.fake.respond(request, {{u"messages"_s, payload}, {u"more_before"_s, s > 0}, {u"start_seq"_s, static_cast<qint64>(s)}, {u"end_seq"_s, static_cast<qint64>(actualEnd)}});
			return;
		}
		f.answer(request);
	};

	QVERIFY(await(f.controller.attach(u"ses_1"_s)));
	QCOMPARE(f.controller.model().residentCount(), 1000);
	QVERIFY(f.controller.model().hasPlaceholders());

	QSignalSpy evictedSpy(&f.controller, &SessionController::messagesEvicted);
	f.controller.setMessageWindowSize(0);
	QTRY_COMPARE(f.controller.model().residentCount(), Total);
	QVERIFY(!f.controller.model().hasPlaceholders());
	QCOMPARE(rangeRequests.size(), 3);
	QCOMPARE(rangeRequests.at(0), (std::pair<quint64, quint64>{500, 1500}));
	QCOMPARE(rangeRequests.at(1), (std::pair<quint64, quint64>{800, 1500}));
	QCOMPARE(rangeRequests.at(2), (std::pair<quint64, quint64>{0, 500}));
	QCOMPARE(evictedSpy.count(), 0);

	// Scrolling and detaching must not evict when unlimited.
	f.controller.setFollowing(false);
	f.controller.setVisibleSeqRange(0, 5);
	QTest::qWait(SettleMs);
	QCOMPARE(f.controller.model().residentCount(), Total);
	QCOMPARE(evictedSpy.count(), 0);

	QVERIFY(await(f.controller.detach()));
	QCOMPARE(f.controller.model().residentCount(), Total);
	QCOMPARE(evictedSpy.count(), 0);

	// Re-attaching while unlimited auto-loads placeholders again.
	rangeRequests.clear();
	firstShortPageDone = false;
	QVERIFY(await(f.controller.attach(u"ses_1"_s)));
	QTRY_COMPARE(f.controller.model().residentCount(), Total);
	QCOMPARE(rangeRequests.size(), 3);

	// Switching back to a positive size while following trims to the new window size.
	f.controller.setFollowing(true);
	f.controller.setMessageWindowSize(200);
	QCOMPARE(f.controller.model().residentCount(), 200);
	QVERIFY(f.controller.model().hasPlaceholders());
	QCOMPARE(f.controller.model().index(0).data(ConversationModel::KindRole).toString(), u"skeleton"_s);
	QCOMPARE(evictedSpy.count(), 1);
}

void TestSessionController::followingDoesNotEvictMaterializedNonTailRange()
{
	Fixture f;
	constexpr int Total = 1500;
	constexpr int StartSeq = 500;
	QJsonArray metas;
	for(int i = 0; i < StartSeq; ++i)
	{
		metas.append(QJsonObject{{u"seq"_s, i},
		                         {u"id"_s, u"u_%1"_s.arg(i)},
		                         {u"role"_s, u"user"_s},
		                         {u"source_kind"_s, u"human"_s},
		                         {u"has_error"_s, false},
		                         {u"parts"_s, QJsonArray{QJsonObject{{u"kind"_s, u"text"_s}, {u"chars"_s, 20}, {u"lines"_s, 1}}}}});
	}
	QJsonArray msgs;
	for(int i = StartSeq; i < Total; ++i)
	{
		msgs.append(messageJson(u"u_%1"_s.arg(i), u"user"_s, u"msg_%1"_s.arg(i), true));
	}
	f.snapshot = {{u"snapshot"_s,
	               QJsonObject{{u"info"_s, infoJson(u"ses_1"_s, u"idle"_s)},
	                           {u"messages"_s, msgs},
	                           {u"more_before"_s, true},
	                           {u"total_messages"_s, Total},
	                           {u"start_seq"_s, StartSeq},
	                           {u"layout_metas"_s, metas}}}};

	f.fake.onRequest = [&](const QJsonObject& request) {
		if(request.value(u"op"_s).toString() == u"messages"_s)
		{
			const quint64 s = static_cast<quint64>(request.value(u"start_seq"_s).toInteger());
			const quint64 e = static_cast<quint64>(request.value(u"end_seq"_s).toInteger());
			QJsonArray payload;
			for(quint64 k = s; k < e; ++k)
			{
				payload.append(messageJson(u"u_%1"_s.arg(k), u"user"_s, u"loaded_%1"_s.arg(k), true));
			}
			f.fake.respond(request, {{u"messages"_s, payload}, {u"more_before"_s, s > 0}, {u"start_seq"_s, static_cast<qint64>(s)}, {u"end_seq"_s, static_cast<qint64>(e)}});
			return;
		}
		f.answer(request);
	};

	QVERIFY(await(f.controller.attach(u"ses_1"_s)));
	QCOMPARE(f.controller.model().residentCount(), 1000);
	QVERIFY(f.controller.model().hasPlaceholders());

	QSignalSpy evictedSpy(&f.controller, &SessionController::messagesEvicted);
	QSignalSpy materializedSpy(&f.controller, &SessionController::materialized);
	f.controller.setFollowing(true);

	// Fetching [0, 500) while following stays true must not immediately evict the newly materialized slots.
	f.controller.setVisibleSeqRange(100, 120);
	QTRY_COMPARE(materializedSpy.count(), 1);
	QCOMPARE(f.controller.model().residentCount(), Total);
	const int row100 = f.controller.model().firstRowForSeq(100);
	QVERIFY(row100 >= 0);
	QCOMPARE(f.controller.model().index(row100).data(ConversationModel::KindRole).toString(), u"user"_s);
	QCOMPARE(evictedSpy.count(), 0);

	// Re-reporting an already-materialized non-tail range while following must not evict it either.
	f.controller.setVisibleSeqRange(100, 120);
	QCOMPARE(f.controller.model().residentCount(), Total);
	QCOMPARE(evictedSpy.count(), 0);

	// Only reporting the tail while following triggers evictOldTail.
	f.controller.setVisibleSeqRange(1490, 1499);
	QCOMPARE(f.controller.model().residentCount(), 1000);
	QCOMPARE(evictedSpy.count(), 1);
	QCOMPARE(f.controller.model().index(0).data(ConversationModel::KindRole).toString(), u"skeleton"_s);
}

QTEST_GUILESS_MAIN(TestSessionController)
#include "tst_session_controller.moc"
