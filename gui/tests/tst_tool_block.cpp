#include "app/clipboard.h"
#include "app/link_colorizer.h"
#include "app/system_fonts.h"
#include "app/tool_info.h"
#include "icons/icon_provider.h"
#include "session/conversation_model.h"
#include "session_test_data.h"

#include <QCoreApplication>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QQuickItem>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QWheelEvent>
#include <QtTest>
#include <cmath>
#include <memory>
#include <utility>

using namespace Qt::StringLiterals;
using refgui::proto::ToolState;
using refgui::session::ConversationModel;

namespace
{

constexpr int WindowWidth = 800;
constexpr int WindowHeight = 600;
constexpr qreal HeaderHeight = 38.0;
constexpr qreal PagePadding = 24.0;
QQuickItem* findLoader(QQuickItem* parent)
{
	for(QQuickItem* child : parent->childItems())
	{
		if(child->metaObject()->className() == QByteArray("QQuickLoader"))
		{
			return child;
		}
	}
	return nullptr;
}
} // namespace


class StubSession : public QObject
{
	Q_OBJECT

public:
	explicit StubSession(ConversationModel& model, QObject* parent = nullptr)
		: QObject(parent)
		, m_model(model)
	{
	}

	Q_INVOKABLE int seqForRow(int row) const { return m_model.seqForRow(row); }
	Q_INVOKABLE int firstRowForSeq(int seq) const
	{
		return seq < 0 ? -1 : m_model.firstRowForSeq(static_cast<quint64>(seq));
	}
	Q_INVOKABLE int rowCountForSeq(int seq) const
	{
		return seq < 0 ? 0 : m_model.rowCountForSeq(static_cast<quint64>(seq));
	}
	Q_INVOKABLE void setVisibleSeqRange(int startSeq, int endSeq)
	{
		visibleRanges.append({startSeq, endSeq});
	}
	Q_INVOKABLE void setColumnWidth(double width) { m_model.setColumnWidth(width); }
	Q_INVOKABLE void setFollowing(bool value) { following = value; }

	QList<std::pair<int, int>> visibleRanges;
	bool following = true;

private:
	ConversationModel& m_model;
};

class TestToolBlock : public QObject
{
	Q_OBJECT

private slots:
	void initTestCase();
	void expandsAndCollapsesToolCard();
	void shrinkingContentKeepsExactPagePaddingFooter();
	void wheelAccumulatesAndBoostsSpeed();
	void wheelReversalTurnsAroundAtOnce();
	void touchpadCoastInertiaAndBounds();
	void touchpadReverseUpdateStopsCoast();
	void wheelGlideSurvivesContentShift();
	void reportVisibleRangeHandlesInterRowGaps();
	void materializePreservesMultiBlockRowAnchor();
	void evictPreservesRowAnchor();
};

void TestToolBlock::initTestCase()
{
	QQuickStyle::setStyle(u"Basic"_s);
}

void TestToolBlock::expandsAndCollapsesToolCard()
{
	QQmlEngine engine;
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);

	ConversationModel model;
	model.reset({
		refgui::test::user(u"u1"_s, u"Run ls"_s),
		refgui::test::assistant(u"a1"_s,
		                        {refgui::test::tool(u"bash"_s, u"List files"_s, ToolState::Done, u"alpha\nbeta\ngamma"_s)}),
	});

	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/ChatView.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));

	QQuickWindow window;
	window.resize(WindowWidth, WindowHeight);
	window.show();

	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* view = qobject_cast<QQuickItem*>(created.get());
	QVERIFY(view);
	view->setParentItem(window.contentItem());
	view->setWidth(WindowWidth);
	view->setHeight(WindowHeight);
	view->setProperty("model", QVariant::fromValue<QObject*>(&model));

	int toolRow = -1;
	for(int row = 0; row < model.rowCount(); ++row)
	{
		if(model.index(row).data(ConversationModel::KindRole).toString() == u"tool")
		{
			toolRow = row;
			break;
		}
	}
	QVERIFY(toolRow >= 0);

	QQuickItem* toolBlock = nullptr;
	QTRY_VERIFY([&] {
		QMetaObject::invokeMethod(view, "itemAtIndex", Q_RETURN_ARG(QQuickItem*, toolBlock), Q_ARG(int, toolRow));
		return toolBlock != nullptr;
	}());

	const qreal columnWidth = toolBlock->property("columnWidth").toReal();
	QVERIFY(columnWidth > 0);
	QVERIFY(!toolBlock->property("open").toBool());

	// Block.qml places its body inside `column` (the last child of Block).
	const QList<QQuickItem*> blockChildren = toolBlock->childItems();
	QVERIFY(!blockChildren.isEmpty());
	QQuickItem* column = blockChildren.last();
	QVERIFY(!column->childItems().isEmpty());
	QQuickItem* card = column->childItems().first();
	QCOMPARE(card->width(), columnWidth);

	// ToolBlock's card children: header, failure, previewBox, body, permission.
	const QList<QQuickItem*> cardChildren = card->childItems();
	QVERIFY(cardChildren.size() >= 4);
	QQuickItem* body = cardChildren.at(3);
	QQuickItem* bodyLoader = findLoader(body);
	QVERIFY(bodyLoader);

	const qreal collapsedHeight = card->height();
	QCOMPARE(collapsedHeight, HeaderHeight);
	QCOMPARE(body->height(), 0.0);

	// Expanding must grow `body` to `columnWidth` and show ToolBody inside the card.
	QVERIFY(QMetaObject::invokeMethod(toolBlock, "toggleExpanded"));
	QVERIFY(toolBlock->property("open").toBool());

	QQuickItem* toolBody = nullptr;
	QTRY_VERIFY([&] {
		toolBody = bodyLoader->property("item").value<QQuickItem*>();
		return toolBody != nullptr && toolBody->height() > 0 && qFuzzyCompare(body->height(), toolBody->height());
	}());
	QCOMPARE(body->width(), columnWidth);
	QCOMPARE(toolBody->width(), columnWidth);
	QVERIFY(card->height() > collapsedHeight);
	QCOMPARE(card->height(), collapsedHeight + toolBody->height());

	// Collapsing returns the card cleanly to its header height.
	QVERIFY(QMetaObject::invokeMethod(toolBlock, "toggleExpanded"));
	QVERIFY(!toolBlock->property("open").toBool());
	QTRY_COMPARE(body->height(), 0.0);
	QTRY_COMPARE(card->height(), collapsedHeight);

	view->setProperty("model", QVariant::fromValue<QObject*>(nullptr));
}

