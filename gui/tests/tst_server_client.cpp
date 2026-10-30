#include "client/server_client.h"
#include "coro_test.h"
#include "fake_backend.h"

#include <QJsonArray>
#include <QSignalSpy>
#include <QtTest>

using namespace Qt::StringLiterals;
using namespace std::chrono_literals;
using namespace refgui;
using namespace refgui::client;
using refgui::test::await;
using refgui::test::FakeBackend;

namespace
{

constexpr auto OpenTimeout = 2000ms;

QJsonObject promptResult(const QString& messageId)
{
	return {{u"queued"_s, false}, {u"message_id"_s, messageId}};
}

// A minimal attach result: one empty session.
QJsonObject attachResult()
{
	const QJsonObject info{{u"id"_s, u"ses_1"_s},
	                       {u"name"_s, u"one"_s},
	                       {u"title"_s, u""_s},
	                       {u"agent"_s, u"build"_s},
	                       {u"model"_s, QJsonObject{{u"provider"_s, u"p"_s}, {u"model"_s, u"m"_s}}},
	                       {u"cwd"_s, u"/tmp"_s},
	                       {u"created_at"_s, 1},
	                       {u"updated_at"_s, 1},
	                       {u"status"_s, QJsonObject{{u"state"_s, u"idle"_s}}},
	                       {u"attached"_s, 1},
	                       {u"live"_s, true}};
	return {{u"snapshot"_s,
	         QJsonObject{{u"info"_s, info}, {u"messages"_s, QJsonArray()}, {u"pending_permissions"_s, QJsonArray()}}}};
}

QJsonObject toastFrame(const QString& text)
{
	return {{u"kind"_s, u"toast"_s}, {u"level"_s, u"info"_s}, {u"text"_s, text}};
}

} // namespace

class TestServerClient : public QObject
{
	Q_OBJECT

private:
	Result<proto::HelloResult> open(ServerClient& client, const FakeBackend& backend)
	{
		return await(client.open({.url = backend.url(), .token = u"secret"_s}, u"/work"_s, OpenTimeout)).value();
	}

private slots:
	void helloCarriesTokenAndVersion()
	{
		FakeBackend backend;
		ServerClient client;

		const auto hello = open(client, backend);

		QVERIFY(hello.has_value());
		QCOMPARE(hello->protocol, 2);
		QCOMPARE(hello->serverVersion, u"0.0.0-test"_s);
		const QJsonObject sent = backend.requests().first();
		QCOMPARE(sent.value(u"op"_s).toString(), u"hello"_s);
		QCOMPARE(sent.value(u"version"_s).toInt(), proto::ProtocolVersion);
		QCOMPARE(sent.value(u"token"_s).toString(), u"secret"_s);
		QCOMPARE(sent.value(u"cwd"_s).toString(), u"/work"_s);
		QVERIFY(client.isOpen());
	}

	void rejectedHelloIsAnError()
	{
		FakeBackend backend;
		backend.onHello = [&](const QJsonObject& hello) { backend.fail(hello, u"invalid or missing token"_s); };
		ServerClient client;

		const auto hello = open(client, backend);

		QVERIFY(!hello.has_value());
		QCOMPARE(hello.error(), u"invalid or missing token"_s);
		QVERIFY(!client.isOpen());
	}

	void unreachableBackendIsAnError()
	{
		ServerClient client;
		const auto hello = await(client.open({.url = QUrl(u"ws://127.0.0.1:1/"_s)}, u"/work"_s, OpenTimeout)).value();
		QVERIFY(!hello.has_value());
		QVERIFY(!hello.error().isEmpty());
	}

	void responsesAreMatchedToRequestsById()
	{
		FakeBackend backend;
		QList<QJsonObject> received;
		backend.onRequest = [&](const QJsonObject& request) {
			received.append(request);
			if(received.size() < 2)
			{
				return;
			}
			// Answer in the opposite order.
			backend.respond(received.at(1), promptResult(u"msg_for_second"_s));
			backend.respond(received.at(0), promptResult(u"msg_for_first"_s));
		};
		ServerClient client;
		QVERIFY(open(client, backend));

		std::optional<Result<proto::PromptResult>> first;
		std::optional<Result<proto::PromptResult>> second;
		// Parameters, not captures: the closure of a coroutine lambda is gone after the call.
		const auto prompt = [](ServerClient& to, QString text, std::optional<Result<proto::PromptResult>>& out) -> QCoro::Task<> {
			out = co_await to.prompt(u"ses_1"_s, std::move(text));
		};
		prompt(client, u"one"_s, first);
		prompt(client, u"two"_s, second);
		QTRY_VERIFY(first && second);

		QCOMPARE((*first)->messageId, u"msg_for_first"_s);
		QCOMPARE((*second)->messageId, u"msg_for_second"_s);
	}

