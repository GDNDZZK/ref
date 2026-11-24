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
#include <QQuickTextDocument>
#include <QQuickWindow>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTextBlock>
#include <QTextDocument>
#include <QTextLayout>
#include <QWheelEvent>
#include <QtTest>
#include <memory>
#include <algorithm>

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

	void setSession(const QString& id, const QJsonObject& info) { m_sessions.insert(id, info); }
	void addHistory(const QString& id, const QJsonObject& info) { m_history.insert(id, info); }
	void broadcastSessions() { m_fake.send({{u"kind"_s, u"sessions"_s}, {u"sessions"_s, list()}}); }
	void setMessages(const QString& id, const QJsonArray& messages) { m_messages.insert(id, messages); }
	void setTodos(const QString& id, const QJsonArray& todos) { m_sessions[id][u"todos"_s] = todos; }
	QString savedConfigRawText() const { return m_configRawText; }
	void setConfigRawText(const QString& text) { m_configRawText = text; }
	int saveConfigCalls() const { return m_saveConfigCalls; }
	int setAgentCalls() const { return m_setAgentCalls; }
	QStringList shellCommands() const { return m_shellCommands; }
	QList<QJsonObject> listDirsRequests() const { return m_listDirsRequests; }
	void setModels(const QJsonArray& models) { m_models = models; }
	QList<QJsonObject> setVariantRequests() const { return m_setVariantRequests; }
	void setFiles(const QStringList& files) { m_files = files; }
