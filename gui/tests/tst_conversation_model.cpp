// Adapted from an early version of qutie by LightBlueCube
#include "session/conversation_model.h"
#include "session_test_data.h"

#include <QAbstractItemModelTester>
#include <QSignalSpy>
#include <QTest>

using namespace Qt::StringLiterals;
using refgui::proto::Delta;
using refgui::proto::ToolState;
using refgui::session::ConversationModel;
using namespace refgui::test;

namespace
{

int streamingRowCount(const ConversationModel& model)
{
	int count = 0;
	for(int row = 0; row < model.rowCount(); ++row)
	{
		count += model.index(row).data(ConversationModel::StreamingRole).toBool() ? 1 : 0;
	}
	return count;
}

} // namespace

class TestConversationModel : public QObject
{
	Q_OBJECT

private slots:
	void splitsAssistantTextIntoBlocks();
	void interleavesReasoningTextAndTools();
	void marksOnlyTheLastBlockStreaming();
	void streamsDeltasWithoutReset();
	void splitsFencesAcrossDeltas();
	void releasesHeldLineWhenMessageEnds();
	void appendedPartEndsStreamingOfThePartBefore();
	void toolStateChangesUpdateTheRow();
	void streamingToolInputShowsPurposeEarly();
	void showsErrorsExceptAbort();
	void describesUserMessagesBySource();
	void opensTurnsOnlyForTurnSources();
	void removeUpdatesOpensTurn();
	void prependKeepsRowsBelow();
	void resetReplacesEverything();
	void ignoresUnknownMessages();
	void replacingAMessageKeepsRowIdentity();
	void everyMessageEndsWithAMetaLine();
	void marksMessagesBeforeTheLastCompaction();
	void keepsTheTailErrorLast();
	void hidesTheTailErrorTheLastMessageShows();
	void togglesFoldingAgainstTheDefault();
	void carriesToolMetaAndImages();
	void givesMessageTexts();
	void summarisesWhatToolsAskOfTheUser();
};

void TestConversationModel::splitsAssistantTextIntoBlocks()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);

	model.setMessage(user(u"u1"_s, u"Fix it"_s));
	model.setMessage(assistant(u"a1"_s, {text(u"Change this:\n\n```cmake\nqt_add_qml_module()\n```\n\nThen rebuild."_s)}));

	const QStringList expected{
		u"user:Fix it"_s,
		u"markdown:Change this:"_s,
		u"code:qt_add_qml_module()@cmake"_s,
		u"markdown:Then rebuild."_s,
	};
	QCOMPARE(render(model), expected);
	QCOMPARE(streamingRowCount(model), 0);
}

void TestConversationModel::interleavesReasoningTextAndTools()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);

	model.setMessage(assistant(u"a1"_s,
	                           {reasoning(u"Need to look at main."_s),
	                            text(u"Reading.\n```\nx"_s), // an unclosed fence ends with its part
	                            tool(u"bash"_s, u"run ls"_s, ToolState::Done, u"main.cpp"_s),
	                            text(u"Done."_s)}));

	const QStringList expected{
		u"reasoning:Need to look at main."_s,
		u"markdown:Reading."_s,
		u"code:x@"_s,
		u"tool:bash run ls"_s,
		u"markdown:Done."_s,
	};
	QCOMPARE(render(model), expected);
	QCOMPARE(roleOf(model, 3, ConversationModel::OutputRole).toString(), u"main.cpp"_s);
	QCOMPARE(roleOf(model, 3, ConversationModel::ToolStateRole).toString(), u"done"_s);
	QVERIFY(roleOf(model, 3, ConversationModel::InputJsonRole).toString().contains(u"\"command\": \"ls\""_s));
}

