#include "app/app_translator.h"
#include "app/chat_image_provider.h"
#include "app/chat_session.h"
#include "app/clipboard.h"
#include "app/composer_model_type.h"
#include "app/link_colorizer.h"
#include "app/local_only_network.h"
#include "app/notification_center_type.h"
#include "app/question_prompt_type.h"
#include "app/qml_types.h"
#include "app/system_fonts.h"
#include "app/tile_model.h"
#include "app/tile_stage.h"
#include "app/tool_info.h"
#include "app/workspace.h"
#include "fake_backend.h"
#include "icons/icon_provider.h"
#include "notify/notification_center.h"
#include "notify/notifier.h"
#include "session_test_data.h"

#include <QCoreApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include "workspace/config_json_patcher.h"
#include <QJsonObject>
#include <QQmlComponent>
#include <QPointer>
#include <QQmlEngine>
#include <QQmlError>
#include <QQuickItem>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QWheelEvent>
#include <QtTest>
#include <memory>

using namespace Qt::StringLiterals;
using namespace refgui;
using namespace refgui::test;
using refgui::client::Backend;

namespace
{

// A 1x1 transparent PNG, the picture the stub serves for every get_image.
const QString Png1x1Base64 = u"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="_s;

class StubMainServer
{
public:
	explicit StubMainServer(FakeBackend& fake)
		: m_fake(fake)
	{
		fake.onRequest = [this](const QJsonObject& request) { answer(request); };
		fake.onHello = [this](const QJsonObject& hello) { m_fake.respondHello(hello, refgui::proto::ProtocolVersion); };
	}

	void add(const QString& id, qint64 created)
	{
		m_sessions.insert(id, sessionInfoJson(id, {.title = u"Session "_s + id, .createdAt = created}));
	}

	void setMessages(const QString& id, const QJsonArray& messages) { m_messages.insert(id, messages); }
	QString savedConfigRawText() const { return m_configRawText; }
	int saveConfigCalls() const { return m_saveConfigCalls; }
	int setAgentCalls() const { return m_setAgentCalls; }
	QStringList shellCommands() const { return m_shellCommands; }
	QList<QJsonObject> listDirsRequests() const { return m_listDirsRequests; }
private:
	QJsonArray list() const
	{
		QJsonArray array;
		for(const QJsonObject& info : m_sessions)
		{
			array.append(info);
		}
		return array;
	}

	void answer(const QJsonObject& request)
	{
		const QString op = request.value(u"op"_s).toString();
		const QString id = request.value(u"session_id"_s).toString();
		if(op == u"attach"_s)
		{
			QJsonObject info = m_sessions.value(id);
			info[u"attached"_s] = request.value(u"foreground"_s).toBool() ? 1 : 0;
			m_sessions[id] = info;
			m_fake.respond(request, {{u"snapshot"_s, QJsonObject{{u"info"_s, info},
			                                                            {u"messages"_s, m_messages.value(id)},
			                                                            {u"pending_questions"_s, QJsonArray()},
			                                                            {u"more_before"_s, false}}}});
			m_fake.send({{u"kind"_s, u"sessions"_s}, {u"sessions"_s, list()}});
		}
		else if(op == u"detach"_s || op == u"background"_s)
		{
			if(m_sessions.contains(id))
			{
				m_sessions[id][u"attached"_s] = 0;
			}
			m_fake.respond(request, {});
		}
		else if(op == u"list_sessions"_s || op == u"list_history"_s || op == u"watch_all"_s)
		{
			m_fake.respond(request, {{u"sessions"_s, list()}});
		}
		else if(op == u"get_config"_s)
		{
			m_fake.respond(request,
			               {{u"config"_s,
			                 QJsonObject{{u"default_agent"_s, u"build"_s},
			                             {u"agents"_s, QJsonArray{QJsonObject{{u"name"_s, u"build"_s}, {u"description"_s, u"Build"_s}},
			                                                      QJsonObject{{u"name"_s, u"plan"_s}, {u"description"_s, u"Plan"_s}}}},
			                             {u"models"_s, QJsonArray()},
			                             {u"commands"_s, QJsonArray{QJsonObject{{u"name"_s, u"review"_s}, {u"description"_s, u"Review diff"_s}, {u"source"_s, u"config"_s}}}}}}});
		}
		else if(op == u"set_agent"_s)
		{
			++m_setAgentCalls;
			m_sessions[id][u"agent"_s] = request.value(u"agent"_s);
			m_fake.respond(request, {{u"session"_s, m_sessions.value(id)}});
		}
		else if(op == u"shell"_s)
		{
			m_shellCommands.append(request.value(u"command"_s).toString());
			m_fake.respond(request, {});
		}
		else if(op == u"list_dirs"_s)
		{
			m_listDirsRequests.append(request);
			const bool wantFiles = request.value(u"include_files"_s).toBool();
			m_fake.respond(request, {{u"path"_s, u"/work"_s}, {u"dirs"_s, QJsonArray{u"docs"_s, u"src"_s}}, {u"files"_s, wantFiles ? QJsonArray{u"README.md"_s} : QJsonArray()}});
		}
		else if(op == u"list_binaries"_s)
		{
			m_fake.respond(request, {{u"binaries"_s, QJsonArray{u"cargo"_s, u"cat"_s, u"catfish"_s}}});
		}
		else if(op == u"mcp_status"_s)
		{
			m_fake.respond(request, {{u"servers"_s, QJsonArray()}});
		}
		else if(op == u"get_config_files"_s || op == u"save_config_file"_s || op == u"create_config_file"_s || op == u"select_config_file"_s || op == u"delete_config_file"_s)
		{
			if(op == u"save_config_file"_s)
			{
				m_configRawText = request.value(u"content"_s).toString();
				++m_saveConfigCalls;
			}
			const auto parsed = refgui::workspace::ConfigJsonPatcher::parseJsonc(m_configRawText);
			const QJsonObject rootObj = parsed ? parsed->root() : QJsonObject{};
			const QJsonObject fileEntry{
				{u"file"_s, u"ref.json"_s},
				{u"active"_s, true},
				{u"exists"_s, true},
				{u"label"_s, rootObj.value(u"$name"_s)},
				{u"summary"_s, u"anthropic/claude-sonnet-4-5"_s},
				{u"modified_at"_s, 1700000000},
				{u"size"_s, m_configRawText.toUtf8().size()},
			};
			QJsonObject infoObj{
				{u"config_dir"_s, u"/tmp/ref-cfg"_s},
				{u"files"_s, QJsonArray{fileEntry}},
				{u"selected_file"_s, u"ref.json"_s},
				{u"raw_text"_s, m_configRawText},
				{u"parsed"_s, parsed ? QJsonValue(rootObj) : QJsonValue::Null},
				{u"has_jsonc_override"_s, false},
				{u"project_overrides"_s, QJsonArray{}},
				{u"warnings"_s, QJsonArray{}},
				{u"env_status"_s, QJsonObject{{u"ANTHROPIC_API_KEY"_s, true}}},
			};
			if(!parsed)
			{
				infoObj.insert(u"parse_error"_s, parsed.error().message);
			}
			m_fake.respond(request, infoObj);
		}
		else if(op == u"get_image"_s)
		{
			m_fake.respond(request, {{u"image"_s, QJsonObject{{u"mime"_s, u"image/png"_s}, {u"data"_s, Png1x1Base64}, {u"id"_s, request.value(u"image_id"_s)}}}});
		}
		else if(op == u"create_session"_s)
		{
			const QString createdId = u"ses_created_%1"_s.arg(++m_created);
			QJsonObject info = sessionInfoJson(createdId, {.title = u"Session "_s + createdId, .cwd = request.value(u"cwd"_s).toString(u"/work"_s), .createdAt = 100 + m_created});
			m_sessions.insert(createdId, info);
			m_fake.respond(request, {{u"session"_s, info}});
			m_fake.send({{u"kind"_s, u"sessions"_s}, {u"sessions"_s, list()}});
		}
		else
		{
			m_fake.respond(request, {});
		}
	}