private:
	QJsonArray list() const
	{
		QJsonArray array;
		for(const QJsonObject& info : m_sessions)
		{
			if(info.value(u"live"_s).toBool(true))
			{
				array.append(info);
			}
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
		else if(op == u"list_sessions"_s || op == u"watch_all"_s)
		{
			m_fake.respond(request, {{u"sessions"_s, list()}});
		}
		else if(op == u"list_history"_s)
		{
			const QString cwdFilter = request.value(u"cwd"_s).toString();
			const QString creatorFilter = request.value(u"creator"_s).toString();
			const QString queryFilter = request.value(u"query"_s).toString();
			QJsonArray array;
			for(const QJsonObject& info : m_sessions.values() + m_history.values())
			{
				if(!cwdFilter.isEmpty() && info.value(u"cwd"_s).toString() != cwdFilter)
				{
					continue;
				}
				const QString parent = info.value(u"parent"_s).toString();
				if(creatorFilter == u"user"_s && !parent.isEmpty())
				{
					continue;
				}
				if(!creatorFilter.isEmpty() && creatorFilter != u"user"_s && parent != creatorFilter)
				{
					continue;
				}
				if(!queryFilter.isEmpty())
				{
					const QString haystack = info.value(u"title"_s).toString() + u' ' + info.value(u"name"_s).toString()
					                         + u' ' + info.value(u"cwd"_s).toString() + u' ' + info.value(u"id"_s).toString();
					if(!haystack.contains(queryFilter, Qt::CaseInsensitive))
					{
						continue;
					}
				}
				array.append(info);
			}
			m_fake.respond(request, {{u"sessions"_s, array}});
		}
		else if(op == u"get_config"_s || op == u"reload_config"_s)
		{
			m_fake.respond(request,
			               {{u"config"_s,
			                 QJsonObject{{u"default_agent"_s, u"build"_s},
			                             {u"agents"_s, QJsonArray{QJsonObject{{u"name"_s, u"build"_s}, {u"description"_s, u"Build"_s}},
			                                                      QJsonObject{{u"name"_s, u"plan"_s}, {u"description"_s, u"Plan"_s}}}},
			                             {u"models"_s, m_models},
			                             {u"commands"_s, QJsonArray{QJsonObject{{u"name"_s, u"review"_s}, {u"description"_s, u"Review diff"_s}, {u"source"_s, u"config"_s}}}}}},
			                {u"warnings"_s, QJsonArray()}});
		}
		else if(op == u"set_agent"_s)
		{
			++m_setAgentCalls;
			m_sessions[id][u"agent"_s] = request.value(u"agent"_s);
			m_fake.respond(request, {{u"session"_s, m_sessions.value(id)}});
		}
		else if(op == u"set_variant"_s)
		{
			m_setVariantRequests.append(request);
			QJsonObject modelObj = m_sessions[id].value(u"model"_s).toObject();
			if(request.contains(u"variant"_s) && !request.value(u"variant"_s).toString().isEmpty())
			{
				modelObj.insert(u"variant"_s, request.value(u"variant"_s).toString());
			}
			else
			{
				modelObj.insert(u"variant"_s, u"high"_s);
			}
			m_sessions[id].insert(u"model"_s, modelObj);
			m_fake.respond(request, {{u"session"_s, m_sessions.value(id)}});
		}
		else if(op == u"prompt"_s)
		{
			m_fake.respond(request, {{u"queued"_s, false}, {u"message_id"_s, u"u_prompt_1"_s}});
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
		else if(op == u"list_files"_s)
		{
			m_fake.respond(request, {{u"files"_s, QJsonArray::fromStringList(m_files)}});
		}
		else if(op == u"list_binaries"_s)
		{
			m_fake.respond(request, {{u"binaries"_s, QJsonArray{u"cargo"_s, u"cat"_s, u"catfish"_s}}});
		}
		else if(op == u"complete_shell"_s)
		{
			m_fake.respond(request, {{u"items"_s, QJsonArray()}});
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
		else if(op == u"list_groups"_s)
		{
			m_fake.respond(request, {{u"groups"_s, m_groups}});
		}
		else if(op == u"stash_all"_s)
		{
			QJsonArray members;
			int pos = 0;
			for(auto it = m_sessions.begin(); it != m_sessions.end(); ++it)
			{
				if(!it->value(u"live"_s).toBool(true))
				{
					continue;
				}
				(*it)[u"live"_s] = false;
				(*it)[u"attached"_s] = 0;
				members.append(QJsonObject{{u"session_id"_s, it.key()}, {u"position"_s, pos++}, {u"info"_s, *it}});
			}
			if(members.isEmpty())
			{
				m_fake.respond(request, {{u"group"_s, QJsonValue::Null}, {u"groups"_s, m_groups}});
				return;
			}
			const QString gid = u"grp_%1"_s.arg(++m_groupSeq);
			const QString name = request.value(u"name"_s).toString().isEmpty() ? u"Stash %1"_s.arg(m_groupSeq) : request.value(u"name"_s).toString();
			const QJsonObject group{{u"id"_s, gid}, {u"name"_s, name}, {u"locked"_s, false}, {u"created_at"_s, 1000}, {u"updated_at"_s, 1000}, {u"members"_s, members}};
			m_groups.prepend(group);
			m_fake.respond(request, {{u"group"_s, group}, {u"groups"_s, m_groups}});
			broadcastSessions();
		}
		else if(op == u"create_group"_s)
		{
			const QString gid = u"grp_%1"_s.arg(++m_groupSeq);
			const QString name = request.value(u"name"_s).toString().isEmpty() ? u"Group %1"_s.arg(m_groupSeq) : request.value(u"name"_s).toString();
			const bool stashLive = request.value(u"stash_live"_s).toBool(true);
			QJsonArray members;
			int pos = 0;
			for(const QJsonValue& val : request.value(u"session_ids"_s).toArray())
			{
				const QString sid = val.toString();
				if(m_sessions.contains(sid))
				{
					if(stashLive)
					{
						m_sessions[sid][u"live"_s] = false;
						m_sessions[sid][u"attached"_s] = 0;
					}
					members.append(QJsonObject{{u"session_id"_s, sid}, {u"position"_s, pos++}, {u"info"_s, m_sessions.value(sid)}});
				}
			}
			const QJsonObject group{{u"id"_s, gid}, {u"name"_s, name}, {u"locked"_s, false}, {u"created_at"_s, 1000}, {u"updated_at"_s, 1000}, {u"members"_s, members}};
			m_groups.prepend(group);
			m_fake.respond(request, {{u"group"_s, group}, {u"groups"_s, m_groups}});
			if(stashLive)
			{
				broadcastSessions();
			}
		}
		else if(op == u"rename_group"_s)
		{
			const QString gid = request.value(u"group_id"_s).toString();
			const QString name = request.value(u"name"_s).toString();
			for(int i = 0; i < m_groups.size(); ++i)
			{
				QJsonObject g = m_groups.at(i).toObject();
				if(g.value(u"id"_s).toString() == gid)
				{
					g[u"name"_s] = name;
					m_groups[i] = g;
				}
			}
			m_fake.respond(request, {{u"groups"_s, m_groups}});
		}
		else if(op == u"set_group_locked"_s)
		{
			const QString gid = request.value(u"group_id"_s).toString();
			const bool locked = request.value(u"locked"_s).toBool();
			for(int i = 0; i < m_groups.size(); ++i)
			{
				QJsonObject g = m_groups.at(i).toObject();
				if(g.value(u"id"_s).toString() == gid)
				{
					g[u"locked"_s] = locked;
					m_groups[i] = g;
				}
			}
			m_fake.respond(request, {{u"groups"_s, m_groups}});
		}
		else if(op == u"delete_group"_s)
		{
			const QString gid = request.value(u"group_id"_s).toString();
			QJsonArray next;
			for(const QJsonValue& val : m_groups)
			{
				if(val.toObject().value(u"id"_s).toString() != gid)
				{
					next.append(val);
				}
			}
			m_groups = next;
			m_fake.respond(request, {{u"groups"_s, m_groups}});
		}
		else if(op == u"restore_group"_s)
		{
			const QString gid = request.value(u"group_id"_s).toString();
			QJsonArray restoredIds;
			for(int i = 0; i < m_groups.size(); ++i)
			{
				QJsonObject g = m_groups.at(i).toObject();
				if(g.value(u"id"_s).toString() == gid)
				{
					QJsonArray members = g.value(u"members"_s).toArray();
					for(int j = 0; j < members.size(); ++j)
					{
						QJsonObject m = members.at(j).toObject();
						const QString sid = m.value(u"session_id"_s).toString();
						if(m_sessions.contains(sid) && !m_sessions[sid].value(u"live"_s).toBool(true))
						{
							m_sessions[sid][u"live"_s] = true;
							restoredIds.append(sid);
						}
						m[u"info"_s] = m_sessions.value(sid);
						members[j] = m;
					}
					g[u"members"_s] = members;
					m_groups[i] = g;
				}
			}
			m_fake.respond(request, {{u"restored_ids"_s, restoredIds}, {u"groups"_s, m_groups}});
			broadcastSessions();
		}
		else
		{
			m_fake.respond(request, {});
		}
	}

	FakeBackend& m_fake;
	QMap<QString, QJsonObject> m_sessions;
	QMap<QString, QJsonObject> m_history;
	QMap<QString, QJsonArray> m_messages;
	int m_created = 0;
	QString m_configRawText = u"// Keep user header comment\n{\n  \"model\": \"anthropic/claude-sonnet-4-5\",\n  \"extraFutureSetting\": {\"preserve\": true}\n}\n"_s;
	int m_saveConfigCalls = 0;
	int m_setAgentCalls = 0;
	QStringList m_shellCommands;
	QList<QJsonObject> m_listDirsRequests;
	QJsonArray m_models;
	QList<QJsonObject> m_setVariantRequests;
	QStringList m_files;
	QJsonArray m_groups;
	int m_groupSeq = 0;
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

void typeAsciiText(QQuickWindow* window, const QString& text)
{
	for(const QChar ch : text)
	{
		const Qt::Key key = ch.isLetter() ? static_cast<Qt::Key>(ch.toUpper().unicode()) : static_cast<Qt::Key>(ch.unicode());
		QTest::keyClick(window, key);
	}
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
		QCoreApplication::processEvents();
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
	void configControlsValidateStrictInputsAndPreserveJsonStrings();
	void configDialogSupportsOpencodeAliasesInlineRenameTopBarLockAndQuitGuard();
	void tableBlockRendersAndSelectionLeavesAboveRowsUntouched();
	void shimmerTextModelsHoverCtrlTAtomicImageAndCompletionsWork();
	void splitJumpToEndResizeBlurFadeAndSidebarDragWork();
	void sidebarSectionsCollapseExpandAndResizeWithoutOscillation();
	void composerUndoRedoSyncsAttachmentsAndHistoryAvoidsCollision();
	void switchClicksWithHitPaddingInsideConfigFlickableAndResetButtonsClearFields();
	void sidebarRunningRowsReorderByLongPressDragAndPersistAcrossFrames();
	void uiZoomSettingsShortcutsResizeAndPopupsScaleCleanly();
	void sidebarVerticalSplittersResizePersistAndReset();
	void tileEnlargeKeepsResizingLayoutUntilWidthAnimationSettles();
	void composerAndUserBlockHighlightImageTokensAndAgentVariantSelects();
	void splitUnfocusClampsScrollAndTileHeaderHasNoBindingLoop();
	void queueStripShowsQueuedPromptsAndCoexistsWithTileComposer();
	void lightboxNavigatesZoomsAndResets();
	void sidebarHistoryFilterMenuAndCustomHeightCollapseWork();
	void splitViewSidebarToggleBlursFadesAndFreezesLayoutWidth();
	void backendDialogSwitchAndStatusPillActionWorkWithoutWarnings();
	void stashButtonDialogAndSplitFilterCapsuleWorkWithoutWarnings();
	void floatingBallModeTransitionsBetweenBallHoverCardAndMiniPanelWithoutWarnings();
	void cleanupTestCase();
};

void TestMainWindow::initTestCase()
{
	QQuickStyle::setStyle(u"Basic"_s);
}

void TestMainWindow::cleanupTestCase()
{
	QCoreApplication::processEvents();
	QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
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
	QVariantMap img{{u"id"_s, u"img_1"_s}, {u"name"_s, u"shot.png"_s}, {u"size"_s, 1024}};
	QMetaObject::invokeMethod(chatView, "imageClicked", Q_ARG(QVariant, img));
	QQuickItem* lightbox = nullptr;
	QTRY_VERIFY([&] {
		lightbox = findByTypePrefix(window->contentItem(), "Lightbox");
		return lightbox != nullptr && lightbox->property("shown").toBool();
	}());
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
	const qreal padX = retryTip->property("padX").toReal();
	const qreal uiScale = retryTip->property("uiScale").toReal();
	QVERIFY(retryTip->property("width").toReal() <= (maxContentWidth + 2.0 * padX) * uiScale + 0.5);
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

	QQuickItem* contextBar = findItem(line, [](QQuickItem* item) { return item->objectName() == u"contextBar"_s; });
	QQuickItem* recommendTick = findItem(line, [](QQuickItem* item) { return item->objectName() == u"contextRecommendTick"_s; });
	QQuickItem* autoTick = findItem(line, [](QQuickItem* item) { return item->objectName() == u"contextAutoTick"_s; });
	QVERIFY(contextBar && recommendTick && autoTick);
	QVERIFY(recommendTick->isVisible());
	QVERIFY(autoTick->isVisible());
	const qreal expectedRecX = std::max(0.0, std::min(contextBar->width() - recommendTick->width(),
	                                                  contextBar->width() * (150000.0 / 200000.0) - recommendTick->width() / 2.0));
	const qreal expectedAutoX = std::max(0.0, std::min(contextBar->width() - autoTick->width(),
	                                                   contextBar->width() * (180000.0 / 200000.0) - autoTick->width() / 2.0));
	QVERIFY(qFuzzyCompare(recommendTick->x(), expectedRecX));
	QVERIFY(qFuzzyCompare(autoTick->x(), expectedAutoX));
	QVERIFY(recommendTick->x() < autoTick->x());

	// Thresholds of 0 or >= max hide the ticks.
	retryInfo[u"context"_s] = QJsonObject{{u"used"_s, 1234}, {u"max"_s, 200000}, {u"recommend"_s, 0}, {u"auto"_s, 200000}};
	f.fake.send({{u"kind"_s, u"event"_s},
	             {u"session_id"_s, u"ses_a"_s},
	             {u"event"_s, QJsonObject{{u"type"_s, u"info"_s}, {u"info"_s, retryInfo}}}});
	QTRY_COMPARE(f.workspace->current()->contextRecommend(), qint64(0));
	QCOMPARE(f.workspace->current()->contextAuto(), qint64(200000));
	QVERIFY(!recommendTick->isVisible());
	QVERIFY(!autoTick->isVisible());

	retryInfo[u"context"_s] = QJsonObject{{u"used"_s, 1234}, {u"max"_s, 200000}, {u"recommend"_s, 250000}, {u"auto"_s, 0}};
	f.fake.send({{u"kind"_s, u"event"_s},
	             {u"session_id"_s, u"ses_a"_s},
	             {u"event"_s, QJsonObject{{u"type"_s, u"info"_s}, {u"info"_s, retryInfo}}}});
	QTRY_COMPARE(f.workspace->current()->contextRecommend(), qint64(250000));
	QVERIFY(!recommendTick->isVisible());
	QVERIFY(!autoTick->isVisible());

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
	QTRY_VERIFY(window->isActive());
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
	QTRY_VERIFY(paletteInput->hasActiveFocus());
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
	QTRY_VERIFY(window->activeFocusItem() != nullptr && window->activeFocusItem()->hasActiveFocus());
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

void TestMainWindow::configControlsValidateStrictInputsAndPreserveJsonStrings()
{
	MainFixture f;
	QQmlEngine engine;
	QSignalSpy warningSpy(&engine, &QQmlEngine::warnings);
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);

	QQuickWindow window;
	window.resize(900, 700);
	window.show();
	QVERIFY(QTest::qWaitForWindowExposed(&window));

	QQmlComponent component(&engine);
	component.setData(R"qml(
		import QtQuick
		import RefGui
		Column {
			width: 520
			spacing: 8
			NumberField {
				objectName: "strictRatio"
				decimals: true
				minimum: 0
				maximum: 1
				exclusiveMinimum: true
				value: 0.6
			}
			KeyValueEditor {
				objectName: "jsonKv"
				valueMode: "json"
				allowReorder: true
			}
			ColorSwatchPicker {
				objectName: "strictSwatch"
				value: "#336699"
			}
		}
	)qml", QUrl(u"qrc:/qt/qml/RefGui/StrictControls.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));
	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* root = qobject_cast<QQuickItem*>(created.get());
	QVERIFY(root);
	root->setParentItem(window.contentItem());

	// 1) NumberField rejects non-numeric text and `0` under `exclusiveMinimum: true` without silent clamping.
	auto* ratio = root->findChild<QQuickItem*>(u"strictRatio"_s);
	QVERIFY(ratio);
	auto* ratioInput = findByTypePrefix(ratio, "QQuickTextInput");
	QVERIFY(ratioInput);
	ratioInput->setProperty("text", u"abc"_s);
	QMetaObject::invokeMethod(ratio, "parseAndEmit", Q_ARG(QVariant, false));
	QVERIFY(ratio->property("hasValidationError").toBool());
	QVERIFY(ratio->property("invalidSyntax").toBool());
	QCOMPARE(ratio->property("value").toDouble(), 0.6);

	ratioInput->setProperty("text", u"0"_s);
	QMetaObject::invokeMethod(ratio, "parseAndEmit", Q_ARG(QVariant, false));
	QVERIFY(ratio->property("hasValidationError").toBool());
	QVERIFY(ratio->property("outOfRange").toBool());
	QCOMPARE(ratio->property("value").toDouble(), 0.6);
	QMetaObject::invokeMethod(ratio, "resetState");
	QVERIFY(!ratio->property("hasValidationError").toBool());
	QCOMPARE(ratioInput->property("text").toString(), u"0.6"_s);

	// 2) KeyValueEditor in "json" mode preserves quotes on strings, flags malformed JSON, and commits pending drafts.
	auto* jsonKv = root->findChild<QQuickItem*>(u"jsonKv"_s);
	QVERIFY(jsonKv);
	const QVariantList initialEntries{
		QVariantMap{{u"key"_s, u"code"_s}, {u"value"_s, u"123"_s}, {u"json"_s, u"\"123\""_s}},
		QVariantMap{{u"key"_s, u"tier"_s}, {u"value"_s, u"high"_s}, {u"json"_s, u"\"high\""_s}},
	};
	QMetaObject::invokeMethod(jsonKv, "loadFromEntries", Q_ARG(QVariant, initialEntries));
	QVariant formattedCode;
	QMetaObject::invokeMethod(jsonKv, "formatEntryValue", Q_RETURN_ARG(QVariant, formattedCode), Q_ARG(QVariant, initialEntries.at(0)));
	QCOMPARE(formattedCode.toString(), u"\"123\""_s);

	QMetaObject::invokeMethod(jsonKv, "onRowEdited", Q_ARG(QVariant, 0), Q_ARG(QVariant, u"code"_s), Q_ARG(QVariant, u"{bad"_s));
	QVERIFY(jsonKv->property("hasValidationError").toBool());
	QVariant commitResult;
	QMetaObject::invokeMethod(jsonKv, "commitPending", Q_RETURN_ARG(QVariant, commitResult));
	QVERIFY(!commitResult.toBool());
	QMetaObject::invokeMethod(jsonKv, "resetState");
	QVERIFY(!jsonKv->property("hasValidationError").toBool());

	// Pending bottom draft commits via commitPending() and reordering works.
	QList<QQuickItem*> kvInputs;
	findItem(jsonKv, [&](QQuickItem* item) {
		if(QByteArray(item->metaObject()->className()).startsWith("QQuickTextInput"))
		{
			kvInputs.append(item);
		}
		return false;
	});
	QVERIFY(kvInputs.size() >= 6); // 2 rows * 2 inputs + 2 bottom draft inputs
	QQuickItem* draftKeyInput = kvInputs.at(kvInputs.size() - 2);
	QQuickItem* draftValInput = kvInputs.at(kvInputs.size() - 1);
	draftKeyInput->setProperty("text", u"extra"_s);
	draftValInput->setProperty("text", u"\"quoted\""_s);
	QMetaObject::invokeMethod(jsonKv, "updateDraftValidation");
	QVERIFY(jsonKv->property("hasUncommittedEdit").toBool());
	QMetaObject::invokeMethod(jsonKv, "commitPending", Q_RETURN_ARG(QVariant, commitResult));
	QVERIFY(commitResult.toBool());
	QCOMPARE(jsonKv->property("entries").toList().size(), 3);
	QMetaObject::invokeMethod(jsonKv, "moveRow", Q_ARG(QVariant, 2), Q_ARG(QVariant, -1));
	QCOMPARE(jsonKv->property("entries").toList().at(1).toMap().value(u"key"_s).toString(), u"extra"_s);

	// 3) ColorSwatchPicker keeps `value` intact when invalid `#zzzzzz` is typed into the hex input.
	auto* swatch = root->findChild<QQuickItem*>(u"strictSwatch"_s);
	QVERIFY(swatch);
	auto* hexInput = findByTypePrefix(swatch, "QQuickTextInput");
	QVERIFY(hexInput);
	hexInput->forceActiveFocus();
	hexInput->setProperty("text", u"#zzzzzz"_s);
	QMetaObject::invokeMethod(hexInput, "textEdited");
	QVERIFY(swatch->property("hasValidationError").toBool());
	QCOMPARE(swatch->property("value").toString(), u"#336699"_s);
	QMetaObject::invokeMethod(swatch, "resetState");
	QVERIFY(!swatch->property("hasValidationError").toBool());
	QCOMPARE(hexInput->property("text").toString(), u"#336699"_s);
	QVERIFY2(warningSpy.isEmpty(), "Strict config controls emitted QML warnings");
}

void TestMainWindow::configDialogSupportsOpencodeAliasesInlineRenameTopBarLockAndQuitGuard()
{
	MainFixture f;
	f.stub.setConfigRawText(u"{\n"
	                        "  \"provider\": {\n"
	                        "    \"legacy\": {\n"
	                        "      \"options\": {\n"
	                        "        \"api_key\": \"{env:LEGACY_KEY}\",\n"
	                        "        \"base_url\": \"https://legacy.example.com/v1\"\n"
	                        "      },\n"
	                        "      \"models\": {\n"
	                        "        \"vision-pro\": {\n"
	                        "          \"thinking\": true,\n"
	                        "          \"modalities\": { \"input\": [\"image\", \"pdf\"] }\n"
	                        "        }\n"
	                        "      }\n"
	                        "    }\n"
	                        "  },\n"
	                        "  \"agent\": {\n"
	                        "    \"helper\": {\n"
	                        "      \"spawn\": false,\n"
	                        "      \"disabled\": true\n"
	                        "    }\n"
	                        "  },\n"
	                        "  \"mcp\": {\n"
	                        "    \"local-srv\": {\n"
	                        "      \"command\": \"npx\",\n"
	                        "      \"args\": [\"-y\", \"mcp-server\"],\n"
	                        "      \"env\": { \"MCP_TOKEN\": \"secret\" }\n"
	                        "    }\n"
	                        "  },\n"
	                        "  \"permission\": {\n"
	                        "    \"edit\": { \"*.env\": \"deny\", \"*\": \"allow\" },\n"
	                        "    \"bash\": \"ask\"\n"
	                        "  }\n"
	                        "}\n"_s);

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
	QTRY_VERIFY(window->isActive());
	const auto findByName = [](QQuickItem* root, const QString& name) {
		return findItem(root, [&name](QQuickItem* item) { return item->objectName() == name; });
	};
	auto* themeToggle = findByTypePrefix(window->contentItem(), "ThemeToggle");
	auto* splitBtn = findItem(window->contentItem(), [](QQuickItem* item) {
		return item->property("iconName").toString() == u"split"_s;
	});
	QVERIFY(themeToggle && splitBtn);
	QVERIFY(themeToggle->isEnabled() && splitBtn->isEnabled());

	// 1) Opening ConfigDialog disables top-bar buttons while `dialogs.open` is true.
	f.workspace->commands()->run(u"config"_s);
	QQuickItem* configDialog = nullptr;
	QTRY_VERIFY([&] {
		configDialog = findByName(window->contentItem(), u"configDialog"_s);
		return configDialog != nullptr && !f.workspace->configEditor()->isLoading();
	}());
	QVERIFY(!themeToggle->isEnabled());
	QVERIFY(!splitBtn->isEnabled());

	// 2) Opencode schema aliases populate Providers, Agents, and MCP sections accurately.
	QMetaObject::invokeMethod(configDialog, "selectCategory", Q_ARG(QVariant, u"providers"_s));
	QCoreApplication::processEvents();
	auto* provSection = findByName(configDialog, u"configProvidersSection"_s);
	QVERIFY(provSection);
	QMetaObject::invokeMethod(provSection, "setProviderExpanded", Q_ARG(QVariant, u"legacy"_s), Q_ARG(QVariant, true));
	QMetaObject::invokeMethod(provSection, "setModelExpanded", Q_ARG(QVariant, u"legacy"_s), Q_ARG(QVariant, u"vision-pro"_s), Q_ARG(QVariant, true));
	QCoreApplication::processEvents();
	QCOMPARE(findByName(provSection, u"providerBaseUrl_legacy"_s)->property("text").toString(), u"https://legacy.example.com/v1"_s);
	QCOMPARE(findByName(provSection, u"providerApiKey_legacy"_s)->property("text").toString(), u"{env:LEGACY_KEY}"_s);
	QVERIFY(findByName(provSection, u"modelReasoningSwitch_legacy_vision-pro"_s)->property("checked").toBool());
	auto* attachSw = findByName(provSection, u"modelAttachmentSwitch_legacy_vision-pro"_s);
	QVERIFY(attachSw && attachSw->property("checked").toBool());
	// Toggling attachment off removes "image" from modalities.input while preserving "pdf".
	QMetaObject::invokeMethod(attachSw, "toggled", Q_ARG(bool, false));
	QCOMPARE(f.workspace->configEditor()->valueAt({u"provider"_s, u"legacy"_s, u"models"_s, u"vision-pro"_s, u"modalities"_s, u"input"_s}).toStringList(),
	         (QStringList{u"pdf"_s}));
	f.workspace->configEditor()->discard();

	QMetaObject::invokeMethod(configDialog, "selectCategory", Q_ARG(QVariant, u"agents"_s));
	QCoreApplication::processEvents();
	auto* agentsSection = findByName(configDialog, u"configAgentsSection"_s);
	QVERIFY(agentsSection);
	QMetaObject::invokeMethod(agentsSection, "setExpanded", Q_ARG(QVariant, u"helper"_s), Q_ARG(QVariant, true));
	QCoreApplication::processEvents();
	QVERIFY(!findByName(agentsSection, u"agentCanSpawnSwitch_helper"_s)->property("checked").toBool());
	QVERIFY(findByName(agentsSection, u"agentDisableSwitch_helper"_s)->property("checked").toBool());

	QMetaObject::invokeMethod(configDialog, "selectCategory", Q_ARG(QVariant, u"mcp"_s));
	QCoreApplication::processEvents();
	auto* mcpSection = findByName(configDialog, u"configMcpSection"_s);
	QVERIFY(mcpSection);
	QMetaObject::invokeMethod(mcpSection, "setExpanded", Q_ARG(QVariant, u"local-srv"_s), Q_ARG(QVariant, true));
	QCoreApplication::processEvents();
	QCOMPARE(findByName(mcpSection, u"mcpCommandEditor_local-srv"_s)->property("values").toStringList(),
	         (QStringList{u"npx"_s, u"-y"_s, u"mcp-server"_s}));
	QCOMPARE(findByName(mcpSection, u"mcpEnvEditor_local-srv"_s)->property("entries").toList().size(), 1);

	QMetaObject::invokeMethod(configDialog, "selectCategory", Q_ARG(QVariant, u"permissions"_s));
	QCoreApplication::processEvents();
	auto* permEditor = findByName(configDialog, u"globalPermissionEditor"_s);
	QVERIFY(permEditor);
	const QVariantList loadedRules = permEditor->property("rules").toList();
	QCOMPARE(loadedRules.size(), 3);
	QCOMPARE(loadedRules.at(0).toMap().value(u"pattern"_s).toString(), u"*.env"_s);
	QCOMPARE(loadedRules.at(1).toMap().value(u"pattern"_s).toString(), u"*"_s);
	QCOMPARE(loadedRules.at(2).toMap().value(u"tool"_s).toString(), u"bash"_s);
	// 3) Inline rename commits on triggerSave() via focusSink without pressing Enter or Rename.
	QMetaObject::invokeMethod(configDialog, "selectCategory", Q_ARG(QVariant, u"providers"_s));
	QCoreApplication::processEvents();
	provSection = findByName(configDialog, u"configProvidersSection"_s);
	QMetaObject::invokeMethod(provSection, "setProviderExpanded", Q_ARG(QVariant, u"legacy"_s), Q_ARG(QVariant, true));
	QCoreApplication::processEvents();
	auto* renameField = findByName(provSection, u"providerRenameField_legacy"_s);
	QVERIFY(renameField);
	auto* renameInput = qobject_cast<QQuickItem*>(renameField->property("inputItem").value<QObject*>());
	QVERIFY(renameInput);
	renameInput->forceActiveFocus();
	renameField->setProperty("text", u"renamed-prov"_s);
	QMetaObject::invokeMethod(renameField, "edited");
	QVERIFY(findByName(configDialog, u"configSaveButton"_s)->isEnabled());
	QMetaObject::invokeMethod(configDialog, "triggerSave");
	QTRY_COMPARE(f.stub.saveConfigCalls(), 1);
	QVERIFY(f.stub.savedConfigRawText().contains(u"\"renamed-prov\""_s));
	QVERIFY(!f.stub.savedConfigRawText().contains(u"\"legacy\""_s));

	// 4) Closing the window while ConfigDialog has unsaved changes prompts `quitWindow` confirmation.
	f.workspace->configEditor()->setStringAt({u"$name"_s}, u"Unsaved Edit"_s);
	QVERIFY(f.workspace->configEditor()->isDirty());
	QVERIFY(!window->close());
	QVERIFY(window->isVisible());
	auto* dirtyBar = findByName(configDialog, u"configDirtyConfirmBar"_s);
	QVERIFY(dirtyBar && dirtyBar->isVisible());
	QCOMPARE(configDialog->property("pendingDirtyAction").toMap().value(u"type"_s).toString(), u"quitWindow"_s);
	auto* dirtyDiscardBtn = findByName(configDialog, u"configDirtyDiscardButton"_s);
	QVERIFY(dirtyDiscardBtn);
	QMetaObject::invokeMethod(dirtyDiscardBtn, "clicked");
	QTRY_VERIFY(!window->isVisible());
	QVERIFY2(warningSpy.isEmpty(), "ConfigDialog aliases, inline rename, and quit guard emitted QML warnings");
}
void TestMainWindow::tableBlockRendersAndSelectionLeavesAboveRowsUntouched()
{
	MainFixture f;
	f.stub.setMessages(u"ses_b"_s, QJsonArray{
		QJsonObject{{u"id"_s, u"u1"_s}, {u"role"_s, u"user"_s}, {u"time"_s, 1}, {u"parts"_s, QJsonArray{QJsonObject{{u"type"_s, u"text"_s}, {u"text"_s, u"Show table"_s}, {u"time"_s, 1}}}}},
		QJsonObject{{u"id"_s, u"a1"_s}, {u"role"_s, u"assistant"_s}, {u"time"_s, 2}, {u"time_end"_s, 3}, {u"parts"_s, QJsonArray{QJsonObject{{u"type"_s, u"text"_s}, {u"text"_s, u"| Col A | Col B |\n| :--- | ---: |\n| Alpha | 100 |\n| Bravo | 200 |\n| Charlie | 300 |"_s}, {u"time"_s, 2}}}}},
	});
	f.workspace->openSession(u"ses_b"_s);
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
	QQuickItem* tableBlock = nullptr;
	QTRY_VERIFY([&] {
		tableBlock = findByTypePrefix(window->contentItem(), "TableBlock_QMLTYPE");
		return tableBlock != nullptr && tableBlock->height() > 60;
	}());
	QList<QQuickItem*> cells;
	findItem(tableBlock, [&](QQuickItem* item) {
		if(QByteArray(item->metaObject()->className()).startsWith("ProseText_QMLTYPE"))
		{
			cells.append(item);
		}
		return false;
	});
	QCOMPARE(cells.size(), 8); // 2 header cells + 3x2 body cells
	QQuickItem* row0Cell = cells.at(2);
	QQuickItem* row1Cell = cells.at(4);
	QQuickItem* row2Cell = cells.at(6);
	QCOMPARE(row2Cell->property("text").toString().trimmed(), u"Charlie"_s);
	const auto sceneRectOf = [](QQuickItem* item) {
		const QPointF tl = item->mapToScene(QPointF(0, 0));
		return QRect(qRound(tl.x()), qRound(tl.y()), qRound(item->width()), qRound(item->height()));
	};
	const QRect row0Rect = sceneRectOf(row0Cell);
	const QRect row1Rect = sceneRectOf(row1Cell);
	const QImage before = window->grabWindow();
	const QPoint start = row2Cell->mapToScene(QPointF(2, row2Cell->height() / 2.0)).toPoint();
	const QPoint end = row2Cell->mapToScene(QPointF(row2Cell->width() - 2, row2Cell->height() / 2.0)).toPoint();
	QTest::mousePress(window, Qt::LeftButton, Qt::NoModifier, start);
	QTest::mouseMove(window, end);
	QTest::mouseRelease(window, Qt::LeftButton, Qt::NoModifier, end);
	QTRY_VERIFY(!row2Cell->property("selectedText").toString().isEmpty());
	const QImage after = window->grabWindow();
	QCOMPARE(after.copy(row0Rect), before.copy(row0Rect));
	QCOMPARE(after.copy(row1Rect), before.copy(row1Rect));
	QVERIFY2(warningSpy.isEmpty(), "TableBlock emitted QML warnings");
}
void TestMainWindow::shimmerTextModelsHoverCtrlTAtomicImageAndCompletionsWork()
{
	MainFixture f;
	f.stub.setModels(QJsonArray{
		QJsonObject{{u"provider"_s, u"p"_s}, {u"provider_name"_s, u"Provider"_s}, {u"id"_s, u"m1"_s}, {u"name"_s, u"Model One"_s}, {u"variants"_s, QJsonArray{u"low"_s, u"high"_s}}},
		QJsonObject{{u"provider"_s, u"p"_s}, {u"provider_name"_s, u"Provider"_s}, {u"id"_s, u"m2"_s}, {u"name"_s, u"Model Two"_s}, {u"variants"_s, QJsonArray{u"low"_s, u"high"_s}}},
	});
	// Update ses_a to have a configured model with variants.
	QJsonObject sesA = sessionInfoJson(u"ses_a"_s, {.title = u"Session ses_a"_s, .model = QJsonObject{{u"provider"_s, u"p"_s}, {u"model"_s, u"m1"_s}, {u"variant"_s, u"low"_s}}, .createdAt = 1});
	f.fake.send({{u"kind"_s, u"sessions"_s}, {u"sessions"_s, QJsonArray{sesA, sessionInfoJson(u"ses_b"_s, {.title = u"Session ses_b"_s, .createdAt = 2})}}});
	f.fake.send({{u"kind"_s, u"event"_s}, {u"session_id"_s, u"ses_a"_s}, {u"event"_s, QJsonObject{{u"type"_s, u"info"_s}, {u"info"_s, sesA}}}});
	f.workspace->reloadConfig();
	QQmlEngine engine;
	QSignalSpy warningSpy(&engine, &QQmlEngine::warnings);
	engine.addImageProvider(u"icons"_s, new refgui::icons::IconProvider);
	engine.addImageProvider(u"chatimage"_s, new ChatImageProvider([ws = QPointer<Workspace>(f.workspace.get())](const QString& id) {
		return ws ? ws->session(id) : nullptr;
	}));
	// 1) ShimmerText Canvas paints bright highlight sweep pixels without warnings.
	{
		QQmlComponent shimmerComp(&engine, QUrl(u"qrc:/qt/qml/RefGui/ShimmerText.qml"_s));
		QVERIFY2(shimmerComp.isReady(), qPrintable(shimmerComp.errorString()));
		QQuickWindow shimmerWin;
		shimmerWin.resize(160, 48);
		shimmerWin.setColor(QColor(u"#16171a"_s));
		shimmerWin.show();
		QVERIFY(QTest::qWaitForWindowExposed(&shimmerWin));
		std::unique_ptr<QObject> shimmerObj(shimmerComp.createWithInitialProperties({
			{u"parent"_s, QVariant::fromValue(shimmerWin.contentItem())},
			{u"x"_s, 10},
			{u"y"_s, 10},
			{u"label"_s, u"Working"_s},
			{u"color"_s, QColor(u"#505050"_s)},
			{u"highlight"_s, QColor(u"#ffffff"_s)},
			{u"running"_s, true},
			{u"phase"_s, 0.5},
		}));
		QVERIFY2(shimmerObj, qPrintable(shimmerComp.errorString()));
		auto* shimmerItem = qobject_cast<QQuickItem*>(shimmerObj.get());
		QVERIFY(shimmerItem && shimmerItem->width() > 0);
		QCoreApplication::processEvents();
		QTest::qWait(50);
		const QImage shot = shimmerWin.grabWindow();
		int brightPixels = 0;
		for(int y = 0; y < shot.height(); ++y)
		{
			for(int x = 0; x < shot.width(); ++x)
			{
				const QColor c = shot.pixelColor(x, y);
				if(c.red() > 170 && c.green() > 170 && c.blue() > 170)
				{
					++brightPixels;
				}
			}
		}
		QVERIFY2(brightPixels > 0, "Expected ShimmerText Canvas to paint bright highlight pixels");
	}
	QQmlComponent component(&engine, QUrl(u"qrc:/qt/qml/RefGui/Main.qml"_s));
	QVERIFY2(component.isReady(), qPrintable(component.errorString()));
	std::unique_ptr<QObject> created(component.create());
	QVERIFY2(created, qPrintable(component.errorString()));
	auto* window = qobject_cast<QQuickWindow*>(created.get());
	QVERIFY(window);
	QVERIFY(QTest::qWaitForWindowExposed(window));
	window->requestActivate();
	QTRY_VERIFY(window->isActive());
	QTRY_VERIFY(f.workspace->current()->composer()->variantAvailable());
	// 2) Ctrl+T cycles thinking variant via Workspace.setVariant(sessionId, "").
	QTest::keyClick(window, Qt::Key_T, Qt::ControlModifier);
	QTRY_COMPARE(f.stub.setVariantRequests().size(), 1);
	QVERIFY(!f.stub.setVariantRequests().first().contains(u"variant"_s) || f.stub.setVariantRequests().first().value(u"variant"_s).toString().isEmpty());
	// 3) ModelsDialog hover updates PickList currentIndex.
	f.workspace->commands()->run(u"models"_s);
	QQuickItem* pickList = nullptr;
	QTRY_VERIFY([&] {
		pickList = findByTypePrefix(window->contentItem(), "PickList_QMLTYPE");
		return pickList != nullptr && pickList->property("count").toInt() == 2;
	}());
	QList<QQuickItem*> pickRows;
	findItem(pickList, [&](QQuickItem* item) {
		if(QByteArray(item->metaObject()->className()).startsWith("PickRow_QMLTYPE"))
		{
			pickRows.append(item);
		}
		return false;
	});
	QCOMPARE(pickRows.size(), 2);
	const QPoint r0 = pickRows.at(0)->mapToScene(QPointF(pickRows.at(0)->width() / 2.0, pickRows.at(0)->height() / 2.0)).toPoint();
	const QPoint r1 = pickRows.at(1)->mapToScene(QPointF(pickRows.at(1)->width() / 2.0, pickRows.at(1)->height() / 2.0)).toPoint();
	QTest::mouseMove(window, r0);
	QTest::mouseMove(window, r1);
	QTRY_COMPARE(pickList->property("currentIndex").toInt(), 1);
	QTest::keyClick(window, Qt::Key_Escape);
	QTRY_VERIFY(f.workspace->dialog().isEmpty());
	// 4) Atomic [Image N] snapping + deletion and slash completion in Composer.qml.
	auto* composerItem = findByTypePrefix(window->contentItem(), "Composer");
	QVERIFY(composerItem);
	QMetaObject::invokeMethod(composerItem, "focusInput");
	QQuickItem* textArea = nullptr;
	QTRY_VERIFY([&] {
		textArea = qobject_cast<QQuickItem*>(window->activeFocusItem());
		return textArea != nullptr && textArea->hasActiveFocus();
	}());
	QImage img(4, 4, QImage::Format_RGB32);
	img.fill(Qt::red);
	QGuiApplication::clipboard()->setImage(img);
	QVERIFY(f.workspace->current()->composer()->pasteFromClipboard());
	QCOMPARE(f.workspace->current()->composer()->text(), u"[Image 1] "_s);
	QCOMPARE(textArea->property("cursorPosition").toInt(), 10);
	QTest::keyClick(window, Qt::Key_Left); // moves from 10 to 9 (end of "[Image 1]")
	QCOMPARE(textArea->property("cursorPosition").toInt(), 9);
	QTest::keyClick(window, Qt::Key_Left); // snaps across "[Image 1]" to 0!
	QCOMPARE(textArea->property("cursorPosition").toInt(), 0);
	QTest::keyClick(window, Qt::Key_Delete); // deletes "[Image 1] " atomically
	QCOMPARE(f.workspace->current()->composer()->text(), QString());
	QCOMPARE(f.workspace->current()->composer()->attachments()->rowCount(), 0);
	QVERIFY(f.workspace->current()->composer()->pasteFromClipboard());
	QCOMPARE(f.workspace->current()->composer()->text(), u"[Image 1] "_s);
	f.workspace->current()->composer()->clearDraft();
	const auto typeText = [&](const QString& text) { typeAsciiText(window, text); };
	// Standalone `/mod` Tab completes without trailing space.
	typeText(u"/mod"_s);
	QTest::keyClick(window, Qt::Key_Tab);
	QCOMPARE(f.workspace->current()->composer()->text(), u"/models"_s);
	f.workspace->current()->composer()->clearDraft();
	// Mid-text `/comp` Enter completes without submitting the prompt.
	typeText(u"check /comp"_s);
	QTest::keyClick(window, Qt::Key_Return);
	QCOMPARE(f.workspace->current()->composer()->text(), u"check /compact"_s);
	f.workspace->current()->composer()->clearDraft();
	// `@` file completion populates CompletionPopup via ComposerModel.files Q_PROPERTY.
	f.stub.setFiles({u"src/main.rs"_s, u"README.md"_s});
	typeText(u"@ma"_s);
	auto* popup = findByTypePrefix(composerItem, "CompletionPopup");
	QVERIFY(popup);
	QTRY_VERIFY(popup->property("open").toBool() && popup->property("kind").toString() == u"file"_s && popup->property("count").toInt() == 2);
	QTest::keyClick(window, Qt::Key_Tab);
	QCOMPARE(f.workspace->current()->composer()->text(), u"@src/main.rs "_s);
	QVERIFY2(warningSpy.isEmpty(), "ShimmerText, ModelsDialog hover, Ctrl+T, or Composer emitted QML warnings");
}
void TestMainWindow::splitJumpToEndResizeBlurFadeAndSidebarDragWork()
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
	window->requestActivate();
	QTRY_VERIFY(window->isActive());
	// 1) Resizable sidebar: drag handle wider, then double-click to reset.
	auto* sidebar = findByTypePrefix(window->contentItem(), "Sidebar_QMLTYPE");
	auto* handle = findItem(window->contentItem(), [](QQuickItem* item) { return item->objectName() == u"sidebarResizeHandle"_s; });
	QVERIFY(sidebar && handle);
	QCOMPARE(qRound(sidebar->width()), 264);
	const QPoint hCenter = handle->mapToScene(QPointF(handle->width() / 2.0, handle->height() / 2.0)).toPoint();
	QTest::mousePress(window, Qt::LeftButton, Qt::NoModifier, hCenter);
	QTest::mouseMove(window, hCenter + QPoint(20, 0));
	QTest::mouseMove(window, hCenter + QPoint(80, 0));
	QTest::mouseRelease(window, Qt::LeftButton, Qt::NoModifier, hCenter + QPoint(80, 0));
	QTRY_VERIFY(f.workspace->settings()->sidebarWidth() > 300);
	QTRY_COMPARE(qRound(sidebar->width()), f.workspace->settings()->sidebarWidth());
	const QPoint hNewCenter = handle->mapToScene(QPointF(handle->width() / 2.0, handle->height() / 2.0)).toPoint();
	QTest::mouseDClick(window, Qt::LeftButton, Qt::NoModifier, hNewCenter);
	QTRY_COMPARE(f.workspace->settings()->sidebarWidth(), 264);
	// Collapsing sidebar mid-drag disables drag/handle and animates smoothly to 0.
	const QPoint hResetCenter = handle->mapToScene(QPointF(handle->width() / 2.0, handle->height() / 2.0)).toPoint();
	QTest::mousePress(window, Qt::LeftButton, Qt::NoModifier, hResetCenter);
	QTest::mouseMove(window, hResetCenter + QPoint(40, 0));
	QTRY_VERIFY(sidebar->property("resizing").toBool());
	const int draggedWidth = f.workspace->settings()->sidebarWidth();
	QCOMPARE(draggedWidth, 304);
	f.workspace->commands()->run(u"sidebar"_s);
	QTest::mouseRelease(window, Qt::LeftButton, Qt::NoModifier, hResetCenter + QPoint(40, 0));
	QTRY_VERIFY(!sidebar->property("resizing").toBool() && !handle->isVisible());
	QTRY_COMPARE(qRound(sidebar->width()), 0);
	f.workspace->commands()->run(u"sidebar"_s);
	QTRY_COMPARE(qRound(sidebar->width()), draggedWidth);
	// 2) Enter split view, click jumpToEnd on unfocused tile without opening Composer.
	f.workspace->toggleSplit();
	QQuickItem* tile = nullptr;
	QTRY_VERIFY([&] {
		tile = findByTypePrefix(window->contentItem(), "Tile_QMLTYPE");
		return tile != nullptr && tile->property("arrived").toBool();
	}());
	f.workspace->focusTile({});
	QVERIFY(f.workspace->focusedTileId().isEmpty());
	auto* chat = findByTypePrefix(tile, "ChatView_QMLTYPE");
	auto* pane = findByTypePrefix(tile, "SessionPane_QMLTYPE");
	QVERIFY(chat && pane);
	const qreal minY = chat->property("minContentY").toReal();
	chat->setProperty("following", false);
	chat->setProperty("contentY", minY);
	QQuickItem* jumpBtn = nullptr;
	QTRY_VERIFY([&] {
		jumpBtn = findItem(pane, [](QQuickItem* item) {
			return QByteArray(item->metaObject()->className()).startsWith("CircleButton_QMLTYPE") && item->isVisible();
		});
		return jumpBtn != nullptr && jumpBtn->opacity() > 0.9;
	}());
	const QPoint jPos = jumpBtn->mapToScene(QPointF(jumpBtn->width() / 2.0, jumpBtn->height() / 2.0)).toPoint();
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, jPos);
	QTRY_VERIFY(chat->property("following").toBool());
	QVERIFY(f.workspace->focusedTileId().isEmpty());
	// 3) Window resize in split mode updates layout and settles cleanly without blur/fade.
	window->setWidth(window->width() + 140);
	QTRY_VERIFY(tile->property("resizingLayout").toBool());
	QVERIFY(qFuzzyCompare(pane->property("opacity").toReal(), 1.0));
	QVERIFY(!pane->property("resizeBlurAmount").isValid());
	QTRY_VERIFY(!tile->property("resizingLayout").toBool() && qFuzzyCompare(pane->property("opacity").toReal(), 1.0));
	QVERIFY2(warningSpy.isEmpty(), "Sidebar drag, jumpToEnd, or split resize emitted QML warnings");
}