void TestToolBlock::shrinkingContentKeepsExactPagePaddingFooter()
{
	QQmlEngine engine;
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);

	QStringList longLines;
	for(int i = 1; i <= 30; ++i)
	{
		longLines.append(u"Tall streaming reasoning line %1 to exceed a quarter of the viewport height."_s.arg(i));
	}

	ConversationModel model;
	model.reset({
		refgui::test::user(u"u1"_s, u"Explain"_s),
		refgui::test::assistant(u"a1"_s, {refgui::test::reasoning(longLines.join(u'\n'), false)}, false),
	});

	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/ChatView.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));

	QQuickWindow window;
	window.resize(WindowWidth, WindowHeight);
	window.show();
	QVERIFY(QTest::qWaitForWindowExposed(&window));

	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* view = qobject_cast<QQuickItem*>(created.get());
	QVERIFY(view);
	view->setParentItem(window.contentItem());
	view->setWidth(WindowWidth);
	view->setHeight(WindowHeight);
	view->setProperty("model", QVariant::fromValue<QObject*>(&model));

	QVERIFY(!view->property("heldSpace").isValid());
	auto* footer = view->property("footerItem").value<QQuickItem*>();
	QVERIFY(footer);
	QCOMPARE(footer->height(), PagePadding);

	int reasoningRow = -1;
	for(int row = 0; row < model.rowCount(); ++row)
	{
		if(model.index(row).data(ConversationModel::KindRole).toString() == u"reasoning")
		{
			reasoningRow = row;
			break;
		}
	}
	QVERIFY(reasoningRow >= 0);

	QQuickItem* userBlock = nullptr;
	QQuickItem* reasoningBlock = nullptr;
	QTRY_VERIFY([&] {
		QMetaObject::invokeMethod(view, "itemAtIndex", Q_RETURN_ARG(QQuickItem*, userBlock), Q_ARG(int, 0));
		QMetaObject::invokeMethod(view, "itemAtIndex", Q_RETURN_ARG(QQuickItem*, reasoningBlock), Q_ARG(int, reasoningRow));
		return userBlock != nullptr && reasoningBlock != nullptr && reasoningBlock->height() > WindowHeight * 0.4;
	}());

	// Shrinking while streaming leaves no artificial held space in the footer.
	model.setPart(u"a1"_s, 0, refgui::test::reasoning(u"Short"_s, false));
	QTRY_VERIFY(reasoningBlock->height() < 100.0);
	QCOMPARE(footer->height(), PagePadding);
	QCOMPARE(view->property("naturalEnd").toReal(), footer->y() + PagePadding);

	// Auto-collapsing finished reasoning keeps footer strictly at pagePadding and keeps rows visible.
	model.setPart(u"a1"_s, 0, refgui::test::reasoning(longLines.join(u'\n'), false));
	QTRY_VERIFY(reasoningBlock->height() > 150.0);
	model.setPart(u"a1"_s, 0, refgui::test::reasoning(longLines.join(u'\n'), true));
	QVERIFY(!reasoningBlock->property("expanded").toBool());
	QTRY_VERIFY(reasoningBlock->height() < 60.0);
	QCOMPARE(footer->height(), PagePadding);

	const qreal contentY = view->property("contentY").toReal();
	QVERIFY(userBlock->y() >= contentY - 1.0);
	QVERIFY(reasoningBlock->y() + reasoningBlock->height() > contentY);
	QVERIFY(reasoningBlock->y() + reasoningBlock->height() <= contentY + view->height());

	view->setProperty("model", QVariant::fromValue<QObject*>(nullptr));
}