	FakeBackend& m_fake;
	QMap<QString, QJsonObject> m_sessions;
	QMap<QString, QJsonArray> m_messages;
	int m_created = 0;
	QString m_configRawText = u"// Keep user header comment\n{\n  \"model\": \"anthropic/claude-sonnet-4-5\",\n  \"extraFutureSetting\": {\"preserve\": true}\n}\n"_s;
	int m_saveConfigCalls = 0;
	int m_setAgentCalls = 0;
	QStringList m_shellCommands;
	QList<QJsonObject> m_listDirsRequests;
};

QJsonArray manyMessagesJson()
{
	QJsonArray messages;
	for(int i = 0; i < 24; ++i)
	{
		messages.append(QJsonObject{{u"id"_s, u"u%1"_s.arg(i)},
		                            {u"role"_s, u"user"_s},
		                            {u"time"_s, 10 + i * 2},
		                            {u"parts"_s, QJsonArray{QJsonObject{{u"type"_s, u"text"_s}, {u"text"_s, u"Prompt %1"_s.arg(i)}, {u"time"_s, 10 + i * 2}}}}});
		messages.append(QJsonObject{{u"id"_s, u"a%1"_s.arg(i)},
		                            {u"role"_s, u"assistant"_s},
		                            {u"time"_s, 11 + i * 2},
		                            {u"time_end"_s, 12 + i * 2},
		                            {u"parts"_s, QJsonArray{QJsonObject{{u"type"_s, u"text"_s}, {u"text"_s, u"Reply %1\nLine 2\nLine 3"_s.arg(i)}, {u"time"_s, 11 + i * 2}}}}});
	}
	return messages;
}

template<typename Predicate>
QQuickItem* findItem(QQuickItem* root, Predicate&& predicate)
{
	if(root == nullptr)
	{
		return nullptr;
	}
	if(predicate(root))
	{
		return root;
	}
	for(QQuickItem* child : root->childItems())
	{
		if(QQuickItem* found = findItem(child, predicate))
		{
			return found;
		}
	}
	return nullptr;
}

QQuickItem* findByTypePrefix(QQuickItem* root, const char* prefix)
{
	return findItem(root, [prefix](QQuickItem* item) {
		return QByteArray(item->metaObject()->className()).startsWith(prefix);
	});
}

void collectTiles(QQuickItem* item, QList<QQuickItem*>& tiles)
{
	if(QByteArray(item->metaObject()->className()).startsWith("Tile_QMLTYPE"))
	{
		tiles.append(item);
	}
	for(QQuickItem* child : item->childItems())
	{
		collectTiles(child, tiles);
	}
}

// Whether every tile sits on the rect the grid computes for its place, as TileGrid's delegate
// does; a tile that stopped following its slot (a frozen binding) misses after the grid moved.
bool tilesOnTheGrid(QQuickItem* windowContent, QQuickItem* grid)
{
	constexpr qreal Tolerance = 0.5;
	QList<QQuickItem*> tiles;
	collectTiles(windowContent, tiles);
	bool any = false;
	for(QQuickItem* tile : tiles)
	{
		if(tile->property("leaving").toBool())
		{
			continue;
		}
		const int columns = grid->property("columns").toInt();
		const int rows = grid->property("rows").toInt();
		const int count = grid->property("count").toInt();
		const qreal padding = grid->property("padding").toReal();
		const qreal gap = grid->property("gap").toReal();
		const qreal tileHeight = grid->property("tileHeight").toReal();
		const int place = tile->property("place").toInt();
		const int row = place / columns;
		const int inRow = std::max(1, row == rows - 1 ? count - row * columns : columns);
		const qreal slotWidth = (grid->width() - 2 * padding - (inRow - 1) * gap) / inRow;
		const qreal slotX = padding + (place - row * columns) * (slotWidth + gap);
		const qreal slotY = grid->property("contentTop").toReal() + row * (tileHeight + gap);
		if(std::abs(tile->x() - slotX) > Tolerance || std::abs(tile->y() - slotY) > Tolerance
		   || std::abs(tile->width() - slotWidth) > Tolerance || std::abs(tile->height() - tileHeight) > Tolerance)
		{
			return false;
		}
		any = true;
	}
	return any;
}

struct MainFixture
{
	MainFixture()
	{
		stub.add(u"ses_a"_s, 1);
		stub.add(u"ses_b"_s, 2);
		stub.setMessages(u"ses_a"_s, manyMessagesJson());
		stub.setMessages(u"ses_b"_s, manyMessagesJson());

		refgui::cli::LaunchOptions options;
		options.directory = u"/work"_s;
		options.source = Backend::Mode::Remote{{fake.url(), u"token"_s}};
		options.start = {.kind = refgui::cli::SessionStart::Kind::Attach, .sessionId = u"ses_a"_s};
		workspace = std::make_unique<Workspace>(options, dir.filePath(u"gui.json"_s));
		Workspace::setInstance(workspace.get());

		notifications = std::make_unique<refgui::notify::NotificationCenter>(workspace->connection().backend(),
		                                                                     refgui::notify::createPlatformNotifier());
		NotificationCenterForeign::instance = notifications.get();
		QTRY_VERIFY(workspace->current() != nullptr);
	}

	~MainFixture()
	{
		QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
		NotificationCenterForeign::instance = nullptr;
		Workspace::setInstance(nullptr);
	}
	QTemporaryDir dir;
	FakeBackend fake;
	StubMainServer stub{fake};
	std::unique_ptr<Workspace> workspace;
	std::unique_ptr<refgui::notify::NotificationCenter> notifications;
};

} // namespace

class TestMainWindow : public QObject
{
	Q_OBJECT

private slots:
	void initTestCase();
	void mainWindowInstantiatesFramelessAndBlocksModalWheel();
	void tileHeaderInsetsAndRoundsInsideFrameBorder();
	void splitViewQuickToggleKeepsTilesOnTheGrid();
	void sidebarFolderButtonDoesNotTriggerNewSession();
	void standaloneDialogHostBlocksWheelEvents();
	void statusLineRetryTooltipShowsFullErrorWithoutWarnings();
	void emptyWorkspaceStateAndDetachTransitionCleanly();
	void tileZoomRemovesBorderRepeatsSlowTravelAndCompensatesScroll();
	void reusableConfigFormControlsInstantiateAndOperateWithoutWarnings();
	void configDialogSwitchesCategoriesEditsVisualAndJsoncAndGuardsDirtyClose();
	void paletteAndComposerCompletionsAndShellModeOperateWithoutWarnings();
};

void TestMainWindow::initTestCase()
{
	QQuickStyle::setStyle(u"Basic"_s);
}