// Only the block receiving content streams; a running tool counts as streaming too.
void TestConversationModel::marksOnlyTheLastBlockStreaming()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);

	model.setMessage(assistant(u"a1"_s, {reasoning(u"hm"_s, false), text(u"Hello ```"_s)}, false));
	QCOMPARE(streamingRowCount(model), 1);
	QVERIFY(roleOf(model, contentRowCount(model) - 1, ConversationModel::StreamingRole).toBool());

	model.setMessage(assistant(u"a2"_s, {text(u"Running"_s), tool(u"bash"_s, u"x"_s, ToolState::Running)}, false));
	const int tools = contentRowCount(model) - 1;
	QVERIFY(roleOf(model, tools, ConversationModel::StreamingRole).toBool());
	QVERIFY(!roleOf(model, tools - 1, ConversationModel::StreamingRole).toBool());
}

void TestConversationModel::streamsDeltasWithoutReset()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
	QSignalSpy resets(&model, &QAbstractItemModel::modelReset);

	model.setMessage(assistant(u"a1"_s, {}, false));
	QVERIFY(model.appendDelta(u"a1"_s, 0, {Delta::Kind::Reasoning, u"Think"_s}));
	QVERIFY(model.appendDelta(u"a1"_s, 0, {Delta::Kind::Reasoning, u"ing"_s}));
	QVERIFY(model.appendDelta(u"a1"_s, 1, {Delta::Kind::Text, u"Hello"_s}));
	QVERIFY(model.appendDelta(u"a1"_s, 1, {Delta::Kind::Text, u" there"_s}));

	QCOMPARE(render(model), (QStringList{u"reasoning:Thinking"_s, u"markdown:Hello there"_s}));
	QCOMPARE(streamingRowCount(model), 1); // the reasoning block ended when the text began
	QCOMPARE(model.partText(u"a1"_s, 1), u"Hello there"_s);
	QCOMPARE(resets.count(), 0);
}

void TestConversationModel::splitsFencesAcrossDeltas()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);

	model.setMessage(assistant(u"a1"_s, {}, false));
	for(const QString& piece : {u"Try:\n\n``"_s, u"`sh\nls "_s, u"-l\n``"_s, u"`\n\nOk"_s})
	{
		model.appendDelta(u"a1"_s, 0, {Delta::Kind::Text, piece});
	}

	QCOMPARE(render(model), (QStringList{u"markdown:Try:"_s, u"code:ls -l@sh"_s, u"markdown:Ok"_s}));
}

// A line that might become a fence is held back while streaming and released at the end.
void TestConversationModel::releasesHeldLineWhenMessageEnds()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);

	model.setMessage(assistant(u"a1"_s, {}, false));
	model.appendDelta(u"a1"_s, 0, {Delta::Kind::Text, u"Use\n``"_s});
	QCOMPARE(render(model), (QStringList{u"markdown:Use"_s}));

	model.setMessage(assistant(u"a1"_s, {text(u"Use\n``"_s)}, true));
	QCOMPARE(render(model), (QStringList{u"markdown:Use\n``"_s}));
	QCOMPARE(streamingRowCount(model), 0);
}

void TestConversationModel::appendedPartEndsStreamingOfThePartBefore()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);

	model.setMessage(assistant(u"a1"_s, {text(u"Look\n``"_s)}, false));
	QCOMPARE(render(model), (QStringList{u"markdown:Look"_s}));

	QVERIFY(model.setPart(u"a1"_s, 1, tool(u"bash"_s, u"run"_s, ToolState::Running)));
	QCOMPARE(render(model), (QStringList{u"markdown:Look\n``"_s, u"tool:bash run"_s}));
	QVERIFY(!roleOf(model, 0, ConversationModel::StreamingRole).toBool());
	QVERIFY(roleOf(model, 1, ConversationModel::StreamingRole).toBool());
}

