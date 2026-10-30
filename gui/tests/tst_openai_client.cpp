#include "fake_server.h"
#include "llm/openai_client.h"

#include <QCoroTask>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QTest>

using namespace Qt::StringLiterals;
using qutie::llm::Completion;
using qutie::llm::Endpoint;
using qutie::llm::OpenAiClient;
using qutie::llm::StopReason;
using qutie::llm::StreamEvent;
using qutie::llm::TextDelta;
using qutie::llm::UserMessage;

namespace
{

QByteArray textEvent(const QByteArray& text)
{
	return FakeServer::event(R"({"choices":[{"index":0,"delta":{"content":")"_ba + text + R"("}}]})"_ba);
}

const QByteArray StopEvent = FakeServer::event(R"({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]})"_ba);
const QByteArray DoneEvent = FakeServer::event("[DONE]"_ba);

// Collects the text deltas.
struct Recorder
{
	QStringList texts;

	std::function<void(const StreamEvent&)> callback()
	{
		return [this](const StreamEvent& event) {
			if(const auto* text = std::get_if<TextDelta>(&event))
			{
				texts.append(text->text);
			}
		};
	}
};

Completion complete(OpenAiClient& client, Recorder& recorder)
{
	return QCoro::waitFor(client.complete({UserMessage{u"hi"_s}}, recorder.callback(), std::stop_token()));
}

} // namespace

class TestOpenAiClient : public QObject
{
	Q_OBJECT

private slots:
	void streamsReply();
	void sendsRequest();
	void splitsAnywhere();
	void reportsHttpError();
	void reportsDroppedConnection();
	void reportsUnreachableServer();
	void stopCompletesAtOnce();
};

void TestOpenAiClient::streamsReply()
{
	FakeServer server;
	server.response.chunks = {textEvent("Hel"), textEvent("lo"), StopEvent, DoneEvent};
	OpenAiClient client(Endpoint{.baseUrl = server.baseUrl(), .model = u"m"_s});
	Recorder recorder;

	const Completion completion = complete(client, recorder);

	QCOMPARE(recorder.texts.join(QString()), u"Hello"_s);
	QCOMPARE(completion.message.text, u"Hello"_s);
	QVERIFY2(completion.end.has_value(), completion.end.has_value() ? "" : qPrintable(completion.end.error().message));
	QCOMPARE(*completion.end, StopReason::Stop);
}

void TestOpenAiClient::sendsRequest()
{
	FakeServer server;
	server.response.chunks = {StopEvent};
	OpenAiClient client(Endpoint{.baseUrl = server.baseUrl(), .model = u"m"_s, .apiKey = u"sk-test"_s});
	Recorder recorder;

	complete(client, recorder);

	QVERIFY2(server.lastHead().startsWith("POST /v1/chat/completions HTTP/1.1"), server.lastHead().constData());
	QVERIFY(server.lastHead().toLower().contains("authorization: bearer sk-test"));
	const QJsonObject body = QJsonDocument::fromJson(server.lastBody()).object();
	QCOMPARE(body.value("model"_L1).toString(), u"m"_s);
	QCOMPARE(body.value("messages"_L1).toArray().size(), 1);
}

// Events split across HTTP chunks, even inside a UTF-8 sequence, decode the same.
void TestOpenAiClient::splitsAnywhere()
{
	const QByteArray stream = textEvent("h\xc3\xa9") + StopEvent + DoneEvent; // "hé"
	FakeServer server;
	for(const char byte : stream)
	{
		server.response.chunks.append(QByteArray(1, byte));
	}
	OpenAiClient client(Endpoint{.baseUrl = server.baseUrl(), .model = u"m"_s});
	Recorder recorder;

	const Completion completion = complete(client, recorder);

	QCOMPARE(completion.message.text, u"hé"_s);
	QVERIFY(completion.end.has_value());
}

void TestOpenAiClient::reportsHttpError()
{
	FakeServer server;
	server.response = {.status = 500, .chunks = {R"({"error":{"message":"Model not loaded"}})"_ba}};
	OpenAiClient client(Endpoint{.baseUrl = server.baseUrl(), .model = u"m"_s});
	Recorder recorder;

	const Completion completion = complete(client, recorder);

	QVERIFY(!completion.end.has_value());
	QVERIFY2(completion.end.error().message.contains(u"500"_s), qPrintable(completion.end.error().message));
	QVERIFY(completion.end.error().message.contains(u"Model not loaded"_s));
	QVERIFY(recorder.texts.isEmpty());
}

void TestOpenAiClient::reportsDroppedConnection()
{
	FakeServer server;
	server.response = {.chunks = {textEvent("partial")}, .ending = FakeServer::Ending::Drop};
	OpenAiClient client(Endpoint{.baseUrl = server.baseUrl(), .model = u"m"_s});
	Recorder recorder;

	const Completion completion = complete(client, recorder);

	QVERIFY(!completion.end.has_value());
	QCOMPARE(completion.message.text, u"partial"_s);
}

void TestOpenAiClient::reportsUnreachableServer()
{
	quint16 closedPort = 0;
	{
		FakeServer server; // takes a free port, then closes it
		closedPort = static_cast<quint16>(server.baseUrl().port());
	}
	OpenAiClient client(Endpoint{.baseUrl = QUrl(u"http://127.0.0.1:%1/v1"_s.arg(closedPort)), .model = u"m"_s});
	Recorder recorder;

	const Completion completion = complete(client, recorder);

	QVERIFY(!completion.end.has_value());
	QVERIFY(completion.end.error().message.startsWith(u"The request failed"_s));
}

// The task must be done when request_stop() returns, so its owner may stop it from
// a destructor.
void TestOpenAiClient::stopCompletesAtOnce()
{
	FakeServer server;
	server.response = {.chunks = {textEvent("Hel")}, .ending = FakeServer::Ending::Hold};
	OpenAiClient client(Endpoint{.baseUrl = server.baseUrl(), .model = u"m"_s});
	Recorder recorder;
	std::stop_source stop;

	QCoro::Task<Completion> task = client.complete({UserMessage{u"hi"_s}}, recorder.callback(), stop.get_token());
	QTRY_VERIFY(!recorder.texts.isEmpty());
	QVERIFY(!task.isReady());

	stop.request_stop();
	QVERIFY(task.isReady());

	const Completion completion = QCoro::waitFor(task);
	QCOMPARE(completion.message.text, u"Hel"_s);
	QVERIFY(completion.end.has_value());
	QCOMPARE(*completion.end, StopReason::Cancelled);
}

QTEST_GUILESS_MAIN(TestOpenAiClient)
#include "tst_openai_client.moc"