void TestMainWindow::mainWindowInstantiatesFramelessAndBlocksModalWheel()
{
	MainFixture f;

	QQmlEngine engine;
	QSignalSpy warningSpy(&engine, &QQmlEngine::warnings);
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);
	engine.addImageProvider(u"chatimage"_s, new ChatImageProvider([ws = QPointer<Workspace>(f.workspace.get())](const QString& id) {
		return ws ? ws->session(id) : nullptr;
	}));
	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/Main.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));

	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* window = qobject_cast<QQuickWindow*>(created.get());
	QVERIFY(window);
	QVERIFY(QTest::qWaitForWindowExposed(window));

	QVERIFY2(warningSpy.isEmpty(), "Main.qml emitted QML warnings during instantiation");
#ifndef Q_OS_MACOS
	QVERIFY(window->flags().testFlag(Qt::FramelessWindowHint));
	QQuickItem* windowControls = findByTypePrefix(window->contentItem(), "WindowControls");
	QVERIFY(windowControls);
	QVERIFY(windowControls->isVisible());
	QQuickItem* resizeHandles = findByTypePrefix(window->contentItem(), "WindowResizeHandles");
	QVERIFY(resizeHandles);
	QVERIFY(resizeHandles->property("active").toBool());
#endif

	QQuickItem* tileGrid = findByTypePrefix(window->contentItem(), "TileGrid");
	QVERIFY(tileGrid);
	QCOMPARE(tileGrid->property("topInset").toInt(), 0);

	QQuickItem* chatView = nullptr;
	QTRY_VERIFY([&] {
		chatView = findByTypePrefix(window->contentItem(), "ChatView");
		return chatView != nullptr && chatView->property("maxContentY").toReal() > chatView->property("minContentY").toReal() + 400.0;
	}());

	const qreal midY = std::round((chatView->property("minContentY").toReal() + chatView->property("maxContentY").toReal()) / 2.0);
	QMetaObject::invokeMethod(chatView, "scrollTo", Q_ARG(QVariant, midY), Q_ARG(QVariant, false));
	QCOMPARE(chatView->property("contentY").toReal(), midY);

	// Open command palette (has a scrollable PickList inside DialogHost) and send wheel events.
	f.workspace->openDialog(u"palette"_s);
	QQuickItem* paletteList = nullptr;
	QTRY_VERIFY([&] {
		QQuickItem* palette = findByTypePrefix(window->contentItem(), "CommandPalette");
		if(!palette || !palette->isVisible() || !palette->property("entered").toBool())
		{
			return false;
		}
		paletteList = findByTypePrefix(palette, "QQuickListView");
		return paletteList != nullptr
		       && paletteList->height() > 200.0
		       && paletteList->property("contentHeight").toReal() > paletteList->height();
	}());

	// 1) Scrolling on the dialog scrim must NOT reach ChatView underneath.
	const QPointF scrimPoint(12.0, 12.0);
	QWheelEvent scrimWheel(scrimPoint, window->mapToGlobal(scrimPoint), QPoint(), QPoint(0, 120),
	                       Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
	QCoreApplication::sendEvent(window, &scrimWheel);
	QCOMPARE(chatView->property("contentY").toReal(), midY);
	// 2) Scrolling inside the palette's PickList scrolls the dialog list, still leaving ChatView untouched.
	// Wait for the open transition to settle so the panel is at its final geometry before sampling listCenter.
	QQuickItem* palette = findByTypePrefix(window->contentItem(), "CommandPalette");
	QVERIFY(palette);
	QQuickItem* panel = findByTypePrefix(palette, "QQuickRectangle");
	QVERIFY(panel);
	QTRY_VERIFY(qFuzzyCompare(panel->scale(), 1.0) && paletteList->height() >= 359.0
	            && panel->height() >= paletteList->y() + paletteList->height());
	const QPointF listCenter = paletteList->mapToScene(QPointF(paletteList->width() / 2.0, paletteList->height() / 2.0));
	QWheelEvent listWheel(listCenter, window->mapToGlobal(listCenter), QPoint(), QPoint(0, -120),
	                      Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
	QCoreApplication::sendEvent(window, &listWheel);
	QTRY_VERIFY(paletteList->property("contentY").toReal() > 0.0);
	// 3) Opening Lightbox blocks wheel events over its scrim as well.
	f.workspace->closeDialog();
	QQuickItem* lightbox = findByTypePrefix(window->contentItem(), "Lightbox");
	QVERIFY(lightbox);
	QVariantMap img{{u"id"_s, u"img_1"_s}, {u"name"_s, u"shot.png"_s}, {u"size"_s, 1024}};
	QMetaObject::invokeMethod(lightbox, "open", Q_ARG(QVariant, u"ses_a"_s), Q_ARG(QVariant, img));
	QTRY_VERIFY(lightbox->property("shown").toBool());
	QWheelEvent lightboxWheel(scrimPoint, window->mapToGlobal(scrimPoint), QPoint(), QPoint(0, 120),
	                          Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
	QCoreApplication::sendEvent(window, &lightboxWheel);
	QCOMPARE(chatView->property("contentY").toReal(), midY);
	QVERIFY(!chatView->property("wheeling").toBool());

	// The Lightbox picture loads through the async chatimage provider: the reader thread asks,
	// the Host answers from the GUI thread. A request still in flight when the engine is
	// destroyed can never be answered (the reader thread is gone) and never deleted, which
	// LeakSanitizer reports. Let the round trip finish before the engine goes away.
	QQuickItem* lightboxPicture = findItem(lightbox, [](QQuickItem* item) {
		return QByteArray(item->metaObject()->className()).startsWith("QQuickImage")
		       && item->property("source").toUrl().toString().contains(u"chatimage"_s);
	});
	QVERIFY(lightboxPicture);
	constexpr int imageReady = 1; // QQuickImage::Status; the header is private, values spelled out
	constexpr int imageError = 3;
	QTRY_VERIFY([&] {
		const int status = lightboxPicture->property("status").toInt();
		return status == imageReady || status == imageError;
	}());

	QVERIFY(warningSpy.isEmpty());
}

void TestMainWindow::tileHeaderInsetsAndRoundsInsideFrameBorder()
{
	MainFixture f;

	QQmlEngine engine;
	QSignalSpy warningSpy(&engine, &QQmlEngine::warnings);
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);
	engine.addImageProvider(u"chatimage"_s, new ChatImageProvider([ws = QPointer<Workspace>(f.workspace.get())](const QString& id) {
		return ws ? ws->session(id) : nullptr;
	}));
	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/Main.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));

	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* window = qobject_cast<QQuickWindow*>(created.get());
	QVERIFY(window);
	QVERIFY(QTest::qWaitForWindowExposed(window));

	f.workspace->toggleSplit();
	QTRY_VERIFY(f.workspace->isSplitView());

	QQuickItem* tile = nullptr;
	QTRY_VERIFY([&] {
		tile = findByTypePrefix(window->contentItem(), "Tile_QMLTYPE");
		return tile != nullptr && tile->width() > 100.0 && tile->height() > 100.0;
	}());

	QQuickItem* frame = nullptr;
	for(QQuickItem* child : tile->childItems())
	{
		if(QByteArray(child->metaObject()->className()).startsWith("QQuickRectangle"))
		{
			frame = child;
			break;
		}
	}
	QVERIFY(frame);

	QQuickItem* header = nullptr;
	QQuickItem* pane = findByTypePrefix(frame, "SessionPane");
	for(QQuickItem* child : frame->childItems())
	{
		if(QByteArray(child->metaObject()->className()).startsWith("QQuickRectangle"))
		{
			header = child;
			break;
		}
	}
	QVERIFY(header);
	QVERIFY(pane);

	const qreal borderWidth = 1.0;
	const qreal frameRadius = frame->property("radius").toReal();
	QCOMPARE(header->x(), borderWidth);
	QCOMPARE(header->y(), borderWidth);
	QVERIFY(qFuzzyCompare(header->width(), frame->width() - 2.0 * borderWidth));
	QCOMPARE(header->property("topLeftRadius").toReal(), frameRadius - borderWidth);
	QCOMPARE(header->property("topRightRadius").toReal(), frameRadius - borderWidth);

	QCOMPARE(pane->x(), borderWidth);
	QVERIFY(qFuzzyCompare(pane->width(), frame->width() - 2.0 * borderWidth));
	QVERIFY(qFuzzyCompare(pane->y() + pane->height(), frame->height() - borderWidth));

	// Exiting split view fades TileGrid out cleanly and hides it once opacity reaches 0.
	QQuickItem* tileGrid = findByTypePrefix(window->contentItem(), "TileGrid");
	QVERIFY(tileGrid);
	f.workspace->toggleSplit();
	QTRY_VERIFY(!tileGrid->isVisible());
	QVERIFY(warningSpy.isEmpty());
}

