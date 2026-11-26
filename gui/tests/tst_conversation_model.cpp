// Adapted from an early version of LightBlueCube/qutie
#include "session/conversation_model.h"
#include "session_test_data.h"

#include "proto/parse.h"
#include <QFile>
#include <QJsonDocument>
#include <QAbstractItemModelTester>
#include <QSignalSpy>
#include <QTest>

using namespace Qt::StringLiterals;
using refgui::proto::Delta;
using refgui::proto::ToolState;
using refgui::session::Block;
using refgui::session::ConversationModel;
using refgui::session::kindName;
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
	void replacingHistoryTouchesOnlyWhatChanged();
	void replacingHistoryWithTheSameLeavesTheRows();
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
	void showsMarkdownWithoutImagesOrTags();
	void exposesEditWarningsFromFixture();
	void attachesAndClearsPermissionPreview();
	void splitsMarkdownTablesAndStreamsRows();
	void virtualizedPlaceholdersMaterializeAndEvict();
	void evictOutsideCapsResidentInsideKeepWindow();
	void configurableWindowAndNewestMissingRange();
	void rowCountForSeqAndEvictionSignals();
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

void TestConversationModel::replacingHistoryTouchesOnlyWhatChanged()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
	model.reset({user(u"u1"_s, u"one"_s), assistant(u"a1"_s, {text(u"first"_s)}), user(u"u2"_s, u"two"_s), assistant(u"a2"_s, {text(u"second"_s)})});
	QSignalSpy resets(&model, &QAbstractItemModel::modelReset);
	QSignalSpy removed(&model, &QAbstractItemModel::rowsRemoved);
	QSignalSpy inserted(&model, &QAbstractItemModel::rowsInserted);
	const int rowsBefore = model.rowCount();

	model.replaceHistory({user(u"u1"_s, u"one"_s), assistant(u"a1"_s, {text(u"first"_s)}), user(u"u3"_s, u"three"_s)});

	QCOMPARE(resets.count(), 0);
	QCOMPARE(removed.count(), 1);
	QCOMPARE(removed.first().at(1).toInt(), 4); // u1 and a1 with their meta lines stay
	QCOMPARE(removed.first().at(2).toInt(), rowsBefore - 1);
	QCOMPARE(inserted.count(), 1);
	QCOMPARE(inserted.first().at(1).toInt(), 4);
	QCOMPARE(render(model).mid(0, 2), (QStringList{u"user:one"_s, u"markdown:first"_s}));
	QCOMPARE(render(model).last(), u"user:three"_s);
	QCOMPARE(model.partCount(u"a2"_s), -1);
	QCOMPARE(model.partCount(u"a1"_s), 1);
}

