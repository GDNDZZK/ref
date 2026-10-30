// The requests the input box and the notifications send, and how their answers are read.
#include "client/server_client.h"
#include "coro_test.h"
#include "fake_backend.h"
#include "proto/parse.h"

#include <QJsonArray>
#include <QtTest>

using namespace Qt::StringLiterals;
using namespace std::chrono_literals;
using namespace refgui;
using namespace refgui::client;
using refgui::test::await;
using refgui::test::FakeBackend;
using Kind = proto::NotifyFrame::Kind;

namespace
{

constexpr auto OpenTimeout = 2000ms;

} // namespace

class TestComposerRequests : public QObject
{
	Q_OBJECT

private:
	// Answers every request with `result` and keeps the connection for the test.
	struct Connection
	{
		FakeBackend backend;
		ServerClient client;

		explicit Connection(QJsonObject result)
		{
			backend.onRequest = [this, result](const QJsonObject& request) { backend.respond(request, result); };
			const auto hello = await(client.open({.url = backend.url()}, u"/work"_s, OpenTimeout, {Kind::Permission, Kind::TurnError}));
			QVERIFY(hello && *hello);
		}

		QJsonObject last() const { return backend.requests().last(); }
	};

private slots:
	void helloNamesTheWantedNotifications()
	{
		Connection connection({});
		const QJsonArray kinds = connection.backend.requests().first().value(u"notify"_s).toArray();
		QCOMPARE(kinds, (QJsonArray{u"permission"_s, u"turn_error"_s}));
	}

	void promptCarriesImages()
	{
		Connection connection({{u"queued"_s, true}, {u"message_id"_s, u"m1"_s}});
		proto::ImageData image;
		image.mime = u"image/png"_s;
		image.data = u"YWJj"_s;

		const auto result = await(connection.client.prompt(u"ses_1"_s, u"look [Image 1]"_s, {image})).value();

		QVERIFY(result && result->queued);
		const QJsonObject sent = connection.last();
		QCOMPARE(sent.value(u"op"_s).toString(), u"prompt"_s);
		QCOMPARE(sent.value(u"text"_s).toString(), u"look [Image 1]"_s);
		const QJsonObject sentImage = sent.value(u"images"_s).toArray().first().toObject();
		QCOMPARE(sentImage.value(u"mime"_s).toString(), u"image/png"_s);
		QCOMPARE(sentImage.value(u"data"_s).toString(), u"YWJj"_s);
	}

	void removeQueuedReturnsTheTextAndImages()
	{
		const QJsonObject image{{u"mime"_s, u"image/png"_s}, {u"data"_s, u"YWJj"_s}, {u"name"_s, u"shot.png"_s}};
		Connection connection({{u"removed"_s, true}, {u"text"_s, u"fix [Image 1]"_s}, {u"images"_s, QJsonArray{image}}});

		const auto result = await(connection.client.removeQueued(u"ses_1"_s, u"m1"_s)).value();

		QVERIFY(result);
		QVERIFY(result->removed);
		QCOMPARE(result->text, u"fix [Image 1]"_s);
		QCOMPARE(result->images.size(), 1);
		QCOMPARE(result->images[0].name, u"shot.png"_s);
		QCOMPARE(connection.last().value(u"op"_s).toString(), u"remove_queued"_s);
		QCOMPARE(connection.last().value(u"message_id"_s).toString(), u"m1"_s);
	}

	void removeQueuedAfterDeliveryIsNotRemoved()
	{
		Connection connection({{u"removed"_s, false}, {u"text"_s, u""_s}, {u"images"_s, QJsonArray()}});
		const auto result = await(connection.client.removeQueued(u"ses_1"_s, u"m1"_s)).value();
		QVERIFY(result);
		QVERIFY(!result->removed);
		QVERIFY(result->images.isEmpty());
	}

	void shellSendsTheCommand()
	{
		Connection connection({});
		const auto result = await(connection.client.shell(u"ses_1"_s, u"ls -l"_s)).value();
		QVERIFY(result);
		QCOMPARE(connection.last().value(u"op"_s).toString(), u"shell"_s);
		QCOMPARE(connection.last().value(u"command"_s).toString(), u"ls -l"_s);
	}

	void listFilesReturnsPaths()
	{
		Connection connection({{u"files"_s, QJsonArray{u"src/main.rs"_s, u"src/"_s}}});
		const auto result = await(connection.client.listFiles(u"/work"_s, u"ma"_s, 20)).value();
		QVERIFY(result);
		QCOMPARE(*result, (QStringList{u"src/main.rs"_s, u"src/"_s}));
		QCOMPARE(connection.last().value(u"query"_s).toString(), u"ma"_s);
		QCOMPARE(connection.last().value(u"limit"_s).toInt(), 20);
		QCOMPARE(connection.last().value(u"cwd"_s).toString(), u"/work"_s);
	}

	void expandCommandReturnsThePromptText()
	{
		Connection connection({{u"text"_s, u"Review src/lib.rs"_s}});
		const auto result = await(connection.client.expandCommand(u"review"_s, u"src/lib.rs"_s, u"/work"_s)).value();
		QVERIFY(result);
		QCOMPARE(*result, u"Review src/lib.rs"_s);
		QCOMPARE(connection.last().value(u"name"_s).toString(), u"review"_s);
		QCOMPARE(connection.last().value(u"args"_s).toString(), u"src/lib.rs"_s);
	}

	void setNotifyAndFocusUseTheProtocolNames()
	{
		Connection connection({});
		QVERIFY(await(connection.client.setNotify({Kind::TurnDone})).value());
		QCOMPARE(connection.last().value(u"op"_s).toString(), u"set_notify"_s);
		QCOMPARE(connection.last().value(u"kinds"_s).toArray(), QJsonArray{u"turn_done"_s});

		QVERIFY(await(connection.client.setFocus(true, {u"a"_s, u"b"_s})).value());
		QCOMPARE(connection.last().value(u"op"_s).toString(), u"set_focus"_s);
		QCOMPARE(connection.last().value(u"focused"_s).toBool(), true);
		QCOMPARE(connection.last().value(u"sessions"_s).toArray(), (QJsonArray{u"a"_s, u"b"_s}));
	}

	void backendErrorsCarryTheirText()
	{
		FakeBackend backend;
		ServerClient client;
		backend.onRequest = [&](const QJsonObject& request) { backend.fail(request, u"Session not found"_s); };
		QVERIFY(await(client.open({.url = backend.url()}, u"/work"_s, OpenTimeout)).value());

		const auto result = await(client.removeQueued(u"nope"_s, u"m"_s)).value();
		QVERIFY(!result);
		QCOMPARE(result.error(), u"Session not found"_s);
	}
};

QTEST_GUILESS_MAIN(TestComposerRequests)
#include "tst_composer_requests.moc"