void TestConversationModel::toolStateChangesUpdateTheRow()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
	model.setMessage(assistant(u"a1"_s, {tool(u"bash"_s, u"run"_s, ToolState::AwaitingPermission)}, false));
	QSignalSpy changed(&model, &QAbstractItemModel::dataChanged);
	QSignalSpy inserted(&model, &QAbstractItemModel::rowsInserted);

	model.setPart(u"a1"_s, 0, tool(u"bash"_s, u"run"_s, ToolState::Error, u"boom"_s));

	QCOMPARE(inserted.count(), 0);
	QCOMPARE(changed.count(), 1);
	const QList<int> roles = changed.first().at(2).value<QList<int>>();
	QVERIFY(roles.contains(ConversationModel::ToolStateRole));
	QVERIFY(roles.contains(ConversationModel::OutputRole));
	QVERIFY(roleOf(model, 0, ConversationModel::FailedRole).toBool());
	QVERIFY(!roleOf(model, 0, ConversationModel::StreamingRole).toBool());
}

void TestConversationModel::streamingToolInputShowsPurposeEarly()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
	model.setMessage(assistant(u"a1"_s, {}, false));

	model.appendDelta(u"a1"_s, 0, {Delta::Kind::ToolInput, uR"({"purpose": "List fi)"_s});
	QCOMPARE(roleOf(model, 0, ConversationModel::PurposeRole).toString(), u"List fi"_s);
	model.appendDelta(u"a1"_s, 0, {Delta::Kind::ToolInput, uR"(les", "command": "ls"})"_s});
	QCOMPARE(roleOf(model, 0, ConversationModel::PurposeRole).toString(), u"List fi"_s);
	QCOMPARE(roleOf(model, 0, ConversationModel::RawInputRole).toString(), uR"({"purpose": "List files", "command": "ls"})"_s);
	QCOMPARE(roleOf(model, 0, ConversationModel::ToolStateRole).toString(), u"streaming"_s);
}

void TestConversationModel::showsErrorsExceptAbort()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);

	refgui::proto::Message failed = assistant(u"a1"_s, {text(u"Partial"_s)}, false);
	failed.error = u"provider said no"_s;
	model.setMessage(failed);
	QCOMPARE(render(model), (QStringList{u"markdown:Partial"_s, u"error:provider said no"_s}));
	QCOMPARE(streamingRowCount(model), 0);

	refgui::proto::Message aborted = assistant(u"a2"_s, {text(u"Cut"_s)}, false);
	aborted.error = u"Aborted"_s;
	model.setMessage(aborted);
	QCOMPARE(render(model).last(), u"markdown:Cut"_s);
}

void TestConversationModel::describesUserMessagesBySource()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);

	refgui::proto::MessageSource peer{.kind = refgui::proto::MessageSource::Kind::Peer, .fromId = u"s2"_s, .fromName = u"helper"_s};
	model.setMessage(user(u"u1"_s, u"from a peer"_s, peer));
	QCOMPARE(roleOf(model, 0, ConversationModel::SourceRole).toString(), u"peer"_s);
	QCOMPARE(roleOf(model, 0, ConversationModel::SenderRole).toString(), u"helper"_s);

	refgui::proto::Message withImage = user(u"u2"_s, u"look"_s);
	withImage.parts.append(refgui::proto::ImagePart{.image = {.mime = u"image/png"_s, .data = {}}, .name = u"shot.png"_s});
	model.setMessage(withImage);
	QCOMPARE(render(model).mid(1), (QStringList{u"user:look"_s, u"image:shot.png"_s}));
	QCOMPARE(roleOf(model, 1, ConversationModel::SourceRole).toString(), u"human"_s);

	// Several text parts form one block.
	refgui::proto::Message split = user(u"u3"_s, u"one"_s);
	split.parts.append(text(u"two"_s));
	model.setMessage(split);
	QCOMPARE(render(model).last(), u"user:one\n\ntwo"_s);
}