void TestConversationModel::replacingHistoryWithTheSameLeavesTheRows()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
	const QList<refgui::proto::Message> history{user(u"u1"_s, u"one"_s), assistant(u"a1"_s, {text(u"first"_s)})};
	model.reset(history);
	QSignalSpy rowChanges(&model, &QAbstractItemModel::rowsRemoved);
	QSignalSpy rowAdditions(&model, &QAbstractItemModel::rowsInserted);

	model.replaceHistory(history);

	QCOMPARE(rowChanges.count(), 0);
	QCOMPARE(rowAdditions.count(), 0);
	model.replaceHistory({});
	QCOMPARE(model.rowCount(), 0);
	QCOMPARE(rowChanges.count(), 1);
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
	const int userMeta = rowOfKind(model, kindName(Block::Kind::Meta), 0);
	const int replyMeta = rowOfKind(model, kindName(Block::Kind::Meta), 1);
	QCOMPARE(userMeta, 1);
	QCOMPARE(replyMeta, 3);
	QVERIFY(model.index(userMeta).data(ConversationModel::HumanRole).toBool());
	QVERIFY(!model.index(replyMeta).data(ConversationModel::HumanRole).toBool());
	QCOMPARE(model.index(replyMeta).data(ConversationModel::AgentRole).toString(), u"build"_s);
	QCOMPARE(model.index(replyMeta).data(ConversationModel::ModelIdRole).toString(), u"m1"_s);
	QCOMPARE(model.index(replyMeta).data(ConversationModel::VariantRole).toString(), u"high"_s);
	QCOMPARE(model.index(replyMeta).data(ConversationModel::TokensInRole).toULongLong(), 111ULL);
	QCOMPARE(model.index(replyMeta).data(ConversationModel::TokensOutRole).toULongLong(), 5ULL);
	QCOMPARE(model.index(0).data(ConversationModel::TimeRole).toLongLong(), StartTime);
	QCOMPARE(model.index(userMeta).data(ConversationModel::TimeRole).toLongLong(), StartTime);
	QCOMPARE(model.index(replyMeta).data(ConversationModel::TimeRole).toLongLong(), StartTime);
	QCOMPARE(model.index(replyMeta).data(ConversationModel::TimeEndRole).toLongLong(), 2LL);

	refgui::proto::Message stopped = assistant(u"a2"_s, {}, false);
	stopped.error = u"Aborted"_s;
	model.setMessage(stopped);
	const int stoppedMeta = rowOfKind(model, kindName(Block::Kind::Meta), 2);
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
	QCOMPARE(rowOfKind(model, kindName(Block::Kind::Meta), 2), 6); // the summary has no meta line: its divider says it all
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
	QCOMPARE(model.index(model.rowCount() - 1).data(ConversationModel::KindRole).toString(), kindName(Block::Kind::Meta));

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

	// Ctrl+Shift+R (setDefaultExpansion(true, ...)) expands finished reasoning rows,
	// and manually collapsing a streaming reasoning row stays collapsed after stream end.
	model.setDefaultExpansion(true, false);
	QVERIFY(roleOf(model, 0, ConversationModel::ExpandedRole).toBool());
	model.setDefaultExpansion(false, false);
	QVERIFY(!roleOf(model, 0, ConversationModel::ExpandedRole).toBool());

	model.setMessage(assistant(u"a2"_s, {reasoning(u"live"_s, false)}, false));
	const int liveModelRow = model.rowCount() - 2; // before a2's meta row
	QVERIFY(model.index(liveModelRow).data(ConversationModel::ExpandedRole).toBool());
	model.toggleExpanded(liveModelRow);
	QVERIFY(!model.index(liveModelRow).data(ConversationModel::ExpandedRole).toBool());
	model.setPart(u"a2"_s, 0, reasoning(u"done"_s, true));
	QVERIFY(!model.index(liveModelRow).data(ConversationModel::ExpandedRole).toBool());
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

void TestConversationModel::showsMarkdownWithoutImagesOrTags()
{
	ConversationModel model;
	model.setMessage(assistant(u"a1"_s, {text(u"look ![x](http://example.com/a.png) <img src=y>"_s), reasoning(u"![r](http://example.com/r.png)"_s)}));
	QCOMPARE(roleOf(model, 0, ConversationModel::ContentRole).toString(), u"look [x](http://example.com/a.png) \\<img src=y>"_s);
	QCOMPARE(roleOf(model, 1, ConversationModel::ContentRole).toString(), u"[r](http://example.com/r.png)"_s);
	// What the user typed is plain text and stays as it is.
	model.setMessage(user(u"u1"_s, u"![x](y)"_s));
	QCOMPARE(roleOf(model, 2, ConversationModel::ContentRole).toString(), u"![x](y)"_s);
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
void TestConversationModel::exposesEditWarningsFromFixture()
{
	const QString path = QStringLiteral(REF_FIXTURES_DIR) + u"/part/tool_done_edit_warnings.json"_s;
	QFile file(path);
	QVERIFY2(file.open(QIODevice::ReadOnly), qPrintable(path));
	const QJsonObject partJson = QJsonDocument::fromJson(file.readAll()).object();
	const auto part = refgui::proto::parsePart(partJson);
	QVERIFY(part);

	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
	model.setMessage(assistant(u"a1"_s, {*part}));
	QCOMPARE(roleOf(model, 0, ConversationModel::WarningsRole).toStringList(),
	         (QStringList{u"Recovered: file changed since read; line numbers remapped."_s}));
}

void TestConversationModel::attachesAndClearsPermissionPreview()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);
	const refgui::proto::ToolPart call{.callId = u"c1"_s, .name = u"edit"_s, .state = ToolState::AwaitingPermission};
	model.setMessage(assistant(u"a1"_s, {call}, false));
	QVERIFY(roleOf(model, 0, ConversationModel::PermissionPreviewRole).toMap().isEmpty());
	const refgui::proto::PermissionRequest request{
		.sessionId = u"s1"_s,
		.messageId = u"a1"_s,
		.callId = u"c1"_s,
		.tool = u"edit"_s,
		.purpose = {},
		.input = {},
		.time = 1,
		.preview = refgui::proto::EditPreview{.diff = u"+a\n-b\n"_s, .added = 1, .removed = 1, .truncated = true},
	};
	model.setPermission(request);
	const QVariantMap preview = roleOf(model, 0, ConversationModel::PermissionPreviewRole).toMap();
	QCOMPARE(preview.value(u"diff"_s).toString(), u"+a\n-b\n"_s);
	QCOMPARE(preview.value(u"added"_s).toUInt(), 1U);
	QCOMPARE(preview.value(u"removed"_s).toUInt(), 1U);
	QVERIFY(preview.value(u"truncated"_s).toBool());

	model.resolvePermission(u"c1"_s);
	QVERIFY(roleOf(model, 0, ConversationModel::PermissionPreviewRole).toMap().isEmpty());
}