void TestMainWindow::splitViewQuickToggleKeepsTilesOnTheGrid()
{
	MainFixture f;

	QQmlEngine engine;
	QSignalSpy warningSpy(&engine, &QQmlEngine::warnings);
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);
	engine.addImageProvider(u"chatimage"_s, new ChatImageProvider([ws = QPointer<Workspace>(f.workspace.get())](const QString& id) {
		return ws ? ws->session(id) : nullptr;
	}));
	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/Main.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));

	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* window = qobject_cast<QQuickWindow*>(created.get());
	QVERIFY(window);
	QVERIFY(QTest::qWaitForWindowExposed(window));

	f.workspace->toggleSplit();
	QTRY_VERIFY(f.workspace->isSplitView());

	QQuickItem* tileGrid = findByTypePrefix(window->contentItem(), "TileGrid");
	QVERIFY(tileGrid);
	QList<QQuickItem*> tiles;
	QTRY_VERIFY([&] {
		tiles.clear();
		collectTiles(window->contentItem(), tiles);
		return tiles.size() == 1 && !tiles.first()->property("leaving").toBool() && tiles.first()->width() > 100.0;
	}());
	QTRY_VERIFY(tilesOnTheGrid(window->contentItem(), tileGrid));
	const QPointer<QQuickItem> revived = tiles.first();

	// Toggle off and on again while the exit linger still holds the rows: the tile is
	// revived in place instead of recreated.
	f.workspace->toggleSplit();
	f.workspace->toggleSplit();
	QTRY_VERIFY(f.workspace->isSplitView());
	QTRY_VERIFY([&] {
		tiles.clear();
		collectTiles(window->contentItem(), tiles);
		return tiles.size() == 1 && !tiles.first()->property("leaving").toBool();
	}());
	QVERIFY(revived != nullptr); // the same tile instance came back

	// The revived tile follows the grid again: after the window grows, its geometry settles
	// on the slot the grid computes for its place.
	window->resize(window->size() + QSize(320, 120));
	QTRY_VERIFY(tilesOnTheGrid(window->contentItem(), tileGrid));

	QVERIFY(warningSpy.isEmpty());
}

void TestMainWindow::sidebarFolderButtonDoesNotTriggerNewSession()
{
	MainFixture f;

	QQmlEngine engine;
	QSignalSpy warningSpy(&engine, &QQmlEngine::warnings);
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);
	engine.addImageProvider(u"chatimage"_s, new ChatImageProvider([ws = QPointer<Workspace>(f.workspace.get())](const QString& id) {
		return ws ? ws->session(id) : nullptr;
	}));
	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/Main.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));

	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* window = qobject_cast<QQuickWindow*>(created.get());
	QVERIFY(window);
	QVERIFY(QTest::qWaitForWindowExposed(window));

	QQuickItem* sidebar = findByTypePrefix(window->contentItem(), "Sidebar");
	QVERIFY(sidebar);
	QQuickItem* folderButton = findItem(sidebar, [](QQuickItem* item) {
		return item->property("iconName").toString() == u"folder-open"_s;
	});
	QQuickItem* newPill = findItem(sidebar, [](QQuickItem* item) {
		return QByteArray(item->metaObject()->className()).startsWith("QQuickRectangle")
		       && qFuzzyCompare(item->property("radius").toReal(), 18.0);
	});
	QVERIFY(folderButton);
	QVERIFY(newPill);
	const QColor baseColor = newPill->property("color").value<QColor>();

	const auto createCount = [&] {
		int count = 0;
		for(const QJsonObject& req : f.fake.requests())
		{
			if(req.value(u"op"_s).toString() == u"create_session"_s)
			{
				++count;
			}
		}
		return count;
	};

	QSignalSpy folderSpy(f.workspace.get(), &Workspace::newSessionFolderRequested);
	const QPoint folderCenter = folderButton->mapToScene(QPointF(folderButton->width() / 2.0, folderButton->height() / 2.0)).toPoint();
	const QPoint newCenter = newPill->mapToScene(QPointF(newPill->width() / 2.0, newPill->height() / 2.0)).toPoint();

	// Hovering the adjacent folder button must not light up the new-session pill.
	QTest::mouseMove(window, folderCenter);
	QTest::qWait(80);
	QCOMPARE(newPill->property("color").value<QColor>(), baseColor);

	// Hovering and clicking the new-session pill lights it up and creates one session.
	QTest::mouseMove(window, newCenter);
	QTRY_VERIFY(newPill->property("color").value<QColor>() != baseColor);
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, newCenter);
	QTRY_COMPARE(createCount(), 1);

	// Clicking the folder button opens the folder picker without creating an extra session.
	QTest::mouseMove(window, folderCenter);
	QTRY_COMPARE(newPill->property("color").value<QColor>(), baseColor);
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, folderCenter);
	QTRY_COMPARE(folderSpy.count(), 1);
	QCOMPARE(createCount(), 1);
	for(QObject* child : window->children())
	{
		if(QByteArray(child->metaObject()->className()).contains("FolderDialog"))
		{
			QMetaObject::invokeMethod(child, "close");
		}
	}

	QVERIFY(warningSpy.isEmpty());
}

