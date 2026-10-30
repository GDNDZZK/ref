#include "llm/openai_wire.h"

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QTest>

using namespace Qt::StringLiterals;
namespace openai = qutie::llm::openai;
using qutie::llm::AssistantMessage;
using qutie::llm::ReasoningDelta;
using qutie::llm::StopReason;
using qutie::llm::StreamEvent;
using qutie::llm::TextDelta;
using qutie::llm::UserMessage;

namespace
{

// Renders events as "r:text" for reasoning and "t:text" for text.
QStringList render(const QList<StreamEvent>& events)
{
	QStringList rendered;
	for(const StreamEvent& event : events)
	{
		if(const auto* text = std::get_if<TextDelta>(&event))
		{
			rendered.append(u"t:"_s + text->text);
		}
		else
		{
			rendered.append(u"r:"_s + std::get<ReasoningDelta>(event).text);
		}
	}
	return rendered;
}

QStringList decodeAll(openai::StreamDecoder& decoder, const QByteArrayList& events)
{
	QStringList rendered;
	for(const QByteArray& data : events)
	{
		rendered += render(decoder.decode(data));
	}
	return rendered;
}

QJsonArray messagesOf(const QByteArray& body)
{
	return QJsonDocument::fromJson(body).object().value("messages"_L1).toArray();
}

} // namespace

class TestOpenAiWire : public QObject
{
	Q_OBJECT

private slots:
	void requestHasModelAndStreams();
	void requestSendsReasoningBackInItsField();
	void requestSkipsAssistantMessagesWithoutText();

	void decodesTextAndReasoning();
	void takesFirstReasoningFieldOnly();
	void mapsFinishReasons_data();
	void mapsFinishReasons();
	void ignoresEverythingAfterTheOutcome();
	void reportsErrorEvent();
	void reportsMalformedEvent();
	void noOutcomeWithoutFinishReason();

	void extractsErrorMessage_data();
	void extractsErrorMessage();
};

void TestOpenAiWire::requestHasModelAndStreams()
{
	const QJsonObject body = QJsonDocument::fromJson(openai::requestBody(u"m1"_s, {UserMessage{u"hi"_s}})).object();

	QCOMPARE(body.value("model"_L1).toString(), u"m1"_s);
	QCOMPARE(body.value("stream"_L1).toBool(), true);
	const QJsonArray messages = body.value("messages"_L1).toArray();
	QCOMPARE(messages.size(), 1);
	QCOMPARE(messages.at(0).toObject().value("role"_L1).toString(), u"user"_s);
	QCOMPARE(messages.at(0).toObject().value("content"_L1).toString(), u"hi"_s);
}

void TestOpenAiWire::requestSendsReasoningBackInItsField()
{
	const AssistantMessage reply{.text = u"Hello"_s, .reasoning = u"Greet back."_s, .reasoningField = u"reasoning"_s};
	const QJsonArray messages = messagesOf(openai::requestBody(u"m"_s, {UserMessage{u"hi"_s}, reply, UserMessage{u"again"_s}}));

	QCOMPARE(messages.size(), 3);
	const QJsonObject assistant = messages.at(1).toObject();
	QCOMPARE(assistant.value("role"_L1).toString(), u"assistant"_s);
	QCOMPARE(assistant.value("content"_L1).toString(), u"Hello"_s);
	QCOMPARE(assistant.value("reasoning"_L1).toString(), u"Greet back."_s);
	QVERIFY(!assistant.contains("reasoning_content"_L1));
}

void TestOpenAiWire::requestSkipsAssistantMessagesWithoutText()
{
	const AssistantMessage stoppedWhileReasoning{.reasoning = u"Hmm"_s, .reasoningField = u"reasoning_content"_s};
	const QJsonArray messages = messagesOf(openai::requestBody(u"m"_s, {UserMessage{u"a"_s}, stoppedWhileReasoning, UserMessage{u"b"_s}}));

	QCOMPARE(messages.size(), 2);
	QCOMPARE(messages.at(1).toObject().value("content"_L1).toString(), u"b"_s);
}

void TestOpenAiWire::decodesTextAndReasoning()
{
	openai::StreamDecoder decoder;
	const QStringList events = decodeAll(decoder,
		{
			R"({"choices":[{"index":0,"delta":{"role":"assistant","content":null,"reasoning_content":"Think"}}]})"_ba,
			R"({"choices":[{"index":0,"delta":{"content":"Hel"}}]})"_ba,
			R"({"choices":[{"index":0,"delta":{"content":"lo"}}]})"_ba,
			R"({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]})"_ba,
			R"({"choices":[],"usage":{"prompt_tokens":5,"completion_tokens":3,"total_tokens":8}})"_ba,
			"[DONE]"_ba,
		});

	QCOMPARE(events, (QStringList{u"r:Think"_s, u"t:Hel"_s, u"t:lo"_s}));
	QCOMPARE(decoder.message().text, u"Hello"_s);
	QCOMPARE(decoder.message().reasoning, u"Think"_s);
	QCOMPARE(decoder.message().reasoningField, u"reasoning_content"_s);
	QVERIFY(decoder.outcome().has_value());
	QCOMPARE(decoder.outcome()->value(), StopReason::Stop);
}