void TestConversationModel::splitsMarkdownTablesAndStreamsRows()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);

	model.setMessage(assistant(u"a1"_s, {text(u"Intro:\n\n| Name | Role | Score |\n| :--- | :---: | ---: |\n| Alice | **Dev** | 10 |\n| Bob | ![x](http://example.com/a.png) | `a | b` |\n\nOutro"_s)}));
	QCOMPARE(render(model).size(), 3);
	QCOMPARE(roleOf(model, 0, ConversationModel::KindRole).toString(), u"markdown"_s);
	QCOMPARE(roleOf(model, 0, ConversationModel::ContentRole).toString(), u"Intro:"_s);
	QCOMPARE(roleOf(model, 1, ConversationModel::KindRole).toString(), u"table"_s);
	QCOMPARE(roleOf(model, 1, ConversationModel::TableHeadersRole).toStringList(), (QStringList{u"Name"_s, u"Role"_s, u"Score"_s}));
	QCOMPARE(roleOf(model, 1, ConversationModel::TableAlignmentsRole).toStringList(), (QStringList{u"left"_s, u"center"_s, u"right"_s}));
	const QVariantList rows = roleOf(model, 1, ConversationModel::TableRowsRole).toList();
	QCOMPARE(rows.size(), 2);
	QCOMPARE(rows.at(0).toStringList(), (QStringList{u"Alice"_s, u"**Dev**"_s, u"10"_s}));
	QCOMPARE(rows.at(1).toStringList(), (QStringList{u"Bob"_s, u"[x](http://example.com/a.png)"_s, u"`a | b`"_s}));
	QCOMPARE(roleOf(model, 2, ConversationModel::KindRole).toString(), u"markdown"_s);
	QCOMPARE(roleOf(model, 2, ConversationModel::ContentRole).toString(), u"Outro"_s);

	// Streaming a table row by row without leading pipe header.
	model.setMessage(assistant(u"a2"_s, {}, false));
	model.appendDelta(u"a2"_s, 0, {Delta::Kind::Text, u"Before\n\nCol1 | Col2\n"_s});
	model.appendDelta(u"a2"_s, 0, {Delta::Kind::Text, u"--- | ---:\n"_s});
	const int baseRow = contentRowCount(model) - 1;
	QCOMPARE(roleOf(model, baseRow - 1, ConversationModel::KindRole).toString(), u"markdown"_s);
	QCOMPARE(roleOf(model, baseRow - 1, ConversationModel::ContentRole).toString(), u"Before"_s);
	QCOMPARE(roleOf(model, baseRow, ConversationModel::KindRole).toString(), u"table"_s);
	QCOMPARE(roleOf(model, baseRow, ConversationModel::TableHeadersRole).toStringList(), (QStringList{u"Col1"_s, u"Col2"_s}));
	QCOMPARE(roleOf(model, baseRow, ConversationModel::TableAlignmentsRole).toStringList(), (QStringList{u"left"_s, u"right"_s}));
	QVERIFY(roleOf(model, baseRow, ConversationModel::StreamingRole).toBool());
	model.appendDelta(u"a2"_s, 0, {Delta::Kind::Text, u"v1 | v2\n"_s});
	QCOMPARE(roleOf(model, baseRow, ConversationModel::TableRowsRole).toList().size(), 1);
	model.appendDelta(u"a2"_s, 0, {Delta::Kind::Text, u"```cpp\nint x = 0;\n```"_s});
	QVERIFY(!roleOf(model, baseRow, ConversationModel::StreamingRole).toBool());
	QCOMPARE(roleOf(model, baseRow + 1, ConversationModel::KindRole).toString(), u"code"_s);
}