void TestConversationModel::opensTurnsOnlyForTurnSources()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
	using Kind = refgui::proto::MessageSource::Kind;

	model.setMessage(user(u"u1"_s, u"first"_s));
	model.setMessage(user(u"u2"_s, u"second"_s));
	model.setMessage(user(u"u3"_s, u"shell out"_s, {.kind = Kind::Shell}));
	model.setMessage(user(u"u4"_s, u"reminder"_s, {.kind = Kind::Reminder}));
	model.setMessage(user(u"u5"_s, u"spawned"_s, {.kind = Kind::Spawn}));

	QVERIFY(!roleOf(model, 0, ConversationModel::OpensTurnRole).toBool()); // nothing above it
	QVERIFY(roleOf(model, 1, ConversationModel::OpensTurnRole).toBool());
	QVERIFY(!roleOf(model, 2, ConversationModel::OpensTurnRole).toBool());
	QVERIFY(!roleOf(model, 3, ConversationModel::OpensTurnRole).toBool());
	QVERIFY(roleOf(model, 4, ConversationModel::OpensTurnRole).toBool());
}

void TestConversationModel::removeUpdatesOpensTurn()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
	model.setMessage(user(u"u1"_s, u"first"_s));
	model.setMessage(user(u"u2"_s, u"second"_s));
	QSignalSpy removed(&model, &QAbstractItemModel::rowsRemoved);
	QSignalSpy changed(&model, &QAbstractItemModel::dataChanged);

	model.removeMessage(u"u1"_s);

	QCOMPARE(removed.count(), 1);
	QCOMPARE(render(model), (QStringList{u"user:second"_s}));
	QCOMPARE(changed.count(), 1);
	QVERIFY(!roleOf(model, 0, ConversationModel::OpensTurnRole).toBool());
}

void TestConversationModel::prependKeepsRowsBelow()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
	model.setMessage(user(u"u2"_s, u"newer"_s));
	QSignalSpy inserted(&model, &QAbstractItemModel::rowsInserted);
	QSignalSpy changed(&model, &QAbstractItemModel::dataChanged);
	QSignalSpy resets(&model, &QAbstractItemModel::modelReset);

	model.prependMessages({user(u"u0"_s, u"oldest"_s), assistant(u"a0"_s, {text(u"old reply"_s)})});

	QCOMPARE(render(model), (QStringList{u"user:oldest"_s, u"markdown:old reply"_s, u"user:newer"_s}));
	QCOMPARE(inserted.count(), 1);
	QCOMPARE(inserted.first().at(1).toInt(), 0);
	QCOMPARE(inserted.first().at(2).toInt(), 3); // each message also brings its meta line
	QCOMPARE(resets.count(), 0);
	QVERIFY(roleOf(model, 2, ConversationModel::OpensTurnRole).toBool());
	QCOMPARE(changed.count(), 1); // the former first row now opens a turn
	QCOMPARE(changed.first().at(0).value<QModelIndex>().row(), 4);
}

void TestConversationModel::resetReplacesEverything()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
	model.setMessage(user(u"u1"_s, u"old"_s));
	QSignalSpy resets(&model, &QAbstractItemModel::modelReset);

	model.reset({user(u"u9"_s, u"new"_s), assistant(u"a9"_s, {text(u"reply"_s)})});

	QCOMPARE(resets.count(), 1);
	QCOMPARE(render(model), (QStringList{u"user:new"_s, u"markdown:reply"_s}));
	QCOMPARE(model.partCount(u"u1"_s), -1);
}

void TestConversationModel::ignoresUnknownMessages()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);

	QVERIFY(!model.appendDelta(u"nope"_s, 0, {Delta::Kind::Text, u"x"_s}));
	QVERIFY(!model.setPart(u"nope"_s, 0, text(u"x"_s)));
	model.removeMessage(u"nope"_s);
	QCOMPARE(model.rowCount(), 0);
}