void TestToolBlock::wheelAccumulatesAndBoostsSpeed()
{
	QQmlEngine engine;
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);

	QList<refgui::proto::Message> messages;
	for(int i = 0; i < 20; ++i)
	{
		messages.append(refgui::test::user(u"u%1"_s.arg(i), u"Question %1 with enough text to fill the view."_s.arg(i)));
		messages.append(refgui::test::assistant(u"a%1"_s.arg(i), {refgui::test::text(u"Answer %1\nSecond line\nThird line"_s.arg(i))}));
	}

	ConversationModel model;
	model.reset(messages);

	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/ChatView.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));

	QQuickWindow window;
	window.resize(WindowWidth, WindowHeight);
	window.show();
	QVERIFY(QTest::qWaitForWindowExposed(&window));

	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* view = qobject_cast<QQuickItem*>(created.get());
	QVERIFY(view);
	view->setParentItem(window.contentItem());
	view->setWidth(WindowWidth);
	view->setHeight(WindowHeight);
	view->setProperty("model", QVariant::fromValue<QObject*>(&model));

	QTRY_VERIFY(view->property("maxContentY").toReal() > view->property("minContentY").toReal() + 600.0);
	QTRY_VERIFY(std::abs(view->property("contentY").toReal() - view->property("followTarget").toReal()) < 1.5);
	const qreal startY = std::floor(view->property("maxContentY").toReal()) - 20.0;
	QMetaObject::invokeMethod(view, "scrollTo", Q_ARG(QVariant, startY), Q_ARG(QVariant, false));
	QCOMPARE(view->property("contentY").toReal(), startY);

	const QPointF localCenter(WindowWidth / 2.0, WindowHeight / 2.0);
	const QPointF globalCenter = window.mapToGlobal(localCenter);
	QWheelEvent firstNotch(localCenter, globalCenter, QPoint(), QPoint(0, 120),
	                       Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
	QCoreApplication::sendEvent(&window, &firstNotch);
	QVERIFY(view->property("wheeling").toBool());
	QVERIFY(!view->property("following").toBool());
	const qreal firstTarget = view->property("wheelTarget").toReal();
	QVERIFY(firstTarget < startY);

	// Rapid second notch in the same direction builds boost and accumulates from wheelTarget.
	QWheelEvent secondNotch(localCenter, globalCenter, QPoint(), QPoint(0, 120),
	                        Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
	QCoreApplication::sendEvent(&window, &secondNotch);
	const qreal secondTarget = view->property("wheelTarget").toReal();
	const qreal wheelStep = view->property("wheelStep").toReal();
	QVERIFY(view->property("wheelBoost").toReal() > 1.0);
	QVERIFY(startY - secondTarget > 2.0 * wheelStep);

	QTRY_VERIFY(!view->property("wheeling").toBool());
	QVERIFY(std::abs(view->property("contentY").toReal() - secondTarget) < 1.5);

	// Reversing direction resets the boost to 1.0.
	QWheelEvent reverseNotch(localCenter, globalCenter, QPoint(), QPoint(0, -120),
	                         Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
	QCoreApplication::sendEvent(&window, &reverseNotch);
	QCOMPARE(view->property("wheelBoost").toReal(), 1.0);

	view->setProperty("model", QVariant::fromValue<QObject*>(nullptr));
}

void TestToolBlock::wheelReversalTurnsAroundAtOnce()
{
	QQmlEngine engine;
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);

	QList<refgui::proto::Message> messages;
	for(int i = 0; i < 20; ++i)
	{
		messages.append(refgui::test::user(u"u%1"_s.arg(i), u"Question %1 with enough text to fill the view."_s.arg(i)));
		messages.append(refgui::test::assistant(u"a%1"_s.arg(i), {refgui::test::text(u"Answer %1\nSecond line\nThird line"_s.arg(i))}));
	}

	ConversationModel model;
	model.reset(messages);

	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/ChatView.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));

	QQuickWindow window;
	window.resize(WindowWidth, WindowHeight);
	window.show();
	QVERIFY(QTest::qWaitForWindowExposed(&window));

	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* view = qobject_cast<QQuickItem*>(created.get());
	QVERIFY(view);
	view->setParentItem(window.contentItem());
	view->setWidth(WindowWidth);
	view->setHeight(WindowHeight);
	view->setProperty("model", QVariant::fromValue<QObject*>(&model));

	QTRY_VERIFY(view->property("maxContentY").toReal() > view->property("minContentY").toReal() + 600.0);
	const qreal minY = view->property("minContentY").toReal();
	const qreal maxY = view->property("maxContentY").toReal();
	const qreal midY = std::round((minY + maxY) / 2.0);
	QMetaObject::invokeMethod(view, "scrollTo", Q_ARG(QVariant, midY), Q_ARG(QVariant, false));
	QCOMPARE(view->property("contentY").toReal(), midY);

	const QPointF localCenter(WindowWidth / 2.0, WindowHeight / 2.0);
	const QPointF globalCenter = window.mapToGlobal(localCenter);
	const auto sendNotch = [&](int angleDeltaY) {
		QWheelEvent ev(localCenter, globalCenter, QPoint(), QPoint(0, angleDeltaY),
		               Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
		QCoreApplication::sendEvent(&window, &ev);
	};

	// A notch up starts the chase; no frame has stepped yet, so the content stands still.
	sendNotch(120);
	QVERIFY(view->property("wheeling").toBool());
	QCOMPARE(view->property("wheelTarget").toReal(), midY - view->property("wheelStep").toReal());

	// A notch down in mid-chase turns the target to the new side of the content at once,
	// instead of adding it to the far-away old target and travelling on the old way.
	sendNotch(-120);
	QVERIFY(view->property("wheeling").toBool());
	QVERIFY(view->property("wheelTarget").toReal() > view->property("contentY").toReal());
	QVERIFY(view->property("wheelTarget").toReal() > midY);
	QCOMPARE(view->property("wheelBoost").toReal(), 1.0); // the reversal reset the boost
	QTRY_VERIFY(view->property("contentY").toReal() > midY);
	QTRY_VERIFY(!view->property("wheeling").toBool());

	view->setProperty("model", QVariant::fromValue<QObject*>(nullptr));
}

