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
	QVERIFY(roleOf(model, model.rowCount() - 1, ConversationModel::StreamingRole).toBool());

	model.setMessage(assistant(u"a2"_s, {text(u"Running"_s), tool(u"bash"_s, u"x"_s, ToolState::Running)}, false));
	const int tools = model.rowCount() - 1;
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
	QCOMPARE(inserted.first().at(2).toInt(), 1);
	QCOMPARE(resets.count(), 0);
	QVERIFY(roleOf(model, 2, ConversationModel::OpensTurnRole).toBool());
	QCOMPARE(changed.count(), 1); // the former first row now opens a turn
	QCOMPARE(changed.first().at(0).value<QModelIndex>().row(), 2);
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
	QCOMPARE(inserted.first().at(1).toInt(), 2);
	QCOMPARE(removed.count(), 0);
	QCOMPARE(resets.count(), 0);
	QCOMPARE(render(model), (QStringList{u"user:go"_s, u"markdown:First."_s, u"tool:bash run"_s}));
}

QTEST_GUILESS_MAIN(TestConversationModel)
#include "tst_conversation_model.moc"