void TestMainWindow::sidebarSectionsCollapseExpandAndResizeWithoutOscillation()
{
	MainFixture f;
	const QJsonArray todos{
		QJsonObject{{u"content"_s, u"Inspect sidebar"_s}, {u"status"_s, u"completed"_s}},
		QJsonObject{{u"content"_s, u"Add collapse sections"_s}, {u"status"_s, u"in_progress"_s}},
	};
	f.stub.setTodos(u"ses_a"_s, todos);
	QJsonArray sessions;
	QJsonObject sesA = sessionInfoJson(u"ses_a"_s, {.title = u"Session ses_a"_s, .createdAt = 1});
	sesA[u"todos"_s] = todos;
	sessions.append(sesA);
	sessions.append(sessionInfoJson(u"ses_b"_s, {.title = u"Session ses_b"_s, .createdAt = 2}));
	for(int i = 0; i < 10; ++i)
	{
		const QString id = u"ses_extra_%1"_s.arg(i);
		f.stub.add(id, 3 + i);
		sessions.append(sessionInfoJson(id, {.title = u"Session "_s + id, .createdAt = 3 + i}));
	}
	f.fake.send({{u"kind"_s, u"sessions"_s}, {u"sessions"_s, sessions}});
	f.fake.send({{u"kind"_s, u"event"_s}, {u"session_id"_s, u"ses_a"_s}, {u"event"_s, QJsonObject{{u"type"_s, u"info"_s}, {u"info"_s, sesA}}}});
	QTRY_COMPARE(f.workspace->current()->todoTotal(), 2);

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
	QTRY_VERIFY(window->isActive());
	const auto findByName = [](QQuickItem* root, const QString& name) {
		return findItem(root, [&name](QQuickItem* item) { return item->objectName() == name; });
	};
	auto* sidebar = findByTypePrefix(window->contentItem(), "Sidebar_QMLTYPE");
	auto* handle = findByName(window->contentItem(), u"sidebarResizeHandle"_s);
	QVERIFY(sidebar && handle);
	QCOMPARE(qRound(sidebar->width()), 264);

	// 1) Pixel-by-pixel drag increases width monotonically without oscillation, and double-click resets to 264.
	const QPoint hCenter = handle->mapToScene(QPointF(handle->width() / 2.0, handle->height() / 2.0)).toPoint();
	QTest::mousePress(window, Qt::LeftButton, Qt::NoModifier, hCenter);
	QTest::mouseMove(window, hCenter + QPoint(10, 0));
	QTRY_COMPARE(f.workspace->settings()->sidebarWidth(), 274);
	QTest::mouseMove(window, hCenter + QPoint(20, 0));
	QTRY_COMPARE(f.workspace->settings()->sidebarWidth(), 284);
	QTest::mouseMove(window, hCenter + QPoint(21, 0));
	QTRY_COMPARE(f.workspace->settings()->sidebarWidth(), 285);
	QTest::mouseMove(window, hCenter + QPoint(80, 0));
	QTRY_COMPARE(f.workspace->settings()->sidebarWidth(), 344);
	QTest::mouseRelease(window, Qt::LeftButton, Qt::NoModifier, hCenter + QPoint(80, 0));
	QTRY_COMPARE(qRound(sidebar->width()), 344);

	const QPoint hNewCenter = handle->mapToScene(QPointF(handle->width() / 2.0, handle->height() / 2.0)).toPoint();
	QTest::mouseDClick(window, Qt::LeftButton, Qt::NoModifier, hNewCenter);
	QTRY_COMPARE(f.workspace->settings()->sidebarWidth(), 264);
	QTRY_COMPARE(qRound(sidebar->width()), 264);

	// 2) Todo, Running, and History sections collapse/expand on header click and share height dynamically.
	auto* todoSection = findByName(sidebar, u"todoSection"_s);
	auto* todoHeader = findByName(sidebar, u"todoSectionHeader"_s);
	auto* todoFlickable = findByName(sidebar, u"todoListFlickable"_s);
	auto* runningHeader = findByName(sidebar, u"runningSectionHeader"_s);
	auto* runningList = findByName(sidebar, u"runningList"_s);
	auto* historyHeader = findByName(sidebar, u"historySectionHeader"_s);
	auto* historyBody = findByName(sidebar, u"historyBody"_s);
	QVERIFY(todoSection && todoHeader && todoFlickable && runningHeader && runningList && historyHeader && historyBody);
	QTRY_VERIFY(todoSection->isVisible());
	QTRY_VERIFY(todoFlickable->isVisible());
	QTRY_VERIFY(runningList->isVisible());
	QTRY_VERIFY(historyBody->isVisible());
	const qreal todoExpandedHeight = todoSection->height();
	const qreal runningHeightWithHistory = runningList->height();
	const qreal historyBodyInitialHeight = historyBody->height();
	QVERIFY(todoExpandedHeight > todoHeader->height());
	QVERIFY(runningHeightWithHistory > 0.0);
	QVERIFY(historyBodyInitialHeight > 0.0);

	const auto clickCenter = [&](QQuickItem* item) {
		const QPoint pt = item->mapToScene(QPointF(item->width() / 2.0, item->height() / 2.0)).toPoint();
		QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, pt);
	};

	// Collapse & expand Todo.
	clickCenter(todoHeader);
	QVERIFY(!f.workspace->settings()->sidebarTodoExpanded());
	QTRY_VERIFY(!todoFlickable->isVisible());
	QTRY_COMPARE(qRound(todoSection->height()), qRound(todoHeader->height()));
	QVERIFY(todoHeader->isVisible());

	clickCenter(todoHeader);
	QVERIFY(f.workspace->settings()->sidebarTodoExpanded());
	QTRY_VERIFY(todoFlickable->isVisible());
	QTRY_COMPARE(qRound(todoSection->height()), qRound(todoExpandedHeight));

	// Focus history search input, then collapse History -> clears focus and lets Running claim freed height.
	auto* searchInput = findByTypePrefix(historyBody, "QQuickTextInput");
	QVERIFY(searchInput);
	searchInput->forceActiveFocus();
	QTRY_VERIFY(searchInput->hasActiveFocus());

	clickCenter(historyHeader);
	QVERIFY(!f.workspace->settings()->sidebarHistoryExpanded());
	QVERIFY(!searchInput->hasActiveFocus());
	QTRY_VERIFY(!historyBody->isVisible());
	QVERIFY(historyHeader->isVisible());
	QTRY_VERIFY(runningList->height() > runningHeightWithHistory + 40.0);

	// Collapse Running -> Running list hides while its header remains visible.
	clickCenter(runningHeader);
	QVERIFY(!f.workspace->settings()->sidebarRunningExpanded());
	QTRY_VERIFY(!runningList->isVisible());
	QVERIFY(runningHeader->isVisible());

	// Expand History while Running is collapsed -> History claims the freed space.
	clickCenter(historyHeader);
	QVERIFY(f.workspace->settings()->sidebarHistoryExpanded());
	QTRY_VERIFY(historyBody->isVisible());
	QTRY_VERIFY(historyBody->height() > historyBodyInitialHeight + 40.0);

	// Expand Running -> restores original split between Running and History.
	clickCenter(runningHeader);
	QVERIFY(f.workspace->settings()->sidebarRunningExpanded());
	QTRY_VERIFY(runningList->isVisible());
	QTRY_COMPARE(qRound(runningList->height()), qRound(runningHeightWithHistory));

	// Keyboard focus and Return / Enter / Space toggle on SectionHeader, plus visible focus border.
	auto* runningBg = findByTypePrefix(runningHeader, "QQuickRectangle");
	QVERIFY(runningBg);
	runningHeader->forceActiveFocus();
	QTRY_VERIFY(runningHeader->hasActiveFocus());
	QTRY_VERIFY(runningBg->property("border").value<QObject*>() != nullptr);
	QCOMPARE(runningBg->property("border").value<QObject*>()->property("width").toReal(), 1.0);
	QTRY_VERIFY(runningBg->property("border").value<QObject*>()->property("color").value<QColor>().alpha() > 0);
	QTest::keyClick(window, Qt::Key_Space);
	QVERIFY(!f.workspace->settings()->sidebarRunningExpanded());
	QTest::keyClick(window, Qt::Key_Return);
	QVERIFY(f.workspace->settings()->sidebarRunningExpanded());
	QTest::keyClick(window, Qt::Key_Enter);
	QVERIFY(!f.workspace->settings()->sidebarRunningExpanded());
	QTest::keyClick(window, Qt::Key_Enter);
	QVERIFY(f.workspace->settings()->sidebarRunningExpanded());
	QTRY_VERIFY(runningList->isVisible());

	// At ~580px height, collapsed Todo must not show a dangling header that vanishes on expand.
	const int originalWindowHeight = window->height();
	f.workspace->settings()->setSidebarTodoExpanded(false);
	window->setHeight(580);
	QTRY_VERIFY(!sidebar->property("todoFits").toBool());
	QTRY_VERIFY(!todoSection->isVisible());
	QCOMPARE(qRound(todoSection->height()), 0);
	window->setHeight(originalWindowHeight);
	QTRY_VERIFY(sidebar->property("todoFits").toBool());
	QTRY_VERIFY(todoSection->isVisible());
	QTRY_COMPARE(qRound(todoSection->height()), qRound(todoHeader->height()));
	f.workspace->settings()->setSidebarTodoExpanded(true);
	QTRY_VERIFY(todoFlickable->isVisible());

	QVERIFY2(warningSpy.isEmpty(), "Sidebar collapse/expand, keyboard toggle, or drag emitted QML warnings");
}