void TestToolBlock::touchpadCoastInertiaAndBounds()
{
	QQmlEngine engine;
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);

	QList<refgui::proto::Message> messages;
	for(int i = 0; i < 25; ++i)
	{
		messages.append(refgui::test::user(u"u%1"_s.arg(i), u"Prompt %1"_s.arg(i)));
		messages.append(refgui::test::assistant(u"a%1"_s.arg(i), {refgui::test::text(u"Reply %1\nLine two\nLine three"_s.arg(i))}));
	}

	ConversationModel model;
	model.reset(messages);

	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/ChatView.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));

	QQuickWindow window;
	window.resize(WindowWidth, WindowHeight);
	window.show();
	QVERIFY(QTest::qWaitForWindowExposed(&window));

	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* view = qobject_cast<QQuickItem*>(created.get());
	QVERIFY(view);
	view->setParentItem(window.contentItem());
	view->setWidth(WindowWidth);
	view->setHeight(WindowHeight);
	view->setProperty("model", QVariant::fromValue<QObject*>(&model));

	QTRY_VERIFY(view->property("maxContentY").toReal() > view->property("minContentY").toReal() + 800.0);
	const qreal minY = view->property("minContentY").toReal();
	const qreal maxY = view->property("maxContentY").toReal();
	const qreal midY = std::round((minY + maxY) / 2.0);
	QMetaObject::invokeMethod(view, "scrollTo", Q_ARG(QVariant, midY), Q_ARG(QVariant, false));
	QCOMPARE(view->property("contentY").toReal(), midY);

	const QPointF localCenter(WindowWidth / 2.0, WindowHeight / 2.0);
	const QPointF globalCenter = window.mapToGlobal(localCenter);
	const auto sendPhase = [&](Qt::ScrollPhase phase, const QPoint& pixelDelta, const QPoint& angleDelta = {}) {
		QWheelEvent ev(localCenter, globalCenter, pixelDelta, angleDelta, Qt::NoButton, Qt::NoModifier, phase, false);
		QCoreApplication::sendEvent(&window, &ev);
	};

	// Upward flick (positive pixelDelta.y moves contentY up toward smaller values).
	sendPhase(Qt::ScrollBegin, QPoint());
	QTest::qWait(16);
	sendPhase(Qt::ScrollUpdate, QPoint(0, 36));
	QTest::qWait(16);
	sendPhase(Qt::ScrollUpdate, QPoint(0, 36));
	const qreal yBeforeEnd = view->property("contentY").toReal();
	sendPhase(Qt::ScrollEnd, QPoint());

	QVERIFY(view->property("coasting").toBool());
	QTRY_VERIFY(view->property("contentY").toReal() < yBeforeEnd);
	QTRY_VERIFY(!view->property("coasting").toBool());

	// Native ScrollMomentum cancels synthetic coasting immediately.
	QMetaObject::invokeMethod(view, "scrollTo", Q_ARG(QVariant, midY), Q_ARG(QVariant, false));
	QCOMPARE(view->property("contentY").toReal(), midY);
	sendPhase(Qt::ScrollBegin, QPoint());
	QTest::qWait(16);
	sendPhase(Qt::ScrollUpdate, QPoint(0, 36));
	QTest::qWait(16);
	sendPhase(Qt::ScrollUpdate, QPoint(0, 36));
	sendPhase(Qt::ScrollEnd, QPoint());
	QVERIFY(view->property("coasting").toBool());
	sendPhase(Qt::ScrollMomentum, QPoint(0, 8));
	QVERIFY(!view->property("coasting").toBool());

	// Mouse wheel cancels synthetic coasting immediately.
	QMetaObject::invokeMethod(view, "scrollTo", Q_ARG(QVariant, midY), Q_ARG(QVariant, false));
	QCOMPARE(view->property("contentY").toReal(), midY);
	sendPhase(Qt::ScrollBegin, QPoint());
	QTest::qWait(16);
	sendPhase(Qt::ScrollUpdate, QPoint(0, 36));
	QTest::qWait(16);
	sendPhase(Qt::ScrollUpdate, QPoint(0, 36));
	sendPhase(Qt::ScrollEnd, QPoint());
	QVERIFY(view->property("coasting").toBool());
	sendPhase(Qt::NoScrollPhase, QPoint(), QPoint(0, 120));
	QVERIFY(!view->property("coasting").toBool());

	// At top boundary, upward flick must not start coasting.
	QMetaObject::invokeMethod(view, "scrollTo", Q_ARG(QVariant, minY), Q_ARG(QVariant, false));
	QCOMPARE(view->property("contentY").toReal(), minY);
	sendPhase(Qt::ScrollBegin, QPoint());
	QTest::qWait(16);
	sendPhase(Qt::ScrollUpdate, QPoint(0, 40));
	sendPhase(Qt::ScrollEnd, QPoint());
	QVERIFY(!view->property("coasting").toBool());
	QCOMPARE(view->property("contentY").toReal(), minY);

	// Downward flick reaching maxContentY stops coasting and re-engages following.
	QMetaObject::invokeMethod(view, "scrollTo", Q_ARG(QVariant, maxY - 120.0), Q_ARG(QVariant, false));
	QVERIFY(!view->property("following").toBool());
	sendPhase(Qt::ScrollBegin, QPoint());
	QTest::qWait(16);
	sendPhase(Qt::ScrollUpdate, QPoint(0, -50));
	QTest::qWait(16);
	sendPhase(Qt::ScrollUpdate, QPoint(0, -50));
	sendPhase(Qt::ScrollEnd, QPoint());
	QTRY_VERIFY(!view->property("coasting").toBool());
	QTRY_VERIFY(view->property("following").toBool());
	QTRY_VERIFY(std::abs(view->property("contentY").toReal() - view->property("maxContentY").toReal()) < 1.5);

	view->setProperty("model", QVariant::fromValue<QObject*>(nullptr));
}

