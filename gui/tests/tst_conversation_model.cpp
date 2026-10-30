// Adapted from an early version of qutie by LightBlueCube
#include "session/conversation_model.h"

#include <QAbstractItemModelTester>
#include <QSignalSpy>
#include <QTest>

using namespace Qt::StringLiterals;
using refgui::session::ConversationModel;

namespace
{

// Renders every block as "kind:content", plus "@language" for code blocks.
QStringList renderBlocks(const ConversationModel& model)
{
	QStringList rendered;
	for(int row = 0; row < model.rowCount(); ++row)
	{
		const QModelIndex index = model.index(row);
		QString block = index.data(ConversationModel::KindRole).toString() + u':' + index.data(ConversationModel::ContentRole).toString();
		if(index.data(ConversationModel::KindRole) == u"code"_s)
		{
			block += u'@' + index.data(ConversationModel::LanguageRole).toString();
		}
		rendered.append(block);
	}
	return rendered;
}

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
	void updatesWithoutReset();
	void appendsErrorAfterTurn();
};

void TestConversationModel::splitsAssistantTextIntoBlocks()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);

	model.appendUserMessage(u"Fix it"_s);
	model.appendAssistantText(u"Change this:\n\n```cmake\nqt_add_qml_module()\n```\n\nThen rebuild."_s);
	model.endAssistantTurn();

	const QStringList expected{
		u"user:Fix it"_s,
		u"markdown:Change this:"_s,
		u"code:qt_add_qml_module()@cmake"_s,
		u"markdown:Then rebuild."_s,
	};
	QCOMPARE(renderBlocks(model), expected);
}

void TestConversationModel::interleavesReasoningTextAndTools()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);

	model.appendReasoning(u"Need to look"_s);
	model.appendReasoning(u" at main."_s);
	model.appendAssistantText(u"Reading.\n```\nx"_s); // unclosed fence, ended by the tool call
	model.beginToolCall(u"read main.cpp"_s);
	model.endToolCall(false, u"no such file"_s);
	model.appendAssistantText(u"Not found."_s);
	model.endAssistantTurn();

	const QStringList expected{
		u"reasoning:Need to look at main."_s,
		u"markdown:Reading."_s,
		u"code:x@"_s,
		u"tool:read main.cpp"_s,
		u"markdown:Not found."_s,
	};
	QCOMPARE(renderBlocks(model), expected);

	const QModelIndex tool = model.index(3);
	QCOMPARE(tool.data(ConversationModel::OutputRole).toString(), u"no such file"_s);
	QVERIFY(tool.data(ConversationModel::FailedRole).toBool());
}

void TestConversationModel::marksOnlyTheLastBlockStreaming()
{
	ConversationModel model;

	model.appendAssistantText(u"a\n```\nx"_s);
	QCOMPARE(model.rowCount(), 2);
	QCOMPARE(streamingRowCount(model), 1);
	QVERIFY(model.index(1).data(ConversationModel::StreamingRole).toBool());

	model.endAssistantTurn();
	QCOMPARE(streamingRowCount(model), 0);
}

void TestConversationModel::updatesWithoutReset()
{
	ConversationModel model;
	QSignalSpy resets(&model, &QAbstractItemModel::modelReset);
	QSignalSpy inserts(&model, &QAbstractItemModel::rowsInserted);
	QSignalSpy changes(&model, &QAbstractItemModel::dataChanged);

	model.appendAssistantText(u"Hel"_s);
	model.appendAssistantText(u"lo"_s);

	QCOMPARE(resets.count(), 0);
	QCOMPARE(inserts.count(), 1);
	QCOMPARE(changes.count(), 2);
	QCOMPARE(changes.last().at(2).value<QList<int>>(), QList<int>{ConversationModel::ContentRole});
}

void TestConversationModel::appendsErrorAfterTurn()
{
	ConversationModel model;
	QAbstractItemModelTester tester(&model, QAbstractItemModelTester::FailureReportingMode::QtTest);

	model.appendAssistantText(u"Half a"_s);
	model.endAssistantTurn();
	model.appendError(u"Connection lost"_s);

	QCOMPARE(renderBlocks(model), (QStringList{u"markdown:Half a"_s, u"error:Connection lost"_s}));
	QCOMPARE(streamingRowCount(model), 0);
}

QTEST_GUILESS_MAIN(TestConversationModel)
#include "tst_conversation_model.moc"
