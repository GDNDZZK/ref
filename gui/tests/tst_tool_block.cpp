#include "app/clipboard.h"
#include "app/link_colorizer.h"
#include "app/system_fonts.h"
#include "app/tool_info.h"
#include "icons/icon_provider.h"
#include "session/conversation_model.h"
#include "session_test_data.h"

#include <QQmlComponent>
#include <QQmlEngine>
#include <QQuickItem>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QtTest>
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
		refgui::test::assistant(u"a1"_s, {refgui::test::reasoning(u"Line 1\nLine 2\nLine 3\nLine 4"_s, false)}, false),
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
	view->setProperty("holding", true);
	view->setProperty("model", QVariant::fromValue<QObject*>(&model));

	QTRY_VERIFY(view->property("naturalEnd").toReal() > 0);
	const qreal initialHighWater = view->property("highWater").toReal();
	QVERIFY(initialHighWater > 0);

	// Shrinking while holding captures heldSpace.
	model.setPart(u"a1"_s, 0, refgui::test::reasoning(u"Short"_s, true));
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
QTEST_MAIN(TestToolBlock)
#include "tst_tool_block.moc"