// Replacing a message with a longer version only inserts at the end.
void TestConversationModel::replacingAMessageKeepsRowIdentity()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
	model.setMessage(user(u"u1"_s, u"go"_s));
	model.setMessage(assistant(u"a1"_s, {text(u"First."_s)}, false));
	QSignalSpy inserted(&model, &QAbstractItemModel::rowsInserted);
	QSignalSpy removed(&model, &QAbstractItemModel::rowsRemoved);
	QSignalSpy resets(&model, &QAbstractItemModel::modelReset);

	model.setMessage(assistant(u"a1"_s, {text(u"First."_s), tool(u"bash"_s, u"run"_s, ToolState::Running)}, false));

	QCOMPARE(inserted.count(), 1);
	QCOMPARE(inserted.first().at(1).toInt(), 3); // between the text and the meta line, after the user's two rows
	QCOMPARE(removed.count(), 0);
	QCOMPARE(resets.count(), 0);
	QCOMPARE(render(model), (QStringList{u"user:go"_s, u"markdown:First."_s, u"tool:bash run"_s}));
}

namespace
{

int rowOfKind(const ConversationModel& model, const QString& kind, int nth = 0)
{
	for(int row = 0; row < model.rowCount(); ++row)
	{
		if(model.index(row).data(ConversationModel::KindRole).toString() == kind && nth-- == 0)
		{
			return row;
		}
	}
	return -1;
}

} // namespace

void TestConversationModel::everyMessageEndsWithAMetaLine()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
	refgui::proto::Message reply = assistant(u"a1"_s, {text(u"Hi"_s)});
	reply.agent = u"build"_s;
	reply.model = refgui::proto::ModelRef{.provider = u"p"_s, .model = u"m1"_s, .variant = u"high"_s};
	reply.usage = refgui::proto::Usage{.input = 10, .output = 5, .cacheRead = 100, .cacheWrite = 1};
	model.setMessage(user(u"u1"_s, u"go"_s));
	model.setMessage(reply);

	QCOMPARE(model.rowCount(), 4);
	const int userMeta = rowOfKind(model, u"meta"_s, 0);
	const int replyMeta = rowOfKind(model, u"meta"_s, 1);
	QCOMPARE(userMeta, 1);
	QCOMPARE(replyMeta, 3);
	QVERIFY(model.index(userMeta).data(ConversationModel::HumanRole).toBool());
	QVERIFY(!model.index(replyMeta).data(ConversationModel::HumanRole).toBool());
	QCOMPARE(model.index(replyMeta).data(ConversationModel::AgentRole).toString(), u"build"_s);
	QCOMPARE(model.index(replyMeta).data(ConversationModel::ModelIdRole).toString(), u"m1"_s);
	QCOMPARE(model.index(replyMeta).data(ConversationModel::VariantRole).toString(), u"high"_s);
	QCOMPARE(model.index(replyMeta).data(ConversationModel::TokensInRole).toULongLong(), 111ULL);
	QCOMPARE(model.index(replyMeta).data(ConversationModel::TokensOutRole).toULongLong(), 5ULL);
	QCOMPARE(model.index(replyMeta).data(ConversationModel::TimeEndRole).toLongLong(), 2LL);

	refgui::proto::Message stopped = assistant(u"a2"_s, {}, false);
	stopped.error = u"Aborted"_s;
	model.setMessage(stopped);
	const int stoppedMeta = rowOfKind(model, u"meta"_s, 2);
	QVERIFY(model.index(stoppedMeta).data(ConversationModel::InterruptedRole).toBool());
	QVERIFY(!model.index(stoppedMeta).data(ConversationModel::ErroredRole).toBool());
	QCOMPARE(rowOfKind(model, u"error"_s), -1);
}

void TestConversationModel::marksMessagesBeforeTheLastCompaction()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
	model.setMessage(user(u"u1"_s, u"old"_s));
	model.setMessage(assistant(u"a1"_s, {text(u"old reply"_s)}));
	QVERIFY(!roleOf(model, 0, ConversationModel::StaleRole).toBool());

	model.setMessage(user(u"c1"_s, u"summary"_s, {.kind = refgui::proto::MessageSource::Kind::Compaction}));
	model.setMessage(user(u"u2"_s, u"new"_s));

	QCOMPARE(render(model), (QStringList{u"user:old"_s, u"markdown:old reply"_s, u"user:summary"_s, u"user:new"_s}));
	QVERIFY(roleOf(model, 0, ConversationModel::StaleRole).toBool());
	QVERIFY(roleOf(model, 1, ConversationModel::StaleRole).toBool());
	QVERIFY(!roleOf(model, 2, ConversationModel::StaleRole).toBool());
	QVERIFY(roleOf(model, 2, ConversationModel::ContextStartRole).toBool());
	QVERIFY(!roleOf(model, 3, ConversationModel::StaleRole).toBool());
	QCOMPARE(rowOfKind(model, u"meta"_s, 2), 6); // the summary has no meta line: its divider says it all
	QVERIFY(model.index(0).data(ConversationModel::StaleRole).toBool());

	model.removeMessage(u"c1"_s);
	QVERIFY(!roleOf(model, 0, ConversationModel::StaleRole).toBool());
}