void TestToolBlock::touchpadReverseUpdateStopsCoast()
{
	QQmlEngine engine;
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);

	QList<refgui::proto::Message> messages;
	for(int i = 0; i < 25; ++i)
	{
		messages.append(refgui::test::user(u"u%1"_s.arg(i), u"Prompt %1"_s.arg(i)));
		messages.append(refgui::test::assistant(u"a%1"_s.arg(i), {refgui::test::text(u"Reply %1\nLine two\nLine three"_s.arg(i))}));
	}

	ConversationModel model;
	model.reset(messages);

	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/ChatView.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));

	QQuickWindow window;
	window.resize(WindowWidth, WindowHeight);
	window.show();
	QVERIFY(QTest::qWaitForWindowExposed(&window));

	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* view = qobject_cast<QQuickItem*>(created.get());
	QVERIFY(view);
	view->setParentItem(window.contentItem());
	view->setWidth(WindowWidth);
	view->setHeight(WindowHeight);
	view->setProperty("model", QVariant::fromValue<QObject*>(&model));

	QTRY_VERIFY(view->property("maxContentY").toReal() > view->property("minContentY").toReal() + 800.0);
	const qreal minY = view->property("minContentY").toReal();
	const qreal maxY = view->property("maxContentY").toReal();
	const qreal midY = std::round((minY + maxY) / 2.0);
	QMetaObject::invokeMethod(view, "scrollTo", Q_ARG(QVariant, midY), Q_ARG(QVariant, false));
	QCOMPARE(view->property("contentY").toReal(), midY);

	const QPointF localCenter(WindowWidth / 2.0, WindowHeight / 2.0);
	const QPointF globalCenter = window.mapToGlobal(localCenter);
	const auto sendPhase = [&](Qt::ScrollPhase phase, const QPoint& pixelDelta) {
		QWheelEvent ev(localCenter, globalCenter, pixelDelta, QPoint(), Qt::NoButton, Qt::NoModifier, phase, false);
		QCoreApplication::sendEvent(&window, &ev);
	};

	// Upward flick starts the synthetic coast.
	sendPhase(Qt::ScrollBegin, QPoint());
	QTest::qWait(16);
	sendPhase(Qt::ScrollUpdate, QPoint(0, 36));
	QTest::qWait(16);
	sendPhase(Qt::ScrollUpdate, QPoint(0, 36));
	sendPhase(Qt::ScrollEnd, QPoint());
	QVERIFY(view->property("coasting").toBool());
	QVERIFY(view->property("coastVelocity").toReal() < 0.0);

	// A downward update, with no ScrollBegin ahead of it (a platform that skips Begin),
	// stops the coast at once instead of letting it move against the fingers.
	sendPhase(Qt::ScrollUpdate, QPoint(0, -36));
	QVERIFY(!view->property("coasting").toBool());
	QCOMPARE(view->property("coastVelocity").toReal(), 0.0);

	view->setProperty("model", QVariant::fromValue<QObject*>(nullptr));
}