void TestConversationModel::virtualizedPlaceholdersMaterializeAndEvict()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);

	constexpr int Total = 1420;
	constexpr int StartSeq = 1320;
	QList<refgui::proto::MessageLayoutMeta> metas;
	for(int i = 0; i < StartSeq; ++i)
	{
		metas.append(refgui::proto::MessageLayoutMeta{
			.seq = static_cast<quint64>(i),
			.id = u"m_%1"_s.arg(i),
			.role = (i % 2 == 0) ? refgui::proto::Role::User : refgui::proto::Role::Assistant,
			.sourceKind = u"human"_s,
			.parts = {refgui::proto::PartLayoutMeta{.kind = u"text"_s, .chars = 240, .lines = 3}},
		});
	}
	QList<refgui::proto::Message> tail;
	for(int i = StartSeq; i < Total; ++i)
	{
		const QString id = u"m_%1"_s.arg(i);
		tail.append(i % 2 == 0 ? user(id, u"hello"_s) : assistant(id, {text(u"reply"_s)}));
	}

	model.reset(tail, Total, StartSeq, metas);
	QCOMPARE(model.slotCount(), Total);
	QCOMPARE(model.residentCount(), 100);
	QVERIFY(model.hasPlaceholders());
	QCOMPARE(model.index(0).data(ConversationModel::KindRole).toString(), u"skeleton"_s);

	const double hWide = model.index(0).data(ConversationModel::EstimatedHeightRole).toDouble();
	QSignalSpy heightChanged(&model, &QAbstractItemModel::dataChanged);
	model.setColumnWidth(260.0);
	QCOMPARE(heightChanged.count(), 1);
	const double hNarrow = model.index(0).data(ConversationModel::EstimatedHeightRole).toDouble();
	QVERIFY(hNarrow >= hWide);

	// Missing range around seq 10..20 returns a bounded page.
	const auto missing = model.missingRangeAround(10, 20, ConversationModel::PageSize);
	QVERIFY(missing.has_value());
	QVERIFY(missing->first <= 10 && missing->second > 20);

	// Materialize 1000 messages at 0..1000 so resident count is 1100 -> evictOutside caps at 1000 and drops outside [v-500, v+500].
	QList<refgui::proto::Message> block0;
	for(int i = 0; i < 1000; ++i)
	{
		const QString id = u"m_%1"_s.arg(i);
		block0.append(i % 2 == 0 ? user(id, u"hello"_s) : assistant(id, {text(u"reply"_s)}));
	}
	model.materializeRange(0, block0);
	QCOMPARE(model.residentCount(), 1100);

	QSignalSpy evictedSpy(&model, &ConversationModel::messagesEvicted);
	const QStringList evicted = model.evictOutside(10, 20);
	QVERIFY(!evicted.isEmpty());
	QVERIFY(model.residentCount() <= ConversationModel::MaxResidentMessages);
	QCOMPARE(evictedSpy.count(), 1);
	// Tail at 1400 should have been evicted back to skeleton.
	const int row1400 = model.firstRowForSeq(1400);
	QCOMPARE(model.index(row1400).data(ConversationModel::KindRole).toString(), u"skeleton"_s);

	// Materialize 420..1420 (1000 newest messages) and test evictOldTail(1000).
	QList<refgui::proto::Message> tailWindow;
	for(int i = 420; i < Total; ++i)
	{
		const QString id = u"m_%1"_s.arg(i);
		tailWindow.append(i % 2 == 0 ? user(id, u"hello"_s) : assistant(id, {text(u"reply"_s)}));
	}
	model.materializeRange(420, tailWindow);
	model.evictOldTail(ConversationModel::TailKeepMessages);
	QCOMPARE(model.residentCount(), ConversationModel::TailKeepMessages);
	QCOMPARE(model.index(0).data(ConversationModel::KindRole).toString(), u"skeleton"_s);
}