void TestConversationModel::keepsTheTailErrorLast()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
	model.setMessage(user(u"u1"_s, u"go"_s));
	model.setTailError(u"offline"_s);
	QCOMPARE(model.rowCount(), 3);
	QCOMPARE(model.index(2).data(ConversationModel::KindRole).toString(), u"error"_s);

	model.setMessage(assistant(u"a1"_s, {text(u"Hi"_s)}));
	QCOMPARE(model.rowCount(), 5);
	QCOMPARE(model.index(4).data(ConversationModel::ContentRole).toString(), u"offline"_s);
	QCOMPARE(model.index(2).data(ConversationModel::KindRole).toString(), u"markdown"_s);

	model.setTailError(u"still offline"_s);
	QCOMPARE(model.index(4).data(ConversationModel::ContentRole).toString(), u"still offline"_s);
	model.setTailError({});
	QCOMPARE(model.rowCount(), 4);
}

// The last message shows its own error, so the same text at the end would say it twice.
void TestConversationModel::hidesTheTailErrorTheLastMessageShows()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
	model.setMessage(user(u"u1"_s, u"go"_s));
	model.setTailError(u"Rate limited"_s);
	QCOMPARE(model.rowCount(), 3);

	refgui::proto::Message failed = assistant(u"a1"_s, {}, false);
	failed.error = u"Rate limited"_s;
	model.setMessage(failed);
	QCOMPARE(model.rowCount(), 4); // the message's own error block and meta line, no tail
	QCOMPARE(rowOfKind(model, u"error"_s), 2);
	QCOMPARE(model.index(model.rowCount() - 1).data(ConversationModel::KindRole).toString(), u"meta"_s);

	model.setTailError(u"Another"_s);
	QCOMPARE(model.rowCount(), 5);
	model.removeMessage(u"a1"_s);
	QCOMPARE(model.rowCount(), 3);
}

void TestConversationModel::togglesFoldingAgainstTheDefault()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
	model.setMessage(assistant(u"a1"_s, {reasoning(u"hm"_s), tool(u"bash"_s, u"x"_s, ToolState::Done)}));
	QVERIFY(!roleOf(model, 0, ConversationModel::ExpandedRole).toBool());
	QVERIFY(!roleOf(model, 1, ConversationModel::ExpandedRole).toBool());

	model.toggleExpanded(0);
	QVERIFY(roleOf(model, 0, ConversationModel::ExpandedRole).toBool());
	QVERIFY(!roleOf(model, 1, ConversationModel::ExpandedRole).toBool());

	model.setDefaultExpansion(false, true); // "show tools" also forgets the single flips
	QVERIFY(!roleOf(model, 0, ConversationModel::ExpandedRole).toBool());
	QVERIFY(roleOf(model, 1, ConversationModel::ExpandedRole).toBool());
	model.toggleExpanded(1);
	QVERIFY(!roleOf(model, 1, ConversationModel::ExpandedRole).toBool());

	// A flip stays with the part when the message is replaced.
	model.setMessage(assistant(u"a1"_s, {reasoning(u"hmm"_s), tool(u"bash"_s, u"x"_s, ToolState::Done)}));
	QVERIFY(!roleOf(model, 1, ConversationModel::ExpandedRole).toBool());
}