void TestToolBlock::wheelGlideSurvivesContentShift()
{
	QQmlEngine engine;
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);

	QList<refgui::proto::Message> messages;
	for(int i = 0; i < 30; ++i)
	{
		messages.append(refgui::test::user(u"u%1"_s.arg(i), u"Question %1 with enough text to fill the view."_s.arg(i)));
		messages.append(refgui::test::assistant(u"a%1"_s.arg(i), {refgui::test::text(u"Answer %1\nSecond line\nThird line"_s.arg(i))}));
	}

	ConversationModel model;
	model.reset(messages);

	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/ChatView.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));

	QQuickWindow window;
	window.resize(WindowWidth, WindowHeight);
	window.show();
	QVERIFY(QTest::qWaitForWindowExposed(&window));

	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* view = qobject_cast<QQuickItem*>(created.get());
	QVERIFY(view);
	view->setParentItem(window.contentItem());
	view->setWidth(WindowWidth);
	view->setHeight(WindowHeight);
	view->setProperty("model", QVariant::fromValue<QObject*>(&model));

	QTRY_VERIFY(view->property("maxContentY").toReal() > view->property("minContentY").toReal() + 1200.0);
	const qreal minY = view->property("minContentY").toReal();
	const qreal maxY = view->property("maxContentY").toReal();
	const qreal startY = std::floor(minY + (maxY - minY) * 0.75);
	QVERIFY(QMetaObject::invokeMethod(view, "scrollTo", Q_ARG(QVariant, startY), Q_ARG(QVariant, false)));
	QCOMPARE(view->property("contentY").toReal(), startY);

	const QPointF localCenter(WindowWidth / 2.0, WindowHeight / 2.0);
	const QPointF globalCenter = window.mapToGlobal(localCenter);
	for(int i = 0; i < 3; ++i)
	{
		QWheelEvent ev(localCenter, globalCenter, QPoint(), QPoint(0, 120),
		               Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
		QCoreApplication::sendEvent(&window, &ev);
	}
	QTest::qWait(20);
	QVERIFY(view->property("wheeling").toBool());
	const qreal remainingBeforeShift = view->property("wheelRemaining").toReal();
	QVERIFY(remainingBeforeShift < -50.0);

	// Simulate ListView originY/delegate-recycling shifting contentY past the old absolute target.
	const qreal shiftedY = view->property("contentY").toReal() - 200.0;
	QVERIFY(shiftedY > minY + 100.0);
	view->setProperty("contentY", shiftedY);
	const qreal targetAfterShift = view->property("wheelTarget").toReal();
	QVERIFY(targetAfterShift < shiftedY);
	QVERIFY(std::abs(targetAfterShift - (shiftedY + remainingBeforeShift)) < 2.0);

	QTRY_VERIFY(!view->property("wheeling").toBool());
	const qreal finalY = view->property("contentY").toReal();
	QVERIFY(finalY < shiftedY);
	QVERIFY(std::abs(finalY - targetAfterShift) < 2.0);

	view->setProperty("model", QVariant::fromValue<QObject*>(nullptr));
}

void TestToolBlock::reportVisibleRangeHandlesInterRowGaps()
{
	QQmlEngine engine;
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);

	QList<refgui::proto::Message> messages;
	for(int i = 0; i < 20; ++i)
	{
		messages.append(refgui::test::user(u"u%1"_s.arg(i), u"Prompt %1"_s.arg(i)));
		messages.append(refgui::test::assistant(u"a%1"_s.arg(i), {refgui::test::text(u"Reply %1\nSecond line"_s.arg(i))}));
	}
	const int lastSeq = static_cast<int>(messages.size()) - 1;

	ConversationModel model;
	model.reset(messages);
	StubSession stub(model);

	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/ChatView.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));

	QQuickWindow window;
	window.resize(WindowWidth, WindowHeight);
	window.show();
	QVERIFY(QTest::qWaitForWindowExposed(&window));

	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* view = qobject_cast<QQuickItem*>(created.get());
	QVERIFY(view);
	view->setParentItem(window.contentItem());
	view->setWidth(WindowWidth);
	view->setHeight(WindowHeight);
	view->setProperty("model", QVariant::fromValue<QObject*>(&model));
	view->setProperty("session", QVariant::fromValue<QObject*>(&stub));

	QTRY_VERIFY(view->property("maxContentY").toReal() > view->property("minContentY").toReal() + 600.0);
	const qreal midY = std::round((view->property("minContentY").toReal() + view->property("maxContentY").toReal()) / 2.0);
	QVERIFY(QMetaObject::invokeMethod(view, "scrollTo", Q_ARG(QVariant, midY), Q_ARG(QVariant, false)));
	QVERIFY(QMetaObject::invokeMethod(view, "forceLayout"));

	const int midRow = model.rowCount() / 2;
	QQuickItem* midItem = nullptr;
	QTRY_VERIFY([&] {
		QMetaObject::invokeMethod(view, "itemAtIndex", Q_RETURN_ARG(QQuickItem*, midItem), Q_ARG(int, midRow));
		return midItem != nullptr && midItem->height() > 0;
	}());

	// Place contentY squarely inside the 14px blockSpacing gap below midItem.
	const qreal gapY = midItem->y() + midItem->height() + 7.0;
	view->setProperty("contentY", gapY);
	QVERIFY(QMetaObject::invokeMethod(view, "forceLayout"));
	int indexAtGap = -1;
	QVERIFY(QMetaObject::invokeMethod(view, "indexAt", Q_RETURN_ARG(int, indexAtGap), Q_ARG(qreal, WindowWidth / 2.0), Q_ARG(qreal, gapY)));
	QCOMPARE(indexAtGap, -1);

	view->setProperty("lastReportedStart", -1);
	view->setProperty("lastReportedEnd", -1);
	stub.visibleRanges.clear();
	QVERIFY(QMetaObject::invokeMethod(view, "reportVisibleRange"));
	QVERIFY(!stub.visibleRanges.isEmpty());
	const auto [startSeq, endSeq] = stub.visibleRanges.last();
	QVERIFY(startSeq > 0);
	QVERIFY(endSeq < lastSeq);
	QVERIFY(startSeq <= endSeq);

	view->setProperty("session", QVariant::fromValue<QObject*>(nullptr));
	view->setProperty("model", QVariant::fromValue<QObject*>(nullptr));
}