void TestConversationModel::evictOutsideCapsResidentInsideKeepWindow()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);

	constexpr int Total = 1420;
	QList<refgui::proto::MessageLayoutMeta> metas;
	metas.reserve(Total);
	for(int i = 0; i < Total; ++i)
	{
		metas.append(refgui::proto::MessageLayoutMeta{
			.seq = static_cast<quint64>(i),
			.id = u"m_%1"_s.arg(i),
			.role = (i % 2 == 0) ? refgui::proto::Role::User : refgui::proto::Role::Assistant,
			.sourceKind = u"human"_s,
			.parts = {refgui::proto::PartLayoutMeta{.kind = u"text"_s, .chars = 80, .lines = 2}},
		});
	}
	model.reset({}, Total, Total, metas);
	QCOMPARE(model.residentCount(), 0);

	// Materialize 1011 messages inside [700 - 500, 710 + 500] = [200, 1210].
	constexpr int KeepStart = 200;
	constexpr int KeepEnd = 1210;
	QList<refgui::proto::Message> windowMsgs;
	windowMsgs.reserve(KeepEnd - KeepStart + 1);
	for(int i = KeepStart; i <= KeepEnd; ++i)
	{
		const QString id = u"m_%1"_s.arg(i);
		windowMsgs.append(i % 2 == 0 ? user(id, u"hello"_s) : assistant(id, {text(u"reply"_s)}));
	}
	model.materializeRange(KeepStart, windowMsgs);
	QCOMPARE(model.residentCount(), 1011);

	QSignalSpy evictedSpy(&model, &ConversationModel::messagesEvicted);
	const QStringList evicted = model.evictOutside(700, 710);
	QCOMPARE(evicted.size(), 11);
	QCOMPARE(model.residentCount(), ConversationModel::MaxResidentMessages);
	QCOMPARE(evictedSpy.count(), 1);
	QCOMPARE(model.index(model.firstRowForSeq(200)).data(ConversationModel::KindRole).toString(), u"skeleton"_s);
	QCOMPARE(model.index(model.firstRowForSeq(1210)).data(ConversationModel::KindRole).toString(), u"skeleton"_s);
	QVERIFY(model.index(model.firstRowForSeq(705)).data(ConversationModel::KindRole).toString() != u"skeleton"_s);
}