// Some servers send the same text in two fields; counting both would double it.
void TestOpenAiWire::takesFirstReasoningFieldOnly()
{
	openai::StreamDecoder decoder;
	const QStringList events = decodeAll(decoder,
		{
			R"({"choices":[{"delta":{"reasoning_content":"a","reasoning":"a"}}]})"_ba,
			R"({"choices":[{"delta":{"reasoning_content":"","reasoning":"b"}}]})"_ba,
		});

	QCOMPARE(events, (QStringList{u"r:a"_s, u"r:b"_s}));
	QCOMPARE(decoder.message().reasoning, u"ab"_s);
	QCOMPARE(decoder.message().reasoningField, u"reasoning_content"_s); // the first one seen
}

void TestOpenAiWire::mapsFinishReasons_data()
{
	// StopReason as int: it is not a registered meta type.
	QTest::addColumn<QString>("finishReason");
	QTest::addColumn<bool>("isError");
	QTest::addColumn<int>("stopReason");

	QTest::newRow("stop") << u"stop"_s << false << static_cast<int>(StopReason::Stop);
	QTest::newRow("end") << u"end"_s << false << static_cast<int>(StopReason::Stop);
	QTest::newRow("length") << u"length"_s << false << static_cast<int>(StopReason::Length);
	QTest::newRow("content_filter") << u"content_filter"_s << false << static_cast<int>(StopReason::ContentFilter);
	QTest::newRow("tool_calls") << u"tool_calls"_s << true << 0;
	QTest::newRow("unknown") << u"weird"_s << true << 0;
}

void TestOpenAiWire::mapsFinishReasons()
{
	QFETCH(QString, finishReason);
	QFETCH(bool, isError);
	QFETCH(int, stopReason);

	openai::StreamDecoder decoder;
	decoder.decode(R"({"choices":[{"delta":{},"finish_reason":")"_ba + finishReason.toUtf8() + R"("}]})"_ba);

	QVERIFY(decoder.outcome().has_value());
	QCOMPARE(!decoder.outcome()->has_value(), isError);
	if(isError)
	{
		QVERIFY(decoder.outcome()->error().message.contains(finishReason));
	}
	else
	{
		QCOMPARE(decoder.outcome()->value(), static_cast<StopReason>(stopReason));
	}
}

void TestOpenAiWire::ignoresEverythingAfterTheOutcome()
{
	openai::StreamDecoder decoder;
	decodeAll(decoder, {R"({"choices":[{"delta":{"content":"a"},"finish_reason":"stop"}]})"_ba});
	const QStringList late = decodeAll(decoder, {R"({"choices":[{"delta":{"content":"b"}}]})"_ba, "not json"_ba});

	QVERIFY(late.isEmpty());
	QCOMPARE(decoder.message().text, u"a"_s);
	QCOMPARE(decoder.outcome()->value(), StopReason::Stop);
}

void TestOpenAiWire::reportsErrorEvent()
{
	openai::StreamDecoder decoder;
	decodeAll(decoder,
		{
			R"({"choices":[{"delta":{"content":"partial"}}]})"_ba,
			R"({"error":{"message":"Overloaded","type":"server_error"}})"_ba,
		});

	QVERIFY(decoder.outcome().has_value());
	QVERIFY(!decoder.outcome()->has_value());
	QVERIFY(decoder.outcome()->error().message.contains(u"Overloaded"_s));
	QCOMPARE(decoder.message().text, u"partial"_s);
}

void TestOpenAiWire::reportsMalformedEvent()
{
	openai::StreamDecoder decoder;
	decodeAll(decoder, {"{\"choices\":"_ba});

	QVERIFY(decoder.outcome().has_value());
	QVERIFY(!decoder.outcome()->has_value());
}

void TestOpenAiWire::noOutcomeWithoutFinishReason()
{
	openai::StreamDecoder decoder;
	decodeAll(decoder, {R"({"choices":[{"delta":{"content":"a"},"finish_reason":null}]})"_ba, "[DONE]"_ba});

	QVERIFY(!decoder.outcome().has_value());
}

void TestOpenAiWire::extractsErrorMessage_data()
{
	QTest::addColumn<QString>("body");
	QTest::addColumn<QString>("expected");

	QTest::newRow("openai object") << uR"({"error":{"message":"Invalid API key","type":"invalid_request_error"}})"_s << u"Invalid API key"_s;
	QTest::newRow("string") << uR"({"error":"model not found"})"_s << u"model not found"_s;
	QTest::newRow("plain text") << u"  Bad Gateway\n"_s << u"Bad Gateway"_s;
}

void TestOpenAiWire::extractsErrorMessage()
{
	QFETCH(QString, body);
	QFETCH(QString, expected);

	QCOMPARE(openai::errorMessage(body), expected);
}

QTEST_GUILESS_MAIN(TestOpenAiWire)
#include "tst_openai_wire.moc"