void TestMainWindow::standaloneDialogHostBlocksWheelEvents()
{
	MainFixture f;

	QQmlEngine engine;
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);

	QQuickWindow window;
	window.resize(800, 600);
	window.show();
	QVERIFY(QTest::qWaitForWindowExposed(&window));

	QQmlComponent chatComponent(&engine, QUrl(u"qrc:/qt/qml/RefGui/ChatView.qml"_s));
	QVERIFY2(chatComponent.isReady(), qPrintable(chatComponent.errorString()));
	std::unique_ptr<QObject> chatObj(chatComponent.create());
	auto* chat = qobject_cast<QQuickItem*>(chatObj.get());
	QVERIFY(chat);
	chat->setParentItem(window.contentItem());
	chat->setWidth(800);
	chat->setHeight(600);
	chat->setProperty("model", QVariant::fromValue<QObject*>(f.workspace->current()->blocks()));

	QTRY_VERIFY(chat->property("maxContentY").toReal() > chat->property("minContentY").toReal() + 300.0);
	const qreal midY = std::round((chat->property("minContentY").toReal() + chat->property("maxContentY").toReal()) / 2.0);
	QMetaObject::invokeMethod(chat, "scrollTo", Q_ARG(QVariant, midY), Q_ARG(QVariant, false));
	QCOMPARE(chat->property("contentY").toReal(), midY);

	QQmlComponent hostComponent(&engine, QUrl(u"qrc:/qt/qml/RefGui/DialogHost.qml"_s));
	QVERIFY2(hostComponent.isReady(), qPrintable(hostComponent.errorString()));
	std::unique_ptr<QObject> hostObj(hostComponent.create());
	auto* host = qobject_cast<QQuickItem*>(hostObj.get());
	QVERIFY(host);
	host->setParentItem(window.contentItem());
	host->setWidth(800);
	host->setHeight(600);

	f.workspace->openDialog(u"help"_s);
	QTRY_VERIFY(host->isVisible());

	const QPointF scrimPoint(20.0, 20.0);
	QWheelEvent wheel(scrimPoint, window.mapToGlobal(scrimPoint), QPoint(), QPoint(0, 120),
	                  Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
	QCoreApplication::sendEvent(&window, &wheel);
	QCOMPARE(chat->property("contentY").toReal(), midY);
	QVERIFY(!chat->property("wheeling").toBool());

	chat->setProperty("model", QVariant::fromValue<QObject*>(nullptr));
}

void TestMainWindow::statusLineRetryTooltipShowsFullErrorWithoutWarnings()
{
	MainFixture f;

	const QString fullError = u"Upstream provider returned HTTP 429 rate limit exceeded for deployment gpt-5 with a very long diagnostic payload that would overflow a single tooltip line without wrapping.\nSecond line: retry after 12 seconds."_s;
	QJsonObject retryInfo = sessionInfoJson(u"ses_a"_s, {.title = u"Session ses_a"_s, .createdAt = 1, .state = u"retrying"_s, .attached = 1});
	retryInfo[u"status"_s] = QJsonObject{{u"state"_s, u"retrying"_s},
	                                     {u"attempt"_s, 2},
	                                     {u"next_at"_s, QDateTime::currentMSecsSinceEpoch() + 10000},
	                                     {u"error"_s, fullError}};
	retryInfo[u"context"_s] = QJsonObject{{u"used"_s, 1234},
	                                      {u"max"_s, 200000},
	                                      {u"recommend"_s, 150000},
	                                      {u"auto"_s, 180000}};
	f.fake.send({{u"kind"_s, u"event"_s},
	             {u"session_id"_s, u"ses_a"_s},
	             {u"event"_s, QJsonObject{{u"type"_s, u"info"_s},
	                                      {u"info"_s, retryInfo}}}});
	QTRY_COMPARE(f.workspace->current()->activity(), u"retrying"_s);
	QCOMPARE(f.workspace->current()->retryError(), fullError);
	QQmlEngine engine;
	QSignalSpy warningSpy(&engine, &QQmlEngine::warnings);
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);

	QQuickWindow window;
	window.resize(800, 200);
	window.show();
	QVERIFY(QTest::qWaitForWindowExposed(&window));

	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/StatusLine.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));

	QVariantMap initialProps{{u"session"_s, QVariant::fromValue<QObject*>(f.workspace->current())}};
	std::unique_ptr<QObject> created(component.createWithInitialProperties(initialProps));
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* line = qobject_cast<QQuickItem*>(created.get());
	QVERIFY(line);
	line->setParentItem(window.contentItem());
	line->setWidth(600);
	line->setY(100);

	QObject* retryTip = nullptr;
	QQuickItem* retryText = findItem(line, [&retryTip](QQuickItem* item) {
		if(!QByteArray(item->metaObject()->className()).startsWith("QQuickText"))
		{
			return false;
		}
		for(QObject* child : item->children())
		{
			if(QByteArray(child->metaObject()->className()).startsWith("PlainToolTip"))
			{
				retryTip = child;
				return true;
			}
		}
		return false;
	});
	QVERIFY(retryText);
	QVERIFY(retryTip);
	QCOMPARE(retryTip->property("text").toString(), fullError);

	// Show the tooltip at the retry text and verify width capping and zero binding loop warnings.
	retryTip->setProperty("delay", 0);
	retryTip->setProperty("visible", true);
	QTRY_VERIFY(retryTip->property("opened").toBool());
	const qreal maxContentWidth = retryTip->property("maxContentWidth").toReal();
	const qreal leftPadding = retryTip->property("leftPadding").toReal();
	const qreal rightPadding = retryTip->property("rightPadding").toReal();
	QVERIFY(retryTip->property("width").toReal() <= maxContentWidth + leftPadding + rightPadding + 0.5);
	QCOMPARE(retryTip->property("text").toString(), fullError);

	QObject* contextTip = nullptr;
	findItem(line, [&contextTip](QQuickItem* item) {
		for(QObject* child : item->children())
		{
			if(QByteArray(child->metaObject()->className()).startsWith("PlainToolTip")
			   && child->property("text").toString().startsWith(u"Context:"_s))
			{
				contextTip = child;
				return true;
			}
		}
		return false;
	});
	QVERIFY(contextTip);
	const QString contextTipText = contextTip->property("text").toString();
	QVERIFY(!contextTipText.contains(u"%1"_s));
	QVERIFY(!contextTipText.contains(u"%2"_s));
	QVERIFY(!contextTipText.contains(u"%3"_s));
	QVERIFY(!contextTipText.contains(u"%4"_s));
	QVERIFY(contextTipText.contains(u"1.2k"_s));
	QVERIFY(contextTipText.contains(u"200.0k"_s));

	QVERIFY2(warningSpy.isEmpty(), "StatusLine / PlainToolTip emitted QML warnings");
}

void TestMainWindow::emptyWorkspaceStateAndDetachTransitionCleanly()
{
	MainFixture f;

	QQmlEngine engine;
	QSignalSpy warningSpy(&engine, &QQmlEngine::warnings);
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);
	engine.addImageProvider(u"chatimage"_s, new ChatImageProvider([ws = QPointer<Workspace>(f.workspace.get())](const QString& id) {
		return ws ? ws->session(id) : nullptr;
	}));
	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/Main.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));

	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* window = qobject_cast<QQuickWindow*>(created.get());
	QVERIFY(window);
	QVERIFY(QTest::qWaitForWindowExposed(window));

	const auto findText = [window](const QString& expected) {
		return findItem(window->contentItem(), [&expected](QQuickItem* item) {
			return QByteArray(item->metaObject()->className()).startsWith("QQuickText")
			       && item->property("text").toString() == expected;
		});
	};
	QQuickItem* emptyTitle = findText(u"No session open"_s);
	QVERIFY(emptyTitle);
	QVERIFY(!emptyTitle->isVisible());
	QVERIFY(window->property("shown").value<QObject*>() != nullptr);

	// Detaching current session crossfades window.shown to null and reveals the empty state.
	f.workspace->detachCurrent();
	QTRY_VERIFY(window->property("shown").isNull() || window->property("shown").value<QObject*>() == nullptr);
	QTRY_VERIFY(emptyTitle->isVisible());
	QQuickItem* newInFolderButton = findText(u"New session in folder…"_s);
	QVERIFY(newInFolderButton && newInFolderButton->isVisible());
	QVERIFY(findText(u"work · /work"_s) != nullptr);
	QVERIFY2(warningSpy.isEmpty(), "Empty Workspace State / detachCurrent emitted QML warnings");
}