void TestMainWindow::composerUndoRedoSyncsAttachmentsAndHistoryAvoidsCollision()
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
	window->requestActivate();
	QTRY_VERIFY(window->isActive());

	auto* composerItem = findByTypePrefix(window->contentItem(), "Composer");
	QVERIFY(composerItem);
	QMetaObject::invokeMethod(composerItem, "focusInput");
	QQuickItem* textArea = nullptr;
	QTRY_VERIFY([&] {
		textArea = qobject_cast<QQuickItem*>(window->activeFocusItem());
		return textArea != nullptr && textArea->hasActiveFocus();
	}());
	auto* composer = f.workspace->current()->composer();
	QVERIFY(composer);

	QImage img(4, 4, QImage::Format_RGB32);
	img.fill(Qt::blue);
	QGuiApplication::clipboard()->setImage(img);
	const auto typeText = [&](const QString& text) {
		for(const QChar ch : text)
		{
			const Qt::Key k = ch.isLetter() ? static_cast<Qt::Key>(ch.toUpper().unicode()) : static_cast<Qt::Key>(ch.unicode());
			const Qt::KeyboardModifiers mods = ch.isUpper() ? Qt::ShiftModifier : Qt::NoModifier;
			QTest::keyClick(window, k, mods);
		}
	};
	const auto triggerRedo = [&] {
		QTest::keyClick(window, Qt::Key_Y, Qt::ControlModifier);
	};

	// 1) Attach -> type word -> Ctrl+Z restores "[Image 1] " with attachment -> Ctrl+Z restores "" -> Redo restores.
	QVERIFY(composer->pasteFromClipboard());
	QCOMPARE(composer->text(), u"[Image 1] "_s);
	QCOMPARE(composer->attachments()->rowCount(), 1);
	typeText(u"word"_s);
	QCOMPARE(composer->text(), u"[Image 1] word"_s);
	QCOMPARE(textArea->property("text").toString(), u"[Image 1] word"_s);

	QTest::keyClick(window, Qt::Key_Z, Qt::ControlModifier);
	QCOMPARE(composer->text(), u"[Image 1] "_s);
	QCOMPARE(textArea->property("text").toString(), u"[Image 1] "_s);
	QCOMPARE(composer->attachments()->rowCount(), 1);

	QTest::keyClick(window, Qt::Key_Z, Qt::ControlModifier);
	QCOMPARE(composer->text(), QString());
	QCOMPARE(textArea->property("text").toString(), QString());
	QCOMPARE(composer->attachments()->rowCount(), 0);
	QVERIFY(composer->pasteFromClipboard());
	QCOMPARE(composer->text(), u"[Image 1] "_s);
	QTest::keyClick(window, Qt::Key_Z, Qt::ControlModifier);
	QCOMPARE(composer->text(), QString());
	QCOMPARE(composer->attachments()->rowCount(), 0);

	// Re-run attach -> type -> Ctrl+Z -> Ctrl+Z -> Ctrl+Y redo path.
	QVERIFY(composer->pasteFromClipboard());
	typeText(u"word"_s);
	QTest::keyClick(window, Qt::Key_Z, Qt::ControlModifier);
	QCOMPARE(composer->text(), u"[Image 1] "_s);
	QCOMPARE(composer->attachments()->rowCount(), 1);
	QTest::keyClick(window, Qt::Key_Z, Qt::ControlModifier);
	QCOMPARE(composer->text(), QString());
	QCOMPARE(composer->attachments()->rowCount(), 0);
	triggerRedo();
	QCOMPARE(composer->text(), u"[Image 1] "_s);
	QCOMPARE(textArea->property("text").toString(), u"[Image 1] "_s);
	QCOMPARE(composer->attachments()->rowCount(), 1);
	triggerRedo();
	QCOMPARE(composer->text(), u"[Image 1] word"_s);
	QCOMPARE(textArea->property("text").toString(), u"[Image 1] word"_s);
	QCOMPARE(composer->attachments()->rowCount(), 1);
	QTest::keyClick(window, Qt::Key_Z, Qt::ControlModifier);
	QCOMPARE(composer->text(), u"[Image 1] "_s);

	// 2) Select-all + Delete / Cut -> Ctrl+Z restores both "[Image 1] " text and attachment (next attach is [Image 2]).
	QTest::keyClick(window, Qt::Key_A, Qt::ControlModifier);
	QTest::keyClick(window, Qt::Key_Delete);
	QCOMPARE(composer->text(), QString());
	QCOMPARE(composer->attachments()->rowCount(), 0);
	QTest::keyClick(window, Qt::Key_Z, Qt::ControlModifier);
	QCOMPARE(composer->text(), u"[Image 1] "_s);
	QCOMPARE(textArea->property("text").toString(), u"[Image 1] "_s);
	QCOMPARE(composer->attachments()->rowCount(), 1);

	QTest::keyClick(window, Qt::Key_A, Qt::ControlModifier);
	QTest::keyClick(window, Qt::Key_X, Qt::ControlModifier);
	QCOMPARE(composer->text(), QString());
	QCOMPARE(composer->attachments()->rowCount(), 0);
	QTest::keyClick(window, Qt::Key_Z, Qt::ControlModifier);
	QCOMPARE(composer->text(), u"[Image 1] "_s);
	QCOMPARE(textArea->property("text").toString(), u"[Image 1] "_s);
	QCOMPARE(composer->attachments()->rowCount(), 1);
	QTest::keyClick(window, Qt::Key_End);
	QGuiApplication::clipboard()->setImage(img);
	QVERIFY(composer->pasteFromClipboard());
	QCOMPARE(composer->text(), u"[Image 1] [Image 2] "_s);
	QCOMPARE(composer->attachments()->rowCount(), 2);
	QCOMPARE(composer->attachments()->index(0, 0).data(composer::AttachmentModel::TokenRole).toString(), u"[Image 1]"_s);
	QCOMPARE(composer->attachments()->index(1, 0).data(composer::AttachmentModel::TokenRole).toString(), u"[Image 2]"_s);

	// Also verify Ctrl+Shift+Z triggers Redo.
	QTest::keyClick(window, Qt::Key_Z, Qt::ControlModifier);
	QCOMPARE(composer->text(), u"[Image 1] "_s);
	QTest::keyClick(window, Qt::Key_Z, Qt::ControlModifier | Qt::ShiftModifier);
	QCOMPARE(composer->text(), u"[Image 1] [Image 2] "_s);
	QCOMPARE(composer->attachments()->rowCount(), 2);

	// 3) Submit -> history Up recall of "[Image 1] explain" -> paste yields [Image 2] without collision.
	composer->clearDraft();
	composer->edit(u"[Image 1] explain"_s, 17);
	QVERIFY(composer->submit());
	QCOMPARE(composer->text(), QString());
	QTest::keyClick(window, Qt::Key_Up);
	QCOMPARE(composer->text(), u"[Image 1] explain"_s);
	QCOMPARE(textArea->property("text").toString(), u"[Image 1] explain"_s);
	QVERIFY(composer->pasteFromClipboard());
	QCOMPARE(composer->text(), u"[Image 1] explain [Image 2] "_s);
	QCOMPARE(textArea->property("text").toString(), u"[Image 1] explain [Image 2] "_s);
	QCOMPARE(composer->attachments()->rowCount(), 1);
	QCOMPARE(composer->attachments()->index(0, 0).data(composer::AttachmentModel::TokenRole).toString(), u"[Image 2]"_s);

	// 4) QTextDocument built-in undo/redo is disabled, and right-click ContextMenu syncs Undo/Redo/Paste + shellMode.
	composer->clearDraft();
	QVERIFY(!textArea->property("canUndo").toBool());
	QVERIFY(!textArea->property("canRedo").toBool());
	QGuiApplication::clipboard()->setImage(img);
	QVERIFY(composer->pasteFromClipboard());
	QCOMPARE(composer->text(), u"[Image 1] "_s);
	QTest::keyClick(window, Qt::Key_A, Qt::ControlModifier);
	QTest::keyClick(window, Qt::Key_Delete);
	QCOMPARE(composer->text(), QString());
	QCOMPARE(composer->attachments()->rowCount(), 0);
	QVERIFY(!textArea->property("canUndo").toBool());
	QMetaObject::invokeMethod(textArea, "undo");
	QCOMPARE(textArea->property("text").toString(), QString());
	QCOMPARE(composer->text(), QString());

	auto* contextMenu = textArea->findChild<QObject*>(u"composerContextMenu"_s);
	auto* menuUndo = textArea->findChild<QObject*>(u"composerMenuUndo"_s);
	auto* menuRedo = textArea->findChild<QObject*>(u"composerMenuRedo"_s);
	auto* menuPaste = textArea->findChild<QObject*>(u"composerMenuPaste"_s);
	QVERIFY(contextMenu && menuUndo && menuRedo && menuPaste);
	QGuiApplication::clipboard()->setImage(img);
	QMetaObject::invokeMethod(contextMenu, "open");
	QTRY_VERIFY(contextMenu->property("visible").toBool());
	QVERIFY(menuUndo->property("enabled").toBool());
	QVERIFY(!menuRedo->property("enabled").toBool());
	QVERIFY(menuPaste->property("enabled").toBool());
	QMetaObject::invokeMethod(menuUndo, "triggered");
	QMetaObject::invokeMethod(contextMenu, "close");
	QTRY_VERIFY(!contextMenu->property("visible").toBool());
	QTRY_VERIFY(textArea->hasActiveFocus());
	QCOMPARE(composer->text(), u"[Image 1] "_s);
	QCOMPARE(textArea->property("text").toString(), u"[Image 1] "_s);
	QCOMPARE(composer->attachments()->rowCount(), 1);
	QVERIFY(menuRedo->property("enabled").toBool());
	QMetaObject::invokeMethod(menuRedo, "triggered");
	QCOMPARE(composer->text(), QString());
	QCOMPARE(composer->attachments()->rowCount(), 0);
	QMetaObject::invokeMethod(menuUndo, "triggered");
	QCOMPARE(composer->text(), u"[Image 1] "_s);
	QCOMPARE(composer->attachments()->rowCount(), 1);
	QVERIFY(textArea->hasActiveFocus());
	QTest::keyClick(window, Qt::Key_End);
	QMetaObject::invokeMethod(menuPaste, "triggered");
	QCOMPARE(composer->text(), u"[Image 1] [Image 2] "_s);
	QCOMPARE(composer->attachments()->rowCount(), 2);

	// 5) ShellMode + attachment undo/redo round-trip end-to-end in Composer.qml.
	composer->clearDraft();
	QVERIFY(textArea->hasActiveFocus());
	typeText(u"!make"_s);
	QVERIFY(composer->shellMode());
	QVERIFY(composer->submit());
	QVERIFY(!composer->shellMode());
	QVERIFY(composer->pasteFromClipboard());
	QCOMPARE(composer->attachments()->rowCount(), 1);
	QVERIFY(textArea->hasActiveFocus());
	QTest::keyClick(window, Qt::Key_Home);
	QTest::keyClick(window, Qt::Key_Up);
	QVERIFY(composer->shellMode());
	QCOMPARE(composer->text(), u"make"_s);
	QCOMPARE(composer->attachments()->rowCount(), 0);
	QTest::keyClick(window, Qt::Key_Z, Qt::ControlModifier);
	QVERIFY(!composer->shellMode());
	QCOMPARE(composer->text(), u"[Image 1] "_s);
	QCOMPARE(composer->attachments()->rowCount(), 1);
	QTest::keyClick(window, Qt::Key_Y, Qt::ControlModifier);
	QVERIFY(composer->shellMode());
	QCOMPARE(composer->text(), u"make"_s);
	QCOMPARE(composer->attachments()->rowCount(), 0);
	QVERIFY2(warningSpy.isEmpty(), "Composer undo/redo and token collision test emitted QML warnings");
}

void TestMainWindow::switchClicksWithHitPaddingInsideConfigFlickableAndResetButtonsClearFields()
{
	MainFixture f;
	f.stub.setConfigRawText(u"{\n"
	                        "  \"enabled_providers\": [\"anthropic\"],\n"
	                        "  \"delay\": { \"bash\": 2 },\n"
	                        "  \"agent\": { \"build\": { \"delay\": { \"bash\": 1 } } },\n"
	                        "  \"provider\": {\n"
	                        "    \"p1\": {\n"
	                        "      \"options\": {\n"
	                        "        \"chunkTimeout\": 12000,\n"
	                        "        \"headers\": { \"X-Test\": \"1\" }\n"
	                        "      },\n"
	                        "      \"models\": {\n"
	                        "        \"m1\": {\n"
	                        "          \"limit\": { \"context\": 64000 }\n"
	                        "        }\n"
	                        "      }\n"
	                        "    }\n"
	                        "  }\n"
	                        "}\n"_s);

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
	QTRY_VERIFY(window->isActive());

	const auto findByName = [](QQuickItem* root, const QString& name) {
		return findItem(root, [&name](QQuickItem* item) { return item->objectName() == name; });
	};
	const auto findFormRowReset = [](QQuickItem* control) -> QQuickItem* {
		QQuickItem* slot = control ? control->parentItem() : nullptr;
		QQuickItem* formRow = slot ? slot->parentItem() : nullptr;
		if(!formRow)
		{
			return nullptr;
		}
		for(QQuickItem* ch : formRow->childItems())
		{
			if(QByteArray(ch->metaObject()->className()).startsWith("TextButton_QMLTYPE"))
			{
				return ch;
			}
		}
		return nullptr;
	};
	const auto scrollIntoView = [window](QQuickItem* scroller, QQuickItem* target) {
		QQuickItem* contentItem = scroller->property("contentItem").value<QQuickItem*>();
		QVERIFY(contentItem);
		QTRY_VERIFY(scroller->property("contentHeight").toReal() + 1.0
		            >= target->mapToItem(contentItem, QPointF(0.0, 0.0)).y() + target->height());
		const QPointF inContent = target->mapToItem(contentItem, QPointF(0.0, 0.0));
		const qreal viewH = scroller->height();
		const qreal maxY = std::max(0.0, scroller->property("contentHeight").toReal() - viewH);
		scroller->setProperty("contentY", std::clamp(inContent.y() - viewH / 2.0 + target->height() / 2.0, 0.0, maxY));
		QCoreApplication::processEvents();
		const QPointF targetCenter = target->mapToScene(QPointF(target->width() / 2.0, target->height() / 2.0));
		const qreal scrollerTop = scroller->mapToScene(QPointF(0.0, 0.0)).y();
		const qreal scrollerBottom = scrollerTop + viewH;
		QVERIFY(targetCenter.y() > scrollerTop && targetCenter.y() < scrollerBottom);
		QVERIFY(targetCenter.y() >= 0.0 && targetCenter.y() < window->height());
	};

	f.workspace->commands()->run(u"config"_s);
	QQuickItem* configDialog = nullptr;
	QTRY_VERIFY([&] {
		configDialog = findByName(window->contentItem(), u"configDialog"_s);
		return configDialog != nullptr && !f.workspace->configEditor()->isLoading();
	}());
	QQuickItem* panel = findByName(configDialog, u"configDialogPanel"_s);
	QVERIFY(panel);
	QTRY_VERIFY(panel->property("shown").toBool() && qFuzzyCompare(panel->scale(), 1.0));
	QQuickItem* scroller = findByTypePrefix(configDialog, "QQuickFlickable");
	QVERIFY(scroller);
	const auto waitFormRow = [scroller](QQuickItem* control) {
		QQuickItem* contentItem = scroller->property("contentItem").value<QQuickItem*>();
		QVERIFY(contentItem);
		QQuickItem* slot = control ? control->parentItem() : nullptr;
		QQuickItem* formRow = slot ? slot->parentItem() : nullptr;
		QVERIFY(formRow);
		QTRY_VERIFY(formRow->isVisible() && formRow->y() > 0.0
		            && scroller->property("contentHeight").toReal()
		                   >= formRow->mapToItem(contentItem, QPointF(0.0, 0.0)).y() + formRow->height());
	};

	// 1) Switch inside ConfigDialog Flickable responds to normal click and slight-drift click in hitArea padding.
	auto* autoSwitch = findByName(configDialog, u"compactionAutoSwitch"_s);
	QVERIFY(autoSwitch);
	scrollIntoView(scroller, autoSwitch);
	QVERIFY(autoSwitch->property("checked").toBool());
	const QPoint swCenter = autoSwitch->mapToScene(QPointF(autoSwitch->width() / 2.0, autoSwitch->height() / 2.0)).toPoint();
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, swCenter);
	QTRY_VERIFY(!autoSwitch->property("checked").toBool());

	// Click inside top hit padding (y = -4) with a 2px pointer drift inside Flickable.
	const QPoint swPad = autoSwitch->mapToScene(QPointF(autoSwitch->width() / 2.0, -4.0)).toPoint();
	QTest::mousePress(window, Qt::LeftButton, Qt::NoModifier, swPad);
	QTest::mouseMove(window, swPad + QPoint(2, 2));
	QTest::mouseRelease(window, Qt::LeftButton, Qt::NoModifier, swPad + QPoint(2, 2));
	QTRY_VERIFY(autoSwitch->property("checked").toBool());

	// 2) Reset buttons clear displayed values/entries and reset state cleanly.
	auto* enabledProvEditor = findByName(configDialog, u"enabledProvidersEditor"_s);
	QVERIFY(enabledProvEditor);
	QCOMPARE(enabledProvEditor->property("values").toStringList(), (QStringList{u"anthropic"_s}));
	QQuickItem* enabledProvReset = findFormRowReset(enabledProvEditor);
	QVERIFY(enabledProvReset);
	scrollIntoView(scroller, enabledProvReset);
	QTRY_VERIFY(enabledProvReset->isVisible());
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier,
	                  enabledProvReset->mapToScene(QPointF(enabledProvReset->width() / 2.0, enabledProvReset->height() / 2.0)).toPoint());
	QTRY_VERIFY(enabledProvEditor->property("values").toStringList().isEmpty());
	QTRY_VERIFY(!enabledProvReset->isVisible());
	QVERIFY(!f.workspace->configEditor()->contains({u"enabled_providers"_s}));

	QMetaObject::invokeMethod(configDialog, "selectCategory", Q_ARG(QVariant, u"providers"_s));
	QCoreApplication::processEvents();
	auto* provSection = findByName(configDialog, u"configProvidersSection"_s);
	QVERIFY(provSection);
	QMetaObject::invokeMethod(provSection, "setProviderExpanded", Q_ARG(QVariant, u"p1"_s), Q_ARG(QVariant, true));
	QMetaObject::invokeMethod(provSection, "setModelExpanded", Q_ARG(QVariant, u"p1"_s), Q_ARG(QVariant, u"m1"_s), Q_ARG(QVariant, true));
	QCoreApplication::processEvents();

	auto* ctxField = findByName(provSection, u"modelContextField_p1_m1"_s);
	auto* chunkField = findByName(provSection, u"providerChunkTimeout_p1"_s);
	auto* headersEd = findByName(provSection, u"providerHeadersEditor_p1"_s);
	QVERIFY(ctxField && chunkField && headersEd);
	waitFormRow(ctxField);
	QCOMPARE(ctxField->property("value").toInt(), 64000);
	QCOMPARE(chunkField->property("value").toInt(), 12000);
	QCOMPARE(headersEd->property("entries").toList().size(), 1);

	QQuickItem* ctxReset = findFormRowReset(ctxField);
	QVERIFY(ctxReset);
	scrollIntoView(scroller, ctxReset);
	QTRY_VERIFY(ctxReset->isVisible());
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier,
	                  ctxReset->mapToScene(QPointF(ctxReset->width() / 2.0, ctxReset->height() / 2.0)).toPoint());
	QTRY_VERIFY(!ctxField->property("hasValue").toBool());
	QCOMPARE(ctxField->property("inputText").toString(), QString());
	QTRY_VERIFY(!ctxReset->isVisible());

	QQuickItem* chunkReset = findFormRowReset(chunkField);
	QVERIFY(chunkReset);
	scrollIntoView(scroller, chunkReset);
	QTRY_VERIFY(chunkReset->isVisible());
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier,
	                  chunkReset->mapToScene(QPointF(chunkReset->width() / 2.0, chunkReset->height() / 2.0)).toPoint());
	QTRY_VERIFY(!chunkField->property("hasValue").toBool());
	QCOMPARE(chunkField->property("inputText").toString(), QString());
	QTRY_VERIFY(!chunkReset->isVisible());

	QQuickItem* headersReset = findFormRowReset(headersEd);
	QVERIFY(headersReset);
	scrollIntoView(scroller, headersReset);
	QTRY_VERIFY(headersReset->isVisible());
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier,
	                  headersReset->mapToScene(QPointF(headersReset->width() / 2.0, headersReset->height() / 2.0)).toPoint());
	QTRY_VERIFY(headersEd->property("entries").toList().isEmpty());
	QTRY_VERIFY(!headersReset->isVisible());

	// 3) Clearing a 1-entry delay map key without blurring then clicking Reset resets delayMode cleanly,
	//    and switching delay modes clears stale invalid syntax on inactive sub-editors.
	QMetaObject::invokeMethod(configDialog, "selectCategory", Q_ARG(QVariant, u"permissions"_s));
	QCoreApplication::processEvents();
	auto* permSection = findByName(configDialog, u"configPermissionsSection"_s);
	QVERIFY(permSection);
	auto* delayMap = findByName(permSection, u"globalDelayMapEditor"_s);
	auto* globalDelayField = findByName(permSection, u"globalDelayField"_s);
	QVERIFY(delayMap && globalDelayField);
	QCOMPARE(permSection->property("delayMode").toString(), u"map"_s);
	QCOMPARE(delayMap->property("entries").toList().size(), 1);

	QQuickItem* delayRowKeyInput = findItem(delayMap, [](QQuickItem* item) {
		return QByteArray(item->metaObject()->className()).startsWith("QQuickTextInput")
		       && item->property("text").toString() == u"bash"_s;
	});
	QVERIFY(delayRowKeyInput);
	scrollIntoView(scroller, delayRowKeyInput);
	delayRowKeyInput->forceActiveFocus();
	QTRY_VERIFY(delayRowKeyInput->hasActiveFocus());
	delayRowKeyInput->setProperty("text", QString());
	QMetaObject::invokeMethod(delayRowKeyInput, "textEdited");
	QCoreApplication::processEvents();

	QQuickItem* delayReset = findFormRowReset(delayMap->parentItem());
	QVERIFY(delayReset);
	scrollIntoView(scroller, delayReset);
	QTRY_VERIFY(delayReset->isVisible());
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier,
	                  delayReset->mapToScene(QPointF(delayReset->width() / 2.0, delayReset->height() / 2.0)).toPoint());
	QTRY_COMPARE(permSection->property("delayMode").toString(), u"none"_s);
	QVERIFY(!f.workspace->configEditor()->contains({u"delay"_s}));
	QVERIFY(delayMap->property("entries").toList().isEmpty());
	QTRY_VERIFY(!delayReset->isVisible());

	QQuickItem* delaySegmented = findByTypePrefix(delayMap->parentItem(), "Segmented_QMLTYPE");
	QVERIFY(delaySegmented);
	scrollIntoView(scroller, delaySegmented);
	const auto clickSegment = [&](QQuickItem* segmented, double fractionX) {
		const QPoint pt = segmented->mapToScene(QPointF(segmented->width() * fractionX, segmented->height() / 2.0)).toPoint();
		QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, pt);
	};
	clickSegment(delaySegmented, 0.5); // "All tools" ("number")
	QTRY_COMPARE(permSection->property("delayMode").toString(), u"number"_s);

	QQuickItem* globalDelayInput = findByTypePrefix(globalDelayField, "QQuickTextInput");
	QVERIFY(globalDelayInput);
	waitFormRow(globalDelayField);
	scrollIntoView(scroller, globalDelayField);
	globalDelayInput->forceActiveFocus();
	QTRY_VERIFY(globalDelayInput->hasActiveFocus());
	globalDelayInput->setProperty("text", u"2abc"_s);
	QMetaObject::invokeMethod(globalDelayInput, "textEdited");
	QTRY_VERIFY(globalDelayField->property("hasValidationError").toBool());
	QVERIFY(permSection->property("hasValidationError").toBool());

	clickSegment(delaySegmented, 1.0 / 6.0); // "No delay" ("none")
	QTRY_COMPARE(permSection->property("delayMode").toString(), u"none"_s);
	QVERIFY(!globalDelayField->property("hasValidationError").toBool());
	QVERIFY(!permSection->property("hasValidationError").toBool());

	clickSegment(delaySegmented, 0.5); // back to "All tools" ("number")
	QTRY_COMPARE(permSection->property("delayMode").toString(), u"number"_s);
	QVERIFY(!globalDelayField->property("hasValidationError").toBool());
	QVERIFY(!permSection->property("hasValidationError").toBool());
	QCOMPARE(globalDelayField->property("inputText").toString(), u"2"_s);

	QMetaObject::invokeMethod(configDialog, "selectCategory", Q_ARG(QVariant, u"agents"_s));
	QCoreApplication::processEvents();
	auto* agentsSection = findByName(configDialog, u"configAgentsSection"_s);
	QVERIFY(agentsSection);
	QMetaObject::invokeMethod(agentsSection, "setExpanded", Q_ARG(QVariant, u"builtin:build"_s), Q_ARG(QVariant, true));
	QCoreApplication::processEvents();
	auto* agentDelayMap = findByName(agentsSection, u"agentDelayMap_build"_s);
	QVERIFY(agentDelayMap);
	waitFormRow(agentDelayMap->parentItem());
	QCOMPARE(agentDelayMap->property("entries").toList().size(), 1);
	QQuickItem* agentDelayKeyInput = findItem(agentDelayMap, [](QQuickItem* item) {
		return QByteArray(item->metaObject()->className()).startsWith("QQuickTextInput")
		       && item->property("text").toString() == u"bash"_s;
	});
	QVERIFY(agentDelayKeyInput);
	scrollIntoView(scroller, agentDelayKeyInput);
	agentDelayKeyInput->forceActiveFocus();
	QTRY_VERIFY(agentDelayKeyInput->hasActiveFocus());
	agentDelayKeyInput->setProperty("text", QString());
	QMetaObject::invokeMethod(agentDelayKeyInput, "textEdited");
	QCoreApplication::processEvents();
	QQuickItem* agentDelayReset = findFormRowReset(agentDelayMap->parentItem());
	QVERIFY(agentDelayReset);
	scrollIntoView(scroller, agentDelayReset);
	QTRY_VERIFY(agentDelayReset->isVisible());
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier,
	                  agentDelayReset->mapToScene(QPointF(agentDelayReset->width() / 2.0, agentDelayReset->height() / 2.0)).toPoint());
	QTRY_VERIFY(agentDelayMap->property("entries").toList().isEmpty());
	QVERIFY(!f.workspace->configEditor()->contains({u"agent"_s, u"build"_s, u"delay"_s}));
	QTRY_VERIFY(!agentDelayReset->isVisible());
	QVERIFY2(warningSpy.isEmpty(), "Switch hit-padding or config reset test emitted QML warnings");
}