void TestToolBlock::materializePreservesMultiBlockRowAnchor()
{
	QQmlEngine engine;
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);

	constexpr int PlaceholderCount = 10;
	QList<refgui::proto::MessageLayoutMeta> metas;
	metas.reserve(PlaceholderCount);
	for(int i = 0; i < PlaceholderCount; ++i)
	{
		metas.append(refgui::proto::MessageLayoutMeta{
			.seq = static_cast<quint64>(i),
			.id = u"m_%1"_s.arg(i),
			.role = (i % 2 == 0) ? refgui::proto::Role::User : refgui::proto::Role::Assistant,
			.sourceKind = u"human"_s,
			.parts = {refgui::proto::PartLayoutMeta{.kind = u"text"_s, .chars = 40, .lines = 1}},
		});
	}
	QList<refgui::proto::Message> tail;
	tail.append(refgui::test::user(u"m_10"_s, u"Show code"_s));
	tail.append(refgui::test::assistant(
		u"m_11"_s,
		{refgui::test::reasoning(u"Thinking line 1\nThinking line 2\nThinking line 3"_s),
		 refgui::test::text(u"First paragraph.\n\n```cpp\nint a = 1;\nint b = 2;\nint c = 3;\nint d = 4;\n```\n\nTrailing paragraph."_s)}));
	for(int i = 12; i < 20; ++i)
	{
		const QString id = u"m_%1"_s.arg(i);
		tail.append(i % 2 == 0 ? refgui::test::user(id, u"Followup %1"_s.arg(i))
		                       : refgui::test::assistant(id, {refgui::test::text(u"Reply %1\nLine 2\nLine 3"_s.arg(i))}));
	}

	ConversationModel model;
	model.reset(tail, static_cast<quint64>(PlaceholderCount + tail.size()), PlaceholderCount, metas);
	StubSession stub(model);

	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/ChatView.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));

	QQuickWindow window;
	window.resize(WindowWidth, WindowHeight);
	window.show();
	QVERIFY(QTest::qWaitForWindowExposed(&window));

	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* view = qobject_cast<QQuickItem*>(created.get());
	QVERIFY(view);
	view->setParentItem(window.contentItem());
	view->setWidth(WindowWidth);
	view->setHeight(WindowHeight);
	view->setProperty("model", QVariant::fromValue<QObject*>(&model));
	view->setProperty("session", QVariant::fromValue<QObject*>(&stub));

	QTRY_VERIFY(view->property("maxContentY").toReal() > view->property("minContentY").toReal() + 400.0);
	// Target block offset 2 (code block) of seq 11.
	constexpr int AnchorSeq = 11;
	constexpr int RowOffset = 2;
	const int anchorRowBefore = model.firstRowForSeq(AnchorSeq) + RowOffset;
	QCOMPARE(model.index(anchorRowBefore).data(ConversationModel::KindRole).toString(), u"code"_s);

	view->setProperty("following", false);
	QVERIFY(QMetaObject::invokeMethod(view, "positionViewAtIndex", Q_ARG(int, anchorRowBefore), Q_ARG(int, 0)));
	QQuickItem* anchorItemBefore = nullptr;
	QTRY_VERIFY([&] {
		QMetaObject::invokeMethod(view, "itemAtIndex", Q_RETURN_ARG(QQuickItem*, anchorItemBefore), Q_ARG(int, anchorRowBefore));
		return anchorItemBefore != nullptr && anchorItemBefore->height() > 20.0;
	}());
	constexpr qreal IntraOffset = 12.0;
	const qreal yBefore = anchorItemBefore->y() + IntraOffset;
	QVERIFY(QMetaObject::invokeMethod(view, "scrollTo", Q_ARG(QVariant, yBefore), Q_ARG(QVariant, false)));
	QVERIFY(!view->property("following").toBool());
	QVERIFY(std::abs(view->property("contentY").toReal() - yBefore) < 1.5);

	QVERIFY(QMetaObject::invokeMethod(view, "prepareMaterialize"));
	QCOMPARE(view->property("anchorSeq").toInt(), AnchorSeq);
	QCOMPARE(view->property("anchorRowOffset").toInt(), RowOffset);

	QList<refgui::proto::Message> batch;
	batch.reserve(PlaceholderCount);
	for(int i = 0; i < PlaceholderCount; ++i)
	{
		const QString id = u"m_%1"_s.arg(i);
		batch.append(i % 2 == 0 ? refgui::test::user(id, u"User %1\nLine 2\nLine 3\nLine 4"_s.arg(i))
		                        : refgui::test::assistant(id, {refgui::test::text(u"Assistant %1\n\n```txt\na\nb\nc\n```"_s.arg(i))}));
	}
	model.materializeRange(0, batch);
	QVERIFY(QMetaObject::invokeMethod(view, "finishMaterialize"));
	QTRY_VERIFY(!view->property("materializing").toBool());

	const int anchorRowAfter = model.firstRowForSeq(AnchorSeq) + RowOffset;
	QVERIFY(anchorRowAfter > anchorRowBefore);
	QQuickItem* anchorItemAfter = nullptr;
	QTRY_VERIFY([&] {
		QMetaObject::invokeMethod(view, "itemAtIndex", Q_RETURN_ARG(QQuickItem*, anchorItemAfter), Q_ARG(int, anchorRowAfter));
		return anchorItemAfter != nullptr;
	}());
	const qreal actualY = view->property("contentY").toReal();
	QVERIFY(std::abs(actualY - (anchorItemAfter->y() + IntraOffset)) < 1.5);
	QVariant topRowVar;
	QVERIFY(QMetaObject::invokeMethod(view, "rowNearY", Q_RETURN_ARG(QVariant, topRowVar), Q_ARG(QVariant, actualY + 2.0), Q_ARG(QVariant, 1)));
	QCOMPARE(topRowVar.toInt(), anchorRowAfter);

	view->setProperty("session", QVariant::fromValue<QObject*>(nullptr));
	view->setProperty("model", QVariant::fromValue<QObject*>(nullptr));
}

