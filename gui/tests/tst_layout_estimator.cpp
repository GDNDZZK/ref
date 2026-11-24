#include "session/layout_estimator.h"

#include <QtTest>

using namespace Qt::StringLiterals;
using namespace refgui::proto;
using refgui::session::LayoutEstimator;

class TestLayoutEstimator : public QObject
{
	Q_OBJECT

private slots:
	void narrowerColumnIncreasesWrappedTextHeight()
	{
		MessageLayoutMeta meta;
		meta.seq = 1;
		meta.id = u"m1"_s;
		meta.role = Role::Assistant;
		meta.parts = {PartLayoutMeta{
			.kind = u"text"_s,
			.chars = 800,
			.lines = 4,
		}};

		const double wide = LayoutEstimator::estimateMessageHeight(meta, 760.0, false, false);
		const double narrow = LayoutEstimator::estimateMessageHeight(meta, 320.0, false, false);
		QVERIFY(narrow > wide);
	}

	void expansionTogglesAffectReasoningAndToolHeights()
	{
		MessageLayoutMeta meta;
		meta.seq = 2;
		meta.id = u"m2"_s;
		meta.role = Role::Assistant;
		meta.parts = {
			PartLayoutMeta{.kind = u"reasoning"_s, .chars = 400, .lines = 8},
			PartLayoutMeta{.kind = u"tool"_s, .tool = u"bash"_s, .toolLines = 10},
		};

		const double collapsed = LayoutEstimator::estimateMessageHeight(meta, 760.0, false, false);
		const double withThinking = LayoutEstimator::estimateMessageHeight(meta, 760.0, true, false);
		const double withBoth = LayoutEstimator::estimateMessageHeight(meta, 760.0, true, true);
		QVERIFY(withThinking > collapsed);
		QVERIFY(withBoth > withThinking);
	}

	void computeMetaExtractsCodeTablesToolsAndImages()
	{
		Message msg;
		msg.id = u"a1"_s;
		msg.role = Role::Assistant;
		msg.parts = {
			ReasoningPart{.text = u"step 1\nstep 2"_s, .time = 1},
			TextPart{.text = u"Intro\n\n```cpp\nint x = 1;\nint y = 2;\n```\n\n| A | B |\n| - | - |\n| 1 | 2 |"_s, .time = 2},
			ToolPart{.callId = u"c1"_s, .name = u"todo"_s, .state = ToolState::Done, .output = u"1\n2\n3"_s},
			ImagePart{.image = ImageData{.mime = u"image/png"_s}},
		};

		const MessageLayoutMeta meta = LayoutEstimator::computeMeta(5, msg);
		QCOMPARE(meta.seq, quint64(5));
		QCOMPARE(meta.parts.size(), 4);
		QCOMPARE(meta.parts.at(0).kind, u"reasoning"_s);
		QCOMPARE(meta.parts.at(1).codeBlocks, quint16(1));
		QCOMPARE(meta.parts.at(1).codeLines, quint32(2));
		QCOMPARE(meta.parts.at(1).tables, quint16(1));
		QCOMPARE(meta.parts.at(2).kind, u"tool"_s);
		QCOMPARE(meta.parts.at(2).tool, u"todo"_s);
		QCOMPARE(meta.parts.at(2).toolLines, quint16(3));
		QCOMPARE(meta.parts.at(3).kind, u"image"_s);

		const QVariantList skel = LayoutEstimator::skeletonParts(meta);
		QCOMPARE(skel.size(), 4);
		QCOMPARE(skel.at(2).toMap().value(u"tool"_s).toString(), u"todo"_s);
	}
};

QTEST_GUILESS_MAIN(TestLayoutEstimator)
#include "tst_layout_estimator.moc"