void TestMainWindow::tileZoomRemovesBorderRepeatsSlowTravelAndCompensatesScroll()
{
	MainFixture f;
	for(int i = 0; i < 5; ++i)
	{
		f.stub.add(u"ses_extra_%1"_s.arg(i), 10 + i);
	}

	QQmlEngine engine;
	QSignalSpy warningSpy(&engine, &QQmlEngine::warnings);
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);
	engine.addImageProvider(u"chatimage"_s, new ChatImageProvider([ws = QPointer<Workspace>(f.workspace.get())](const QString& id) {
		return ws ? ws->session(id) : nullptr;
	}));
	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/Main.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));

	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* window = qobject_cast<QQuickWindow*>(created.get());
	QVERIFY(window);
	window->resize(900, 460);
	QVERIFY(QTest::qWaitForWindowExposed(window));

	f.workspace->toggleSplit();
	QTRY_VERIFY(f.workspace->isSplitView());
	QQuickItem* tileGrid = findByTypePrefix(window->contentItem(), "TileGrid");
	QVERIFY(tileGrid);
	QQuickItem* flick = findByTypePrefix(tileGrid, "QQuickFlickable");
	QVERIFY(flick);
	QQuickItem* tileB = nullptr;
	QTRY_VERIFY([&] {
		QList<QQuickItem*> tiles;
		collectTiles(tileGrid, tiles);
		for(QQuickItem* candidate : tiles)
		{
			if(candidate->property("sessionId").toString() == u"ses_b"_s && candidate->width() > 100.0)
			{
				tileB = candidate;
			}
		}
		return tileB != nullptr && flick->property("contentHeight").toReal() > flick->height() + 100.0;
	}());

	flick->setProperty("contentY", 100.0);
	QCOMPARE(flick->property("contentY").toReal(), 100.0);

	// 1st zoom: travelling is true immediately (Motion.slow = 480ms), zoomed pane fills viewport at (0, 0) without 1px border inset.
	f.workspace->zoomTile(u"ses_b"_s);
	QVERIFY(tileB->property("travelling").toBool());
	QCOMPARE(tileB->property("travelDuration").toInt(), 480);
	QQuickItem* pane = findByTypePrefix(tileB, "SessionPane");
	QVERIFY(pane);
	QTRY_VERIFY(qFuzzyCompare(tileB->width(), tileGrid->width()) && qFuzzyCompare(tileB->height(), tileGrid->height()));
	const QPointF viewportOrigin = tileB->mapToItem(tileGrid, QPointF(0.0, 0.0));
	QVERIFY(qAbs(viewportOrigin.x()) < 0.5 && qAbs(viewportOrigin.y()) < 0.5);
	QCOMPARE(pane->x(), 0.0);
	QTRY_COMPARE(pane->y(), 0.0);
	QVERIFY(qFuzzyCompare(pane->width(), tileGrid->width()));
	QVERIFY(qFuzzyCompare(pane->height(), tileGrid->height()));
	QCOMPARE(flick->property("contentY").toReal(), 100.0);

	// Unzoom preserves scroll offset and settles travelling back to false.
	f.workspace->unzoom();
	QVERIFY(tileB->property("travelling").toBool());
	QTRY_VERIFY_WITH_TIMEOUT(!tileB->property("travelling").toBool(), 2000);
	QCOMPARE(flick->property("contentY").toReal(), 100.0);

	// 2nd zoom: declarative travelling binding still activates immediately (Motion.slow = 480ms).
	f.workspace->zoomTile(u"ses_b"_s);
	QVERIFY(tileB->property("travelling").toBool());
	QCOMPARE(tileB->property("travelDuration").toInt(), 480);
	QTRY_VERIFY(qFuzzyCompare(tileB->width(), tileGrid->width()));

	// Leaving split view while zoomed hands over cleanly to ses_b in single view.
	f.workspace->toggleSplit();
	QCOMPARE(f.workspace->currentId(), u"ses_b"_s);
	QCOMPARE(window->property("shown").value<QObject*>(), f.workspace->session(u"ses_b"_s));
	QTRY_VERIFY(!tileGrid->isVisible());
	QVERIFY2(warningSpy.isEmpty(), "Tile zoom / handover emitted QML warnings");
}