void TestToolBlock::evictPreservesRowAnchor()
{
	QQmlEngine engine;
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);

	constexpr int TotalMessages = 28;
	QList<refgui::proto::Message> messages;
	messages.reserve(TotalMessages);
	for(int i = 0; i < TotalMessages; ++i)
	{
		const QString id = u"m_%1"_s.arg(i);
		if(i % 2 == 0)
		{
			messages.append(refgui::test::user(id, u"Prompt %1"_s.arg(i)));
		}
		else
		{
			messages.append(refgui::test::assistant(
				id,
				{refgui::test::reasoning(u"Reasoning %1"_s.arg(i)),
				 refgui::test::text(u"Body %1.\n\n```cpp\nint v = %1;\nint w = v + 1;\n```\n\nDone."_s.arg(i))}));
		}
	}

	ConversationModel model;
	model.setMessageWindowSize(10);
	model.reset(messages);
	StubSession stub(model);

	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/ChatView.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));

	QQuickWindow window;
	window.resize(WindowWidth, WindowHeight);
	window.show();
	QVERIFY(QTest::qWaitForWindowExposed(&window));

	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* view = qobject_cast<QQuickItem*>(created.get());
	QVERIFY(view);
	view->setParentItem(window.contentItem());
	view->setWidth(WindowWidth);
	view->setHeight(WindowHeight);
	view->setProperty("model", QVariant::fromValue<QObject*>(&model));
	view->setProperty("session", QVariant::fromValue<QObject*>(&stub));

	QTRY_VERIFY(view->property("maxContentY").toReal() > view->property("minContentY").toReal() + 800.0);
	constexpr int AnchorSeq = 19;
	constexpr int RowOffset = 2;
	const int anchorRowBefore = model.firstRowForSeq(AnchorSeq) + RowOffset;
	QCOMPARE(model.index(anchorRowBefore).data(ConversationModel::KindRole).toString(), u"code"_s);

	view->setProperty("following", false);
	QVERIFY(QMetaObject::invokeMethod(view, "positionViewAtIndex", Q_ARG(int, anchorRowBefore), Q_ARG(int, 0)));
	QQuickItem* anchorItemBefore = nullptr;
	QTRY_VERIFY([&] {
		QMetaObject::invokeMethod(view, "itemAtIndex", Q_RETURN_ARG(QQuickItem*, anchorItemBefore), Q_ARG(int, anchorRowBefore));
		return anchorItemBefore != nullptr && anchorItemBefore->height() > 20.0;
	}());
	constexpr qreal IntraOffset = 10.0;
	const qreal yBefore = anchorItemBefore->y() + IntraOffset;
	QVERIFY(QMetaObject::invokeMethod(view, "scrollTo", Q_ARG(QVariant, yBefore), Q_ARG(QVariant, false)));
	QVERIFY(!view->property("following").toBool());
	QVERIFY(std::abs(view->property("contentY").toReal() - yBefore) < 1.5);

	QVERIFY(QMetaObject::invokeMethod(view, "prepareEvict"));
	const QStringList evicted = model.evictOutside(18, 22);
	QVERIFY(!evicted.isEmpty());
	QVERIFY(QMetaObject::invokeMethod(view, "finishEvict"));
	QTRY_VERIFY(!view->property("evicting").toBool());

	const int anchorRowAfter = model.firstRowForSeq(AnchorSeq) + RowOffset;
	QVERIFY(anchorRowAfter < anchorRowBefore);
	QQuickItem* anchorItemAfter = nullptr;
	QTRY_VERIFY([&] {
		QMetaObject::invokeMethod(view, "itemAtIndex", Q_RETURN_ARG(QQuickItem*, anchorItemAfter), Q_ARG(int, anchorRowAfter));
		return anchorItemAfter != nullptr;
	}());
	const qreal actualY = view->property("contentY").toReal();
	QVERIFY(std::abs(actualY - (anchorItemAfter->y() + IntraOffset)) < 1.5);
	QVariant topRowVar;
	QVERIFY(QMetaObject::invokeMethod(view, "rowNearY", Q_RETURN_ARG(QVariant, topRowVar), Q_ARG(QVariant, actualY + 2.0), Q_ARG(QVariant, 1)));
	QCOMPARE(topRowVar.toInt(), anchorRowAfter);

	view->setProperty("session", QVariant::fromValue<QObject*>(nullptr));
	view->setProperty("model", QVariant::fromValue<QObject*>(nullptr));
}
QTEST_MAIN(TestToolBlock)
#include "tst_tool_block.moc"
