// Adapted from an early version of qutie by LightBlueCube
#include "session/stream_pacer.h"

#include <QTest>

using namespace Qt::StringLiterals;
using namespace std::chrono_literals;
using refgui::proto::Delta;
using refgui::session::StreamPacer;
using Chunk = StreamPacer::Chunk;

namespace
{

const StreamPacer::Clock::time_point Start{};

Chunk TextDelta(const QString& text, qsizetype partIndex = 0)
{
	return {u"m1"_s, partIndex, Delta::Kind::Text, text};
}

Chunk ReasoningDelta(const QString& text)
{
	return {u"m1"_s, 0, Delta::Kind::Reasoning, text};
}

// Renders chunks as "kind:text" pieces, so tests see both order and content.
QStringList render(const QList<Chunk>& chunks)
{
	QStringList rendered;
	for(const Chunk& chunk : chunks)
	{
		rendered.append((chunk.kind == Delta::Kind::Text ? u"text:"_s : u"reasoning:"_s) + chunk.text);
	}
	return rendered;
}

qsizetype lengthOf(const QList<Chunk>& chunks)
{
	qsizetype length = 0;
	for(const Chunk& chunk : chunks)
	{
		length += chunk.text.size();
	}
	return length;
}

} // namespace

class TestStreamPacer : public QObject
{
	Q_OBJECT

private slots:
	void firstArrivalSpreadsOverMaxLag();
	void burstsShowAtSteadyRate();
	void tokenStreamsShowPromptly();
	void keepsKindsInOrder();
	void neverSplitsGrapheme();
	void idleTimeDoesNotBank();
	void takeAllEmptiesIt();
	void keepsPartsApart();
	void reportsPendingTextOfAPart();
};

// With no gap seen yet, the pacer assumes the longest one.
void TestStreamPacer::firstArrivalSpreadsOverMaxLag()
{
	StreamPacer pacer;
	pacer.push(TextDelta(QString(100, u'a')), Start);

	QCOMPARE(lengthOf(pacer.take(Start)), 0);
	const qsizetype halfway = lengthOf(pacer.take(Start + 100ms));
	QVERIFY2(halfway >= 45 && halfway <= 55, qPrintable(QString::number(halfway)));
	QCOMPARE(lengthOf(pacer.take(Start + 200ms)), 100 - halfway);
	QVERIFY(pacer.isEmpty());
}

// 120 characters every 100ms: once the gap is known, each frame shows about the same
// amount, and each burst runs out about when the next one comes.
void TestStreamPacer::burstsShowAtSteadyRate()
{
	StreamPacer pacer;
	constexpr int Bursts = 10;
	constexpr auto Gap = 100ms;
	constexpr auto Frame = 10ms;
	QList<qsizetype> perFrame;
	for(int burst = 0; burst < Bursts; ++burst)
	{
		const auto arrival = Start + burst * Gap;
		pacer.push(TextDelta(QString(120, u'a')), arrival);
		for(auto frame = Frame; frame <= Gap; frame += Frame)
		{
			perFrame.append(lengthOf(pacer.take(arrival + frame)));
		}
	}

	// From the third burst on, the gap estimate has settled at 100ms: 12 per 10ms frame.
	for(qsizetype frame = 2 * Gap / Frame; frame < perFrame.size(); ++frame)
	{
		QVERIFY2(perFrame.at(frame) >= 11 && perFrame.at(frame) <= 13, qPrintable(u"frame %1: %2"_s.arg(frame).arg(perFrame.at(frame))));
	}
}

// A server that sends a token every 16ms trails by about a frame, not by MaxLag.
void TestStreamPacer::tokenStreamsShowPromptly()
{
	StreamPacer pacer;
	qsizetype shown = 0;
	qsizetype sent = 0;
	for(int token = 0; token < 50; ++token)
	{
		const auto arrival = Start + token * 16ms;
		pacer.push(TextDelta(u"word "_s), arrival);
		sent += 5;
		shown += lengthOf(pacer.take(arrival + 8ms));
	}
	QVERIFY2(sent - shown <= 10, qPrintable(u"%1 of %2 behind"_s.arg(sent - shown).arg(sent)));
}

void TestStreamPacer::keepsKindsInOrder()
{
	StreamPacer pacer;
	pacer.push(ReasoningDelta(u"thin"_s), Start);
	pacer.push(ReasoningDelta(u"k"_s), Start);
	pacer.push(TextDelta(u"say"_s), Start);

	QCOMPARE(render(pacer.take(Start + 1s)), (QStringList{u"reasoning:think"_s, u"text:say"_s}));
}

// "a😀" is three UTF-16 code units; the emoji is a surrogate pair that must come out whole.
void TestStreamPacer::neverSplitsGrapheme()
{
	StreamPacer pacer;
	const QString text = u"a\U0001F600"_s;
	pacer.push(TextDelta(text), Start);

	QString shown;
	for(auto now = Start; !pacer.isEmpty(); now += 1ms)
	{
		for(const Chunk& chunk : pacer.take(now))
		{
			const QString piece = chunk.text;
			QVERIFY2(!piece.back().isHighSurrogate(), "split a surrogate pair");
			shown += piece;
		}
	}
	QCOMPARE(shown, text);
}

// After a pause the next burst still starts from nothing shown, not all at once.
void TestStreamPacer::idleTimeDoesNotBank()
{
	StreamPacer pacer;
	pacer.push(TextDelta(u"abcd"_s), Start);
	QCOMPARE(lengthOf(pacer.take(Start + 1s)), 4);

	pacer.push(TextDelta(QString(100, u'a')), Start + 5s);
	QVERIFY(lengthOf(pacer.take(Start + 5s + 1ms)) < 10);
}

void TestStreamPacer::takeAllEmptiesIt()
{
	StreamPacer pacer;
	pacer.push(TextDelta(u"ab"_s), Start);
	pacer.push(ReasoningDelta(u"cd"_s), Start);

	QCOMPARE(render(pacer.takeAll()), (QStringList{u"text:ab"_s, u"reasoning:cd"_s}));
	QVERIFY(pacer.isEmpty());
	QCOMPARE(lengthOf(pacer.take(Start + 1s)), 0);
}

void TestStreamPacer::keepsPartsApart()
{
	StreamPacer pacer;
	pacer.push(TextDelta(u"ab"_s, 0), Start);
	pacer.push(TextDelta(u"cd"_s, 1), Start);
	pacer.push(TextDelta(u"ef"_s, 1), Start);

	const QList<Chunk> all = pacer.takeAll();
	QCOMPARE(all.size(), 2);
	QCOMPARE(all.at(1).partIndex, 1);
	QCOMPARE(all.at(1).text, u"cdef"_s);
}

void TestStreamPacer::reportsPendingTextOfAPart()
{
	StreamPacer pacer;
	pacer.push(TextDelta(u"ab"_s, 0), Start);
	pacer.push(TextDelta(u"cd"_s, 1), Start);
	pacer.push(TextDelta(u"ef"_s, 0), Start);

	QCOMPARE(pacer.pendingText(u"m1"_s, 0), u"abef"_s);
	QCOMPARE(pacer.pendingText(u"m1"_s, 1), u"cd"_s);
	QCOMPARE(pacer.pendingText(u"other"_s, 0), QString());
}

QTEST_GUILESS_MAIN(TestStreamPacer)
#include "tst_stream_pacer.moc"
