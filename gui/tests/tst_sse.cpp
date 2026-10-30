#include "infra/sse.h"

#include <QTest>

namespace
{

// Feeds `chunks` in order and renders every event as "type|data" for easy comparison.
QByteArrayList parseChunks(const QByteArrayList& chunks)
{
	refgui::sse::Parser parser;
	QByteArrayList rendered;
	for(const QByteArray& chunk : chunks)
	{
		for(const refgui::sse::Event& event : parser.feed(chunk))
		{
			rendered.append(event.type + '|' + event.data);
		}
	}
	return rendered;
}

} // namespace

class TestSse : public QObject
{
	Q_OBJECT

private slots:
	void parsesStream_data();
	void parsesStream();

	void survivesSplitAtEveryByte();
};

void TestSse::parsesStream_data()
{
	QTest::addColumn<QByteArrayList>("chunks");
	QTest::addColumn<QByteArrayList>("expected");

	QTest::newRow("lf") << QByteArrayList{"data: a\n\n"} << QByteArrayList{"message|a"};
	QTest::newRow("crlf") << QByteArrayList{"data: a\r\n\r\n"} << QByteArrayList{"message|a"};
	QTest::newRow("cr") << QByteArrayList{"data: a\r\r"} << QByteArrayList{"message|a"};
	// Treating the '\n' as a blank line would dispatch "a" and "b" separately.
	QTest::newRow("crlf split across chunks") << QByteArrayList{"data: a\r", "\ndata: b\n\n"} << QByteArrayList{"message|a\nb"};

	QTest::newRow("multi-line data") << QByteArrayList{"data: a\ndata: b\n\n"} << QByteArrayList{"message|a\nb"};
	QTest::newRow("event type") << QByteArrayList{"event: delta\ndata: x\n\n"} << QByteArrayList{"delta|x"};
	QTest::newRow("event type resets after dispatch") << QByteArrayList{"event: delta\ndata: x\n\ndata: y\n\n"} << QByteArrayList{"delta|x", "message|y"};

	QTest::newRow("no space after colon") << QByteArrayList{"data:x\n\n"} << QByteArrayList{"message|x"};
	QTest::newRow("only one space stripped") << QByteArrayList{"data:  x\n\n"} << QByteArrayList{"message| x"};
	QTest::newRow("colon inside value") << QByteArrayList{"data: {\"a\":1}\n\n"} << QByteArrayList{"message|{\"a\":1}"};
	QTest::newRow("bare data field fires empty event") << QByteArrayList{"data\n\n"} << QByteArrayList{"message|"};

	QTest::newRow("comment ignored") << QByteArrayList{": ping\n\ndata: x\n\n"} << QByteArrayList{"message|x"};
	QTest::newRow("unknown fields ignored") << QByteArrayList{"id: 1\nretry: 10\nfoo: bar\ndata: x\n\n"} << QByteArrayList{"message|x"};
	QTest::newRow("block without data fires nothing") << QByteArrayList{"event: x\n\n"} << QByteArrayList{};
	QTest::newRow("unterminated event discarded") << QByteArrayList{"data: x\n"} << QByteArrayList{};

	QTest::newRow("bom") << QByteArrayList{"\xEF\xBB\xBF" "data: x\n\n"} << QByteArrayList{"message|x"};
	QTest::newRow("bom split across chunks") << QByteArrayList{"\xEF", "\xBB\xBF" "data: x\n\n"} << QByteArrayList{"message|x"};

	QTest::newRow("openai style") << QByteArrayList{"data: {\"id\":1}\n\ndata: [DONE]\n\n"} << QByteArrayList{"message|{\"id\":1}", "message|[DONE]"};
}

void TestSse::parsesStream()
{
	QFETCH(QByteArrayList, chunks);
	QFETCH(QByteArrayList, expected);

	QCOMPARE(parseChunks(chunks), expected);
}

void TestSse::survivesSplitAtEveryByte()
{
	const QByteArray stream = "event: delta\r\ndata: h\xC3\xA9llo\r\n\r\ndata: x\r\r";

	QByteArrayList singleBytes;
	for(const char byte : stream)
	{
		singleBytes.append(QByteArray(1, byte));
	}

	QCOMPARE(parseChunks(singleBytes), parseChunks({stream}));
	QCOMPARE(parseChunks({stream}), (QByteArrayList{"delta|h\xC3\xA9llo", "message|x"}));
}

QTEST_GUILESS_MAIN(TestSse)
#include "tst_sse.moc"
