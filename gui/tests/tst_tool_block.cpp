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

using namespace Qt::StringLiterals;
using refgui::proto::ToolState;
using refgui::session::ConversationModel;

namespace
{

constexpr int WindowWidth = 800;
constexpr int WindowHeight = 600;
constexpr qreal HeaderHeight = 38.0;

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


class TestToolBlock : public QObject
{
	Q_OBJECT

private slots:
	void initTestCase();
	void expandsAndCollapsesToolCard();
	void modelResetClearsHeldSpace();
	void reasoningAutoCollapseWhileHoldingKeepsZeroHeldSpace();
	void heldSpaceRespectsViewportRatioCap();
	void wheelAccumulatesAndBoostsSpeed();
	void wheelReversalTurnsAroundAtOnce();
	void touchpadCoastInertiaAndBounds();
	void touchpadReverseUpdateStopsCoast();
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

void TestToolBlock::modelResetClearsHeldSpace()
{
	QQmlEngine engine;
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);
	ConversationModel model;
	model.reset({
		refgui::test::user(u"u1"_s, u"Explain"_s),
		refgui::test::assistant(u"a1"_s, {refgui::test::reasoning(u"Line 1\n\nLine 2\n\nLine 3\n\nLine 4\n\nLine 5"_s, false)}, false),
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
	view->setProperty("holding", true);
	view->setProperty("model", QVariant::fromValue<QObject*>(&model));

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

	QQuickItem* reasoningBlock = nullptr;
	QTRY_VERIFY([&] {
		QMetaObject::invokeMethod(view, "itemAtIndex", Q_RETURN_ARG(QQuickItem*, reasoningBlock), Q_ARG(int, reasoningRow));
		return reasoningBlock != nullptr && reasoningBlock->height() > 60.0;
	}());
	QTRY_VERIFY(view->property("naturalEnd").toReal() > 0);
	const qreal initialHighWater = view->property("highWater").toReal();
	QVERIFY(initialHighWater > 0);

	// Shrinking while streaming and holding captures heldSpace.
	model.setPart(u"a1"_s, 0, refgui::test::reasoning(u"Short"_s, false));
	QTRY_VERIFY(view->property("heldSpace").toReal() > 0);

	// Resetting the model (e.g. Reset event or reattach) must drop stale hold state.
	model.reset({refgui::test::user(u"u1"_s, u"Explain"_s)});
	QCOMPARE(view->property("heldSpace").toReal(), 0.0);
	QTRY_COMPARE(view->property("highWater").toReal(), view->property("naturalEnd").toReal());
	QVERIFY(view->property("highWater").toReal() < initialHighWater);

	model.reset({});
	QCOMPARE(view->property("heldSpace").toReal(), 0.0);
	QTRY_COMPARE(view->property("highWater").toReal(), view->property("naturalEnd").toReal());

	view->setProperty("holding", false);
	model.reset({});
	QCOMPARE(view->property("heldSpace").toReal(), 0.0);
	QCOMPARE(view->property("highWater").toReal(), 0.0);

	view->setProperty("model", QVariant::fromValue<QObject*>(nullptr));
}

void TestToolBlock::reasoningAutoCollapseWhileHoldingKeepsZeroHeldSpace()
{
	QQmlEngine engine;
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);

	QStringList longLines;
	for(int i = 1; i <= 18; ++i)
	{
		longLines.append(u"Reasoning step %1 with extra words to make the block tall."_s.arg(i));
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
	view->setProperty("holding", true);
	view->setProperty("model", QVariant::fromValue<QObject*>(&model));

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
		return userBlock != nullptr && reasoningBlock != nullptr && reasoningBlock->height() > 150.0;
	}());
	QVERIFY(reasoningBlock->property("expanded").toBool());

	// Auto-collapse when reasoning finishes while the turn keeps running (holding stays true).
	model.setPart(u"a1"_s, 0, refgui::test::reasoning(longLines.join(u'\n'), true));
	QVERIFY(!reasoningBlock->property("expanded").toBool());

	QTRY_VERIFY(reasoningBlock->height() < 60.0);
	QTRY_COMPARE(view->property("heldSpace").toReal(), 0.0);
	QCOMPARE(view->property("heldSpace").toReal(), 0.0);

	// The collapsed header and user message remain inside the viewport rather than being pushed off-screen.
	const qreal contentY = view->property("contentY").toReal();
	QVERIFY(userBlock->y() >= contentY - 1.0);
	QVERIFY(reasoningBlock->y() + reasoningBlock->height() > contentY);
	QVERIFY(reasoningBlock->y() + reasoningBlock->height() <= contentY + view->height());

	view->setProperty("model", QVariant::fromValue<QObject*>(nullptr));
}

void TestToolBlock::heldSpaceRespectsViewportRatioCap()
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
	view->setProperty("holding", true);
	view->setProperty("model", QVariant::fromValue<QObject*>(&model));

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

	QQuickItem* reasoningBlock = nullptr;
	QTRY_VERIFY([&] {
		QMetaObject::invokeMethod(view, "itemAtIndex", Q_RETURN_ARG(QQuickItem*, reasoningBlock), Q_ARG(int, reasoningRow));
		return reasoningBlock != nullptr && reasoningBlock->height() > WindowHeight * 0.4;
	}());

	// Shrink while remaining streaming so holdEnd() caps the held space to 0.25 * height.
	model.setPart(u"a1"_s, 0, refgui::test::reasoning(u"Short"_s, false));
	QTRY_VERIFY(view->property("heldSpace").toReal() > 0.0);
	QTRY_VERIFY(reasoningBlock->height() < 100.0);

	const qreal maxAllowed = std::round(WindowHeight * 0.25);
	QVERIFY2(view->property("heldSpace").toReal() <= maxAllowed,
	         qPrintable(u"heldSpace=%1 maxAllowed=%2"_s.arg(view->property("heldSpace").toReal()).arg(maxAllowed)));

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
QTEST_MAIN(TestToolBlock)
#include "tst_tool_block.moc"