void TestMainWindow::sidebarRunningRowsReorderByLongPressDragAndPersistAcrossFrames()
{
	MainFixture f;
	f.stub.add(u"ses_c"_s, 3);
	f.fake.send({{u"kind"_s, u"sessions"_s},
	             {u"sessions"_s, QJsonArray{sessionInfoJson(u"ses_a"_s, {.title = u"Session ses_a"_s, .createdAt = 1}),
	                                      sessionInfoJson(u"ses_b"_s, {.title = u"Session ses_b"_s, .createdAt = 2}),
	                                      sessionInfoJson(u"ses_c"_s, {.title = u"Session ses_c"_s, .createdAt = 3})}}});
	QTRY_COMPARE(f.workspace->sessions()->count(), 3);

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
	QTRY_VERIFY(window->isActive());

	auto* runningList = findItem(window->contentItem(), [](QQuickItem* item) { return item->objectName() == u"runningList"_s; });
	QVERIFY(runningList);
	const auto findRow = [&](const QString& sessionId) -> QQuickItem* {
		return findItem(runningList, [&sessionId](QQuickItem* item) {
			return QByteArray(item->metaObject()->className()).startsWith("SessionRow_QMLTYPE")
			       && item->property("sessionId").toString() == sessionId;
		});
	};
	const auto sessionOrder = [&]() {
		QStringList out;
		const auto* model = f.workspace->sessions();
		for(int row = 0; row < model->rowCount(); ++row)
		{
			out.append(model->index(row).data(refgui::workspace::SessionListModel::SessionIdRole).toString());
		}
		return out;
	};
	QCOMPARE(sessionOrder(), (QStringList{u"ses_c"_s, u"ses_b"_s, u"ses_a"_s}));

	QQuickItem* rowA = nullptr;
	QQuickItem* rowB = nullptr;
	QQuickItem* rowC = nullptr;
	QTRY_VERIFY([&] {
		rowA = findRow(u"ses_a"_s);
		rowB = findRow(u"ses_b"_s);
		rowC = findRow(u"ses_c"_s);
		return rowA != nullptr && rowB != nullptr && rowC != nullptr && rowA->y() > rowC->y();
	}());

	// Short click still opens and detaches sessions.
	const QPoint bPos = rowB->mapToScene(QPointF(rowB->width() / 2.0, rowB->height() / 2.0)).toPoint();
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, bPos);
	QTRY_COMPARE(f.workspace->currentId(), u"ses_b"_s);
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, bPos);
	QTRY_VERIFY(f.workspace->currentId().isEmpty());

	// Long-press (~320ms) + drag moves ses_c down to the bottom of Running.
	const QPoint startPt = rowC->mapToScene(QPointF(rowC->width() / 2.0, rowC->height() / 2.0)).toPoint();
	const QPoint endPt = rowA->mapToScene(QPointF(rowA->width() / 2.0, rowA->height() / 2.0)).toPoint();
	QTest::mousePress(window, Qt::LeftButton, Qt::NoModifier, startPt);
	QTest::qWait(320);
	QTRY_VERIFY(rowC->property("dragging").toBool());
	QTest::mouseMove(window, QPoint(startPt.x(), (startPt.y() + endPt.y()) / 2));
	QTest::mouseMove(window, endPt);
	QTest::mouseRelease(window, Qt::LeftButton, Qt::NoModifier, endPt);
	QTRY_VERIFY(!rowC->property("dragging").toBool());
	QCOMPARE(sessionOrder(), (QStringList{u"ses_b"_s, u"ses_a"_s, u"ses_c"_s}));
	QCOMPARE(f.workspace->settings()->sidebarRunningOrder(), (QStringList{u"ses_b"_s, u"ses_a"_s, u"ses_c"_s}));
	QVERIFY(f.workspace->currentId().isEmpty()); // dragging did not trigger click open

	// New backend `sessions` frame preserves custom order and puts newly created ses_d at the top.
	f.stub.add(u"ses_d"_s, 4);
	f.fake.send({{u"kind"_s, u"sessions"_s},
	             {u"sessions"_s, QJsonArray{sessionInfoJson(u"ses_a"_s, {.title = u"Session ses_a"_s, .createdAt = 1}),
	                                      sessionInfoJson(u"ses_b"_s, {.title = u"Session ses_b"_s, .createdAt = 2}),
	                                      sessionInfoJson(u"ses_c"_s, {.title = u"Session ses_c"_s, .createdAt = 3}),
	                                      sessionInfoJson(u"ses_d"_s, {.title = u"Session ses_d"_s, .createdAt = 4})}}});
	QTRY_COMPARE(f.workspace->sessions()->count(), 4);
	QCOMPARE(sessionOrder(), (QStringList{u"ses_d"_s, u"ses_b"_s, u"ses_a"_s, u"ses_c"_s}));
	QVERIFY2(warningSpy.isEmpty(), "Sidebar running reorder test emitted QML warnings");
}

void TestMainWindow::uiZoomSettingsShortcutsResizeAndPopupsScaleCleanly()
{
	MainFixture f;
	QJsonArray models;
	for(int i = 0; i < 12; ++i)
	{
		models.append(QJsonObject{{u"provider"_s, u"p"_s},
		                          {u"provider_name"_s, u"Provider"_s},
		                          {u"id"_s, u"m%1"_s.arg(i)},
		                          {u"name"_s, u"Model %1"_s.arg(i)}});
	}
	f.stub.setModels(models);
	f.workspace->reloadConfig();
	QTRY_COMPARE(f.workspace->config()->models().size(), 12);

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
	QTRY_VERIFY(window->isActive());

	const auto findByName = [](QQuickItem* root, const QString& name) {
		return findItem(root, [&name](QQuickItem* item) { return item->objectName() == name; });
	};
	auto* scaledRoot = findByName(window->contentItem(), u"scaledRoot"_s);
	auto* handle = findByName(window->contentItem(), u"sidebarResizeHandle"_s);
	QVERIFY(scaledRoot && handle);
	QVERIFY(qFuzzyCompare(scaledRoot->scale(), 1.0));

	// 1) SettingsDialog zoom buttons (5% step) and editable uiScaleInput update uiScale, scaledRoot, and Icon sourceSize.
	f.workspace->commands()->run(u"settings"_s);
	QQuickItem* settingsDialog = nullptr;
	QTRY_VERIFY([&] {
		settingsDialog = findByTypePrefix(window->contentItem(), "SettingsDialog_QMLTYPE");
		return settingsDialog != nullptr && settingsDialog->isVisible();
	}());
	auto* panel = findByTypePrefix(settingsDialog, "QQuickRectangle");
	QVERIFY(panel);
	QTRY_VERIFY(qFuzzyCompare(panel->scale(), 1.0) && qFuzzyCompare(panel->property("dropOffset").toReal() + 1.0, 1.0));

	auto* outBtn = findByName(settingsDialog, u"uiScaleOutButton"_s);
	auto* inBtn = findByName(settingsDialog, u"uiScaleInButton"_s);
	auto* resetBtn = findByName(settingsDialog, u"uiScaleResetButton"_s);
	auto* scaleInput = findByName(settingsDialog, u"uiScaleInput"_s);
	QVERIFY(outBtn && inBtn && resetBtn && scaleInput);
	auto scalePercentText = [scaleInput]() {
		QString text = scaleInput->property("text").toString().trimmed();
		text.remove(u'%');
		return text;
	};
	QCOMPARE(scalePercentText(), u"100"_s);
	QVERIFY(!resetBtn->isVisible());

	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier,
	                  inBtn->mapToScene(QPointF(inBtn->width() / 2.0, inBtn->height() / 2.0)).toPoint());
	QTRY_VERIFY(qFuzzyCompare(f.workspace->settings()->uiScale(), 1.05));
	QCOMPARE(scalePercentText(), u"105"_s);
	QVERIFY(resetBtn->isVisible());
	QVERIFY(qFuzzyCompare(scaledRoot->scale(), 1.05));
	QVERIFY(qFuzzyCompare(scaledRoot->width(), window->width() / 1.05));
	QPointF prevResetCenter;
	bool hasPrevResetCenter = false;
	QTRY_VERIFY([&] {
		if(!resetBtn->isVisible() || resetBtn->width() <= 0.0 || resetBtn->height() <= 0.0)
		{
			hasPrevResetCenter = false;
			return false;
		}
		if(outBtn->x() < resetBtn->width() + 7.0)
		{
			hasPrevResetCenter = false;
			return false;
		}
		const QPointF center = resetBtn->mapToScene(QPointF(resetBtn->width() / 2.0, resetBtn->height() / 2.0));
		if(center.x() < 0.0 || center.x() >= window->width() || center.y() < 0.0 || center.y() >= window->height())
		{
			hasPrevResetCenter = false;
			return false;
		}
		const bool settled = hasPrevResetCenter && center == prevResetCenter;
		prevResetCenter = center;
		hasPrevResetCenter = true;
		return settled;
	}());

	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier,
	                  resetBtn->mapToScene(QPointF(resetBtn->width() / 2.0, resetBtn->height() / 2.0)).toPoint());
	QTRY_VERIFY(qFuzzyCompare(f.workspace->settings()->uiScale(), 1.0));
	QCOMPARE(scalePercentText(), u"100"_s);
	QVERIFY(!resetBtn->isVisible());

	scaleInput->forceActiveFocus();
	QTRY_VERIFY(scaleInput->hasActiveFocus());
	QMetaObject::invokeMethod(scaleInput, "selectAll");
	typeAsciiText(window, u"135"_s);
	QTest::keyClick(window, Qt::Key_Return);
	QTRY_VERIFY(qFuzzyCompare(f.workspace->settings()->uiScale(), 1.35));
	QTRY_VERIFY(qFuzzyCompare(scaledRoot->scale(), 1.35));

	scaleInput->forceActiveFocus();
	QTRY_VERIFY(scaleInput->hasActiveFocus());
	QMetaObject::invokeMethod(scaleInput, "selectAll");
	typeAsciiText(window, u"250"_s);
	QTest::keyClick(window, Qt::Key_Return);
	QTRY_VERIFY(qFuzzyCompare(f.workspace->settings()->uiScale(), 2.5));
	QTRY_VERIFY(qFuzzyCompare(scaledRoot->scale(), 2.5));

	auto* sidebar = findByTypePrefix(window->contentItem(), "Sidebar_QMLTYPE");
	QVERIFY(sidebar);
	auto* sidebarIcon = findByTypePrefix(sidebar, "Icon_QMLTYPE");
	QVERIFY(sidebarIcon);
	const int iconSize = sidebarIcon->property("size").toInt();
	QCOMPARE(sidebarIcon->property("sourceSize").toSize(), QSize(qRound(iconSize * 2.5), qRound(iconSize * 2.5)));

	settingsDialog->forceActiveFocus();
	QTest::keyClick(window, Qt::Key_0, Qt::ControlModifier);
	QTRY_VERIFY(qFuzzyCompare(f.workspace->settings()->uiScale(), 1.0));

	// 2) Shortcuts Ctrl+= / Ctrl+- / Ctrl+0 (5% step) work in SettingsDialog, and are suppressed in CommandPalette.
	QTest::keyClick(window, Qt::Key_Equal, Qt::ControlModifier);
	QTRY_VERIFY(qFuzzyCompare(f.workspace->settings()->uiScale(), 1.05));
	QTest::keyClick(window, Qt::Key_Minus, Qt::ControlModifier);
	QTRY_VERIFY(qFuzzyCompare(f.workspace->settings()->uiScale(), 1.0));
	QTest::keyClick(window, Qt::Key_Equal, Qt::ControlModifier);
	QTest::keyClick(window, Qt::Key_Equal, Qt::ControlModifier);
	QTRY_VERIFY(qFuzzyCompare(f.workspace->settings()->uiScale(), 1.1));
	QTest::keyClick(window, Qt::Key_0, Qt::ControlModifier);
	QTRY_VERIFY(qFuzzyCompare(f.workspace->settings()->uiScale(), 1.0));

	f.workspace->openDialog(u"palette"_s);
	QTRY_VERIFY(findByTypePrefix(window->contentItem(), "CommandPalette") != nullptr);
	QTest::keyClick(window, Qt::Key_Equal, Qt::ControlModifier);
	QCoreApplication::processEvents();
	QVERIFY(qFuzzyCompare(f.workspace->settings()->uiScale(), 1.0));
	f.workspace->closeDialog();
	QTRY_VERIFY(findByTypePrefix(window->contentItem(), "CommandPalette") == nullptr);

	// 3) Zoom to 1.2 and verify Sidebar resize scales delta by 1 / UiScale.factor.
	for(int i = 0; i < 4; ++i)
	{
		QTest::keyClick(window, Qt::Key_Equal, Qt::ControlModifier);
	}
	QTRY_VERIFY(qFuzzyCompare(f.workspace->settings()->uiScale(), 1.2));
	QCOMPARE(f.workspace->settings()->sidebarWidth(), 264);
	const QPoint hCenter = handle->mapToScene(QPointF(handle->width() / 2.0, handle->height() / 2.0)).toPoint();
	QTest::mousePress(window, Qt::LeftButton, Qt::NoModifier, hCenter);
	QTest::mouseMove(window, hCenter + QPoint(20, 0));
	QTRY_VERIFY(sidebar->property("resizing").toBool());
	QTest::mouseMove(window, hCenter + QPoint(60, 0));
	QTest::mouseRelease(window, Qt::LeftButton, Qt::NoModifier, hCenter + QPoint(60, 0));
	QTRY_VERIFY(std::abs(f.workspace->settings()->sidebarWidth() - 314) <= 1);

	// 4) SelectField and ContextMenu popups scale cleanly under zoom, and scrolled SelectField hover hits exact row.
	f.workspace->commands()->run(u"config"_s);
	QQuickItem* configDialog = nullptr;
	QTRY_VERIFY([&] {
		configDialog = findByName(window->contentItem(), u"configDialog"_s);
		return configDialog != nullptr && !f.workspace->configEditor()->isLoading();
	}());
	QQuickItem* profileSelect = findByName(configDialog, u"configProfileSelect"_s);
	QVERIFY(profileSelect);
	QTRY_VERIFY(profileSelect->property("enabled").toBool());
	QMetaObject::invokeMethod(profileSelect, "openPopup");
	QObject* selectPopup = nullptr;
	for(QObject* ch : profileSelect->children())
	{
		if(ch->objectName() == u"selectPopup"_s)
		{
			selectPopup = ch;
			break;
		}
	}
	QVERIFY(selectPopup);
	QTRY_VERIFY(selectPopup->property("opened").toBool());
	QQuickItem* selectContent = selectPopup->property("contentItem").value<QQuickItem*>();
	QVERIFY(selectContent && !selectContent->childItems().isEmpty());
	QQuickItem* selectInner = selectContent->childItems().first();
	QVERIFY(qFuzzyCompare(selectInner->scale(), 1.2));
	QVERIFY(qFuzzyCompare(selectPopup->property("width").toReal(), selectInner->width() * 1.2));
	QMetaObject::invokeMethod(selectPopup, "close");
	QTRY_VERIFY(!selectPopup->property("opened").toBool());

	QMetaObject::invokeMethod(configDialog, "selectCategory", Q_ARG(QVariant, u"agents"_s));
	QCoreApplication::processEvents();
	QQuickItem* agentsSection = findByName(configDialog, u"configAgentsSection"_s);
	QVERIFY(agentsSection);
	QMetaObject::invokeMethod(agentsSection, "setExpanded", Q_ARG(QVariant, u"builtin:build"_s), Q_ARG(QVariant, true));
	QCoreApplication::processEvents();
	QQuickItem* modelSelect = findItem(agentsSection, [](QQuickItem* item) {
		return QByteArray(item->metaObject()->className()).startsWith("SelectField_QMLTYPE")
		       && item->property("placeholderText").toString() == u"Inherit top-level model"_s;
	});
	QVERIFY(modelSelect);
	QMetaObject::invokeMethod(modelSelect, "openPopup");
	QObject* modelPopup = nullptr;
	for(QObject* ch : modelSelect->children())
	{
		if(ch->objectName() == u"selectPopup"_s)
		{
			modelPopup = ch;
			break;
		}
	}
	QVERIFY(modelPopup);
	QTRY_VERIFY(modelPopup->property("opened").toBool());
	QQuickItem* modelPopupContent = modelPopup->property("contentItem").value<QQuickItem*>();
	QVERIFY(modelPopupContent);
	QQuickItem* modelListView = findByTypePrefix(modelPopupContent, "QQuickListView");
	QVERIFY(modelListView);
	QTRY_VERIFY(modelListView->property("contentHeight").toReal() > modelListView->height());
	const qreal optRowH = modelSelect->property("optionRowHeight").toReal();
	modelListView->setProperty("contentY", optRowH * 3.0);
	QCoreApplication::processEvents();
	const QPoint row4Pt = modelListView->mapToScene(QPointF(modelListView->width() / 2.0, optRowH * 1.5)).toPoint();
	const QPoint row5Pt = modelListView->mapToScene(QPointF(modelListView->width() / 2.0, optRowH * 2.5)).toPoint();
	QTest::mouseMove(window, row4Pt);
	QTest::mouseMove(window, row5Pt);
	QTRY_COMPARE(modelListView->property("currentIndex").toInt(), 5);
	QMetaObject::invokeMethod(modelPopup, "close");

	f.workspace->closeDialog();
	QTRY_VERIFY(findByName(window->contentItem(), u"configDialog"_s) == nullptr);

	auto* rowA = findItem(window->contentItem(), [](QQuickItem* item) {
		return QByteArray(item->metaObject()->className()).startsWith("SessionRow_QMLTYPE")
		       && item->property("sessionId").toString() == u"ses_a"_s;
	});
	QVERIFY(rowA);
	QObject* menuObj = nullptr;
	for(QObject* ch : rowA->children())
	{
		if(QByteArray(ch->metaObject()->className()).startsWith("ContextMenu_QMLTYPE"))
		{
			menuObj = ch;
			break;
		}
	}
	QVERIFY(menuObj);
	QMetaObject::invokeMethod(menuObj, "openAt", Q_ARG(QPointF, QPointF(12.0, 12.0)));
	QTRY_VERIFY(menuObj->property("opened").toBool());
	QQuickItem* menuContent = menuObj->property("contentItem").value<QQuickItem*>();
	QVERIFY(menuContent);
	QQuickItem* menuInner = findItem(menuContent, [](QQuickItem* item) { return item->objectName() == u"contextMenuInner"_s; });
	QVERIFY(menuInner);
	QVERIFY(qFuzzyCompare(menuInner->scale(), 1.2));
	QVERIFY(qFuzzyCompare(menuObj->property("width").toReal(), menuInner->width() * 1.2));
	QMetaObject::invokeMethod(menuObj, "close");
	QTRY_VERIFY(!menuObj->property("opened").toBool());

	QQuickItem* settingsBtn = findItem(sidebar, [](QQuickItem* item) {
		return item->property("iconName").toString() == u"sliders"_s
		       && item->property("tooltipText").toString() == u"Settings"_s;
	});
	QVERIFY(settingsBtn);
	QObject* settingsTip = nullptr;
	for(QObject* ch : settingsBtn->findChildren<QObject*>())
	{
		if(QByteArray(ch->metaObject()->className()).startsWith("PlainToolTip")
		   && ch->property("text").toString() == u"Settings"_s)
		{
			settingsTip = ch;
			break;
		}
	}
	QVERIFY(settingsTip);
	settingsTip->setProperty("delay", 0);

	const auto verifyToolTipGeometry = [&](double scale) {
		f.workspace->settings()->setUiScale(scale);
		QTRY_VERIFY(qFuzzyCompare(scaledRoot->scale(), scale));
		QCoreApplication::processEvents();

		settingsTip->setProperty("visible", true);
		QTRY_VERIFY(settingsTip->property("opened").toBool());
		QQuickItem* tipContent = settingsTip->property("contentItem").value<QQuickItem*>();
		QVERIFY(tipContent && !tipContent->childItems().isEmpty());
		QQuickItem* tipInner = tipContent->childItems().first();
		QTRY_VERIFY(qFuzzyCompare(tipInner->scale(), scale));
		const qreal tipW = settingsTip->property("width").toReal();
		const qreal tipH = settingsTip->property("height").toReal();
		QVERIFY(qFuzzyCompare(tipW, tipInner->width() * scale));
		QVERIFY(qFuzzyCompare(tipH, tipInner->height() * scale));

		const QPointF tipTopLeft = tipContent->mapToScene(QPointF(0.0, 0.0));
		const QPointF btnTopLeft = settingsBtn->mapToScene(QPointF(0.0, 0.0));
		const QPointF btnBottomRight = settingsBtn->mapToScene(QPointF(settingsBtn->width(), settingsBtn->height()));
		const qreal tipCenterX = tipTopLeft.x() + tipW / 2.0;
		const qreal btnCenterX = (btnTopLeft.x() + btnBottomRight.x()) / 2.0;
		const qreal tipBottomY = tipTopLeft.y() + tipH;
		QVERIFY2(std::abs(tipCenterX - btnCenterX) <= 1.5,
		         qPrintable(u"tipCenterX=%1 btnCenterX=%2 scale=%3"_s.arg(tipCenterX).arg(btnCenterX).arg(scale)));
		QVERIFY2(tipBottomY < btnTopLeft.y(),
		         qPrintable(u"tipBottomY=%1 btnTopLeft.y=%2 scale=%3"_s.arg(tipBottomY).arg(btnTopLeft.y()).arg(scale)));
		QVERIFY2(std::abs((btnTopLeft.y() - tipBottomY) - 3.0 * scale) <= 1.5,
		         qPrintable(u"gap=%1 expected=%2 scale=%3"_s.arg(btnTopLeft.y() - tipBottomY).arg(3.0 * scale).arg(scale)));

		settingsTip->setProperty("visible", false);
		QTRY_VERIFY(!settingsTip->property("opened").toBool());
	};
	verifyToolTipGeometry(1.5);
	verifyToolTipGeometry(0.7);
	QVERIFY2(warningSpy.isEmpty(), "UI zoom test emitted QML warnings");
}