void TestConversationModel::configurableWindowAndNewestMissingRange()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);

	// Default window settings.
	QCOMPARE(model.messageWindowSize(), ConversationModel::MaxResidentMessages);
	QVERIFY(!model.isWindowUnlimited());
	QCOMPARE(model.pageSize(), 500);
	QCOMPARE(model.evictMargin(), 500);
	QCOMPARE(model.tailKeepMessages(), 1000);

	// Custom positive sizes.
	model.setMessageWindowSize(50);
	QCOMPARE(model.messageWindowSize(), 50);
	QCOMPARE(model.pageSize(), 50);
	QCOMPARE(model.evictMargin(), 25);
	QCOMPARE(model.tailKeepMessages(), 50);

	model.setMessageWindowSize(200);
	QCOMPARE(model.pageSize(), 100);
	QCOMPARE(model.evictMargin(), 100);
	QCOMPARE(model.tailKeepMessages(), 200);

	model.setMessageWindowSize(2500);
	QCOMPARE(model.pageSize(), ConversationModel::MaxServerPageSize);
	QCOMPARE(model.evictMargin(), 1250);
	QCOMPARE(model.tailKeepMessages(), 2500);

	// Negative clamps to 0 (unlimited).
	model.setMessageWindowSize(-10);
	QCOMPARE(model.messageWindowSize(), 0);
	QVERIFY(model.isWindowUnlimited());
	QCOMPARE(model.pageSize(), ConversationModel::MaxServerPageSize);
	QCOMPARE(model.tailKeepMessages(), 0);

	auto makeMetas = [](int count) {
		QList<refgui::proto::MessageLayoutMeta> list;
		list.reserve(count);
		for(int i = 0; i < count; ++i)
		{
			list.append(refgui::proto::MessageLayoutMeta{
				.seq = static_cast<quint64>(i),
				.id = u"m_%1"_s.arg(i),
				.role = (i % 2 == 0) ? refgui::proto::Role::User : refgui::proto::Role::Assistant,
				.sourceKind = u"human"_s,
				.parts = {refgui::proto::PartLayoutMeta{.kind = u"text"_s, .chars = 40, .lines = 1}},
			});
		}
		return list;
	};
	auto makeBatch = [](int start, int end) {
		QList<refgui::proto::Message> batch;
		batch.reserve(end - start);
		for(int i = start; i < end; ++i)
		{
			const QString id = u"m_%1"_s.arg(i);
			batch.append(i % 2 == 0 ? user(id, u"hi"_s) : assistant(id, {text(u"ok"_s)}));
		}
		return batch;
	};

	// Verify model invariants under QAbstractItemModelTester with a compact window.
	constexpr int SmallTotal = 80;
	model.reset({}, SmallTotal, SmallTotal, makeMetas(SmallTotal));
	QVERIFY(model.hasPlaceholders());
	model.materializeRange(0, makeBatch(0, SmallTotal));
	QCOMPARE(model.residentCount(), SmallTotal);
	QVERIFY(model.evictOutside(0, 5).isEmpty());
	QVERIFY(model.evictOldTail().isEmpty());
	QVERIFY(model.evictOldTail(10).isEmpty());

	model.setMessageWindowSize(20);
	const QStringList evictedOutside = model.evictOutside(40, 45);
	QVERIFY(!evictedOutside.isEmpty());
	QVERIFY(model.residentCount() <= 20);
	model.materializeRange(0, makeBatch(0, SmallTotal));
	model.evictOldTail();
	QCOMPARE(model.residentCount(), 20);
	QCOMPARE(model.index(0).data(ConversationModel::KindRole).toString(), u"skeleton"_s);
	QVERIFY(model.index(model.firstRowForSeq(SmallTotal - 20)).data(ConversationModel::KindRole).toString() != u"skeleton"_s);

	// newestMissingRange chunks newest-first up to 1000 and handles disjoint gaps without
	// O(N^2) QAbstractItemModelTester overhead across thousands of bulk row mutations.
	ConversationModel chunkModel;
	chunkModel.setMessageWindowSize(0);
	constexpr int Total = 2500;
	chunkModel.reset({}, Total, Total, makeMetas(Total));
	QVERIFY(chunkModel.hasPlaceholders());
	QCOMPARE(chunkModel.newestMissingRange(), (std::optional<std::pair<quint64, quint64>>{{1500, 2500}}));

	chunkModel.materializeRange(1500, makeBatch(1500, 2500));
	QCOMPARE(chunkModel.newestMissingRange(), (std::optional<std::pair<quint64, quint64>>{{500, 1500}}));
	// Partial materialization in the middle creates two disjoint gaps: [500, 700) and [900, 1500).
	chunkModel.materializeRange(700, makeBatch(700, 900));
	QCOMPARE(chunkModel.newestMissingRange(), (std::optional<std::pair<quint64, quint64>>{{900, 1500}}));
	chunkModel.materializeRange(900, makeBatch(900, 1500));
	QCOMPARE(chunkModel.newestMissingRange(), (std::optional<std::pair<quint64, quint64>>{{0, 700}}));
	chunkModel.materializeRange(0, makeBatch(0, 700));
	QVERIFY(!chunkModel.hasPlaceholders());
	QVERIFY(!chunkModel.newestMissingRange().has_value());
	QCOMPARE(chunkModel.residentCount(), Total);
	QVERIFY(chunkModel.evictOutside(0, 10).isEmpty());
	QVERIFY(chunkModel.evictOldTail().isEmpty());
	chunkModel.setMessageWindowSize(200);
	chunkModel.evictOldTail();
	QCOMPARE(chunkModel.residentCount(), 200);
}