void TestMainWindow::reusableConfigFormControlsInstantiateAndOperateWithoutWarnings()
{
	MainFixture f;

	QQmlEngine engine;
	QSignalSpy warningSpy(&engine, &QQmlEngine::warnings);
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);

	QQuickWindow window;
	window.resize(900, 900);
	window.show();
	QVERIFY(QTest::qWaitForWindowExposed(&window));

	QQmlComponent component(&engine);
	component.setData(R"qml(
		import QtQuick
		import RefGui
		Column {
			id: root
			width: Theme.configDialogWidth
			spacing: 8
			readonly property int configWidth: Theme.configDialogWidth
			Icon { name: "plus"; size: 16 }
			Icon { name: "retry"; size: 20 }
			Icon { name: "eye"; size: 16 }
			Icon { name: "eye-off"; size: 16 }
			FormRow {
				label: "Auto resize"
				description: "Resize pasted images"
				badge: "ref"
				controlWidth: 0
				Switch { id: sw; objectName: "sw"; checked: true; accessibleName: "Auto resize" }
			}
			FormRow {
				label: "Default model"
				SelectField {
					id: sel
					objectName: "sel"
					width: parent.width
					editable: true
					clearable: true
					options: [{ label: "Claude Sonnet", value: "anthropic/claude-sonnet-4-5" }]
					value: "anthropic/claude-sonnet-4-5"
				}
			}
			FormRow {
				label: "API key"
				SecretField {
					id: sec
					objectName: "sec"
					width: parent.width
					text: "{env:OPENAI_API_KEY}"
					envStatus: ({ OPENAI_API_KEY: true, MISSING_KEY: false })
				}
			}
			FormRow {
				label: "Max delay"
				NumberField {
					id: num
					objectName: "num"
					width: parent.width
					minimum: 0
					maximum: 3600
					step: 10
					suffix: "s"
					value: 600
				}
			}
			FormRow {
				label: "System prompt"
				stacked: true
				showReset: true
				MultilineField {
					id: multi
					objectName: "multi"
					width: parent.width
					monospace: true
					placeholderText: "Prompt…"
					text: "Line 1\nLine 2"
				}
			}
			FormRow {
				label: "Command"
				stacked: true
				StringListEditor {
					id: strList
					objectName: "strList"
					allowCommandSplit: true
					monospace: true
					values: ["npx", "-y"]
				}
			}
			FormRow {
				label: "Headers"
				stacked: true
				KeyValueEditor {
					id: kv
					objectName: "kv"
					envStatus: ({ TEAM_TOKEN: false })
					entries: [{ key: "Authorization", value: "Bearer {env:TEAM_TOKEN}" }]
				}
			}
			FormRow {
				label: "Permissions"
				stacked: true
				PermissionRulesEditor {
					id: perms
					objectName: "perms"
					allowInherit: true
				}
			}
			FormRow {
				label: "Agent color"
				stacked: true
				ColorSwatchPicker {
					id: swatch
					objectName: "swatch"
					value: "purple"
				}
			}
		}
	)qml", QUrl(u"qrc:/qt/qml/RefGui/TestControls.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));
	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* root = qobject_cast<QQuickItem*>(created.get());
	QVERIFY(root);
	root->setParentItem(window.contentItem());
	QCOMPARE(root->property("configWidth").toInt(), 860);

	auto* secret = root->findChild<QQuickItem*>(u"sec"_s);
	QVERIFY(secret);
	QVERIFY(secret->property("isInterpolated").toBool());
	QVERIFY(secret->property("showEnvBadge").toBool());
	QVERIFY(secret->property("envAllSet").toBool());
	secret->setProperty("text", u"{env:MISSING_KEY}"_s);
	QVERIFY(secret->property("showEnvBadge").toBool());
	QVERIFY(!secret->property("envAllSet").toBool());
	secret->setProperty("text", u"sk-literal-secret"_s);
	QVERIFY(!secret->property("isInterpolated").toBool());
	QVERIFY(secret->property("showEyeButton").toBool());

	auto* num = root->findChild<QQuickItem*>(u"num"_s);
	QVERIFY(num);
	QMetaObject::invokeMethod(num, "stepBy", Q_ARG(QVariant, 1));
	QCOMPARE(num->property("value").toInt(), 610);

	auto* strList = root->findChild<QQuickItem*>(u"strList"_s);
	QVERIFY(strList);
	QVariant splitTokens;
	QMetaObject::invokeMethod(strList, "splitCommandLine", Q_RETURN_ARG(QVariant, splitTokens), Q_ARG(QVariant, u"node \"my script.js\" --flag"_s));
	QCOMPARE(splitTokens.toStringList(), (QStringList{u"node"_s, u"my script.js"_s, u"--flag"_s}));

	auto* perms = root->findChild<QQuickItem*>(u"perms"_s);
	QVERIFY(perms);
	const QVariantMap permInput{{u"bash"_s, QVariantMap{{u"*"_s, u"ask"_s}, {u"git diff*"_s, u"allow"_s}}}, {u"delete"_s, u"deny"_s}};
	QMetaObject::invokeMethod(perms, "loadFromValue", Q_ARG(QVariant, permInput));
	QCOMPARE(perms->property("mode").toString(), u"rules"_s);
	QVariant roundTripped;
	QMetaObject::invokeMethod(perms, "toValue", Q_RETURN_ARG(QVariant, roundTripped));
	QCOMPARE(roundTripped.toMap().value(u"delete"_s).toString(), u"deny"_s);
	QCOMPARE(roundTripped.toMap().value(u"bash"_s).toMap().value(u"git diff*"_s).toString(), u"allow"_s);

	auto* swatch = root->findChild<QQuickItem*>(u"swatch"_s);
	QVERIFY(swatch);
	QMetaObject::invokeMethod(swatch, "selectColor", Q_ARG(QVariant, u"#ff8800"_s));
	QCOMPARE(swatch->property("value").toString(), u"#ff8800"_s);
	QVERIFY(swatch->property("isValidSpec").toBool());

	QCoreApplication::processEvents();
	QVERIFY2(warningSpy.isEmpty(), "Reusable config form controls emitted QML warnings");
}

void TestMainWindow::configDialogSwitchesCategoriesEditsVisualAndJsoncAndGuardsDirtyClose()
{
	MainFixture f;

	QQmlEngine engine;
	QSignalSpy warningSpy(&engine, &QQmlEngine::warnings);
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);
	engine.addImageProvider(u"chatimage"_s, new ChatImageProvider([ws = QPointer<Workspace>(f.workspace.get())](const QString& id) {
		return ws ? ws->session(id) : nullptr;
	}));
	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/Main.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));

	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* window = qobject_cast<QQuickWindow*>(created.get());
	QVERIFY(window);
	QVERIFY(QTest::qWaitForWindowExposed(window));

	f.workspace->commands()->run(u"config"_s);
	QCOMPARE(f.workspace->dialog(), u"config"_s);

	QQuickItem* configDialog = nullptr;
	const auto findByName = [](QQuickItem* root, const QString& name) {
		return findItem(root, [&name](QQuickItem* item) { return item->objectName() == name; });
	};
	QTRY_VERIFY([&] {
		configDialog = findByName(window->contentItem(), u"configDialog"_s);
		return configDialog != nullptr && !f.workspace->configEditor()->isLoading();
	}());
	QCOMPARE(f.workspace->configEditor()->selectedFile(), u"ref.json"_s);
	QCOMPARE(f.workspace->configEditor()->mode(), u"visual"_s);

	// Walk all 6 visual categories to verify every section instantiates cleanly with zero QML warnings.
	for(const QString& cat : {u"providers"_s, u"agents"_s, u"permissions"_s, u"mcp"_s, u"commands"_s, u"general"_s})
	{
		QMetaObject::invokeMethod(configDialog, "selectCategory", Q_ARG(QVariant, cat));
		QCoreApplication::processEvents();
	}
	QVERIFY2(warningSpy.isEmpty(), "Switching ConfigDialog categories emitted QML warnings");

	// Unmodified Visual -> JSON preserves raw JSONC comments verbatim.
	QVERIFY(f.workspace->configEditor()->setMode(u"json"_s));
	auto* rawEditor = findByName(configDialog, u"configRawJsonEditor"_s);
	QVERIFY(rawEditor);
	QVERIFY(rawEditor->property("text").toString().contains(u"// Keep user header comment"_s));
	QVERIFY(f.workspace->configEditor()->setMode(u"visual"_s));

	// Add Anthropic provider preset and verify dirty guard blocks accidental close.
	QMetaObject::invokeMethod(configDialog, "selectCategory", Q_ARG(QVariant, u"providers"_s));
	QCoreApplication::processEvents();
	auto* presetBtn = findByName(configDialog, u"presetProvider_anthropic"_s);
	QVERIFY(presetBtn);
	QMetaObject::invokeMethod(presetBtn, "clicked");
	QVERIFY(f.workspace->configEditor()->isDirty());

	QMetaObject::invokeMethod(configDialog, "requestClose");
	QCOMPARE(f.workspace->dialog(), u"config"_s);
	auto* dirtyBar = findByName(configDialog, u"configDirtyConfirmBar"_s);
	QVERIFY(dirtyBar && dirtyBar->isVisible());
	configDialog->setProperty("pendingDirtyAction", QVariant());
	QVERIFY(!dirtyBar->isVisible());

	// Switch to JSON mode: new provider preset and untouched extraFutureSetting are both present.
	QVERIFY(f.workspace->configEditor()->setMode(u"json"_s));
	QVERIFY(rawEditor->property("text").toString().contains(u"\"anthropic\""_s));
	QVERIFY(rawEditor->property("text").toString().contains(u"\"extraFutureSetting\""_s));

	// Broken JSONC shows parse error banner and blocks Visual mode until fixed/discarded.
	rawEditor->setProperty("text", u"{ broken json "_s);
	QVERIFY(!f.workspace->configEditor()->parseError().isEmpty());
	auto* parseBanner = findByName(configDialog, u"configParseErrorBanner"_s);
	QVERIFY(parseBanner && parseBanner->isVisible());
	QVERIFY(!f.workspace->configEditor()->setMode(u"visual"_s));

	// Discard restores clean baseline (including header comment), switch back to Visual, set $name, Save, and Close.
	f.workspace->configEditor()->discard();
	QVERIFY(!f.workspace->configEditor()->isDirty());
	QVERIFY(f.workspace->configEditor()->parseError().isEmpty());
	QVERIFY(rawEditor->property("text").toString().contains(u"// Keep user header comment"_s));
	QVERIFY(f.workspace->configEditor()->setMode(u"visual"_s));
	f.workspace->configEditor()->setStringAt({u"$name"_s}, u"Main Profile"_s);
	QVERIFY(f.workspace->configEditor()->isDirty());
	QMetaObject::invokeMethod(configDialog, "triggerSave");
	QTRY_COMPARE(f.stub.saveConfigCalls(), 1);
	QTRY_VERIFY(!f.workspace->configEditor()->isDirty());
	QVERIFY(f.stub.savedConfigRawText().contains(u"\"$name\": \"Main Profile\""_s));
	QVERIFY(f.stub.savedConfigRawText().contains(u"\"extraFutureSetting\""_s));
	QMetaObject::invokeMethod(configDialog, "requestClose");
	QCOMPARE(f.workspace->dialog(), QString());
	QVERIFY2(warningSpy.isEmpty(), "ConfigDialog operations emitted QML warnings");
}