void TestMainWindow::sidebarVerticalSplittersResizePersistAndReset()
{
	MainFixture f;
	const QJsonArray todos{
		QJsonObject{{u"content"_s, u"First todo"_s}, {u"status"_s, u"completed"_s}},
		QJsonObject{{u"content"_s, u"Second todo"_s}, {u"status"_s, u"in_progress"_s}},
	};
	f.stub.setTodos(u"ses_a"_s, todos);
	QJsonArray sessions;
	QJsonObject sesA = sessionInfoJson(u"ses_a"_s, {.title = u"Session ses_a"_s, .createdAt = 1});
	sesA[u"todos"_s] = todos;
	sessions.append(sesA);
	sessions.append(sessionInfoJson(u"ses_b"_s, {.title = u"Session ses_b"_s, .createdAt = 2}));
	for(int i = 0; i < 10; ++i)
	{
		const QString id = u"ses_extra_%1"_s.arg(i);
		f.stub.add(id, 3 + i);
		sessions.append(sessionInfoJson(id, {.title = u"Session "_s + id, .createdAt = 3 + i}));
	}
	f.fake.send({{u"kind"_s, u"sessions"_s}, {u"sessions"_s, sessions}});
	f.fake.send({{u"kind"_s, u"event"_s}, {u"session_id"_s, u"ses_a"_s}, {u"event"_s, QJsonObject{{u"type"_s, u"info"_s}, {u"info"_s, sesA}}}});
	QTRY_COMPARE(f.workspace->current()->todoTotal(), 2);

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
	QTRY_VERIFY(window->isActive());

	const auto findByName = [](QQuickItem* root, const QString& name) {
		return findItem(root, [&name](QQuickItem* item) { return item->objectName() == name; });
	};
	auto* sidebar = findByTypePrefix(window->contentItem(), "Sidebar_QMLTYPE");
	auto* todoSection = findByName(sidebar, u"todoSection"_s);
	auto* runningList = findByName(sidebar, u"runningList"_s);
	auto* todoHandle = findByName(sidebar, u"todoResizeHandle"_s);
	auto* runningHandle = findByName(sidebar, u"runningResizeHandle"_s);
	QVERIFY(sidebar && todoSection && runningList && todoHandle && runningHandle);
	QTRY_VERIFY(todoHandle->isVisible() && runningHandle->isVisible());
	QTRY_VERIFY(todoSection->height() > 0.0 && qFuzzyCompare(todoSection->height(), sidebar->property("todoHeight").toReal()));
	QTRY_VERIFY(runningList->height() > 0.0 && qFuzzyCompare(runningList->height(), sidebar->property("runningTargetHeight").toReal()));

	const qreal initialTodoH = todoSection->height();
	const QPoint todoPt = todoHandle->mapToScene(QPointF(todoHandle->width() / 2.0, todoHandle->height() / 2.0)).toPoint();
	QTest::mouseMove(window, todoPt);
	QTest::mousePress(window, Qt::LeftButton, Qt::NoModifier, todoPt);
	QTest::mouseMove(window, todoPt + QPoint(0, 15));
	QTest::mouseMove(window, todoPt + QPoint(0, 40));
	QTest::mouseRelease(window, Qt::LeftButton, Qt::NoModifier, todoPt + QPoint(0, 40));
	QTRY_VERIFY(f.workspace->settings()->sidebarTodoHeight() > 0);
	QVERIFY2(std::abs(f.workspace->settings()->sidebarTodoHeight() - qRound(initialTodoH + 40.0)) <= 2,
	         qPrintable(u"todoHeight=%1 initial=%2"_s.arg(f.workspace->settings()->sidebarTodoHeight()).arg(initialTodoH)));
	QTRY_COMPARE(qRound(todoSection->height()), f.workspace->settings()->sidebarTodoHeight());
	QTRY_VERIFY(qFuzzyCompare(runningList->height(), sidebar->property("runningTargetHeight").toReal()));

	const qreal initialRunningH = runningList->height();
	const QPoint runPt = runningHandle->mapToScene(QPointF(runningHandle->width() / 2.0, runningHandle->height() / 2.0)).toPoint();
	QTest::mouseMove(window, runPt);
	QTest::mousePress(window, Qt::LeftButton, Qt::NoModifier, runPt);
	QTest::mouseMove(window, runPt + QPoint(0, -15));
	QTest::mouseMove(window, runPt + QPoint(0, -40));
	QTest::mouseRelease(window, Qt::LeftButton, Qt::NoModifier, runPt + QPoint(0, -40));
	QTRY_VERIFY(f.workspace->settings()->sidebarRunningHeight() > 0);
	QVERIFY2(std::abs(f.workspace->settings()->sidebarRunningHeight() - qRound(initialRunningH - 40.0)) <= 2,
	         qPrintable(u"runningHeight=%1 initial=%2"_s.arg(f.workspace->settings()->sidebarRunningHeight()).arg(initialRunningH)));
	QTRY_COMPARE(qRound(runningList->height()), f.workspace->settings()->sidebarRunningHeight());
	// Double-click resets each section height to 0 (auto).
	const QPoint todoNewPt = todoHandle->mapToScene(QPointF(todoHandle->width() / 2.0, todoHandle->height() / 2.0)).toPoint();
	QTest::mouseDClick(window, Qt::LeftButton, Qt::NoModifier, todoNewPt);
	QTRY_COMPARE(f.workspace->settings()->sidebarTodoHeight(), 0);

	const QPoint runNewPt = runningHandle->mapToScene(QPointF(runningHandle->width() / 2.0, runningHandle->height() / 2.0)).toPoint();
	QTest::mouseDClick(window, Qt::LeftButton, Qt::NoModifier, runNewPt);
	QTRY_COMPARE(f.workspace->settings()->sidebarRunningHeight(), 0);
	QVERIFY2(warningSpy.isEmpty(), "Sidebar vertical splitter resize emitted QML warnings");
}

void TestMainWindow::tileEnlargeKeepsResizingLayoutUntilWidthAnimationSettles()
{
	MainFixture f;
	for(int i = 0; i < 3; ++i)
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
	window->resize(1200, 720);
	QVERIFY(QTest::qWaitForWindowExposed(window));

	f.workspace->toggleSplit();
	QTRY_VERIFY(f.workspace->isSplitView());
	QQuickItem* tileGrid = findByTypePrefix(window->contentItem(), "TileGrid");
	QVERIFY(tileGrid);
	QQuickItem* tileB = nullptr;
	QTRY_VERIFY([&] {
		QList<QQuickItem*> tiles;
		collectTiles(tileGrid, tiles);
		for(QQuickItem* candidate : tiles)
		{
			if(candidate->property("sessionId").toString() == u"ses_b"_s && candidate->property("arrived").toBool())
			{
				tileB = candidate;
			}
		}
		return tileB != nullptr;
	}());
	QQuickItem* pane = findByTypePrefix(tileB, "SessionPane");
	QVERIFY(pane);
	QVERIFY(tileGrid->width() > pane->width() + 100.0);

	// Zooming in enters resizingLayout and keeps it active past resizeSettleMs (130ms) until widthAnim (480ms) ends.
	f.workspace->zoomTile(u"ses_b"_s);
	QTRY_VERIFY(tileB->property("resizingLayout").toBool());
	QTest::qWait(180);
	QVERIFY(tileB->property("resizingLayout").toBool());
	QVERIFY(qFuzzyCompare(pane->property("opacity").toReal(), 1.0));

	QTRY_VERIFY_WITH_TIMEOUT(!tileB->property("resizingLayout").toBool(), 2000);
	QVERIFY(qFuzzyCompare(tileB->property("settledLayoutWidth").toReal(), tileB->property("paneTargetWidth").toReal()));
	QTRY_VERIFY(qFuzzyCompare(pane->property("opacity").toReal(), 1.0));
	QVERIFY2(warningSpy.isEmpty(), "Tile enlarge transition emitted QML warnings");
}

void TestMainWindow::composerAndUserBlockHighlightImageTokensAndAgentVariantSelects()
{
	MainFixture f;
	f.stub.setMessages(u"ses_b"_s, QJsonArray{
		QJsonObject{{u"id"_s, u"u_img"_s},
		            {u"role"_s, u"user"_s},
		            {u"time"_s, 1},
		            {u"parts"_s, QJsonArray{QJsonObject{{u"type"_s, u"text"_s}, {u"text"_s, u"[Image 1] inspect this"_s}, {u"time"_s, 1}}}}},
	});
	f.workspace->openSession(u"ses_b"_s);
	QTRY_COMPARE(f.workspace->currentId(), u"ses_b"_s);
	f.stub.setModels(QJsonArray{
		QJsonObject{{u"provider"_s, u"p"_s},
		            {u"provider_name"_s, u"Provider"_s},
		            {u"id"_s, u"m1"_s},
		            {u"name"_s, u"Model One"_s},
		            {u"variants"_s, QJsonArray{u"low"_s, u"medium"_s, u"high"_s}}},
	});
	f.stub.setConfigRawText(u"{\n  \"model\": \"p/m1\",\n  \"agent\": { \"build\": { \"model\": \"p/m1\" } }\n}\n"_s);
	f.workspace->reloadConfig();
	QTRY_COMPARE(f.workspace->config()->models().size(), 1);

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
	QTRY_VERIFY(window->isActive());

	const auto hasTokenForegroundNoBg = [](const QTextDocument* doc, int pos) {
		if(!doc)
		{
			return false;
		}
		const QTextBlock block = doc->firstBlock();
		for(const QTextLayout::FormatRange& range : block.layout()->formats())
		{
			if(pos >= range.start && pos < range.start + range.length
			   && range.format.hasProperty(QTextFormat::ForegroundBrush)
			   && range.format.foreground().color().isValid())
			{
				return range.format.background().style() == Qt::NoBrush;
			}
		}
		return false;
	};
	const auto hasAnyForeground = [](const QTextDocument* doc, int pos) {
		if(!doc)
		{
			return false;
		}
		const QTextBlock block = doc->firstBlock();
		for(const QTextLayout::FormatRange& range : block.layout()->formats())
		{
			if(pos >= range.start && pos < range.start + range.length
			   && range.format.hasProperty(QTextFormat::ForegroundBrush))
			{
				return true;
			}
		}
		return false;
	};

	// 1) UserBlock highlights [Image 1] with foreground color and no background brush.
	QQuickItem* userBlock = nullptr;
	QQuickItem* userProse = nullptr;
	QTRY_VERIFY([&] {
		userBlock = findByTypePrefix(window->contentItem(), "UserBlock_QMLTYPE");
		if(!userBlock)
		{
			return false;
		}
		userProse = findItem(userBlock, [](QQuickItem* item) {
			return QByteArray(item->metaObject()->className()).startsWith("ProseText_QMLTYPE")
			       && item->property("text").toString().startsWith(u"[Image 1]"_s);
		});
		return userProse != nullptr;
	}());
	auto* userDocWrap = userProse->property("textDocument").value<QQuickTextDocument*>();
	QVERIFY(userDocWrap);
	QTRY_VERIFY(hasTokenForegroundNoBg(userDocWrap->textDocument(), 0));
	QVERIFY(hasTokenForegroundNoBg(userDocWrap->textDocument(), 8));
	QVERIFY(!hasAnyForeground(userDocWrap->textDocument(), 10));

	// 2) Composer highlights [Image 1] with foreground color and no background brush.
	auto* composerItem = findByTypePrefix(window->contentItem(), "Composer");
	QVERIFY(composerItem);
	QMetaObject::invokeMethod(composerItem, "focusInput");
	QQuickItem* textArea = nullptr;
	QTRY_VERIFY([&] {
		textArea = qobject_cast<QQuickItem*>(window->activeFocusItem());
		return textArea != nullptr && textArea->hasActiveFocus();
	}());
	QImage img(4, 4, QImage::Format_RGB32);
	img.fill(Qt::green);
	QGuiApplication::clipboard()->setImage(img);
	QVERIFY(f.workspace->current()->composer()->pasteFromClipboard());
	typeAsciiText(window, u"hello"_s);
	QCOMPARE(f.workspace->current()->composer()->text(), u"[Image 1] hello"_s);
	auto* composerDocWrap = textArea->property("textDocument").value<QQuickTextDocument*>();
	QVERIFY(composerDocWrap);
	QVERIFY(!hasAnyForeground(composerDocWrap->textDocument(), 0));
	QVERIFY(!hasAnyForeground(composerDocWrap->textDocument(), 8));

	QQuickItem* tokenOverlay = findItem(composerItem, [](QQuickItem* item) {
		return item->objectName() == u"composerTokenOverlay"_s;
	});
	QVERIFY(tokenOverlay);
	QVERIFY(tokenOverlay->isVisible());
	QCOMPARE(tokenOverlay->property("text").toString(), u"[Image 1] hello"_s);
	auto* overlayDocWrap = tokenOverlay->property("textDocument").value<QQuickTextDocument*>();
	QVERIFY(overlayDocWrap);
	QTRY_VERIFY(hasTokenForegroundNoBg(overlayDocWrap->textDocument(), 0));
	QTRY_VERIFY(hasTokenForegroundNoBg(overlayDocWrap->textDocument(), 8));
	QVERIFY(!hasAnyForeground(overlayDocWrap->textDocument(), 10));
	// 3) ConfigAgentsSection variant SelectField populates from model variants and selects a variant.
	const auto findByName = [](QQuickItem* root, const QString& name) {
		return findItem(root, [&name](QQuickItem* item) { return item->objectName() == name; });
	};
	f.workspace->commands()->run(u"config"_s);
	QQuickItem* configDialog = nullptr;
	QTRY_VERIFY([&] {
		configDialog = findByName(window->contentItem(), u"configDialog"_s);
		return configDialog != nullptr && !f.workspace->configEditor()->isLoading();
	}());
	QMetaObject::invokeMethod(configDialog, "selectCategory", Q_ARG(QVariant, u"agents"_s));
	QCoreApplication::processEvents();
	QQuickItem* agentsSection = findByName(configDialog, u"configAgentsSection"_s);
	QVERIFY(agentsSection);
	QMetaObject::invokeMethod(agentsSection, "setExpanded", Q_ARG(QVariant, u"builtin:build"_s), Q_ARG(QVariant, true));
	QCoreApplication::processEvents();

	QQuickItem* variantSelect = findByName(agentsSection, u"agentVariantSelect_build"_s);
	QVERIFY(variantSelect);
	QCOMPARE(variantSelect->property("placeholderText").toString(), u"Default"_s);
	const QVariantList opts = variantSelect->property("normalizedOptions").toList();
	QStringList optionValues;
	for(const QVariant& entry : opts)
	{
		optionValues.append(entry.toMap().value(u"value"_s).toString());
	}
	QVERIFY(optionValues.contains(u"low"_s));
	QVERIFY(optionValues.contains(u"medium"_s));
	QVERIFY(optionValues.contains(u"high"_s));

	QMetaObject::invokeMethod(variantSelect, "openPopup");
	QObject* variantPopup = nullptr;
	for(QObject* ch : variantSelect->children())
	{
		if(ch->objectName() == u"selectPopup"_s)
		{
			variantPopup = ch;
			break;
		}
	}
	QVERIFY(variantPopup);
	QTRY_VERIFY(variantPopup->property("opened").toBool());
	QMetaObject::invokeMethod(variantSelect, "chooseOption", Q_ARG(QVariant, u"high"_s));
	QCOMPARE(variantSelect->property("value").toString(), u"high"_s);
	QCOMPARE(f.workspace->configEditor()->valueAt({u"agent"_s, u"build"_s, u"variant"_s}).toString(), u"high"_s);
	QVERIFY2(warningSpy.isEmpty(), "Image token highlighting or agent variant SelectField emitted QML warnings");
}