void TestConversationModel::rowCountForSeqAndEvictionSignals()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);

	const QList<refgui::proto::MessageLayoutMeta> metas{
		refgui::proto::MessageLayoutMeta{
			.seq = 0,
			.id = u"m_0"_s,
			.role = refgui::proto::Role::User,
			.sourceKind = u"human"_s,
			.parts = {refgui::proto::PartLayoutMeta{.kind = u"text"_s, .chars = 20, .lines = 1}},
		},
	};
	const refgui::proto::Message multi = assistant(
		u"a2"_s,
		{reasoning(u"Thinking"_s), text(u"Intro\n\n```cpp\nint x = 1;\n```\n\nOutro"_s)});
	model.reset({user(u"u1"_s, u"go"_s), multi}, 3, 1, metas);
	model.setMessageWindowSize(1);

	QCOMPARE(model.rowCountForSeq(0ULL), 1);
	QCOMPARE(model.rowCountForSeq(1ULL), 2);
	QCOMPARE(model.rowCountForSeq(2ULL), 5);
	QCOMPARE(model.rowCountForSeq(3ULL), 0);
	QCOMPARE(model.rowCountForSeq(999ULL), 0);

	QStringList signalOrder;
	QSignalSpy aboutSpy(&model, &ConversationModel::aboutToEvict);
	QSignalSpy evictedSpy(&model, &ConversationModel::evicted);
	QSignalSpy idsSpy(&model, &ConversationModel::messagesEvicted);
	connect(&model, &ConversationModel::aboutToEvict, this, [&] { signalOrder.append(u"aboutToEvict"_s); });
	connect(&model, &ConversationModel::rowsRemoved, this, [&] { signalOrder.append(u"rowsRemoved"_s); });
	connect(&model, &ConversationModel::evicted, this, [&] { signalOrder.append(u"evicted"_s); });
	connect(&model, &ConversationModel::messagesEvicted, this, [&] { signalOrder.append(u"messagesEvicted"_s); });

	// No-op eviction (keepCount >= slotCount) emits zero eviction signals.
	QVERIFY(model.evictOldTail(10).isEmpty());
	QCOMPARE(aboutSpy.count(), 0);
	QCOMPARE(evictedSpy.count(), 0);
	QCOMPARE(idsSpy.count(), 0);

	// Evicting seq 2 emits symmetric aboutToEvict / evicted around model mutations.
	const QStringList evictedIds = model.evictOutside(1, 1);
	QCOMPARE(evictedIds, QStringList{u"a2"_s});
	QCOMPARE(aboutSpy.count(), 1);
	QCOMPARE(evictedSpy.count(), 1);
	QCOMPARE(idsSpy.count(), 1);
	QCOMPARE(model.rowCountForSeq(2ULL), 1);
	QCOMPARE(signalOrder, (QStringList{u"aboutToEvict"_s, u"rowsRemoved"_s, u"evicted"_s, u"messagesEvicted"_s}));

	// Repeating eviction when nothing eligible remains is a silent no-op.
	QVERIFY(model.evictOutside(1, 1).isEmpty());
	QCOMPARE(aboutSpy.count(), 1);
	QCOMPARE(evictedSpy.count(), 1);
	QCOMPARE(idsSpy.count(), 1);

	// An unended (streaming) message outside the keep window is never evicted.
	model.setMessage(assistant(u"a3"_s, {text(u"streaming"_s)}, false));
	QVERIFY(model.evictOutside(1, 1).isEmpty());
	QCOMPARE(aboutSpy.count(), 1);
	QCOMPARE(evictedSpy.count(), 1);
	QCOMPARE(idsSpy.count(), 1);
	QCOMPARE(model.rowCountForSeq(3ULL), 2);

	// Materializing across [placeholder (0), already-materialized (1), placeholder (2)] tracks `first` incrementally.
	model.materializeRange(0, {user(u"m_0"_s, u"first"_s), user(u"u1"_s, u"go"_s), multi});
	QCOMPARE(model.firstRowForSeq(0ULL), 0);
	QCOMPARE(model.rowCountForSeq(0ULL), 2);
	QCOMPARE(model.firstRowForSeq(1ULL), 2);
	QCOMPARE(model.rowCountForSeq(1ULL), 2);
	QCOMPARE(model.firstRowForSeq(2ULL), 4);
	QCOMPARE(model.rowCountForSeq(2ULL), 5);
	QCOMPARE(model.firstRowForSeq(3ULL), 9);
	QCOMPARE(model.index(model.firstRowForSeq(0ULL)).data(ConversationModel::ContentRole).toString(), u"first"_s);
	QCOMPARE(model.index(model.firstRowForSeq(1ULL)).data(ConversationModel::ContentRole).toString(), u"go"_s);
	QCOMPARE(model.index(model.firstRowForSeq(2ULL) + 2).data(ConversationModel::KindRole).toString(), u"code"_s);
	QCOMPARE(model.seqForRow(8), 2);
}

QTEST_GUILESS_MAIN(TestConversationModel)
#include "tst_conversation_model.moc"