void TestMainWindow::paletteAndComposerCompletionsAndShellModeOperateWithoutWarnings()
{
	MainFixture f;
	QTRY_COMPARE(f.workspace->config()->agents().size(), 2);

	QQmlEngine engine;
	QSignalSpy warningSpy(&engine, &QQmlEngine::warnings);
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);
	engine.addImageProvider(u"chatimage"_s, new ChatImageProvider([ws = QPointer<Workspace>(f.workspace.get())](const QString& id) {
		return ws ? ws->session(id) : nullptr;
	}));
	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/Main.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));
	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* window = qobject_cast<QQuickWindow*>(created.get());
	QVERIFY(window);
	QVERIFY(QTest::qWaitForWindowExposed(window));
	window->requestActivate();

	// 1) CommandPalette: choosing "cd" opens CdDialog directly; choosing "custom:review" enters askingArgument, and Esc returns to list.
	f.workspace->openDialog(u"palette"_s);
	QQuickItem* palette = nullptr;
	QTRY_VERIFY([&] {
		palette = findByTypePrefix(window->contentItem(), "CommandPalette");
		return palette != nullptr && palette->isVisible() && palette->property("entered").toBool();
	}());
	const QVariantMap cdCmd{{u"commandId"_s, u"cd"_s}, {u"title"_s, u"Change dir"_s}, {u"hint"_s, u"[DIR]"_s}};
	QMetaObject::invokeMethod(palette, "choose", Q_ARG(QVariant, cdCmd));
	QCOMPARE(f.workspace->dialog(), u"cd"_s);
	QTRY_VERIFY(!f.stub.listDirsRequests().isEmpty());
	QCOMPARE(f.stub.listDirsRequests().last().value(u"cwd"_s).toString(), u"/work"_s);
	QCOMPARE(f.stub.listDirsRequests().last().value(u"include_files"_s).toBool(), false);
	f.workspace->closeDialog();
	QTRY_VERIFY(findByTypePrefix(window->contentItem(), "CommandPalette") == nullptr);

	// ExportDialog passes session cwd and includeFiles: true to PathField.
	f.workspace->commands()->run(u"export"_s);
	QCOMPARE(f.workspace->dialog(), u"export"_s);
	QTRY_VERIFY([&] {
		const auto reqs = f.stub.listDirsRequests();
		return !reqs.isEmpty() && reqs.last().value(u"include_files"_s).toBool();
	}());
	QCOMPARE(f.stub.listDirsRequests().last().value(u"cwd"_s).toString(), u"/work"_s);
	f.workspace->closeDialog();

	// CommandPalette Esc while askingArgument returns to command list instead of closing palette.
	f.workspace->openDialog(u"palette"_s);
	palette = nullptr;
	QTRY_VERIFY([&] {
		palette = findByTypePrefix(window->contentItem(), "CommandPalette");
		return palette != nullptr && palette->isVisible() && palette->property("entered").toBool();
	}());
	const QVariantMap reviewCmd{{u"commandId"_s, u"custom:review"_s}, {u"title"_s, u"Review diff"_s}, {u"hint"_s, u"[args]"_s}};
	QMetaObject::invokeMethod(palette, "choose", Q_ARG(QVariant, reviewCmd));
	QVERIFY(palette->property("askingArgument").toBool());
	QQuickItem* paletteInput = findByTypePrefix(palette, "QQuickTextInput");
	QVERIFY(paletteInput);
	paletteInput->forceActiveFocus();
	QTest::keyClick(window, Qt::Key_Escape);
	QVERIFY(!palette->property("askingArgument").toBool());
	QCOMPARE(f.workspace->dialog(), u"palette"_s);
	f.workspace->closeDialog();
	QCOMPARE(f.workspace->dialog(), QString());

	// 2) Composer: unknown single-token /modles warns once (no duplicate toast) and keeps draft.
	QQuickItem* composerItem = findByTypePrefix(window->contentItem(), "Composer");
	QVERIFY(composerItem);
	QMetaObject::invokeMethod(composerItem, "focusInput");
	QSignalSpy toastSpy(f.workspace.get(), &Workspace::toastRequested);
	auto* composerModel = f.workspace->current()->composer();
	composerModel->edit(u"/modles"_s, 7);
	QMetaObject::invokeMethod(composerItem, "submit");
	QCOMPARE(toastSpy.count(), 1);
	QVERIFY(toastSpy.first().at(1).toString().contains(u"/models"_s));
	QCOMPARE(composerModel->text(), u"/modles"_s);
	composerModel->clearDraft();

	// 3) `!` shell mode: Tab and Backtab never cycle agent; unnavigated Enter runs free-form typed command directly.
	QMetaObject::invokeMethod(composerItem, "focusInput");
	composerModel->edit(u"!"_s, 1);
	QVERIFY(composerModel->shellMode());
	QTest::keyClick(window, Qt::Key_Tab);
	QCOMPARE(f.stub.setAgentCalls(), 0);
	QTRY_VERIFY(composerModel->completions()->rowCount() > 0);
	QTest::keyClick(window, Qt::Key_Backtab, Qt::ShiftModifier);
	QCOMPARE(f.stub.setAgentCalls(), 0);

	composerModel->edit(u"ca"_s, 2);
	QQuickItem* popup = findByTypePrefix(composerItem, "CompletionPopup");
	QVERIFY(popup);
	QTRY_VERIFY(popup->property("open").toBool());
	QVERIFY(!popup->property("explicitlySelected").toBool());
	QTest::keyClick(window, Qt::Key_Return); // free-form Enter submits "ca", NOT "cargo "!
	QTRY_COMPARE(f.stub.shellCommands(), (QStringList{u"ca"_s}));
	QVERIFY(!composerModel->shellMode());
	QVERIFY2(warningSpy.isEmpty(), "Palette, PathField, and Composer completions emitted QML warnings");
}

QTEST_MAIN(TestMainWindow)
#include "tst_main_window.moc"
