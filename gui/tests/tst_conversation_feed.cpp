#include "session/conversation_feed.h"
#include "session_test_data.h"

#include <QSignalSpy>
#include <QTest>

using namespace Qt::StringLiterals;
using namespace refgui::test;
using refgui::proto::Delta;
using refgui::proto::DeltaEvent;
using refgui::proto::MessageEvent;
using refgui::proto::PartEvent;
using refgui::proto::RemovedEvent;
using refgui::proto::ResetEvent;
using refgui::proto::ToolState;
using refgui::session::ConversationFeed;
using refgui::session::ConversationModel;

namespace
{

constexpr int PaceTimeoutMs = 2000; // the pacer shows text within 200ms of its arrival

DeltaEvent textDelta(const QString& messageId, qsizetype index, const QString& text)
{
	return {messageId, index, {Delta::Kind::Text, text}};
}

} // namespace

class TestConversationFeed : public QObject
{
	Q_OBJECT

private slots:
	void paceTextDeltas();
	void showsToolInputAtOnce();
	void wholePartExtendingShownTextIsPaced();
	void textThatDoesNotContinueReplacesAtOnce();
	void newPartWaitsForEarlierText();
	void endOfMessageDoesNotCutTheText();
	void resetDropsWaitingText();
	void removalFlushesFirst();
	void ignoresDeltasOfUnknownMessages();
};

void TestConversationFeed::paceTextDeltas()
{
	ConversationModel model;
	ConversationFeed feed(model);
	QSignalSpy pending(&feed, &ConversationFeed::pendingChanged);
	feed.apply(MessageEvent{assistant(u"a1"_s, {}, false)});

	const QString sentence = u"The quick brown fox jumps over the lazy dog. "_s;
	feed.apply(textDelta(u"a1"_s, 0, sentence));
	feed.apply(textDelta(u"a1"_s, 0, sentence));

	QVERIFY(feed.hasPending());
	QVERIFY(model.partText(u"a1"_s, 0)->size() < 2 * sentence.size());
	QTRY_VERIFY_WITH_TIMEOUT(!feed.hasPending(), PaceTimeoutMs);
	QCOMPARE(model.partText(u"a1"_s, 0), sentence + sentence);
	QCOMPARE(pending.count(), 2); // started, then drained
}

void TestConversationFeed::showsToolInputAtOnce()
{
	ConversationModel model;
	ConversationFeed feed(model);
	feed.apply(MessageEvent{assistant(u"a1"_s, {}, false)});

	feed.apply(DeltaEvent{u"a1"_s, 0, {Delta::Kind::ToolInput, u"{\"purpose\": \"Look\""_s}});

	QVERIFY(!feed.hasPending());
	QCOMPARE(roleOf(model, 0, ConversationModel::PurposeRole).toString(), u"Look"_s);
}

void TestConversationFeed::wholePartExtendingShownTextIsPaced()
{
	ConversationModel model;
	ConversationFeed feed(model);
	feed.apply(MessageEvent{assistant(u"a1"_s, {text(u""_s)}, false)});
	feed.apply(textDelta(u"a1"_s, 0, u"Hello "_s));
	QVERIFY(feed.hasPending());

	const QString full = u"Hello world, this is the whole text of the part."_s;
	feed.apply(PartEvent{u"a1"_s, 0, text(full)});

	QVERIFY(model.partText(u"a1"_s, 0)->size() < full.size());
	QTRY_COMPARE_WITH_TIMEOUT(model.partText(u"a1"_s, 0), full, PaceTimeoutMs);
	QVERIFY(!feed.hasPending());
}

void TestConversationFeed::textThatDoesNotContinueReplacesAtOnce()
{
	ConversationModel model;
	ConversationFeed feed(model);
	feed.apply(MessageEvent{assistant(u"a1"_s, {text(u""_s)}, false)});
	feed.apply(textDelta(u"a1"_s, 0, u"abc def ghi"_s));

	feed.apply(PartEvent{u"a1"_s, 0, text(u"completely different"_s)});

	QVERIFY(!feed.hasPending());
	QCOMPARE(model.partText(u"a1"_s, 0), u"completely different"_s);
}