void TestConversationModel::carriesToolMetaAndImages()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
	refgui::proto::ToolPart running{.callId = u"c1"_s, .name = u"bash"_s, .state = ToolState::Running, .time = 5};
	running.meta = QJsonObject{{u"live_output"_s, u"tail"_s}, {u"exit_code"_s, 0}};
	running.runAt = 99;
	refgui::proto::ToolPart finished = running;
	finished.state = ToolState::Done;
	finished.timeEnd = 8;
	finished.images = {refgui::proto::ImageData{.mime = u"image/png"_s, .data = {}, .id = u"a1:0:0"_s, .size = 12}};

	model.setMessage(assistant(u"a1"_s, {running}, false));
	QCOMPARE(roleOf(model, 0, ConversationModel::LiveOutputRole).toString(), u"tail"_s);
	QVERIFY(!roleOf(model, 0, ConversationModel::MetaRole).toMap().contains(u"live_output"_s));
	QCOMPARE(roleOf(model, 0, ConversationModel::RunAtRole).toLongLong(), 99LL);

	model.setMessage(assistant(u"a1"_s, {finished}, false));
	QCOMPARE(roleOf(model, 0, ConversationModel::TimeEndRole).toLongLong(), 8LL);
	const QVariantList images = roleOf(model, 0, ConversationModel::ImagesRole).toList();
	QCOMPARE(images.size(), 1);
	QCOMPARE(images.first().toMap().value(u"id"_s).toString(), u"a1:0:0"_s);
	QCOMPARE(images.first().toMap().value(u"size"_s).toLongLong(), 12LL);
}

void TestConversationModel::summarisesWhatToolsAskOfTheUser()
{
	ConversationModel model;
	QSignalSpy changed(&model, &ConversationModel::toolSummaryChanged);
	const auto call = [](const QString& id, ToolState state) {
		return refgui::proto::ToolPart{.callId = id, .name = u"bash"_s, .state = state};
	};

	model.setMessage(assistant(u"a1"_s, {call(u"c1"_s, ToolState::AwaitingPermission), call(u"c2"_s, ToolState::AwaitingPermission), call(u"c3"_s, ToolState::Delayed)}, false));
	QCOMPARE(model.toolSummary().awaitingPermission, 2);
	QCOMPARE(model.toolSummary().oldestAwaitingCallId, u"c1"_s);
	QVERIFY(model.toolSummary().lastToolDelayed);
	QVERIFY(changed.count() >= 1);

	model.setPart(u"a1"_s, 0, call(u"c1"_s, ToolState::Running));
	QCOMPARE(model.toolSummary().awaitingPermission, 1);
	QCOMPARE(model.toolSummary().oldestAwaitingCallId, u"c2"_s);

	model.setPart(u"a1"_s, 2, call(u"c3"_s, ToolState::Running));
	QVERIFY(!model.toolSummary().lastToolDelayed);

	model.reset({});
	QCOMPARE(model.toolSummary().awaitingPermission, 0);
	QVERIFY(model.toolSummary().oldestAwaitingCallId.isEmpty());
}

void TestConversationModel::givesMessageTexts()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
	QVERIFY(model.lastReplyId().isEmpty());
	model.setMessage(user(u"u1"_s, u"go"_s));
	model.setMessage(assistant(u"a1"_s, {text(u"one"_s), tool(u"bash"_s, u"x"_s, ToolState::Done), text(u"two"_s)}));
	model.setMessage(assistant(u"a2"_s, {tool(u"bash"_s, u"y"_s, ToolState::Done)}));

	QCOMPARE(model.messageText(u"a1"_s), u"one\n\ntwo"_s);
	QCOMPARE(model.lastReplyId(), u"a1"_s);
	QCOMPARE(model.firstMessageId(), u"u1"_s);
	QVERIFY(!model.messageText(u"nope"_s));
}

QTEST_GUILESS_MAIN(TestConversationModel)
#include "tst_conversation_model.moc"