void TestMainWindow::splitUnfocusClampsScrollAndTileHeaderHasNoBindingLoop()
{
	MainFixture f;
	const QString longTitle = u"Very long split tile session title that forces header text elision beside the root chip"_s;
	f.stub.setSession(u"ses_a"_s, sessionInfoJson(u"ses_a"_s, {.title = longTitle, .createdAt = 1, .attached = 1}));
	f.stub.setSession(u"ses_b"_s, sessionInfoJson(u"ses_b"_s, {.title = longTitle, .createdAt = 2, .parent = u"ses_a"_s}));
	f.stub.setSession(u"ses_c"_s, sessionInfoJson(u"ses_c"_s, {.title = longTitle, .createdAt = 3}));
	f.stub.setMessages(u"ses_c"_s, manyMessagesJson());
	f.stub.broadcastSessions();
	QTRY_COMPARE(f.workspace->sessions()->count(), 3);

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
	window->resize(1200, 720);
	QVERIFY(QTest::qWaitForWindowExposed(window));

	f.workspace->toggleSplit();
	QTRY_VERIFY(f.workspace->isSplitView());
	QQuickItem* tileGrid = findByTypePrefix(window->contentItem(), "TileGrid");
	QVERIFY(tileGrid);
	QQuickItem* tileB = nullptr;
	QQuickItem* tileC = nullptr;
	QTRY_VERIFY([&] {
		QList<QQuickItem*> tiles;
		collectTiles(tileGrid, tiles);
		tileB = nullptr;
		tileC = nullptr;
		for(QQuickItem* candidate : tiles)
		{
			const QString id = candidate->property("sessionId").toString();
			if(id == u"ses_b"_s && candidate->property("arrived").toBool())
			{
				tileB = candidate;
			}
			else if(id == u"ses_c"_s && candidate->property("arrived").toBool())
			{
				tileC = candidate;
			}
		}
		return tileB != nullptr && tileC != nullptr;
	}());

	QQuickItem* paneB = findByTypePrefix(tileB, "SessionPane");
	QQuickItem* chatB = findByTypePrefix(tileB, "ChatView");
	QQuickItem* statusLineB = findByTypePrefix(tileB, "StatusLine");
	QQuickItem* composerLoaderB = findItem(paneB, [](QQuickItem* item) { return item->objectName() == u"composerLoader"_s; });
	QQuickItem* chatC = findByTypePrefix(tileC, "ChatView");
	QVERIFY(paneB && chatB && statusLineB && composerLoaderB && chatC);
	QTRY_VERIFY(tileB->property("hasRoot").toBool() && tileB->property("rootName").toString() == longTitle);
	QTRY_VERIFY(chatB->property("maxContentY").toReal() > chatB->property("minContentY").toReal() + 100.0);
	QTRY_VERIFY(chatC->property("maxContentY").toReal() > chatC->property("minContentY").toReal() + 100.0);

	f.workspace->focusTile(u"ses_b"_s);
	QTRY_COMPARE(f.workspace->focusedTileId(), u"ses_b"_s);
	QTRY_VERIFY(paneB->property("composerShown").toBool() && composerLoaderB->height() > 0.0);
	QTRY_VERIFY(chatB->property("maxContentY").toReal() > chatB->property("minContentY").toReal() + 100.0);
	const qreal maxYBefore = chatB->property("maxContentY").toReal();
	chatB->setProperty("following", false);
	chatB->setProperty("contentY", maxYBefore);
	QTRY_VERIFY(!chatB->property("following").toBool());
	QTRY_VERIFY(qFuzzyCompare(chatB->property("contentY").toReal(), maxYBefore));

	// Unfocusing tile B smoothly animates composer box collapse instead of snapping to 0 on frame 0,
	// collapses composerLoader margin at rest, and clamps chatB scroll to the grown viewport.
	QQuickItem* composerB = findByTypePrefix(composerLoaderB, "Composer");
	QVERIFY(composerB);
	QQuickItem* boxB = findItem(composerB, [](QQuickItem* item) {
		return QByteArray(item->metaObject()->className()).startsWith("QQuickRectangle")
		       && qFuzzyCompare(item->property("radius").toReal(), 18.0);
	});
	QVERIFY(boxB);
	QTRY_VERIFY(boxB->height() > 40.0 && qRound(boxB->height()) == qRound(boxB->property("fullHeight").toReal()));
	const qreal openBoxHeight = boxB->height();
	QVERIFY(openBoxHeight > 40.0);

	f.workspace->focusTile(u"ses_c"_s);
	QTRY_COMPARE(f.workspace->focusedTileId(), u"ses_c"_s);
	QTRY_VERIFY(!paneB->property("composerShown").toBool());
	// Animation starts from openBoxHeight rather than snapping to 0 on frame 0.
	QVERIFY(boxB->height() > 0.0);
	QTRY_COMPARE(composerLoaderB->height(), 0.0);
	QObject* composerAnchors = composerLoaderB->property("anchors").value<QObject*>();
	QVERIFY(composerAnchors);
	QCOMPARE(composerAnchors->property("bottomMargin").toReal(), 0.0);
	QTRY_VERIFY(std::abs((statusLineB->y() + statusLineB->height()) - paneB->height()) <= 0.5);
	QTRY_VERIFY(chatB->property("contentY").toReal() <= chatB->property("maxContentY").toReal() + 1.0);
	QTRY_VERIFY(chatB->property("contentY").toReal() >= chatB->property("minContentY").toReal() - 0.5);

	// Focusing tile B back animates boxB from 0 to fullHeight smoothly as well.
	f.workspace->focusTile(u"ses_b"_s);
	QTRY_VERIFY(paneB->property("composerShown").toBool());
	QVERIFY(boxB->height() < openBoxHeight);
	QTRY_COMPARE(qRound(boxB->height()), qRound(openBoxHeight));
	QVERIFY2(warningSpy.isEmpty(), "Split unfocus scroll clamp or Tile header with root chip emitted QML warnings");
}

void TestMainWindow::queueStripShowsQueuedPromptsAndCoexistsWithTileComposer()
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
	window->resize(1000, 700);
	QVERIFY(QTest::qWaitForWindowExposed(window));

	// 1) QueueEvent in single view reveals QueueStrip with height > 0 and displays the queued prompt.
	const QJsonObject queuedMsg{{u"id"_s, u"q_1"_s},
	                            {u"role"_s, u"user"_s},
	                            {u"time"_s, 100},
	                            {u"source"_s, QJsonObject{{u"kind"_s, u"human"_s}}},
	                            {u"parts"_s, QJsonArray{QJsonObject{{u"type"_s, u"text"_s}, {u"text"_s, u"Follow-up task"_s}, {u"time"_s, 100}}}}};
	f.fake.send({{u"kind"_s, u"event"_s},
	             {u"session_id"_s, u"ses_a"_s},
	             {u"event"_s, QJsonObject{{u"type"_s, u"queue"_s}, {u"messages"_s, QJsonArray{queuedMsg}}}}});
	QTRY_COMPARE(f.workspace->current()->composer()->queueCount(), 1);

	QQuickItem* queueStrip = findByTypePrefix(window->contentItem(), "QueueStrip");
	QVERIFY(queueStrip);
	QTRY_VERIFY(queueStrip->isVisible());
	QTRY_COMPARE(qRound(queueStrip->height()), queueStrip->property("queueRowHeight").toInt());
	QQuickItem* rowText = findItem(queueStrip, [](QQuickItem* item) {
		return QByteArray(item->metaObject()->className()).startsWith("QQuickText")
		       && item->property("text").toString() == u"Follow-up task"_s;
	});
	QVERIFY(rowText && rowText->isVisible());

	// 2) In split view, an unfocused tile with a queued prompt has zero extra bottom gap and box never covers QueueStrip.
	f.workspace->toggleSplit();
	QTRY_VERIFY(f.workspace->isSplitView());
	QQuickItem* tileGrid = findByTypePrefix(window->contentItem(), "TileGrid");
	QVERIFY(tileGrid);
	QQuickItem* tileB = nullptr;
	QTRY_VERIFY([&] {
		QList<QQuickItem*> tiles;
		collectTiles(tileGrid, tiles);
		for(QQuickItem* candidate : tiles)
		{
			if(candidate->property("sessionId").toString() == u"ses_b"_s && candidate->property("arrived").toBool())
			{
				tileB = candidate;
			}
		}
		return tileB != nullptr;
	}());
	QCOMPARE(f.workspace->focusedTileId(), QString());
	f.fake.send({{u"kind"_s, u"event"_s},
	             {u"session_id"_s, u"ses_b"_s},
	             {u"event"_s, QJsonObject{{u"type"_s, u"queue"_s}, {u"messages"_s, QJsonArray{queuedMsg}}}}});
	QTRY_COMPARE(f.workspace->session(u"ses_b"_s)->composer()->queueCount(), 1);

	QQuickItem* paneB = findByTypePrefix(tileB, "SessionPane");
	QQuickItem* composerLoaderB = findItem(paneB, [](QQuickItem* item) { return item->objectName() == u"composerLoader"_s; });
	QQuickItem* composerB = findByTypePrefix(composerLoaderB, "Composer");
	QQuickItem* stripB = findByTypePrefix(composerB, "QueueStrip");
	QQuickItem* boxB = findItem(composerB, [](QQuickItem* item) {
		return QByteArray(item->metaObject()->className()).startsWith("QQuickRectangle")
		       && qFuzzyCompare(item->property("radius").toReal(), 18.0);
	});
	QVERIFY(paneB && composerLoaderB && composerB && stripB && boxB);
	QTRY_COMPARE(qRound(stripB->height()), stripB->property("queueRowHeight").toInt());
	QTRY_COMPARE(qRound(composerLoaderB->height()), qRound(stripB->height()));
	QCOMPARE(qRound(boxB->height()), 0);
	QVERIFY(boxB->y() >= stripB->height());

	// Focusing tile B animates boxB open below stripB + queueGap without ever overlapping stripB.
	f.workspace->focusTile(u"ses_b"_s);
	QTRY_VERIFY(boxB->height() > 20.0);
	QVERIFY(boxB->y() >= stripB->height());
	QTRY_COMPARE(qRound(boxB->y()), qRound(stripB->height() + composerB->property("queueGap").toReal()));
	QVERIFY2(warningSpy.isEmpty(), "QueueStrip or tile composer queue coexistence emitted QML warnings");
}

void TestMainWindow::lightboxNavigatesZoomsAndResets()
{
	MainFixture f;
	f.stub.setMessages(u"ses_a"_s, QJsonArray{
		QJsonObject{{u"id"_s, u"m1"_s},
		            {u"role"_s, u"user"_s},
		            {u"time"_s, 1},
		            {u"parts"_s, QJsonArray{QJsonObject{{u"type"_s, u"image"_s},
		                                               {u"id"_s, u"img_1"_s},
		                                               {u"mime"_s, u"image/png"_s},
		                                               {u"data"_s, QString()},
		                                               {u"size"_s, 1024},
		                                               {u"name"_s, u"first.png"_s}}}}},
		QJsonObject{{u"id"_s, u"m2"_s},
		            {u"role"_s, u"assistant"_s},
		            {u"time"_s, 2},
		            {u"parts"_s, QJsonArray{QJsonObject{{u"type"_s, u"tool"_s},
		                                               {u"call_id"_s, u"c1"_s},
		                                               {u"name"_s, u"read"_s},
		                                               {u"input"_s, QJsonObject()},
		                                               {u"state"_s, u"done"_s},
		                                               {u"images"_s, QJsonArray{QJsonObject{{u"id"_s, u"img_2"_s}, {u"mime"_s, u"image/png"_s}, {u"data"_s, QString()}, {u"size"_s, 2048}}}}}}}},
	});
	f.workspace->openSession(u"ses_b"_s);
	QTRY_COMPARE(f.workspace->currentId(), u"ses_b"_s);
	f.workspace->openSession(u"ses_a"_s);
	QTRY_COMPARE(f.workspace->currentId(), u"ses_a"_s);
	QCOMPARE(f.workspace->current()->allImages().size(), 2);

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
	QTRY_VERIFY(window->isActive());

	QQuickItem* chatView = nullptr;
	QTRY_VERIFY([&] {
		chatView = findByTypePrefix(window->contentItem(), "ChatView");
		return chatView != nullptr;
	}());

	const QVariantMap firstImg = f.workspace->current()->allImages().first().toMap();
	QMetaObject::invokeMethod(chatView, "imageClicked", Q_ARG(QVariant, firstImg));

	QQuickItem* lightbox = nullptr;
	QTRY_VERIFY([&] {
		lightbox = findByTypePrefix(window->contentItem(), "Lightbox");
		return lightbox != nullptr && lightbox->property("shown").toBool();
	}());

	auto* prevBtn = findItem(lightbox, [](QQuickItem* i) { return i->objectName() == u"lightboxPrevButton"_s; });
	auto* nextBtn = findItem(lightbox, [](QQuickItem* i) { return i->objectName() == u"lightboxNextButton"_s; });
	auto* zoomInBtn = findItem(lightbox, [](QQuickItem* i) { return i->objectName() == u"lightboxZoomInButton"_s; });
	auto* zoomOutBtn = findItem(lightbox, [](QQuickItem* i) { return i->objectName() == u"lightboxZoomOutButton"_s; });
	auto* zoomResetBtn = findItem(lightbox, [](QQuickItem* i) { return i->objectName() == u"lightboxZoomResetButton"_s; });
	auto* caption = findItem(lightbox, [](QQuickItem* i) { return i->objectName() == u"lightboxCaption"_s; });
	QVERIFY(prevBtn && nextBtn && zoomInBtn && zoomOutBtn && zoomResetBtn && caption);
	QTRY_VERIFY(qFuzzyCompare(lightbox->opacity(), 1.0));
	QVERIFY(prevBtn->isVisible() && nextBtn->isVisible());
	QVERIFY(caption->property("text").toString().startsWith(u"1 / 2"_s));
	QQuickItem* lightboxPicture = findItem(lightbox, [](QQuickItem* item) {
		return QByteArray(item->metaObject()->className()).startsWith("QQuickImage")
		       && item->property("source").toUrl().toString().contains(u"chatimage"_s);
	});
	QVERIFY(lightboxPicture);
	QTRY_VERIFY([&] {
		const int status = lightboxPicture->property("status").toInt();
		return lightboxPicture->property("source").toUrl().toString().contains(u"img_1"_s)
		       && (status == 1 || status == 3);
	}());
	const auto clickCenter = [&](QQuickItem* item) {
		const QPoint pt = item->mapToScene(QPointF(item->width() / 2.0, item->height() / 2.0)).toPoint();
		QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, pt);
	};

	// Zoom in -> Zoom out -> Reset.
	clickCenter(zoomInBtn);
	QTRY_VERIFY(lightbox->property("zoom").toReal() > 1.2);
	QCOMPARE(zoomResetBtn->property("text").toString(), u"125%"_s);
	clickCenter(zoomOutBtn);
	QTRY_VERIFY(qFuzzyCompare(lightbox->property("zoom").toReal(), 1.0));
	QTest::keyClick(window, Qt::Key_Equal);
	QTRY_VERIFY(lightbox->property("zoom").toReal() > 1.2);
	clickCenter(zoomResetBtn);
	QTRY_VERIFY(qFuzzyCompare(lightbox->property("zoom").toReal(), 1.0));

	// Next / Prev navigation resets zoom and updates index & caption.
	QTest::keyClick(window, Qt::Key_Equal);
	QTRY_VERIFY(lightbox->property("zoom").toReal() > 1.2);
	clickCenter(nextBtn);
	QTRY_COMPARE(lightbox->property("currentIndex").toInt(), 1);
	QVERIFY(caption->property("text").toString().startsWith(u"2 / 2"_s));
	QVERIFY(qFuzzyCompare(lightbox->property("zoom").toReal(), 1.0));
	QTRY_VERIFY([&] {
		const int status = lightboxPicture->property("status").toInt();
		return lightboxPicture->property("source").toUrl().toString().contains(u"img_2"_s)
		       && (status == 1 || status == 3);
	}());
	QTest::keyClick(window, Qt::Key_Left);
	QTRY_COMPARE(lightbox->property("currentIndex").toInt(), 0);
	QVERIFY(caption->property("text").toString().startsWith(u"1 / 2"_s));
	// Let async image fetch finish before teardown.
	QTRY_VERIFY([&] {
		const int status = lightboxPicture->property("status").toInt();
		return lightboxPicture->property("source").toUrl().toString().contains(u"img_1"_s)
		       && (status == 1 || status == 3);
	}());
	QVERIFY2(warningSpy.isEmpty(), "Lightbox navigation or zoom emitted QML warnings");
}