	void errorResponseCarriesTheBackendsText()
	{
		FakeBackend backend;
		backend.onRequest = [&](const QJsonObject& request) { backend.fail(request, u"Session not found"_s); };
		ServerClient client;
		QVERIFY(open(client, backend));

		const auto attached = await(client.attach(u"ses_nope"_s)).value();

		QVERIFY(!attached.has_value());
		QCOMPARE(attached.error(), u"Session not found"_s);
	}

	// The snapshot of an Attach must be applied before the events that follow it, which the
	// backend may send in the same breath.
	void responseIsHandledBeforeTheFrameThatFollowsIt()
	{
		FakeBackend backend;
		backend.onRequest = [&](const QJsonObject& request) {
			backend.respond(request, attachResult());
			backend.send(toastFrame(u"after"_s));
		};
		ServerClient client;
		QVERIFY(open(client, backend));

		QStringList order;
		connect(&client, &ServerClient::frameReceived, this, [&] { order << u"frame"_s; });
		std::optional<Result<proto::SessionSnapshot>> attached;
		[](ServerClient& to, std::optional<Result<proto::SessionSnapshot>>& out, QStringList& log) -> QCoro::Task<> {
			out = co_await to.attach(u"ses_1"_s);
			log << u"response"_s;
		}(client, attached, order);
		QTRY_COMPARE(order, (QStringList{u"response"_s, u"frame"_s}));
		QVERIFY(attached->has_value());
	}

	void framesAreDeliveredInArrivalOrder()
	{
		FakeBackend backend;
		ServerClient client;
		QVERIFY(open(client, backend));
		QStringList texts;
		connect(&client, &ServerClient::frameReceived, this, [&](const proto::ServerFrame& frame) { texts << std::get<proto::ToastFrame>(frame).text; });

		backend.send(toastFrame(u"a"_s));
		backend.send(toastFrame(u"b"_s));
		backend.send(toastFrame(u"c"_s));

		QTRY_COMPARE(texts, (QStringList{u"a"_s, u"b"_s, u"c"_s}));
	}

	void frameKindsFromTheFutureAreSkipped()
	{
		FakeBackend backend;
		ServerClient client;
		QVERIFY(open(client, backend));
		QSignalSpy frames(&client, &ServerClient::frameReceived);

		backend.send({{u"kind"_s, u"from_the_future"_s}});
		backend.send(toastFrame(u"still works"_s));

		QTRY_COMPARE(frames.size(), 1);
	}

	void closingTheConnectionFailsPendingRequests()
	{
		FakeBackend backend;
		backend.onRequest = [&](const QJsonObject&) { backend.socket()->close(); };
		ServerClient client;
		QVERIFY(open(client, backend));
		QSignalSpy closed(&client, &ServerClient::closed);

		const auto result = await(client.prompt(u"ses_1"_s, u"hi"_s)).value();

		QVERIFY(!result.has_value());
		QCOMPARE(result.error(), u"connection closed"_s);
		QTRY_COMPARE(closed.size(), 1);
		QVERIFY(!client.isOpen());
	}

	void requestsWithoutAConnectionFail()
	{
		ServerClient client;
		const auto result = await(client.prompt(u"ses_1"_s, u"hi"_s)).value();
		QVERIFY(!result.has_value());
		QCOMPARE(result.error(), u"not connected"_s);
	}

	void closeFailsPendingRequestsAtOnce()
	{
		FakeBackend backend;
		backend.onRequest = [](const QJsonObject&) {}; // never answers
		ServerClient client;
		QVERIFY(open(client, backend));

		std::optional<Result<proto::PromptResult>> result;
		[](ServerClient& to, std::optional<Result<proto::PromptResult>>& out) -> QCoro::Task<> {
			out = co_await to.prompt(u"ses_1"_s, u"hi"_s);
		}(client, result);
		QVERIFY(!result);

		client.close();

		QVERIFY(result); // resumed before close() returned
		QVERIFY(!result->has_value());
	}
};

QTEST_GUILESS_MAIN(TestServerClient)
#include "tst_server_client.moc"