void TestConversationFeed::newPartWaitsForEarlierText()
{
	ConversationModel model;
	ConversationFeed feed(model);
	feed.apply(MessageEvent{assistant(u"a1"_s, {text(u""_s)}, false)});
	const QString sentence = u"Some text that is still on its way. "_s;
	feed.apply(textDelta(u"a1"_s, 0, sentence));

	feed.apply(PartEvent{u"a1"_s, 1, tool(u"bash"_s, u"run"_s, ToolState::Running)});

	QCOMPARE(model.partText(u"a1"_s, 0), sentence); // shown in full before the tool appears
	QCOMPARE(model.partCount(u"a1"_s), 2);
	QVERIFY(!feed.hasPending());
}

// The final Message arrives with the whole text while the pacer is still showing it.
void TestConversationFeed::endOfMessageDoesNotCutTheText()
{
	ConversationModel model;
	ConversationFeed feed(model);
	feed.apply(MessageEvent{assistant(u"a1"_s, {text(u""_s)}, false)});
	const QString full = u"First paragraph of the reply.\n\nSecond paragraph of the reply."_s;
	feed.apply(textDelta(u"a1"_s, 0, full));

	feed.apply(MessageEvent{assistant(u"a1"_s, {text(full)}, true)});

	QVERIFY(feed.hasPending());
	QVERIFY(model.partText(u"a1"_s, 0)->size() < full.size());
	QTRY_VERIFY_WITH_TIMEOUT(!feed.hasPending(), PaceTimeoutMs);
	QCOMPARE(render(model), (QStringList{u"markdown:First paragraph of the reply.\n\nSecond paragraph of the reply."_s}));
	QCOMPARE(model.partText(u"a1"_s, 0), full);
	QVERIFY(!roleOf(model, model.rowCount() - 1, ConversationModel::StreamingRole).toBool());
}

void TestConversationFeed::resetDropsWaitingText()
{
	ConversationModel model;
	ConversationFeed feed(model);
	feed.apply(MessageEvent{assistant(u"a1"_s, {text(u""_s)}, false)});
	feed.apply(textDelta(u"a1"_s, 0, u"never shown"_s));
	QVERIFY(feed.hasPending());

	feed.apply(ResetEvent{{user(u"u9"_s, u"fresh"_s)}, false});

	QVERIFY(!feed.hasPending());
	QCOMPARE(render(model), (QStringList{u"user:fresh"_s}));
}

void TestConversationFeed::removalFlushesFirst()
{
	ConversationModel model;
	ConversationFeed feed(model);
	feed.apply(MessageEvent{user(u"u1"_s, u"hi"_s)});
	feed.apply(MessageEvent{assistant(u"a1"_s, {text(u""_s)}, false)});
	feed.apply(textDelta(u"a1"_s, 0, u"text"_s));

	feed.apply(RemovedEvent{u"a1"_s});

	QVERIFY(!feed.hasPending());
	QCOMPARE(render(model), (QStringList{u"user:hi"_s}));
	QTest::qWait(50); // nothing is left to arrive late
	QCOMPARE(render(model), (QStringList{u"user:hi"_s}));
}

void TestConversationFeed::ignoresDeltasOfUnknownMessages()
{
	ConversationModel model;
	ConversationFeed feed(model);

	feed.apply(textDelta(u"nope"_s, 0, u"x"_s));
	feed.apply(PartEvent{u"nope"_s, 0, text(u"x"_s)});

	QVERIFY(!feed.hasPending());
	QCOMPARE(model.rowCount(), 0);
}

QTEST_GUILESS_MAIN(TestConversationFeed)
#include "tst_conversation_feed.moc"