void TestMainWindow::sidebarHistoryFilterMenuAndCustomHeightCollapseWork()
{
	MainFixture f;
	f.stub.setSession(u"ses_a"_s, sessionInfoJson(u"ses_a"_s, {.name = u"Planner"_s, .cwd = u"/work"_s, .createdAt = 1}));
	for(int i = 0; i < 10; ++i)
	{
		f.stub.add(u"ses_extra_%1"_s.arg(i), 10 + i);
	}
	f.stub.addHistory(u"hist_spawned"_s, sessionInfoJson(u"hist_spawned"_s, {.title = u"Spawned job"_s, .cwd = u"/work"_s, .createdAt = 5, .live = false, .parent = u"ses_a"_s}));
	f.stub.addHistory(u"hist_other"_s, sessionInfoJson(u"hist_other"_s, {.title = u"Other dir job"_s, .cwd = u"/other"_s, .createdAt = 6, .live = false}));
	f.stub.broadcastSessions();
	f.workspace->refreshHistory();
	QTRY_COMPARE(f.workspace->history()->rowCount(), 2);

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
	QTRY_VERIFY(window->isActive());

	auto* sidebar = findByTypePrefix(window->contentItem(), "Sidebar_QMLTYPE");
	QVERIFY(sidebar);
	auto* runningList = findItem(sidebar, [](QQuickItem* i) { return i->objectName() == u"runningList"_s; });
	auto* historyHeader = findItem(sidebar, [](QQuickItem* i) { return i->objectName() == u"historySectionHeader"_s; });
	auto* historyBody = findItem(sidebar, [](QQuickItem* i) { return i->objectName() == u"historyBody"_s; });
	auto* filterBtn = findItem(sidebar, [](QQuickItem* i) { return i->objectName() == u"historyFilterButton"_s; });
	QVERIFY(runningList && historyHeader && historyBody && filterBtn);

	// 1) HistoryRow displays "· ⟲ Planner" for spawned session.
	QTRY_VERIFY(findItem(historyBody, [](QQuickItem* item) {
		return QByteArray(item->metaObject()->className()).startsWith("QQuickText")
		       && item->property("text").toString().contains(u"\u27f2 Planner"_s);
	}) != nullptr);

	// 2) Scheme B history filter menu toggles User-created and Only current directory.
	const auto clickCenter = [&](QQuickItem* item) {
		const QPoint pt = item->mapToScene(QPointF(item->width() / 2.0, item->height() / 2.0)).toPoint();
		QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, pt);
	};
	clickCenter(filterBtn);
	QObject* filterMenu = filterBtn->findChild<QObject*>(u"historyFilterMenu"_s);
	QVERIFY(filterMenu);
	QTRY_VERIFY(filterMenu->property("opened").toBool());
	QQuickItem* contentItem = filterMenu->property("contentItem").value<QQuickItem*>();
	QVERIFY(contentItem);
	auto* userFilterItem = findItem(contentItem, [](QQuickItem* i) { return i->objectName() == u"historyFilterUser"_s; });
	QVERIFY(userFilterItem);
	QQuickItem* historyRow = findByTypePrefix(historyBody, "HistoryRow_QMLTYPE");
	QVERIFY(historyRow);
	const QString initialSessionId = f.workspace->currentId();
	const QPointF userFilterCenter = userFilterItem->mapToScene(QPointF(userFilterItem->width() / 2.0, userFilterItem->height() / 2.0));
	const QPointF rowTopLeft = historyRow->mapToScene(QPointF(0.0, 0.0));
	const QPointF rowBottomRight = historyRow->mapToScene(QPointF(historyRow->width(), historyRow->height()));
	QVERIFY(userFilterCenter.x() >= rowTopLeft.x() && userFilterCenter.x() <= rowBottomRight.x());
	QVERIFY(userFilterCenter.y() >= rowTopLeft.y() && userFilterCenter.y() <= rowBottomRight.y());

	// Clicking inside non-item padding of historyFilterMenu neither closes nor clicks through to HistoryRow.
	const QPoint menuTopPad = contentItem->mapToScene(QPointF(contentItem->width() / 2.0, 2.0)).toPoint();
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, menuTopPad);
	QTest::qWait(40);
	QVERIFY(filterMenu->property("opened").toBool());
	QCOMPARE(f.workspace->currentId(), initialSessionId);

	clickCenter(userFilterItem);
	QTRY_VERIFY(!filterMenu->property("opened").toBool());
	QTRY_VERIFY(!filterMenu->property("visible").toBool());
	QTRY_COMPARE(f.workspace->historyCreatorFilter(), u"user"_s);
	QTRY_COMPARE(f.workspace->history()->rowCount(), 1);
	QVERIFY(sidebar->property("historyFilterActive").toBool());
	QCOMPARE(f.workspace->currentId(), initialSessionId);

	// Reset filter so multiple HistoryRows exist, then verify ContextMenu modal click blocking and action triggering.
	f.workspace->setHistoryFilter(u"all"_s, false);
	QTRY_COMPARE(f.workspace->history()->rowCount(), 2);
	QQuickItem* historyList = findItem(historyBody, [](QQuickItem* i) { return i->objectName() == u"historyList"_s; });
	QVERIFY(historyList);
	QPointer<QQuickItem> spawnedRowPtr;
	QQuickItem* otherRow = nullptr;
	QTRY_VERIFY([&] {
		spawnedRowPtr = findItem(historyList, [](QQuickItem* item) {
			return QByteArray(item->metaObject()->className()).startsWith("HistoryRow_QMLTYPE")
			       && item->property("sessionId").toString() == u"hist_spawned"_s;
		});
		otherRow = findItem(historyList, [](QQuickItem* item) {
			return QByteArray(item->metaObject()->className()).startsWith("HistoryRow_QMLTYPE")
			       && item->property("sessionId").toString() == u"hist_other"_s;
		});
		return spawnedRowPtr != nullptr && otherRow != nullptr;
	}());
	QPointer<QObject> rowMenu;
	for(QObject* ch : spawnedRowPtr->children())
	{
		if(QByteArray(ch->metaObject()->className()).startsWith("ContextMenu_QMLTYPE"))
		{
			rowMenu = ch;
			break;
		}
	}
	QVERIFY(rowMenu);
	QMetaObject::invokeMethod(rowMenu.data(), "openAt", Q_ARG(QPointF, QPointF(10.0, 10.0)));
	QTRY_VERIFY(rowMenu->property("opened").toBool());
	// Clicking a point on a HistoryRow strictly outside the ContextMenu rect dismisses the menu without opening that row.
	QQuickItem* rowMenuContent = rowMenu->property("contentItem").value<QQuickItem*>();
	QVERIFY(rowMenuContent);
	const QRectF menuRect(rowMenuContent->mapToScene(QPointF(0.0, 0.0)),
	                      QSizeF(rowMenu->property("width").toReal(), rowMenu->property("height").toReal()));
	const QRectF otherRowRect(otherRow->mapToScene(QPointF(0.0, 0.0)), QSizeF(otherRow->width(), otherRow->height()));
	QQuickItem* outsideTargetRow = !menuRect.contains(otherRowRect.center()) ? otherRow : spawnedRowPtr.data();
	const QRectF targetRect(outsideTargetRow->mapToScene(QPointF(0.0, 0.0)),
	                        QSizeF(outsideTargetRow->width(), outsideTargetRow->height()));
	const QPoint outsideRowPt(qRound(targetRect.right() - 10.0), qRound(targetRect.center().y()));
	QVERIFY(targetRect.contains(outsideRowPt));
	QVERIFY(!menuRect.contains(outsideRowPt));
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, outsideRowPt);
	QTRY_VERIFY(!rowMenu->property("opened").toBool());
	QTRY_VERIFY(!rowMenu->property("visible").toBool());
	QCOMPARE(f.workspace->currentId(), initialSessionId);

	// Pressing Escape closes the modal ContextMenu cleanly.
	QMetaObject::invokeMethod(rowMenu.data(), "openAt", Q_ARG(QPointF, QPointF(10.0, 10.0)));
	QTRY_VERIFY(rowMenu->property("opened").toBool());
	QTest::keyClick(window, Qt::Key_Escape);
	QTRY_VERIFY(!rowMenu->property("opened").toBool());
	QTRY_VERIFY(!rowMenu->property("visible").toBool());
	QCOMPARE(f.workspace->currentId(), initialSessionId);

	// Re-open ContextMenu over HistoryRow and click an action entry -> triggers filter without opening underlying row.
	QMetaObject::invokeMethod(rowMenu.data(), "openAt", Q_ARG(QPointF, QPointF(10.0, 10.0)));
	QTRY_VERIFY(rowMenu->property("opened").toBool());
	rowMenuContent = rowMenu->property("contentItem").value<QQuickItem*>();
	QVERIFY(rowMenuContent);
	QQuickItem* filterCreatorEntry = findItem(rowMenuContent, [](QQuickItem* item) {
		return QByteArray(item->metaObject()->className()).startsWith("QQuickText")
		       && item->property("text").toString() == u"Filter history by creator"_s;
	});
	QVERIFY(filterCreatorEntry);
	clickCenter(filterCreatorEntry);
	QTRY_COMPARE(f.workspace->historyCreatorFilter(), u"creator"_s);
	QVERIFY(rowMenu && spawnedRowPtr);
	QTRY_VERIFY(rowMenu && !rowMenu->property("opened").toBool());
	QTRY_VERIFY(rowMenu && !rowMenu->property("visible").toBool());
	QCOMPARE(f.workspace->currentId(), initialSessionId);

	f.workspace->setHistoryFilter(u"all"_s, false);
	QTRY_COMPARE(f.workspace->history()->rowCount(), 2);

	// 3) Custom running height smoothly animates to remainingAfterTodo when History collapses, then back on expand.
	f.workspace->settings()->setSidebarRunningHeight(180);
	QTRY_COMPARE(qRound(runningList->height()), 180);
	clickCenter(historyHeader);
	QTRY_VERIFY(!f.workspace->settings()->sidebarHistoryExpanded());
	QVERIFY(runningList->height() < sidebar->property("remainingAfterTodo").toReal());
	QTRY_COMPARE(qRound(runningList->height()), qRound(sidebar->property("remainingAfterTodo").toReal()));
	QTRY_VERIFY(!historyBody->isVisible());

	clickCenter(historyHeader);
	QTRY_VERIFY(f.workspace->settings()->sidebarHistoryExpanded());
	QVERIFY(runningList->height() > 180.0);
	QTRY_COMPARE(qRound(runningList->height()), 180);
	QVERIFY2(warningSpy.isEmpty(), "Sidebar filter menu or custom-height collapse emitted QML warnings");
}

void TestMainWindow::splitViewSidebarToggleBlursFadesAndFreezesLayoutWidth()
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
	window->resize(1200, 720);
	QVERIFY(QTest::qWaitForWindowExposed(window));

	f.workspace->toggleSplit();
	QTRY_VERIFY(f.workspace->isSplitView());
	QQuickItem* tileGrid = findByTypePrefix(window->contentItem(), "TileGrid");
	QVERIFY(tileGrid);
	QQuickItem* tile = nullptr;
	QTRY_VERIFY([&] {
		QList<QQuickItem*> tiles;
		collectTiles(tileGrid, tiles);
		for(QQuickItem* candidate : tiles)
		{
			if(candidate->property("arrived").toBool() && !candidate->property("resizingLayout").toBool())
			{
				tile = candidate;
				return true;
			}
		}
		return false;
	}());
	QQuickItem* pane = findByTypePrefix(tile, "SessionPane");
	QVERIFY(pane);
	QTRY_VERIFY(qFuzzyCompare(pane->property("layoutWidth").toReal(), tile->property("settledLayoutWidth").toReal()));
	const qreal initialSettledWidth = tile->property("settledLayoutWidth").toReal();
	QVERIFY(initialSettledWidth > 100.0);

	// Collapsing the sidebar keeps opacity 1.0 (no blur/fade), freezes layoutWidth, then settles to wider width.
	f.workspace->commands()->run(u"sidebar"_s);
	QTRY_VERIFY(tile->property("resizingLayout").toBool());
	QVERIFY(qFuzzyCompare(pane->property("opacity").toReal(), 1.0));
	QVERIFY(!pane->property("resizeBlurAmount").isValid());
	QCOMPARE(pane->property("layoutWidth").toReal(), initialSettledWidth);

	QTRY_VERIFY_WITH_TIMEOUT(!tile->property("resizingLayout").toBool(), 2000);
	const qreal collapsedSettledWidth = tile->property("settledLayoutWidth").toReal();
	QVERIFY(collapsedSettledWidth > initialSettledWidth + 50.0);
	QVERIFY(qFuzzyCompare(collapsedSettledWidth, tile->property("paneTargetWidth").toReal()));
	QVERIFY(qFuzzyCompare(pane->property("opacity").toReal(), 1.0));
	QVERIFY(!pane->property("resizeBlurAmount").isValid());
	QTRY_VERIFY(qFuzzyCompare(pane->property("layoutWidth").toReal(), pane->width()));

	// Expanding the sidebar keeps opacity 1.0 (no blur/fade), freezes layoutWidth, then settles back to initial width.
	f.workspace->commands()->run(u"sidebar"_s);
	QTRY_VERIFY(tile->property("resizingLayout").toBool());
	QVERIFY(qFuzzyCompare(pane->property("opacity").toReal(), 1.0));
	QVERIFY(!pane->property("resizeBlurAmount").isValid());
	QCOMPARE(pane->property("layoutWidth").toReal(), collapsedSettledWidth);

	QTRY_VERIFY_WITH_TIMEOUT(!tile->property("resizingLayout").toBool(), 2000);
	QVERIFY(qFuzzyCompare(tile->property("settledLayoutWidth").toReal(), initialSettledWidth));
	QVERIFY(qFuzzyCompare(pane->property("opacity").toReal(), 1.0));
	QVERIFY(!pane->property("resizeBlurAmount").isValid());
	QTRY_VERIFY(qFuzzyCompare(pane->property("layoutWidth").toReal(), pane->width()));

	// Rapid double-toggle of sidebar in split view settles cleanly to initial width without warnings.
	f.workspace->commands()->run(u"sidebar"_s);
	f.workspace->commands()->run(u"sidebar"_s);
	QTRY_VERIFY_WITH_TIMEOUT(!tile->property("resizingLayout").toBool(), 2000);
	QVERIFY(qFuzzyCompare(tile->property("settledLayoutWidth").toReal(), initialSettledWidth));
	QVERIFY(qFuzzyCompare(pane->property("opacity").toReal(), 1.0));
	QVERIFY(!pane->property("resizeBlurAmount").isValid());
	QVERIFY2(warningSpy.isEmpty(), "Split-view sidebar toggle transition emitted QML warnings");
}

void TestMainWindow::backendDialogSwitchAndStatusPillActionWorkWithoutWarnings()
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
	window->requestActivate();
	QTRY_VERIFY(window->isActive());

	// Open BackendDialog via SettingsDialog "Switch backend..." button.
	f.workspace->openDialog(u"settings"_s);
	QQuickItem* settingsDialog = nullptr;
	QTRY_VERIFY([&] {
		settingsDialog = findByTypePrefix(window->contentItem(), "SettingsDialog_QMLTYPE");
		return settingsDialog != nullptr && settingsDialog->isVisible();
	}());
	QQuickItem* settingsPanel = findByTypePrefix(settingsDialog, "QQuickRectangle");
	QVERIFY(settingsPanel);
	QTRY_VERIFY(settingsPanel->property("shown").toBool()
	            && qFuzzyCompare(settingsPanel->scale(), 1.0)
	            && qFuzzyCompare(settingsPanel->property("dropOffset").toReal() + 1.0, 1.0));
	QQuickItem* scroller = findByTypePrefix(settingsDialog, "QQuickFlickable");
	QVERIFY(scroller);
	QQuickItem* switchBtn = nullptr;
	QTRY_VERIFY([&] {
		switchBtn = findItem(settingsDialog, [](QQuickItem* item) { return item->objectName() == u"settingsSwitchBackendButton"_s; });
		return switchBtn != nullptr && switchBtn->isVisible();
	}());
	QQuickItem* contentItem = scroller->property("contentItem").value<QQuickItem*>();
	QVERIFY(contentItem);
	QTRY_VERIFY(scroller->property("contentHeight").toReal() + 1.0
	            >= switchBtn->mapToItem(contentItem, QPointF(0.0, 0.0)).y() + switchBtn->height());
	const QPointF inContent = switchBtn->mapToItem(contentItem, QPointF(0.0, 0.0));
	const qreal viewH = scroller->height();
	const qreal maxY = std::max(0.0, scroller->property("contentHeight").toReal() - viewH);
	scroller->setProperty("contentY", std::clamp(inContent.y() - viewH / 2.0 + switchBtn->height() / 2.0, 0.0, maxY));
	QCoreApplication::processEvents();
	const QPointF switchCenterF = switchBtn->mapToScene(QPointF(switchBtn->width() / 2.0, switchBtn->height() / 2.0));
	const qreal scrollerTop = scroller->mapToScene(QPointF(0.0, 0.0)).y();
	const qreal scrollerBottom = scrollerTop + viewH;
	QVERIFY(switchCenterF.y() > scrollerTop && switchCenterF.y() < scrollerBottom);
	QVERIFY(switchCenterF.y() >= 0.0 && switchCenterF.y() < window->height());
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, switchCenterF.toPoint());
	QTRY_COMPARE(f.workspace->dialog(), u"backend"_s);

	QQuickItem* backendDialog = nullptr;
	QTRY_VERIFY([&] {
		backendDialog = findByTypePrefix(window->contentItem(), "BackendDialog_QMLTYPE");
		return backendDialog != nullptr && backendDialog->isVisible();
	}());
	QQuickItem* backendPanel = findByTypePrefix(backendDialog, "QQuickRectangle");
	QVERIFY(backendPanel);
	QTRY_VERIFY(backendPanel->property("shown").toBool()
	            && qFuzzyCompare(backendPanel->scale(), 1.0)
	            && qFuzzyCompare(backendPanel->property("dropOffset").toReal() + 1.0, 1.0));
	QQuickItem* urlField = nullptr;
	QQuickItem* tempBtn = nullptr;
	QTRY_VERIFY([&] {
		urlField = findItem(backendDialog, [](QQuickItem* item) { return item->objectName() == u"backendUrlField"_s; });
		tempBtn = findItem(backendDialog, [](QQuickItem* item) { return item->objectName() == u"backendTemporaryButton"_s; });
		return urlField != nullptr && tempBtn != nullptr && tempBtn->isVisible();
	}());

	// Invalid URL shows inline error without closing.
	urlField->setProperty("text", u"http://bad-scheme"_s);
	const QPoint tempCenter = tempBtn->mapToScene(QPointF(tempBtn->width() / 2.0, tempBtn->height() / 2.0)).toPoint();
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, tempCenter);
	QCOMPARE(f.workspace->dialog(), u"backend"_s);
	QVERIFY(!urlField->property("errorText").toString().isEmpty());

	// Valid URL switches temporarily to a second FakeBackend.
	FakeBackend secondFake;
	StubMainServer secondStub(secondFake);
	secondStub.add(u"ses_second"_s, 5);
	urlField->setProperty("text", secondFake.url().toString());
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, tempCenter);
	QTRY_COMPARE(f.workspace->dialog(), QString());
	QCOMPARE(f.workspace->backendSource(), u"temporary"_s);
	QTRY_COMPARE(f.workspace->connectionState(), u"connected"_s);
	QTRY_VERIFY(f.workspace->sessions()->indexOf(u"ses_second"_s) >= 0);

	// Disconnected StatusPill shows "Switch Backend..." action button that opens BackendDialog.
	QVERIFY(f.workspace->switchBackend(u"ws://127.0.0.1:1/"_s, {}, false));
	QTRY_VERIFY(f.workspace->connectionState() != u"connected"_s);
	QQuickItem* pillBtn = nullptr;
	QTRY_VERIFY([&] {
		pillBtn = findItem(window->contentItem(), [](QQuickItem* item) { return item->objectName() == u"statusPillActionButton"_s; });
		return pillBtn != nullptr && pillBtn->isVisible();
	}());
	const QPoint pillCenter = pillBtn->mapToScene(QPointF(pillBtn->width() / 2.0, pillBtn->height() / 2.0)).toPoint();
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, pillCenter);
	QTRY_COMPARE(f.workspace->dialog(), u"backend"_s);

	QVERIFY2(warningSpy.isEmpty(), "BackendDialog or StatusPill emitted QML warnings");
}

void TestMainWindow::stashButtonDialogAndSplitFilterCapsuleWorkWithoutWarnings()
{
	MainFixture f;
	f.stub.setSession(u"ses_child"_s, sessionInfoJson(u"ses_child"_s, {.title = u"Child"_s, .createdAt = 3, .parent = u"ses_a"_s}));
	f.stub.broadcastSessions();
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 3);

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
	window->resize(1100, 720);
	QVERIFY(QTest::qWaitForWindowExposed(window));

	// Split view filter capsule filters by User-created and All without warnings.
	f.workspace->toggleSplit();
	QTRY_VERIFY(f.workspace->isSplitView());
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 2);

	QQuickItem* userPill = nullptr;
	QQuickItem* allPill = nullptr;
	QTRY_VERIFY([&] {
		userPill = findItem(window->contentItem(), [](QQuickItem* item) { return item->objectName() == u"splitFilterUser"_s; });
		allPill = findItem(window->contentItem(), [](QQuickItem* item) { return item->objectName() == u"splitFilterAll"_s; });
		return userPill != nullptr && allPill != nullptr && userPill->isVisible();
	}());
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, userPill->mapToScene(QPointF(userPill->width() / 2.0, userPill->height() / 2.0)).toPoint());
	QTRY_COMPARE(f.workspace->splitFilter(), u"user"_s);
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 1);
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, allPill->mapToScene(QPointF(allPill->width() / 2.0, allPill->height() / 2.0)).toPoint());
	QTRY_COMPARE(f.workspace->splitFilter(), u"all"_s);
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 2);
	f.workspace->toggleSplit();
	QTRY_VERIFY(!f.workspace->isSplitView());

	// Sidebar stash button left-click stashes all running sessions; second left-click opens StashDialog.
	QQuickItem* stashBtn = findItem(window->contentItem(), [](QQuickItem* item) { return item->objectName() == u"sidebarStashButton"_s; });
	QVERIFY(stashBtn);
	const QPoint stashCenter = stashBtn->mapToScene(QPointF(stashBtn->width() / 2.0, stashBtn->height() / 2.0)).toPoint();
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, stashCenter);
	QTRY_COMPARE(f.workspace->stash()->groupCount(), 1);
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 0);

	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, stashCenter);
	QTRY_COMPARE(f.workspace->dialog(), u"stash"_s);
	QQuickItem* newGroupBtn = nullptr;
	QTRY_VERIFY([&] {
		newGroupBtn = findItem(window->contentItem(), [](QQuickItem* item) { return item->objectName() == u"newStashGroupButton"_s; });
		return newGroupBtn != nullptr && newGroupBtn->isVisible();
	}());
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, newGroupBtn->mapToScene(QPointF(newGroupBtn->width() / 2.0, newGroupBtn->height() / 2.0)).toPoint());
	QTRY_COMPARE(f.workspace->stash()->groupCount(), 2);

	QVERIFY2(warningSpy.isEmpty(), "Stash UI or split filter capsule emitted QML warnings");
}

void TestMainWindow::floatingBallModeTransitionsBetweenBallHoverCardAndMiniPanelWithoutWarnings()
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
	window->requestActivate();
	QTRY_VERIFY(window->isActive());

	// Enter floating ball mode via topbar button.
	QQuickItem* floatBtn = findItem(window->contentItem(), [](QQuickItem* item) { return item->objectName() == u"floatingBallButton"_s; });
	QVERIFY(floatBtn);
	QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, floatBtn->mapToScene(QPointF(floatBtn->width() / 2.0, floatBtn->height() / 2.0)).toPoint());
	QTRY_VERIFY(f.workspace->isFloatingMode());
	QTRY_VERIFY(!window->isVisible());

	QQuickWindow* ballWin = nullptr;
	QQuickWindow* panelWin = nullptr;
	QTRY_VERIFY([&] {
		ballWin = window->findChild<QQuickWindow*>(u"floatingBallWindow"_s);
		panelWin = window->findChild<QQuickWindow*>(u"floatingPanelWindow"_s);
		return ballWin != nullptr && panelWin != nullptr && ballWin->isVisible();
	}());
	QCOMPARE(ballWin->width(), 56);
	QCOMPARE(ballWin->height(), 56);
	QVERIFY(QTest::qWaitForWindowExposed(ballWin));
	ballWin->requestActivate();
	QCoreApplication::processEvents();

	// Click ball to open mini panel.
	QTest::mouseMove(ballWin, QPoint(28, 28));
	QTest::mouseClick(ballWin, Qt::LeftButton, Qt::NoModifier, QPoint(28, 28));
	QTRY_VERIFY(panelWin->isVisible());
	QTRY_VERIFY(!ballWin->isVisible());
	QVERIFY(QTest::qWaitForWindowExposed(panelWin));
	panelWin->requestActivate();
	QCoreApplication::processEvents();

	// Return to full window from panel button.
	QQuickItem* returnBtn = nullptr;
	QTRY_VERIFY([&] {
		returnBtn = findItem(panelWin->contentItem(), [](QQuickItem* item) { return item->objectName() == u"floatingReturnButton"_s; });
		return returnBtn != nullptr && returnBtn->isVisible();
	}());
	const QPoint returnCenter = returnBtn->mapToScene(QPointF(returnBtn->width() / 2.0, returnBtn->height() / 2.0)).toPoint();
	QTest::mouseMove(panelWin, returnCenter);
	QTest::mouseClick(panelWin, Qt::LeftButton, Qt::NoModifier, returnCenter);
	QTRY_VERIFY(window->isVisible());

	QVERIFY2(warningSpy.isEmpty(), "FloatingBallWindow emitted QML warnings");
}

QTEST_MAIN(TestMainWindow)
#include "tst_main_window.moc"
