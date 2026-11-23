#include "app/workspace.h"
#include "coro_test.h"
#include "fake_backend.h"

#include <QJSEngine>
#include <QJsonArray>
#include <QJsonValue>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QtTest>

#include <functional>
#include <optional>
#include <utility>

using namespace Qt::StringLiterals;
using namespace refgui;
using namespace refgui::test;
using refgui::client::Backend;

namespace
{

constexpr int ReconnectTimeoutMs = 8000; // the backend retries after a second

constexpr int settleMs = 100; // time for the messages in flight to be handled
constexpr int QuietMs = 200;  // long enough for a request that should not go out to show


// A backend with a few sessions that answers like the real one for what the workspace asks.
class StubServer
{
public:
	explicit StubServer(FakeBackend& fake)
		: m_fake(fake)
	{
		fake.onRequest = [this](const QJsonObject& request) { answer(request); };
		fake.onHello = [this](const QJsonObject& hello) {
			m_fake.respondHello(hello, m_protocol);
		};
	}

	void add(const QString& id, qint64 created, const QString& cwd = u"/work"_s, const std::optional<QString>& parent = std::nullopt)
	{
		m_sessions.insert(id, sessionInfoJson(id, {.title = u"Title "_s + id, .cwd = cwd, .createdAt = created, .parent = parent}));
	}
	void addHistory(const QString& id, qint64 created, const QString& cwd = u"/work"_s, const std::optional<QString>& parent = std::nullopt)
	{
		m_sessions.insert(id, sessionInfoJson(id, {.title = u"Title "_s + id, .cwd = cwd, .createdAt = created, .live = false, .parent = parent}));
	}
	void setState(const QString& id, const QString& state)
	{
		m_sessions[id][u"status"_s] = QJsonObject{{u"state"_s, state}};
		broadcast();
	}
	void setProtocol(int protocol) { m_protocol = protocol; }

	// The to-do list the session's info carries.
	void setTodos(const QString& id, const QJsonArray& todos) { m_sessions[id][u"todos"_s] = todos; }

	// The messages an attach brings for the session.
	void setMessages(const QString& id, const QJsonArray& messages) { m_messages.insert(id, messages); }

	// The questions an attach brings for the session (pending_questions).
	void setQuestions(const QString& id, const QJsonArray& questions) { m_questions.insert(id, questions); }

	// While held, a question reply is recorded but not answered.
	void holdQuestionReply(bool hold) { m_holdQuestionReply = hold; }

	// Answers the oldest held question reply (refused when failQuestionReply is set).
	void releaseQuestionReply()
	{
		Q_ASSERT(!m_heldQuestionReplies.isEmpty());
		const QJsonObject request = m_heldQuestionReplies.takeFirst();
		if(m_questionError.isEmpty())
		{
			m_fake.respond(request, {});
		}
		else
		{
			m_fake.fail(request, m_questionError);
		}
	}

	// While set, a question reply is refused with this text and optional code.
	void failQuestionReply(const QString& error, const QString& code = {}) { m_questionError = error; m_questionCode = code; }
	void failPermissionReply(const QString& error, const QString& code = {}) { m_permissionError = error; m_permissionCode = code; }
	void failListDirs(const QString& error, const QString& code = {}) { m_listDirsError = error; m_listDirsCode = code; }

	void failAttach(const QString& sessionId, const QString& error) { m_failAttachId = sessionId; m_failAttachError = error; }
	// The base64 text get_image answers with.
	void setImageData(const QString& data) { m_imageData = data; }

	// How the stub answers shutdown: with this error, or by closing the connection.
	void failShutdown(const QString& error) { m_shutdownError = error; }
	void closeOnShutdown() { m_closeOnShutdown = true; }
	void setModels(const QJsonArray& models) { m_models = models; }
	void setConfigWarnings(const QJsonArray& warnings) { m_configWarnings = warnings; }

	// While held, a detach is recorded but neither carried out nor answered.
	void holdDetach(bool hold) { m_holdDetach = hold; }

	// While held, watch_all is recorded and left unanswered.
	void holdWatchAll(bool hold) { m_holdWatchAll = hold; }

	void releaseDetach()
	{
		m_holdDetach = false;
		const std::optional<QJsonObject> held = std::exchange(m_heldDetach, std::nullopt);
		if(held)
		{
			answer(*held);
		}
	}

	QStringList ops() const
	{
		QStringList result;
		for(const QJsonObject& request : m_fake.requests())
		{
			result.append(request.value(u"op"_s).toString());
		}
		return result;
	}

	// The requests of one kind.
	QList<QJsonObject> requests(const QString& op) const
	{
		QList<QJsonObject> result;
		for(const QJsonObject& request : m_fake.requests())
		{
			if(request.value(u"op"_s).toString() == op)
			{
				result.append(request);
			}
		}
		return result;
	}

	void broadcast() { m_fake.send({{u"kind"_s, u"sessions"_s}, {u"sessions"_s, list()}}); }

	QJsonObject info(const QString& id) const { return m_sessions.value(id); }

	void remove(const QString& id)
	{
		m_sessions.remove(id);
		broadcast();
	}

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
		if(op == u"create_session"_s)
		{
			const QString created = u"ses_new%1"_s.arg(++m_created);
			QJsonObject info = sessionInfoJson(created, {.title = u"Title "_s + created, .cwd = request.value(u"cwd"_s).toString(), .createdAt = 100 + m_created});
			if(request.contains(u"agent"_s) && request.value(u"agent"_s).isString())
			{
				info[u"agent"_s] = request.value(u"agent"_s).toString();
			}
			m_sessions.insert(created, info);
			m_fake.respond(request, {{u"session"_s, info}});
			broadcast();
		}
		else if(op == u"attach"_s)
		{
			if(id == m_failAttachId && !m_failAttachError.isEmpty())
			{
				m_fake.fail(request, m_failAttachError);
				return;
			}
			if(!m_sessions.contains(id))
			{
				m_fake.fail(request, u"no such session"_s);
				return;
			}
			QJsonObject info = m_sessions.value(id);
			info[u"live"_s] = true;
			info[u"attached"_s] = request.value(u"foreground"_s).toBool() ? 1 : 0;
			m_sessions[id] = info;
			m_fake.respond(request, {{u"snapshot"_s, QJsonObject{{u"info"_s, info},
			                                                            {u"messages"_s, m_messages.value(id)},
			                                                            {u"pending_questions"_s, m_questions.value(id)},
			                                                            {u"more_before"_s, false}}}});
			broadcast();
		}
		else if(op == u"detach"_s)
		{
			if(m_holdDetach)
			{
				m_heldDetach = request;
				return;
			}
			if(m_sessions.contains(id))
			{
				m_sessions[id][u"attached"_s] = 0;
			}
			m_fake.respond(request, {});
			broadcast();
		}
		else if(op == u"close"_s)
		{
			m_fake.respond(request, {});
			remove(id);
		}
		else if(op == u"rename"_s)
		{
			const QString name = request.value(u"name"_s).toString();
			if(name == u"taken"_s)
			{
				m_fake.fail(request, u"That name is taken."_s);
				return;
			}
			m_sessions[id][u"name"_s] = name;
			m_fake.respond(request, {{u"session"_s, m_sessions.value(id)}});
			broadcast();
		}
		else if(op == u"watch_all"_s && m_holdWatchAll)
		{
			return;
		}
		else if(op == u"list_history"_s)
		{
			const QString cwdFilter = request.value(u"cwd"_s).toString();
			const QString creatorFilter = request.value(u"creator"_s).toString();
			const QString queryFilter = request.value(u"query"_s).toString();
			const int limit = request.value(u"limit"_s).toInt(0);
			QList<QJsonObject> matching;
			for(const QJsonObject& info : m_sessions)
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
				matching.append(info);
			}
			std::ranges::sort(matching, [](const QJsonObject& a, const QJsonObject& b) {
				return a.value(u"created_at"_s).toInteger() > b.value(u"created_at"_s).toInteger();
			});
			QJsonArray array;
			for(qsizetype i = 0; i < matching.size() && (limit <= 0 || i < limit); ++i)
			{
				array.append(matching.at(i));
			}
			m_fake.respond(request, {{u"sessions"_s, array}});
		}
		else if(op == u"list_sessions"_s || op == u"watch_all"_s)
		{
			m_fake.respond(request, {{u"sessions"_s, list()}});
		}
		else if(op == u"list_dirs"_s)
		{
			if(!m_listDirsError.isEmpty())
			{
				m_fake.fail(request, m_listDirsError, m_listDirsCode);
				return;
			}
			m_fake.respond(request, {{u"path"_s, u"/work"_s}, {u"dirs"_s, QJsonArray{u"a"_s, u"b"_s}}});
		}
		else if(op == u"get_config"_s)
		{
			m_fake.respond(request,
			               {{u"config"_s,
			                 QJsonObject{{u"default_agent"_s, u"build"_s},
			                             {u"agents"_s, QJsonArray{QJsonObject{{u"name"_s, u"build"_s}, {u"description"_s, u""_s}}, QJsonObject{{u"name"_s, u"plan"_s}, {u"description"_s, u""_s}}}},
			                             {u"models"_s, m_models},
			                             {u"warnings"_s, m_configWarnings}}}});
		}
		else if(op == u"reload_config"_s)
		{
			m_fake.send({{u"kind"_s, u"config_changed"_s}, {u"warnings"_s, m_configWarnings}});
			m_fake.respond(request,
			               {{u"config"_s,
			                 QJsonObject{{u"default_agent"_s, u"build"_s},
			                             {u"agents"_s, QJsonArray{QJsonObject{{u"name"_s, u"build"_s}, {u"description"_s, u""_s}}, QJsonObject{{u"name"_s, u"plan"_s}, {u"description"_s, u""_s}}}},
			                             {u"models"_s, m_models},
			                             {u"warnings"_s, m_configWarnings}}},
			                {u"mcp_servers"_s, QJsonArray()},
			                {u"warnings"_s, m_configWarnings}});
		}
		else if(op == u"get_config_files"_s)
		{
			m_fake.respond(request, {{u"config_dir"_s, u"/work/.ref"_s},
			                         {u"files"_s, QJsonArray()},
			                         {u"selected_file"_s, u"ref.json"_s},
			                         {u"raw_text"_s, u"{\n}\n"_s},
			                         {u"parsed"_s, QJsonObject()}});
		}
		else if(op == u"mcp_status"_s)
		{
			m_fake.respond(request, {{u"servers"_s, QJsonArray()}});
		}
		else if(op == u"set_agent"_s)
		{
			m_sessions[id][u"agent"_s] = request.value(u"agent"_s);
			m_fake.respond(request, {{u"session"_s, m_sessions.value(id)}});
		}
		else if(op == u"set_model"_s)
		{
			m_sessions[id][u"model"_s] = request.value(u"model"_s).toObject();
			m_fake.respond(request, {{u"session"_s, m_sessions.value(id)}});
		}
		else if(op == u"set_variant"_s)
		{
			QJsonObject model = m_sessions[id].value(u"model"_s).toObject();
			const QString v = request.value(u"variant"_s).toString();
			if(v.isEmpty())
			{
				model.remove(u"variant"_s);
			}
			else
			{
				model.insert(u"variant"_s, v);
			}
			m_sessions[id][u"model"_s] = model;
			m_fake.respond(request, {{u"session"_s, m_sessions.value(id)}});
		}
		else if(op == u"undo"_s)
		{
			const QJsonObject image{{u"mime"_s, u"image/png"_s}, {u"data"_s, u"YWJj"_s}, {u"name"_s, u"shot.png"_s}};
			m_fake.respond(request, {{u"text"_s, u"take this back"_s}, {u"images"_s, QJsonArray{image}}});
		}
		else if(op == u"question_reply"_s && m_holdQuestionReply)
		{
			m_heldQuestionReplies.append(request);
		}
		else if(op == u"question_reply"_s && !m_questionError.isEmpty())
		{
			m_fake.fail(request, m_questionError, m_questionCode);
		}
		else if(op == u"permission_reply"_s && !m_permissionError.isEmpty())
		{
			m_fake.fail(request, m_permissionError, m_permissionCode);
		}
		else if(op == u"get_image"_s)
		{
			m_fake.respond(request, {{u"image"_s, QJsonObject{{u"mime"_s, u"image/png"_s}, {u"data"_s, m_imageData}}}});
		}
		else if(op == u"shutdown"_s && m_closeOnShutdown)
		{
			m_fake.socket()->close();
		}
		else if(op == u"shutdown"_s && !m_shutdownError.isEmpty())
		{
			m_fake.fail(request, m_shutdownError);
		}
		else if(op == u"shutdown"_s)
		{
			m_fake.respond(request, {});
		}
		else
		{
			m_fake.respond(request, {});
		}
	}

	FakeBackend& m_fake;
	QMap<QString, QJsonObject> m_sessions;
	QMap<QString, QJsonArray> m_messages;
	QMap<QString, QJsonArray> m_questions;
	bool m_holdQuestionReply = false;
	QList<QJsonObject> m_heldQuestionReplies;
	QString m_questionError;
	QString m_questionCode;
	QString m_permissionError;
	QString m_permissionCode;
	QString m_listDirsError;
	QString m_listDirsCode;
	QString m_shutdownError;
	QString m_failAttachId;
	QString m_failAttachError;
	QString m_imageData;
	bool m_closeOnShutdown = false;
	int m_created = 0;
	int m_protocol = refgui::proto::ProtocolVersion;
	bool m_holdDetach = false;
	bool m_holdWatchAll = false;
	std::optional<QJsonObject> m_heldDetach;
	QJsonArray m_models;
	QJsonArray m_configWarnings;
};

// An assistant message whose only tool call waits for the user's permission.
QJsonArray awaitingPermission()
{
	const QJsonObject tool{{u"type"_s, u"tool"_s},
	                       {u"call_id"_s, u"call_1"_s},
	                       {u"name"_s, u"edit"_s},
	                       {u"input"_s, QJsonObject()},
	                       {u"state"_s, u"awaiting_permission"_s}};
	return {QJsonObject{{u"id"_s, u"msg_1"_s}, {u"role"_s, u"assistant"_s}, {u"time"_s, 1}, {u"parts"_s, QJsonArray{tool}}}};
}

// The model asks a question with two options (the first is the default) and a custom answer.
QJsonObject questionRequest()
{
	const QJsonArray options{QJsonObject{{u"label"_s, u"Yes"_s}, {u"description"_s, u"Do it"_s}, {u"default"_s, true}},
	                         QJsonObject{{u"label"_s, u"No"_s}, {u"description"_s, u"Leave it"_s}, {u"default"_s, false}}};
	const QJsonObject question{{u"question"_s, u"Go on?"_s}, {u"header"_s, u"Go"_s}, {u"options"_s, options}, {u"multiple"_s, false}, {u"custom"_s, true}};
	const QJsonObject request{{u"session_id"_s, u"ses_a"_s},
	                          {u"message_id"_s, u"msg_1"_s},
	                          {u"call_id"_s, u"call_q"_s},
	                          {u"questions"_s, QJsonArray{question}},
	                          {u"time"_s, 1}};
	return request;
}

QJsonObject questionEvent()
{
	return {{u"kind"_s, u"event"_s},
	        {u"session_id"_s, u"ses_a"_s},
	        {u"event"_s, QJsonObject{{u"type"_s, u"question"_s}, {u"request"_s, questionRequest()}}}};
}

// What the sidebar row of a session says (IsCurrentRole, IsTileRole).
bool rowFlag(Workspace& workspace, const QString& id, refgui::workspace::SessionListModel::Role role)
{
	const refgui::workspace::SessionListModel* model = workspace.sessions();
	for(int row = 0; row < model->rowCount(); ++row)
	{
		if(model->data(model->index(row, 0), refgui::workspace::SessionListModel::SessionIdRole).toString() == id)
		{
			return model->data(model->index(row, 0), role).toBool();
		}
	}
	return false;
}

struct Fixture
{
	explicit Fixture(int protocol = refgui::proto::ProtocolVersion, bool withSecond = false, const QJsonArray& messagesOfA = {}, const std::function<void(StubServer&)>& prepare = {})
	{
		stub.setProtocol(protocol);
		stub.add(u"ses_a"_s, 1);
		stub.setMessages(u"ses_a"_s, messagesOfA);
		if(withSecond)
		{
			stub.add(u"ses_b"_s, 2);
		}
		if(prepare)
		{
			prepare(stub);
		}
		refgui::cli::LaunchOptions options;
		options.directory = u"/work"_s;
		options.source = Backend::Mode::Remote{{fake.url(), u"token"_s}};
		options.start = {.kind = refgui::cli::SessionStart::Kind::Attach, .sessionId = u"ses_a"_s};
		workspace = std::make_unique<Workspace>(options, dir.filePath(u"gui.json"_s));
		Workspace::setInstance(workspace.get());
		QTRY_VERIFY(workspace->current() != nullptr);
	}

	~Fixture() { Workspace::setInstance(nullptr); }

	QTemporaryDir dir;
	FakeBackend fake;
	StubServer stub{fake};
	std::unique_ptr<Workspace> workspace;
};

} // namespace

class TestWorkspace : public QObject
{
	Q_OBJECT

private slots:
	void opensTheSessionFromTheCommandLine();
	void firstConnectionAsksForTheListsBeforeAttaching();
	void newSessionBecomesCurrentAndLetsGoOfTheOld();
	void openingAnotherSessionSwitches();
	void closingTheCurrentSessionOpensAnother();
	void closingTheLastSessionLeavesWorkspaceEmpty();
	void startsEmptyByDefaultAndNewSessionUpdatesCwd();
	void continueOpensNewestOrCreatesFallback();
	void detachCurrentMovesSessionToBackgroundAndSplitFocusEnablesCommands();
	void renameReportsToTheCallback();
	void failedRequestsBecomeToasts();
	void slashCommandsRunTheirHandlers();
	void cyclesAgents();
	void serverToastsArePassedOn();
	void splitViewWatchesAndComesBack();
	void zoomUpgradesTheTileToTheForeground();
	void unzoomKeepsTheTileWhileItsForegroundIsGivenUp();
	void aClosedTileKeepsItsSessionUntilItHasLeft();
	void closingTheCurrentTileSendsNoAttach();
	void aRevivedTileLeavesOnItsOwnTimer();
	void anotherTileGetsTheFocusWhenTheFocusedOneCloses();
	void theSidebarMarksTheFocusedTileAndTheTiles();
	void permissionKeysComeAfterTheGuardAndTakeTheTyping();
	void closingTheCurrentSessionLetsGoOfItBeforeItIsDeleted();
	void theSessionInFrontIsTheCurrentOneOrTheFocusedTile();
	void destroyingTheWorkspaceWhileLeavingSplitViewIsSafe();
	void destroyingTheWorkspaceWithADetachOutIsSafe();
	void questionKeysComeAfterTheGuardAndAnswerTheCard();
	void aQuestionAnsweredElsewhereGoesQuietly();
	void aCodedQuestionAnsweredElsewhereGoesQuietly();
	void aPermissionAnsweredElsewhereGoesQuietly();
	void aPermissionSnapshotPreviewIsAttachedToTheToolRow();
	void aRefusedAnswerKeepsTheCardAndSaysWhy();
	void aLostConnectionFreesTheCardAndItCanBeSentAgain();
	void aReplyThatFailsLateLeavesTheNextQuestionAlone();
	void gettingTheKeyboardStartsTheGuardAgain();
	void theTodoListFollowsTheSessionInfo();
	void typingACustomAnswerTakesTheKeysAwayFromTheCard();
	void sessionsHaveTheMethodsCommandsCall();
	void undoPutsThePromptBackIntoTheInputBox();
	void oldBackendOffersARestart();
	void refusedRestartIsAToast();
	void invalidImageDataIsAnError();
	void restartClosingTheConnectionIsQuiet();
	void commandsFollowTheState();
	void listDirsPassesErrorCodeToTheCallback();
	void infoResolvesRootTaskFromLiveSessions();
	void leavingSplitWhileZoomedPromotesZoomedSession();
	void openingNonTileInSplitViewExitsSplitAndShowsTarget();
	void requestCloseSessionConfirmsWhenBusyAndClosesWhenIdle();
	void reloadAndConfigCommandsUpdateStateAndDeduplicateBroadcast();
	void directArgumentsForModelsAgentsVariantsSessionsAndMultiClose();
	void closingOrRemovingZoomedTileClearsZoomAndShowsRemainingTiles();
	void zoomingTileRemovedBeforeAttachFinishesLeavesNoDanglingZoom();
	void historyFiltersPersistAndCreatorFilterIsEphemeralAndAllImagesCollectsOrder();
};

void TestWorkspace::opensTheSessionFromTheCommandLine()
{
	Fixture f;

	QCOMPARE(f.workspace->currentId(), u"ses_a"_s);
	QCOMPARE(f.workspace->connectionState(), u"connected"_s);
	const auto attaches = f.stub.requests(u"attach"_s);
	QCOMPARE(attaches.size(), 1);
	QVERIFY(attaches.first().value(u"foreground"_s).toBool());
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 1);
	QTRY_VERIFY(!f.workspace->config()->agents().isEmpty());
}

// The lists and the configuration are asked for first; then the session from the command line is attached.
void TestWorkspace::firstConnectionAsksForTheListsBeforeAttaching()
{
	Fixture f;
	QTRY_VERIFY(f.workspace->current()->isAttached());

	QStringList ops;
	for(const QJsonObject& request : f.fake.requests())
	{
		ops.append(request.value(u"op"_s).toString());
	}
	const QStringList expected{u"hello"_s, u"list_sessions"_s, u"list_history"_s, u"get_config"_s, u"mcp_status"_s, u"attach"_s};
	QCOMPARE(ops.mid(0, expected.size()), expected);
}

void TestWorkspace::newSessionBecomesCurrentAndLetsGoOfTheOld()
{
	Fixture f;
	QSignalSpy changed(f.workspace.get(), &Workspace::currentChanged);

	f.workspace->newSession(u"/other"_s);

	QTRY_COMPARE(f.workspace->currentId(), u"ses_new1"_s);
	QVERIFY(changed.count() >= 1);
	QTRY_COMPARE(f.stub.requests(u"detach"_s).size(), 1);
	QCOMPARE(f.stub.requests(u"detach"_s).first().value(u"session_id"_s).toString(), u"ses_a"_s);
	QCOMPARE(f.stub.requests(u"create_session"_s).first().value(u"cwd"_s).toString(), u"/other"_s);
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 2);
}

void TestWorkspace::openingAnotherSessionSwitches()
{
	Fixture f(2, true);

	f.workspace->openSession(u"ses_b"_s);

	QTRY_COMPARE(f.workspace->currentId(), u"ses_b"_s);
	// The window keeps the chat session of the one it left.
	QVERIFY(f.workspace->session(u"ses_a"_s) != nullptr);
	QTRY_VERIFY(!f.workspace->session(u"ses_a"_s)->isAttached());
}

void TestWorkspace::closingTheCurrentSessionOpensAnother()
{
	Fixture f(2, true);
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 2);

	f.workspace->closeSession(u"ses_a"_s);

	QTRY_COMPARE(f.workspace->currentId(), u"ses_b"_s);
	QCOMPARE(f.stub.requests(u"close"_s).size(), 1);
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 1);
}

void TestWorkspace::closingTheLastSessionLeavesWorkspaceEmpty()
{
	Fixture f;
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 1);

	f.workspace->closeSession(u"ses_a"_s);

	QTRY_VERIFY(f.workspace->current() == nullptr);
	QCOMPARE(f.workspace->currentId(), QString());
	QTest::qWait(QuietMs);
	QCOMPARE(f.stub.requests(u"create_session"_s).size(), 0);
}

void TestWorkspace::startsEmptyByDefaultAndNewSessionUpdatesCwd()
{
	QTemporaryDir dir;
	FakeBackend fake;
	StubServer stub(fake);
	stub.add(u"ses_existing"_s, 1, u"/work"_s);

	refgui::cli::LaunchOptions options;
	options.directory = u"/initial"_s;
	options.source = Backend::Mode::Remote{{fake.url(), u"token"_s}};
	options.start = {.kind = refgui::cli::SessionStart::Kind::None,
	                 .agent = u"plan"_s,
	                 .model = refgui::proto::ModelRef{u"anthropic"_s, u"claude-sonnet-4"_s, u"high"_s}};
	Workspace workspace(options, dir.filePath(u"gui.json"_s));
	Workspace::setInstance(&workspace);
	QSignalSpy opened(&workspace, &Workspace::opened);
	QSignalSpy cwdChanged(&workspace, &Workspace::cwdChanged);

	QTRY_COMPARE(opened.count(), 1);
	QCOMPARE(opened.first().first().toString(), QString());
	QCOMPARE(workspace.current(), nullptr);
	QCOMPARE(workspace.currentId(), QString());
	QCOMPARE(workspace.cwd(), u"/initial"_s);
	QCOMPARE(stub.requests(u"create_session"_s).size(), 0);
	QCOMPARE(stub.requests(u"attach"_s).size(), 0);

	// Manual newSession in a chosen folder updates m_cwd and passes CLI agent/model defaults.
	workspace.newSession(u"/chosen/project"_s);
	QTRY_COMPARE(workspace.currentId(), u"ses_new1"_s);
	QCOMPARE(workspace.cwd(), u"/chosen/project"_s);
	QCOMPARE(cwdChanged.count(), 1);
	const QJsonObject firstCreate = stub.requests(u"create_session"_s).first();
	QCOMPARE(firstCreate.value(u"cwd"_s).toString(), u"/chosen/project"_s);
	QCOMPARE(firstCreate.value(u"agent"_s).toString(), u"plan"_s);
	QCOMPARE(firstCreate.value(u"model"_s).toObject().value(u"provider"_s).toString(), u"anthropic"_s);
	QCOMPARE(firstCreate.value(u"model"_s).toObject().value(u"model"_s).toString(), u"claude-sonnet-4"_s);
	QCOMPARE(firstCreate.value(u"model"_s).toObject().value(u"variant"_s).toString(), u"high"_s);

	// Subsequent default newSession uses the last chosen project directory.
	workspace.newSession();
	QTRY_COMPARE(workspace.currentId(), u"ses_new2"_s);
	QCOMPARE(stub.requests(u"create_session"_s).last().value(u"cwd"_s).toString(), u"/chosen/project"_s);
	Workspace::setInstance(nullptr);
}

void TestWorkspace::continueOpensNewestOrCreatesFallback()
{
	QTemporaryDir dir;
	FakeBackend fake;
	StubServer stub(fake);
	stub.add(u"ses_old"_s, 10, u"/work"_s);
	stub.add(u"ses_newest"_s, 20, u"/work"_s);

	{
		refgui::cli::LaunchOptions options;
		options.directory = u"/work"_s;
		options.source = Backend::Mode::Remote{{fake.url(), u"token"_s}};
		options.start = {.kind = refgui::cli::SessionStart::Kind::Continue};
		Workspace workspace(options, dir.filePath(u"gui1.json"_s));
		Workspace::setInstance(&workspace);
		QSignalSpy opened(&workspace, &Workspace::opened);
		QTRY_COMPARE(opened.count(), 1);
		QCOMPARE(opened.first().first().toString(), u"ses_newest"_s);
		QCOMPARE(workspace.currentId(), u"ses_newest"_s);
		QCOMPARE(stub.requests(u"create_session"_s).size(), 0);
		Workspace::setInstance(nullptr);
	}
	{
		refgui::cli::LaunchOptions options;
		options.directory = u"/empty_dir"_s;
		options.source = Backend::Mode::Remote{{fake.url(), u"token"_s}};
		options.start = {.kind = refgui::cli::SessionStart::Kind::Continue};
		Workspace workspace(options, dir.filePath(u"gui2.json"_s));
		Workspace::setInstance(&workspace);
		QSignalSpy opened(&workspace, &Workspace::opened);
		QTRY_COMPARE(opened.count(), 1);
		QCOMPARE(opened.first().first().toString(), u"ses_new1"_s);
		QCOMPARE(workspace.currentId(), u"ses_new1"_s);
		QCOMPARE(stub.requests(u"create_session"_s).size(), 1);
		Workspace::setInstance(nullptr);
	}
}

void TestWorkspace::detachCurrentMovesSessionToBackgroundAndSplitFocusEnablesCommands()
{
	Fixture f(refgui::proto::ProtocolVersion, true);
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 2);
	const auto commandEnabled = [&f](const QString& id) {
		for(const QVariant& entry : f.workspace->commands()->matching({}, false))
		{
			if(entry.toMap().value(u"id"_s).toString() == id)
			{
				return entry.toMap().value(u"enabled"_s).toBool();
			}
		}
		return false;
	};

	// Single view: detachCurrent() detaches ses_a and clears current.
	QVERIFY(commandEnabled(u"detach"_s));
	QVERIFY(commandEnabled(u"models"_s));
	f.workspace->detachCurrent();
	QCOMPARE(f.workspace->current(), nullptr);
	QCOMPARE(f.workspace->currentId(), QString());
	QTRY_COMPARE(f.stub.requests(u"detach"_s).size(), 1);
	QCOMPARE(f.stub.requests(u"detach"_s).last().value(u"session_id"_s).toString(), u"ses_a"_s);
	QVERIFY(!commandEnabled(u"detach"_s));
	QVERIFY(!commandEnabled(u"models"_s));

	// Re-open ses_a, enter split view (ses_a hidden as current, ses_b in tiles), then /detach moves ses_a into tiles too.
	f.workspace->openSession(u"ses_a"_s);
	QTRY_COMPARE(f.workspace->currentId(), u"ses_a"_s);
	f.workspace->toggleSplit();
	QTRY_VERIFY(f.workspace->isSplitView());
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 1);
	QVERIFY(commandEnabled(u"detach"_s));

	QVERIFY(f.workspace->commands()->runSlash(u"/detach"_s));
	QCOMPARE(f.workspace->current(), nullptr);
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 2);
	QVERIFY(!commandEnabled(u"detach"_s));
	QVERIFY(!commandEnabled(u"models"_s)); // no focused tile yet

	// Focusing a tile in split view with m_current == nullptr enables session commands on that tile!
	f.workspace->focusTile(u"ses_b"_s);
	QVERIFY(commandEnabled(u"models"_s));
	QVERIFY(commandEnabled(u"rename"_s));
	QVERIFY(commandEnabled(u"close"_s));
	QVERIFY(!commandEnabled(u"detach"_s)); // unzoomed tile is already in background

	// Zooming enables detach again, and detachCurrent() unzooms back to grid.
	f.workspace->zoomTile(u"ses_b"_s);
	QTRY_COMPARE(f.workspace->zoomedId(), u"ses_b"_s);
	QVERIFY(commandEnabled(u"detach"_s));
	f.workspace->detachCurrent();
	QCOMPARE(f.workspace->zoomedId(), QString());
	QVERIFY(!commandEnabled(u"detach"_s));
}

void TestWorkspace::renameReportsToTheCallback()
{
	Fixture f;
	QJSEngine engine;
	Workspace::create(nullptr, &engine);
	const QJSValue callback = engine.evaluate(u"var answer; (function(ok, result) { answer = [ok, result]; })"_s);
	QVERIFY2(callback.isCallable(), qPrintable(callback.toString()));

	f.workspace->renameSession(u"ses_a"_s, u"taken"_s, callback);
	QTRY_VERIFY(!engine.globalObject().property(u"answer"_s).isUndefined());
	QCOMPARE(engine.globalObject().property(u"answer"_s).property(0).toBool(), false);
	QCOMPARE(engine.globalObject().property(u"answer"_s).property(1).toString(), u"That name is taken."_s);

	engine.globalObject().setProperty(u"answer"_s, QJSValue());
	f.workspace->renameSession(u"ses_a"_s, u"Mine"_s, callback);
	QTRY_VERIFY(!engine.globalObject().property(u"answer"_s).isUndefined());
	QCOMPARE(engine.globalObject().property(u"answer"_s).property(0).toBool(), true);
	QTRY_COMPARE(f.workspace->sessions()->index(0, 0).data(refgui::workspace::SessionListModel::NameRole).toString(), u"Mine"_s);
}

void TestWorkspace::listDirsPassesErrorCodeToTheCallback()
{
	Fixture f;
	QJSEngine engine;
	Workspace::create(nullptr, &engine);
	const QJSValue callback = engine.evaluate(u"var answer; (function(ok, result, code) { answer = [ok, result, code]; })"_s);
	QVERIFY2(callback.isCallable(), qPrintable(callback.toString()));

	f.stub.failListDirs(u"Directory does not exist: /missing"_s, u"not_found"_s);
	f.workspace->listDirs(u"/missing"_s, callback);
	QTRY_VERIFY(!engine.globalObject().property(u"answer"_s).isUndefined());
	QCOMPARE(engine.globalObject().property(u"answer"_s).property(0).toBool(), false);
	QCOMPARE(engine.globalObject().property(u"answer"_s).property(1).toString(), u"Directory does not exist: /missing"_s);
	QCOMPARE(engine.globalObject().property(u"answer"_s).property(2).toString(), u"not_found"_s);

	engine.globalObject().setProperty(u"answer"_s, QJSValue());
	f.stub.failListDirs({});
	f.workspace->listDirs(u"/work"_s, callback);
	QTRY_VERIFY(!engine.globalObject().property(u"answer"_s).isUndefined());
	QCOMPARE(engine.globalObject().property(u"answer"_s).property(0).toBool(), true);
	QCOMPARE(engine.globalObject().property(u"answer"_s).property(1).property(u"path"_s).toString(), u"/work"_s);
	QCOMPARE(engine.globalObject().property(u"answer"_s).property(2).toString(), QString());
}

void TestWorkspace::failedRequestsBecomeToasts()
{
	Fixture f;
	QSignalSpy toasts(f.workspace.get(), &Workspace::toastRequested);

	f.workspace->renameSession(u"ses_a"_s, u"taken"_s);

	QTRY_COMPARE(toasts.count(), 1);
	QCOMPARE(toasts.first().at(0).toString(), u"error"_s);
	QCOMPARE(toasts.first().at(1).toString(), u"That name is taken."_s);
}

void TestWorkspace::slashCommandsRunTheirHandlers()
{
	Fixture f;
	auto* commands = f.workspace->commands();

	QVERIFY(commands->runSlash(u"/rename Docs"_s));
	QTRY_COMPARE(f.stub.requests(u"rename"_s).size(), 1);
	QCOMPARE(f.stub.requests(u"rename"_s).first().value(u"name"_s).toString(), u"Docs"_s);

	QVERIFY(commands->runSlash(u"/models"_s));
	QCOMPARE(f.workspace->dialog(), u"models"_s);
	QCOMPARE(f.workspace->dialogArgs().value(u"sessionId"_s).toString(), u"ses_a"_s);
	f.workspace->closeDialog();
	QCOMPARE(f.workspace->dialog(), QString());

	// /variant without arguments opens the variants dialog; setVariant() without arguments rotates; clearVariant() sends empty string.
	QVERIFY(commands->run(u"variant"_s));
	QCOMPARE(f.workspace->dialog(), u"variants"_s);
	f.workspace->closeDialog();
	f.workspace->setVariant(u"ses_a"_s);
	QTRY_COMPARE(f.stub.requests(u"set_variant"_s).size(), 1);
	QVERIFY(!f.stub.requests(u"set_variant"_s).first().contains(u"variant"_s) || f.stub.requests(u"set_variant"_s).first().value(u"variant"_s).isNull());
	QVERIFY(!commands->runSlash(u"/no-such-command"_s));
	QVERIFY(commands->run(u"sidebar"_s));
	QVERIFY(!f.workspace->settings()->sidebarVisible());
}

void TestWorkspace::cyclesAgents()
{
	Fixture f;
	QTRY_VERIFY(!f.workspace->config()->agents().isEmpty());

	f.workspace->cycleAgent(u"ses_a"_s, 1);
	QTRY_COMPARE(f.stub.requests(u"set_agent"_s).size(), 1);
	QCOMPARE(f.stub.requests(u"set_agent"_s).first().value(u"agent"_s).toString(), u"plan"_s);

	// The list wraps around, in both directions.
	QTRY_COMPARE(f.workspace->sessions()->index(0, 0).data(refgui::workspace::SessionListModel::AgentRole).toString(), u"plan"_s);
	f.workspace->cycleAgent(u"ses_a"_s, 1);
	QTRY_COMPARE(f.stub.requests(u"set_agent"_s).size(), 2);
	QCOMPARE(f.stub.requests(u"set_agent"_s).last().value(u"agent"_s).toString(), u"build"_s);
	QTRY_COMPARE(f.workspace->sessions()->index(0, 0).data(refgui::workspace::SessionListModel::AgentRole).toString(), u"build"_s);
	f.workspace->cycleAgent(u"ses_a"_s, -1);
	QTRY_COMPARE(f.stub.requests(u"set_agent"_s).size(), 3);
	QCOMPARE(f.stub.requests(u"set_agent"_s).last().value(u"agent"_s).toString(), u"plan"_s);
}

void TestWorkspace::serverToastsArePassedOn()
{
	Fixture f;
	QSignalSpy toasts(f.workspace.get(), &Workspace::toastRequested);

	f.fake.send({{u"kind"_s, u"toast"_s}, {u"level"_s, u"warn"_s}, {u"text"_s, u"Careful"_s}});

	QTRY_COMPARE(toasts.count(), 1);
	QCOMPARE(toasts.first().at(0).toString(), u"warn"_s);
	QCOMPARE(toasts.first().at(1).toString(), u"Careful"_s);
}

void TestWorkspace::splitViewWatchesAndComesBack()
{
	Fixture f(2, true);
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 2);

	f.workspace->toggleSplit();

	QTRY_VERIFY(f.workspace->isSplitView());
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 1);
	QTRY_COMPARE(f.stub.requests(u"watch_all"_s).size(), 1);
	QVERIFY(f.stub.requests(u"watch_all"_s).first().value(u"enabled"_s).toBool());
	// The foreground session stays hidden; the background session is attached as a watcher.
	QVERIFY(f.workspace->current()->isForeground());
	QCOMPARE(f.stub.requests(u"detach"_s).size(), 0);
	QTRY_VERIFY(f.workspace->session(u"ses_b"_s)->isAttached());
	QCOMPARE(f.stub.requests(u"attach"_s).last().value(u"session_id"_s).toString(), u"ses_b"_s);
	QVERIFY(!f.stub.requests(u"attach"_s).last().value(u"foreground"_s).toBool());
	QCOMPARE(f.workspace->focusedTileId(), QString());
	QCOMPARE(f.workspace->visibleSessionIds(), (QStringList{u"ses_b"_s}));
	QCOMPARE(f.workspace->tileStage()->placed(), 1);

	f.workspace->focusTile(u"ses_b"_s);
	QCOMPARE(f.workspace->focusedTileId(), u"ses_b"_s);
	f.workspace->focusTile({});
	QCOMPARE(f.workspace->focusedTileId(), QString());

	f.workspace->toggleSplit();

	QTRY_VERIFY(!f.workspace->isSplitView());
	QCOMPARE(f.workspace->tiles()->rowCount(), 0);
	// The stage lets the tiles go after their exit animation.
	QCOMPARE(f.workspace->tileStage()->placed(), 0);
	QTRY_COMPARE(f.workspace->tileStage()->rowCount(), 0);
	QTRY_COMPARE(f.stub.requests(u"watch_all"_s).size(), 2);
	QVERIFY(!f.stub.requests(u"watch_all"_s).last().value(u"enabled"_s).toBool());
	QTRY_VERIFY(!f.workspace->session(u"ses_b"_s)->isAttached());
	QCOMPARE(f.workspace->visibleSessionIds(), (QStringList{u"ses_a"_s}));
}

void TestWorkspace::zoomUpgradesTheTileToTheForeground()
{
	Fixture f(2, true);
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 2);
	f.workspace->toggleSplit();
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 1);
	QTRY_VERIFY(f.workspace->session(u"ses_b"_s)->isAttached());

	f.workspace->zoomTile(u"ses_b"_s);

	QCOMPARE(f.workspace->zoomedId(), u"ses_b"_s);
	QCOMPARE(f.workspace->visibleSessionIds(), (QStringList{u"ses_b"_s}));
	QTRY_VERIFY(f.stub.requests(u"attach"_s).last().value(u"foreground"_s).toBool());
	QCOMPARE(f.stub.requests(u"attach"_s).last().value(u"session_id"_s).toString(), u"ses_b"_s);

	f.workspace->unzoom();
	QCOMPARE(f.workspace->zoomedId(), QString());
	QTRY_COMPARE(f.stub.requests(u"detach"_s).size(), 1);
	QCOMPARE(f.stub.requests(u"detach"_s).last().value(u"session_id"_s).toString(), u"ses_b"_s);
	QCOMPARE(f.workspace->visibleSessionIds(), (QStringList{u"ses_b"_s}));
}

// The backend still counts the unzoomed session as attached in the foreground until it has answered the detach;
// a sessions update in that time must not take the tile away.
void TestWorkspace::unzoomKeepsTheTileWhileItsForegroundIsGivenUp()
{
	Fixture f(2, true);
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 2);
	f.workspace->toggleSplit();
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 1);
	QTRY_VERIFY(f.workspace->session(u"ses_b"_s)->isAttached());
	f.workspace->zoomTile(u"ses_b"_s);
	QTRY_VERIFY(f.stub.requests(u"attach"_s).last().value(u"foreground"_s).toBool());
	f.stub.broadcast(); // ses_b counts as attached
	QTest::qWait(settleMs);

	f.stub.holdDetach(true);
	f.workspace->unzoom();
	QTRY_COMPARE(f.stub.requests(u"detach"_s).size(), 1);
	QCOMPARE(f.stub.requests(u"detach"_s).last().value(u"session_id"_s).toString(), u"ses_b"_s);
	f.stub.broadcast(); // still counts as attached
	QTest::qWait(settleMs);

	QCOMPARE(f.workspace->tiles()->rowCount(), 1);
	QCOMPARE(f.workspace->visibleSessionIds(), (QStringList{u"ses_b"_s}));
	QVERIFY(f.workspace->session(u"ses_b"_s)->isAttached());

	f.stub.releaseDetach();
	QTRY_COMPARE(f.workspace->visibleSessionIds(), (QStringList{u"ses_b"_s}));
	QTest::qWait(settleMs);
	QCOMPARE(f.workspace->tiles()->rowCount(), 1);
}

void TestWorkspace::aClosedTileKeepsItsSessionUntilItHasLeft()
{
	Fixture f(2, true);
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 2);
	const int lingerMs = 1000;
	f.workspace->tileStage()->setLingerMs(lingerMs);
	f.workspace->toggleSplit();
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 1);
	QPointer<ChatSession> closed = f.workspace->session(u"ses_b"_s);
	QVERIFY(closed != nullptr);

	f.workspace->closeSession(u"ses_b"_s);

	QTRY_COMPARE(f.workspace->tileStage()->placed(), 0);
	QCOMPARE(f.workspace->tileStage()->rowCount(), 1); // the closed tile is still on stage
	QVERIFY(!closed.isNull());
	QTRY_COMPARE_WITH_TIMEOUT(f.workspace->tileStage()->rowCount(), 0, 2 * lingerMs);
	QTRY_VERIFY(closed.isNull());
}

void TestWorkspace::closingTheCurrentTileSendsNoAttach()
{
	Fixture f(2, true);

	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 2);
	f.workspace->toggleSplit();
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 1);
	QTRY_VERIFY(f.workspace->session(u"ses_b"_s)->isAttached());
	QSignalSpy toasts(f.workspace.get(), &Workspace::toastRequested);
	const qsizetype attachesBefore = f.stub.requests(u"attach"_s).size();

	f.workspace->closeSession(u"ses_a"_s);

	QTRY_VERIFY(f.workspace->current() == nullptr);
	QCOMPARE(f.workspace->currentId(), QString());
	QCOMPARE(f.workspace->tiles()->rowCount(), 1);
	QTest::qWait(QuietMs);
	const QList<QJsonObject> attaches = f.stub.requests(u"attach"_s);
	QCOMPARE(attaches.size(), attachesBefore);
	QCOMPARE(toasts.count(), 0);
}

void TestWorkspace::aRevivedTileLeavesOnItsOwnTimer()
{
	Fixture f;
	TileModel tiles;
	TileStage stage(&tiles);
	const int lingerMs = 400;
	stage.setLingerMs(lingerMs);
	ChatSession session(f.workspace->connection().backend(), *f.workspace->config(), u"ses_x"_s);
	tiles.setTiles({&session});
	tiles.setTiles({}); // leaves
	QTest::qWait(lingerMs / 2);
	tiles.setTiles({&session}); // comes back
	tiles.setTiles({}); // leaves again, later than the first time

	// The timer of the first leave is over; it must not take the second one with it.
	QTest::qWait(lingerMs / 2 + lingerMs / 4);
	QCOMPARE(stage.rowCount(), 1);
	QTRY_COMPARE(stage.rowCount(), 0);
}

void TestWorkspace::anotherTileGetsTheFocusWhenTheFocusedOneCloses()
{
	Fixture f(2, true, {}, [](StubServer& stub) { stub.add(u"ses_c"_s, 3); });
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 3);
	f.workspace->toggleSplit();
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 2);
	f.workspace->focusTile(u"ses_b"_s);
	QCOMPARE(f.workspace->focusedTileId(), u"ses_b"_s);

	f.workspace->closeSession(u"ses_b"_s);

	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 1);
	QCOMPARE(f.workspace->focusedTileId(), QString());
}

// The highlighted row is the session in front: in split view the one whose tile has the focus, however it got it.
void TestWorkspace::theSidebarMarksTheFocusedTileAndTheTiles()
{
	using Model = refgui::workspace::SessionListModel;
	Fixture f(2, true);
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 2);
	QVERIFY(rowFlag(*f.workspace, u"ses_a"_s, Model::IsCurrentRole));
	QVERIFY(!rowFlag(*f.workspace, u"ses_a"_s, Model::IsTileRole));

	f.workspace->toggleSplit();
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 1);

	QVERIFY(rowFlag(*f.workspace, u"ses_a"_s, Model::IsCurrentRole));
	QVERIFY(!rowFlag(*f.workspace, u"ses_a"_s, Model::IsTileRole));
	QVERIFY(rowFlag(*f.workspace, u"ses_b"_s, Model::IsTileRole));

	f.workspace->focusTile(u"ses_b"_s); // what a click or Alt+arrow does
	QVERIFY(rowFlag(*f.workspace, u"ses_b"_s, Model::IsCurrentRole));
	QVERIFY(!rowFlag(*f.workspace, u"ses_a"_s, Model::IsCurrentRole));
	QCOMPARE(f.workspace->currentId(), u"ses_a"_s); // the session that comes back when split view ends

	f.workspace->focusTile({});
	QVERIFY(rowFlag(*f.workspace, u"ses_a"_s, Model::IsCurrentRole));
	QVERIFY(!rowFlag(*f.workspace, u"ses_b"_s, Model::IsCurrentRole));

	f.workspace->toggleSplit();
	QTRY_VERIFY(!f.workspace->isSplitView());
	QTRY_COMPARE(f.stub.requests(u"watch_all"_s).size(), 2); // the requests of leaving are answered
	QTRY_VERIFY(f.workspace->current()->isForeground());
	QTRY_COMPARE(f.workspace->tileStage()->rowCount(), 0);
	QVERIFY(rowFlag(*f.workspace, u"ses_a"_s, Model::IsCurrentRole));
	QVERIFY(!rowFlag(*f.workspace, u"ses_b"_s, Model::IsCurrentRole));
	QVERIFY(!rowFlag(*f.workspace, u"ses_a"_s, Model::IsTileRole));
	QVERIFY(!rowFlag(*f.workspace, u"ses_b"_s, Model::IsTileRole));
}

// As in the TUI and the WebUI: after the guard time the request owns a, Enter, s, d and Esc and all other typing;
// before it, and for shortcuts and editing keys, the input box keeps the keys.
void TestWorkspace::permissionKeysComeAfterTheGuardAndTakeTheTyping()
{
	Fixture f(2, false, awaitingPermission());
	ChatSession* session = f.workspace->current();
	QTRY_COMPARE(session->permissionCount(), 1);
	const auto replies = [&] { return f.stub.requests(u"permission_reply"_s); };

	QVERIFY(!session->answerKeysActive());
	QVERIFY(!session->answerKey(Qt::Key_A, u"a"_s, Qt::NoModifier, false)); // typed into the box
	QTRY_VERIFY(session->answerKeysActive());
	QCOMPARE(replies().size(), 0);

	QVERIFY(session->answerKey(Qt::Key_X, u"x"_s, Qt::NoModifier, false)); // not for the box
	QVERIFY(session->answerKey(Qt::Key_A, u"A"_s, Qt::ShiftModifier, false)); // shifted letters do not answer
	QVERIFY(session->answerKey(Qt::Key_A, u"a"_s, Qt::NoModifier, true) && replies().isEmpty()); // nor a held key
	QVERIFY(!session->answerKey(Qt::Key_A, u"\x01"_s, Qt::ControlModifier, false)); // Ctrl+A selects all
	QVERIFY(!session->answerKey(Qt::Key_Backspace, u"\b"_s, Qt::NoModifier, false));
	QVERIFY(!session->answerKey(Qt::Key_Left, QString(), Qt::NoModifier, false));
	QCOMPARE(replies().size(), 0);

	QVERIFY(session->answerKey(Qt::Key_S, u"s"_s, Qt::NoModifier, false));
	QTRY_COMPARE(replies().size(), 1);
	QVERIFY(replies().first().value(u"allow"_s).toBool());
	QVERIFY(replies().first().value(u"always"_s).toBool());
	QCOMPARE(replies().first().value(u"call_id"_s).toString(), u"call_1"_s);
}

// The card takes the keys by the same route and guard as a permission request.
void TestWorkspace::questionKeysComeAfterTheGuardAndAnswerTheCard()
{
	Fixture f(2);
	ChatSession* session = f.workspace->current();
	f.fake.send(questionEvent());
	QTRY_VERIFY(session->question()->isActive());
	const auto replies = [&] { return f.stub.requests(u"question_reply"_s); };

	QVERIFY(!session->answerKeysActive());
	QVERIFY(!session->answerKey(Qt::Key_Down, QString(), Qt::NoModifier, false)); // the input box has the keys meanwhile
	QTRY_VERIFY(session->answerKeysActive());

	QVERIFY(session->answerKey(Qt::Key_X, u"x"_s, Qt::NoModifier, false)); // typing is the card's
	QVERIFY(session->answerKey(Qt::Key_Return, QString(), Qt::NoModifier, true) && replies().isEmpty()); // a held key does not answer
	QVERIFY(!session->answerKey(Qt::Key_Backspace, u"\b"_s, Qt::NoModifier, false));
	QVERIFY(!session->answerKey(Qt::Key_1, u"1"_s, Qt::ControlModifier, false));
	QVERIFY(session->answerKey(Qt::Key_Down, QString(), Qt::NoModifier, false));
	QVERIFY(session->answerKey(Qt::Key_Return, QString(), Qt::NoModifier, false));

	QTRY_COMPARE(replies().size(), 1);
	QCOMPARE(replies().first().value(u"call_id"_s).toString(), u"call_q"_s);
	QCOMPARE(replies().first().value(u"answers"_s).toArray(), (QJsonArray{QJsonValue(QJsonArray{u"No"_s})}));
	QVERIFY(session->question()->isBusy()); // until the backend says it is resolved
	f.fake.send({{u"kind"_s, u"event"_s},
	             {u"session_id"_s, u"ses_a"_s},
	             {u"event"_s, QJsonObject{{u"type"_s, u"question_resolved"_s}, {u"call_id"_s, u"call_q"_s}}}});
	QTRY_VERIFY(!session->question()->isActive());
	QVERIFY(!session->answerKeysActive());
}

void TestWorkspace::aQuestionAnsweredElsewhereGoesQuietly()
{
	Fixture f(2);
	f.stub.failQuestionReply(u"No pending question with that id."_s);
	ChatSession* session = f.workspace->current();
	f.fake.send(questionEvent());
	QTRY_VERIFY(session->question()->isActive());

	session->question()->activateRow(0);

	QTRY_VERIFY(!session->question()->isActive());
	QVERIFY(session->statusText().isEmpty());
}
void TestWorkspace::aCodedQuestionAnsweredElsewhereGoesQuietly()
{
	Fixture f(2);
	f.stub.failQuestionReply(u"different server wording"_s, u"no_pending_question"_s);
	ChatSession* session = f.workspace->current();
	f.fake.send(questionEvent());
	QTRY_VERIFY(session->question()->isActive());

	session->question()->activateRow(0);

	QTRY_VERIFY(!session->question()->isActive());
	QVERIFY(session->statusText().isEmpty());
}

void TestWorkspace::aPermissionAnsweredElsewhereGoesQuietly()
{
	for(const auto& [error, code] : {std::pair{u"different wording"_s, u"no_pending_permission"_s},
	                                  std::pair{u"No pending permission request with that id."_s, QString()}})
	{
		Fixture f(2, false, awaitingPermission());
		f.stub.failPermissionReply(error, code);
		ChatSession* session = f.workspace->current();
		QTRY_COMPARE(session->permissionCount(), 1);

		session->permissionReply(u"call_1"_s, true, false);
		QTRY_COMPARE(f.stub.requests(u"permission_reply"_s).size(), 1);
		QTest::qWait(QuietMs);
		QVERIFY(session->statusText().isEmpty());
	}
}

void TestWorkspace::aPermissionSnapshotPreviewIsAttachedToTheToolRow()
{
	Fixture f(2, false, awaitingPermission());
	ChatSession* session = f.workspace->current();
	QTRY_COMPARE(session->permissionCount(), 1);
	const QJsonObject preview{{u"diff"_s, u"+new\n-old\n"_s}, {u"added"_s, 1}, {u"removed"_s, 1}, {u"truncated"_s, false}};
	const QJsonObject request{{u"session_id"_s, u"ses_a"_s},
	                          {u"message_id"_s, u"msg_1"_s},
	                          {u"call_id"_s, u"call_1"_s},
	                          {u"tool"_s, u"edit"_s},
	                          {u"purpose"_s, u"edit"_s},
	                          {u"input"_s, QJsonObject()},
	                          {u"time"_s, 1},
	                          {u"preview"_s, preview}};
	f.fake.send({{u"kind"_s, u"event"_s}, {u"session_id"_s, u"ses_a"_s}, {u"event"_s, QJsonObject{{u"type"_s, u"permission"_s}, {u"request"_s, request}}}});
	QAbstractItemModel* blocks = session->blocks();
	const int previewRole = blocks->roleNames().key("permissionPreview");
	QTRY_COMPARE(blocks->index(0, 0).data(previewRole).toMap().value(u"added"_s).toInt(), 1);
	f.fake.send({{u"kind"_s, u"event"_s}, {u"session_id"_s, u"ses_a"_s}, {u"event"_s, QJsonObject{{u"type"_s, u"permission_resolved"_s}, {u"call_id"_s, u"call_1"_s}}}});
	QTRY_VERIFY(blocks->index(0, 0).data(previewRole).toMap().isEmpty());
}

void TestWorkspace::aRefusedAnswerKeepsTheCardAndSaysWhy()
{
	Fixture f(2);
	f.stub.failQuestionReply(u"Invalid answers"_s);
	ChatSession* session = f.workspace->current();
	f.fake.send(questionEvent());
	QTRY_VERIFY(session->question()->isActive());

	session->question()->activateRow(0);

	QTRY_COMPARE(session->statusText(), u"Invalid answers"_s);
	QVERIFY(session->question()->isActive());
	QVERIFY(!session->question()->isBusy()); // it can be tried again
}

// The reply is on its way when the connection drops: the card is not stuck, keeps its picks, and the question comes
// back with the snapshot of the new attach.
void TestWorkspace::aLostConnectionFreesTheCardAndItCanBeSentAgain()
{
	Fixture f(2);
	ChatSession* session = f.workspace->current();
	f.fake.send(questionEvent());
	QTRY_VERIFY(session->question()->isActive());
	const auto replies = [&] { return f.stub.requests(u"question_reply"_s); };
	f.stub.holdQuestionReply(true);
	session->question()->activateRow(1); // "No"
	QTRY_COMPARE(replies().size(), 1);
	QVERIFY(session->question()->isBusy());
	f.stub.setQuestions(u"ses_a"_s, QJsonArray{questionRequest()});
	const qsizetype attaches = f.stub.requests(u"attach"_s).size();

	f.fake.socket()->close();

	QTRY_VERIFY_WITH_TIMEOUT(!session->question()->isBusy(), ReconnectTimeoutMs);
	QVERIFY(session->question()->isActive());
	QCOMPARE(session->question()->answers(), (QList<QStringList>{{u"No"_s}})); // the pick stays
	QVERIFY(!session->statusText().isEmpty());
	QTRY_COMPARE_WITH_TIMEOUT(f.stub.requests(u"attach"_s).size(), attaches + 1, ReconnectTimeoutMs);
	QTRY_VERIFY(session->isReady());
	QVERIFY(session->question()->isActive()); // the snapshot brings the same question back, nothing is reset
	QCOMPARE(session->question()->answers(), (QList<QStringList>{{u"No"_s}}));

	f.stub.holdQuestionReply(false);
	session->question()->activateRow(1);

	QTRY_COMPARE(replies().size(), 2);
	QCOMPARE(replies().last().value(u"answers"_s).toArray(), (QJsonArray{QJsonValue(QJsonArray{u"No"_s})}));
	QCOMPARE(replies().last().value(u"call_id"_s).toString(), u"call_q"_s);
}

// Reply A fails after the card has moved on to question B, which is being sent: B stays locked.
void TestWorkspace::aReplyThatFailsLateLeavesTheNextQuestionAlone()
{
	Fixture f(2);
	ChatSession* session = f.workspace->current();
	f.fake.send(questionEvent());
	QTRY_VERIFY(session->question()->isActive());
	f.stub.failQuestionReply(u"Invalid answers"_s);
	f.stub.holdQuestionReply(true); // the refusal comes only when released
	session->question()->activateRow(0);
	QTRY_COMPARE(f.stub.requests(u"question_reply"_s).size(), 1);

	// The question goes away and another one takes its place; its answer is on its way, too.
	f.fake.send({{u"kind"_s, u"event"_s},
	             {u"session_id"_s, u"ses_a"_s},
	             {u"event"_s, QJsonObject{{u"type"_s, u"question_resolved"_s}, {u"call_id"_s, u"call_q"_s}}}});
	QTRY_VERIFY(!session->question()->isActive());
	QJsonObject next = questionRequest();
	next[u"call_id"_s] = u"call_r"_s;
	f.fake.send({{u"kind"_s, u"event"_s},
	             {u"session_id"_s, u"ses_a"_s},
	             {u"event"_s, QJsonObject{{u"type"_s, u"question"_s}, {u"request"_s, next}}}});
	QTRY_COMPARE(session->question()->callId(), u"call_r"_s);
	session->question()->activateRow(0);
	QTRY_COMPARE(f.stub.requests(u"question_reply"_s).size(), 2);
	QVERIFY(session->question()->isBusy());

	f.stub.releaseQuestionReply(); // the refusal of the first one

	QTRY_COMPARE(session->statusText(), u"Invalid answers"_s);
	QCOMPARE(session->question()->callId(), u"call_r"_s);
	QVERIFY(session->question()->isBusy()); // still waiting for its own answer
	f.stub.releaseQuestionReply();
	QTRY_VERIFY(!session->question()->isBusy());
}

// The guard time starts again when the pane gets the keyboard, for a question and for a permission request.
void TestWorkspace::gettingTheKeyboardStartsTheGuardAgain()
{
	Fixture f(2, false, awaitingPermission());
	ChatSession* session = f.workspace->current();
	QTRY_VERIFY(session->answerKeysActive());

	session->restartAnswerGuard();

	QVERIFY(!session->answerKeysActive());
	QVERIFY(!session->answerKey(Qt::Key_A, u"a"_s, Qt::NoModifier, false));
	QTRY_VERIFY(session->answerKeysActive());

	f.fake.send(questionEvent());
	QTRY_VERIFY(session->question()->isActive());
	QTRY_VERIFY(session->answerKeysActive());
	session->restartAnswerGuard();
	QVERIFY(!session->answerKeysActive());
	QVERIFY(!session->answerKey(Qt::Key_Return, QString(), Qt::NoModifier, false)); // the default is not taken
	QCOMPARE(f.stub.requests(u"question_reply"_s).size(), 0);
	QTRY_VERIFY(session->answerKeysActive());
}

// The status line item and its popup read these properties; the counting is the terminal UI's.
void TestWorkspace::theTodoListFollowsTheSessionInfo()
{
	Fixture f;
	ChatSession* session = f.workspace->current();
	QCOMPARE(session->todoTotal(), 0);
	QVERIFY(session->todoCurrent().isEmpty());
	const auto sendTodos = [&](const QJsonArray& todos) {
		QJsonObject info = f.stub.info(u"ses_a"_s);
		info[u"todos"_s] = todos;
		f.fake.send({{u"kind"_s, u"event"_s}, {u"session_id"_s, u"ses_a"_s}, {u"event"_s, QJsonObject{{u"type"_s, u"info"_s}, {u"info"_s, info}}}});
	};
	const auto item = [](const QString& content, const QString& status) { return QJsonObject{{u"content"_s, content}, {u"status"_s, status}}; };
	QSignalSpy changed(session, &ChatSession::infoChanged);

	sendTodos({item(u"Read the parser"_s, u"completed"_s),
	           item(u"Fix\nthe loop"_s, u"in_progress"_s),
	           item(u"Skipped"_s, u"cancelled"_s),
	           item(u"Run tests"_s, u"pending"_s)});

	QTRY_COMPARE(session->todoTotal(), 3);
	QCOMPARE(session->todoDone(), 1);
	QCOMPARE(session->todoCurrent(), u"Fix the loop"_s); // on one line
	QCOMPARE(session->todoCurrentStatus(), u"in_progress"_s);
	QCOMPARE(session->todoItems().size(), 4); // the popup lists every item, cancelled ones too
	QCOMPARE(session->todoItems().at(2).toMap().value(u"status"_s).toString(), u"cancelled"_s);

	sendTodos({item(u"Read the parser"_s, u"completed"_s), item(u"Run tests"_s, u"completed"_s)});

	QTRY_COMPARE(session->todoDone(), 2);
	QVERIFY(session->todoCurrent().isEmpty()); // all done
	QVERIFY(session->todoCurrentStatus().isEmpty());
	QVERIFY(changed.count() >= 2);
}

void TestWorkspace::typingACustomAnswerTakesTheKeysAwayFromTheCard()
{
	Fixture f(2);
	ChatSession* session = f.workspace->current();
	f.fake.send(questionEvent());
	QTRY_VERIFY(session->question()->isActive());
	QTRY_VERIFY(session->answerKeysActive());

	session->question()->activateRow(2); // "Type your own answer"

	QVERIFY(session->question()->isEditing());
	QVERIFY(!session->answerKeysActive());
	QVERIFY(!session->answerKey(Qt::Key_A, u"a"_s, Qt::NoModifier, false)); // the text field has them
	session->question()->cancelCustom();
	QVERIFY(session->answerKeysActive());
}

// What the sidebar shows the to-do list of: the session in front, which in split view follows the focus.
void TestWorkspace::theSessionInFrontIsTheCurrentOneOrTheFocusedTile()
{
	const auto todo = [](const QString& content, const QString& status) {
		return QJsonObject{{u"content"_s, content}, {u"status"_s, status}};
	};
	Fixture f(2, true, {}, [&](StubServer& stub) {
		// Cancelled items count in neither number.
		stub.setTodos(u"ses_a"_s, {todo(u"Find the bug"_s, u"completed"_s), todo(u"Fix it"_s, u"in_progress"_s), todo(u"Idea"_s, u"cancelled"_s)});
	});
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 2);
	QSignalSpy changed(f.workspace.get(), &Workspace::frontSessionChanged);
	ChatSession* a = f.workspace->session(u"ses_a"_s);
	QCOMPARE(f.workspace->frontSession(), a);
	QTRY_COMPARE(a->todoTotal(), 2);
	QCOMPARE(a->todoDone(), 1);
	QCOMPARE(a->todoItems().size(), 3); // shown as they are, the cancelled one too
	// The sidebar reads one value; its parts agree with the separate properties.
	const auto snapshot = [](ChatSession* session) { return session->todo(); };
	QCOMPARE(snapshot(a).value(u"items"_s).toList(), a->todoItems());
	QCOMPARE(snapshot(a).value(u"done"_s).toInt(), 1);
	QCOMPARE(snapshot(a).value(u"total"_s).toInt(), 2);

	f.workspace->toggleSplit();
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 1);
	ChatSession* b = f.workspace->session(u"ses_b"_s);
	QTRY_VERIFY(b->isAttached());
	QCOMPARE(f.workspace->frontSession(), a); // no tile focused yet
	f.workspace->focusTile(u"ses_b"_s);
	QCOMPARE(f.workspace->frontSession(), b);
	QVERIFY(b->todoItems().isEmpty()); // so the section is hidden
	QCOMPARE(b->todoTotal(), 0);
	QVERIFY(snapshot(b).value(u"items"_s).toList().isEmpty());
	// Back to no focused tile: the current session's list is back in front.
	f.workspace->focusTile({});
	QCOMPARE(f.workspace->frontSession(), a);
	QCOMPARE(snapshot(f.workspace->frontSession()).value(u"items"_s).toList().size(), 3);
	QCOMPARE(snapshot(f.workspace->frontSession()).value(u"total"_s).toInt(), 2);
	QCOMPARE(changed.count(), 2);
	QCOMPARE(f.workspace->currentId(), u"ses_a"_s); // focusing does not change the current session

	f.workspace->toggleSplit();
	QTRY_VERIFY(!f.workspace->isSplitView());
	QTRY_COMPARE(f.stub.requests(u"watch_all"_s).size(), 2);
	QTRY_COMPARE(f.workspace->tileStage()->rowCount(), 0);
}

// The window shows the current session under a crossfade that takes a frame or more to start, so it can still show
// the closed session when that is deleted; its pane then read properties of null. The window gets to move on first.
void TestWorkspace::closingTheCurrentSessionLetsGoOfItBeforeItIsDeleted()
{
	Fixture f(2, true);
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 2);
	QPointer<ChatSession> closing = f.workspace->current();
	QVERIFY(closing);
	bool announcedInTime = false;
	connect(f.workspace.get(), &Workspace::sessionDropped, this, [&](ChatSession* dropped) {
		announcedInTime = dropped == closing.data() && f.workspace->current() != dropped;
	});

	f.workspace->closeSession(u"ses_a"_s);

	QTRY_VERIFY(announcedInTime);
	QTRY_VERIFY(closing.isNull());
	QCOMPARE(f.workspace->currentId(), u"ses_b"_s);
}

// Closing the connection resumes the coroutines whose requests are out. leaveSplit() waits for watch_all and then
// goes through the sessions, so they must still exist when the workspace is destroyed with that request unanswered
// (a use after free, found by an address sanitizer run).
void TestWorkspace::destroyingTheWorkspaceWhileLeavingSplitViewIsSafe()
{
	Fixture f(2, true);
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 2);
	f.workspace->toggleSplit();
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 1);
	QTRY_COMPARE(f.stub.requests(u"watch_all"_s).size(), 1);

	f.stub.holdWatchAll(true);
	f.workspace->toggleSplit();
	QTRY_COMPARE(f.stub.requests(u"watch_all"_s).size(), 2);
	QVERIFY(!f.stub.requests(u"watch_all"_s).last().value(u"enabled"_s).toBool());
	// Leaving ends here: the fixture goes with the request out.
}

// The same for show(), which waits for the old session's detach.
void TestWorkspace::destroyingTheWorkspaceWithADetachOutIsSafe()
{
	Fixture f;
	f.stub.holdDetach(true);
	f.workspace->newSession(u"/other"_s);
	QTRY_COMPARE(f.stub.requests(u"detach"_s).size(), 1);
}

void TestWorkspace::oldBackendOffersARestart()
{
	constexpr int OldProtocol = refgui::proto::ProtocolVersion - 1;
	Fixture f(OldProtocol);

	QVERIFY(f.workspace->isProtocolOutdated());
	QCOMPARE(f.workspace->protocolVersion(), OldProtocol);
	QCOMPARE(f.workspace->backendVersion(), FakeServerVersion);
	QTRY_VERIFY(f.workspace->canRestartBackend());

	f.workspace->restartBackend();
	QTRY_COMPARE(f.stub.requests(u"shutdown"_s).size(), 1);
}

void TestWorkspace::invalidImageDataIsAnError()
{
	Fixture f;
	ChatSession* session = f.workspace->session(u"ses_a"_s);
	QTRY_VERIFY(session->isAttached());

	f.stub.setImageData(u"not base64!"_s);
	const auto invalid = await(session->fetchImage(u"img_1"_s));
	QVERIFY(invalid.has_value() && !invalid->has_value());
	QCOMPARE(invalid->error().message, u"invalid image data"_s);

	f.stub.setImageData(QString::fromLatin1(QByteArray("png bytes").toBase64()));
	const auto valid = await(session->fetchImage(u"img_1"_s));
	QVERIFY(valid.has_value() && valid->has_value());
	QCOMPARE(**valid, QByteArray("png bytes"));
}

void TestWorkspace::refusedRestartIsAToast()
{
	Fixture f(refgui::proto::ProtocolVersion - 1);
	QTRY_VERIFY(f.workspace->canRestartBackend());
	f.stub.failShutdown(u"sessions are busy"_s);
	QSignalSpy toasts(f.workspace.get(), &Workspace::toastRequested);

	f.workspace->restartBackend();

	QTRY_COMPARE(toasts.count(), 1);
	QCOMPARE(toasts.first().at(0).toString(), refgui::workspace::ToastLevel::Error);
	QCOMPARE(toasts.first().at(1).toString(), u"sessions are busy"_s);
}

// The backend going away is what a restart asks for: the request's "connection closed" is no error.
void TestWorkspace::restartClosingTheConnectionIsQuiet()
{
	Fixture f(refgui::proto::ProtocolVersion - 1);
	QTRY_VERIFY(f.workspace->canRestartBackend());
	f.stub.closeOnShutdown();
	QSignalSpy toasts(f.workspace.get(), &Workspace::toastRequested);

	f.workspace->restartBackend();

	QTRY_COMPARE(f.stub.requests(u"shutdown"_s).size(), 1);
	QTest::qWait(QuietMs);
	for(const QList<QVariant>& toast : std::as_const(toasts))
	{
		QVERIFY2(toast.at(0).toString() != refgui::workspace::ToastLevel::Error, qPrintable(toast.at(1).toString()));
	}
}

void TestWorkspace::commandsFollowTheState()
{
	Fixture f;
	const auto enabled = [&f](const QString& id) {
		for(const QVariant& entry : f.workspace->commands()->matching({}, false))
		{
			if(entry.toMap().value(u"id"_s).toString() == id)
			{
				return entry.toMap().value(u"enabled"_s).toBool();
			}
		}
		return false;
	};

	QVERIFY(enabled(u"new"_s));
	QVERIFY(enabled(u"models"_s));
	QVERIFY(!enabled(u"zoom"_s)); // only in split view
	f.workspace->toggleSplit();
	QTRY_VERIFY(enabled(u"zoom"_s));
}

// The commands call these by name (Workspace::runOnSession), so a rename would only show at run time.
void TestWorkspace::sessionsHaveTheMethodsCommandsCall()
{
	for(const char* method : {"undoLast()", "copyLastReply()", "toggleReasoning()", "toggleDetails()"})
	{
		QVERIFY2(ChatSession::staticMetaObject.indexOfMethod(method) >= 0, method);
	}
}

// ChatSession connects the prompt that Undo takes back to the input box; the two know nothing of each other otherwise.
void TestWorkspace::undoPutsThePromptBackIntoTheInputBox()
{
	Fixture f;
	ChatSession* session = f.workspace->current();
	session->composer()->edit(u"!"_s, 1); // a shell line is being typed
	QVERIFY(session->composer()->shellMode());

	session->undoLast();

	QTRY_VERIFY(session->composer()->text().startsWith(u"take this back"_s)); // the image adds its tag
	QCOMPARE(session->composer()->attachments()->rowCount(), 1);
	QVERIFY(!session->composer()->shellMode()); // it was a prompt
	QVERIFY(f.stub.ops().contains(u"undo"_s));
}

void TestWorkspace::infoResolvesRootTaskFromLiveSessions()
{
	Fixture f(refgui::proto::ProtocolVersion, false, {}, [](StubServer& stub) {
		stub.add(u"ses_sub"_s, 2, u"/work"_s, u"ses_a"_s);
		stub.add(u"ses_leaf"_s, 3, u"/work"_s, u"ses_sub"_s);
	});
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 3);
	const QVariantMap rootInfo = f.workspace->info(u"ses_a"_s);
	const QVariantMap leafInfo = f.workspace->info(u"ses_leaf"_s);
	QCOMPARE(rootInfo.value(u"rootId"_s).toString(), QString());
	QCOMPARE(leafInfo.value(u"parentId"_s).toString(), u"ses_sub"_s);
	QCOMPARE(leafInfo.value(u"rootId"_s).toString(), u"ses_a"_s);
	QCOMPARE(leafInfo.value(u"rootName"_s).toString(), u"Title ses_a"_s);
}

void TestWorkspace::leavingSplitWhileZoomedPromotesZoomedSession()
{
	Fixture f(refgui::proto::ProtocolVersion, true);
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 2);
	ChatSession* sessionA = f.workspace->session(u"ses_a"_s);
	QVERIFY(sessionA != nullptr && sessionA->isForeground());

	f.workspace->toggleSplit();
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 1);
	ChatSession* sessionB = f.workspace->session(u"ses_b"_s);
	QTRY_VERIFY(sessionB != nullptr && sessionB->isAttached());

	f.workspace->zoomTile(u"ses_b"_s);
	QTRY_VERIFY(sessionB->isForeground());
	QCOMPARE(f.workspace->zoomedId(), u"ses_b"_s);

	f.workspace->toggleSplit();
	QVERIFY(!f.workspace->isSplitView());
	QCOMPARE(f.workspace->zoomedId(), QString());
	QCOMPARE(f.workspace->currentId(), u"ses_b"_s);
	QCOMPARE(f.workspace->visibleSessionIds(), (QStringList{u"ses_b"_s}));
	QTRY_VERIFY(sessionB->isForeground());
	QTRY_VERIFY(!sessionA->isAttached());
}

void TestWorkspace::openingNonTileInSplitViewExitsSplitAndShowsTarget()
{
	Fixture f(refgui::proto::ProtocolVersion, true, {}, [](StubServer& stub) { stub.addHistory(u"ses_hist"_s, 3); });
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 2);

	// 1) Clicking the hidden current session in the sidebar leaves split view for ses_a even if ses_b was zoomed.
	f.workspace->toggleSplit();
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 1);
	f.workspace->zoomTile(u"ses_b"_s);
	QTRY_VERIFY(f.workspace->session(u"ses_b"_s)->isForeground());

	f.workspace->openSession(u"ses_a"_s);
	QVERIFY(!f.workspace->isSplitView());
	QCOMPARE(f.workspace->zoomedId(), QString());
	QCOMPARE(f.workspace->currentId(), u"ses_a"_s);
	QTRY_VERIFY(f.workspace->session(u"ses_a"_s)->isForeground());
	QTRY_VERIFY(!f.workspace->session(u"ses_b"_s)->isAttached());

	// 2) Clicking a history session while in split view leaves split view and attaches the history session in front.
	f.workspace->toggleSplit();
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 1);
	QTRY_VERIFY(f.workspace->session(u"ses_b"_s)->isAttached());

	f.workspace->openSession(u"ses_hist"_s);
	QVERIFY(!f.workspace->isSplitView());
	QTRY_COMPARE(f.workspace->currentId(), u"ses_hist"_s);
	QTRY_VERIFY(f.workspace->session(u"ses_hist"_s)->isForeground());
	QTRY_VERIFY(!f.workspace->session(u"ses_a"_s)->isAttached());
	QTRY_VERIFY(!f.workspace->session(u"ses_b"_s)->isAttached());
}

void TestWorkspace::requestCloseSessionConfirmsWhenBusyAndClosesWhenIdle()
{
	Fixture f(refgui::proto::ProtocolVersion, true);
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 2);
	f.stub.setState(u"ses_b"_s, u"running"_s);
	QTRY_COMPARE(f.workspace->info(u"ses_b"_s).value(u"activity"_s).toString(), u"running"_s);

	f.workspace->requestCloseSession(u"ses_b"_s);
	QCOMPARE(f.workspace->dialog(), u"close"_s);
	QCOMPARE(f.workspace->dialogArgs().value(u"sessionId"_s).toString(), u"ses_b"_s);
	QCOMPARE(f.stub.requests(u"close"_s).size(), 0);

	f.workspace->closeDialog();
	f.stub.setState(u"ses_b"_s, u"idle"_s);
	QTRY_COMPARE(f.workspace->info(u"ses_b"_s).value(u"activity"_s).toString(), u"idle"_s);

	f.workspace->requestCloseSession(u"ses_b"_s);
	QCOMPARE(f.workspace->dialog(), QString());
	QTRY_COMPARE(f.stub.requests(u"close"_s).size(), 1);
	QCOMPARE(f.stub.requests(u"close"_s).first().value(u"session_id"_s).toString(), u"ses_b"_s);
}

void TestWorkspace::reloadAndConfigCommandsUpdateStateAndDeduplicateBroadcast()
{
	Fixture f;
	QTRY_VERIFY(f.workspace->current() != nullptr);

	// /config opens the ConfigDialog.
	f.workspace->commands()->run(u"config"_s);
	QCOMPARE(f.workspace->dialog(), u"config"_s);
	f.workspace->closeDialog();
	QCOMPARE(f.workspace->dialog(), QString());

	// /reload sends reload_config for the active session's cwd, updates models, and deduplicates ConfigChanged warnings.
	const int getConfigsBefore = int(f.stub.requests(u"get_config"_s).size());
	f.stub.setModels(QJsonArray{QJsonObject{{u"id"_s, u"claude-sonnet-4-5"_s}, {u"provider"_s, u"anthropic"_s}, {u"provider_name"_s, u"Anthropic"_s}, {u"name"_s, u"Claude Sonnet 4.5"_s}}});
	f.stub.setConfigWarnings(QJsonArray{u"bad mcp server entry"_s});
	QSignalSpy toasts(f.workspace.get(), &Workspace::toastRequested);

	f.workspace->commands()->run(u"reload"_s);
	QTRY_COMPARE(f.stub.requests(u"reload_config"_s).size(), 1);
	QCOMPARE(f.stub.requests(u"reload_config"_s).first().value(u"cwd"_s).toString(), u"/work"_s);
	QTRY_COMPARE(f.workspace->config()->models().size(), 1);
	QCOMPARE(f.stub.requests(u"get_config"_s).size(), getConfigsBefore);
	QCOMPARE(toasts.count(), 2);
	QCOMPARE(toasts.at(0).at(0).toString(), refgui::workspace::ToastLevel::Info);
	QCOMPARE(toasts.at(1).at(0).toString(), refgui::workspace::ToastLevel::Warn);
	QVERIFY(toasts.at(1).at(1).toString().contains(u"bad mcp server entry"_s));

	// External ConfigChanged broadcast (from another client) refreshes BackendConfig via get_config.
	toasts.clear();
	f.stub.setModels(QJsonArray{QJsonObject{{u"id"_s, u"gpt-5"_s}, {u"provider"_s, u"openai"_s}, {u"provider_name"_s, u"OpenAI"_s}, {u"name"_s, u"GPT-5"_s}}});
	f.stub.setConfigWarnings(QJsonArray{u"external warning"_s});
	f.fake.send({{u"kind"_s, u"config_changed"_s}, {u"warnings"_s, QJsonArray{u"external warning"_s}}});
	QTRY_COMPARE(f.stub.requests(u"get_config"_s).size(), getConfigsBefore + 1);
	QTRY_COMPARE(f.workspace->config()->models().first().toMap().value(u"id"_s).toString(), u"gpt-5"_s);
	QTRY_COMPARE(toasts.count(), 1);
	QCOMPARE(toasts.at(0).at(0).toString(), refgui::workspace::ToastLevel::Warn);
	// ConfigEditorController::reloadFromDisk() also suppresses duplicate get_config on config_changed broadcast.
	const qsizetype getConfigsBeforeEditorReload = f.stub.requests(u"get_config"_s).size();
	f.workspace->configEditor()->reloadFromDisk();
	QTRY_VERIFY(!f.workspace->configEditor()->isLoading());
	QCOMPARE(f.stub.requests(u"reload_config"_s).size(), 2);
	QCOMPARE(f.stub.requests(u"get_config"_s).size(), getConfigsBeforeEditorReload);
}

void TestWorkspace::directArgumentsForModelsAgentsVariantsSessionsAndMultiClose()
{
	Fixture f(refgui::proto::ProtocolVersion, true, {}, [](StubServer& stub) {
		stub.add(u"ses_c"_s, 3);
		stub.addHistory(u"ses_hist"_s, 4);
		stub.setModels(QJsonArray{QJsonObject{{u"id"_s, u"claude-sonnet-4"_s},
		                                      {u"provider"_s, u"anthropic"_s},
		                                      {u"provider_name"_s, u"Anthropic"_s},
		                                      {u"name"_s, u"Claude Sonnet 4"_s},
		                                      {u"variants"_s, QJsonArray{u"low"_s, u"high"_s}}}});
	});
	QTRY_COMPARE(f.workspace->config()->models().size(), 1);
	QTRY_VERIFY(f.workspace->historyLoaded());
	auto* commands = f.workspace->commands();
	QSignalSpy toasts(f.workspace.get(), &Workspace::toastRequested);

	// /model (singular alias, case-insensitive) with provider/model#variant, bare id, and unknown warnings.
	QVERIFY(commands->runSlash(u"/MODEL anthropic/claude-sonnet-4#HIGH"_s));
	QTRY_COMPARE(f.stub.requests(u"set_model"_s).size(), 1);
	QCOMPARE(f.stub.requests(u"set_model"_s).last().value(u"model"_s).toObject().value(u"provider"_s).toString(), u"anthropic"_s);
	QCOMPARE(f.stub.requests(u"set_model"_s).last().value(u"model"_s).toObject().value(u"model"_s).toString(), u"claude-sonnet-4"_s);
	QCOMPARE(f.stub.requests(u"set_model"_s).last().value(u"model"_s).toObject().value(u"variant"_s).toString(), u"high"_s);

	QVERIFY(commands->runSlash(u"/model claude-sonnet-4#nope"_s));
	QCOMPARE(toasts.count(), 1);
	QCOMPARE(toasts.last().at(0).toString(), refgui::workspace::ToastLevel::Warn);
	QCOMPARE(f.stub.requests(u"set_model"_s).size(), 1);

	QVERIFY(commands->runSlash(u"/models no-such-model"_s));
	QCOMPARE(toasts.count(), 2);
	QCOMPARE(toasts.last().at(0).toString(), refgui::workspace::ToastLevel::Warn);

	// /variant <v> and /variant default.
	QVERIFY(commands->runSlash(u"/variant low"_s));
	QTRY_COMPARE(f.stub.requests(u"set_variant"_s).size(), 1);
	QCOMPARE(f.stub.requests(u"set_variant"_s).last().value(u"variant"_s).toString(), u"low"_s);
	QVERIFY(commands->runSlash(u"/variant DEFAULT"_s));
	QTRY_COMPARE(f.stub.requests(u"set_variant"_s).size(), 2);
	QVERIFY(f.stub.requests(u"set_variant"_s).last().contains(u"variant"_s));
	QCOMPARE(f.stub.requests(u"set_variant"_s).last().value(u"variant"_s).toString(), QString());

	// /agent (singular alias) and unknown agent warning.
	QVERIFY(commands->runSlash(u"/agent PLAN"_s));
	QTRY_COMPARE(f.stub.requests(u"set_agent"_s).size(), 1);
	QCOMPARE(f.stub.requests(u"set_agent"_s).last().value(u"agent"_s).toString(), u"plan"_s);
	toasts.clear();
	QVERIFY(commands->runSlash(u"/agents ghost"_s));
	QCOMPARE(toasts.count(), 1);
	QCOMPARE(toasts.last().at(0).toString(), refgui::workspace::ToastLevel::Warn);

	// /sessions <s> opens live or history session directly, warns on unknown.
	QVERIFY(commands->runSlash(u"/sessions Title ses_hist"_s));
	QTRY_COMPARE(f.workspace->currentId(), u"ses_hist"_s);
	toasts.clear();
	QVERIFY(commands->runSlash(u"/sessions missing_session"_s));
	QCOMPARE(toasts.count(), 1);
	QCOMPARE(toasts.last().at(0).toString(), refgui::workspace::ToastLevel::Warn);

	// /close-session <s1> <s2> <missing> closes multiple live sessions and warns on unknown token.
	toasts.clear();
	QVERIFY(commands->runSlash(u"/close-session ses_b ses_c nope_ses"_s));
	QTRY_COMPARE(f.stub.requests(u"close"_s).size(), 2);
	QCOMPARE(toasts.count(), 1);
	QVERIFY(toasts.last().at(1).toString().contains(u"nope_ses"_s));
}

void TestWorkspace::closingOrRemovingZoomedTileClearsZoomAndShowsRemainingTiles()
{
	Fixture f(refgui::proto::ProtocolVersion, true, {}, [](StubServer& stub) { stub.add(u"ses_c"_s, 3); });
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 3);
	f.workspace->toggleSplit();
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 2);
	QTRY_VERIFY(f.workspace->session(u"ses_b"_s)->isAttached());
	QTRY_VERIFY(f.workspace->session(u"ses_c"_s)->isAttached());

	QSignalSpy zoomedSpy(f.workspace.get(), &Workspace::zoomedChanged);
	f.workspace->zoomTile(u"ses_b"_s);
	QTRY_VERIFY(f.workspace->session(u"ses_b"_s)->isForeground());
	QCOMPARE(f.workspace->zoomedId(), u"ses_b"_s);
	QCOMPARE(f.workspace->visibleSessionIds(), (QStringList{u"ses_b"_s}));
	zoomedSpy.clear();

	// Closing the zoomed tile clears zoomedId and makes the remaining tile visible again.
	f.workspace->closeSession(u"ses_b"_s);
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 1);
	QCOMPARE(f.workspace->zoomedId(), QString());
	QVERIFY(zoomedSpy.count() >= 1);
	QCOMPARE(f.workspace->visibleSessionIds(), (QStringList{u"ses_c"_s}));

	// External removal of a zoomed session also clears zoomedId via syncTiles().
	f.workspace->zoomTile(u"ses_c"_s);
	QTRY_COMPARE(f.workspace->zoomedId(), u"ses_c"_s);
	zoomedSpy.clear();
	f.stub.remove(u"ses_c"_s);
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 0);
	QCOMPARE(f.workspace->zoomedId(), QString());
	QVERIFY(zoomedSpy.count() >= 1);
}

void TestWorkspace::zoomingTileRemovedBeforeAttachFinishesLeavesNoDanglingZoom()
{
	Fixture f(refgui::proto::ProtocolVersion, true, {}, [](StubServer& stub) { stub.add(u"ses_c"_s, 3); });
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 3);
	f.workspace->toggleSplit();
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 2);
	QTRY_VERIFY(f.workspace->session(u"ses_b"_s)->isAttached());
	QTRY_VERIFY(f.workspace->session(u"ses_c"_s)->isAttached());

	// 1) Target tile closes while the previously zoomed tile is giving up the foreground.
	f.workspace->zoomTile(u"ses_b"_s);
	QTRY_VERIFY(f.workspace->session(u"ses_b"_s)->isForeground());
	QCOMPARE(f.workspace->zoomedId(), u"ses_b"_s);

	f.stub.holdDetach(true);
	f.workspace->zoomTile(u"ses_c"_s);
	QTRY_COMPARE(f.stub.requests(u"detach"_s).size(), 1);
	f.stub.remove(u"ses_c"_s);
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 1);
	f.stub.releaseDetach();
	QTRY_COMPARE(f.workspace->zoomedId(), QString());
	QCOMPARE(f.workspace->visibleSessionIds(), (QStringList{u"ses_b"_s}));
	QTRY_VERIFY(!f.workspace->session(u"ses_b"_s)->isForeground());

	// 2) Foreground attach fails because the session vanished right before attach arrived.
	f.stub.failAttach(u"ses_b"_s, u"no such session"_s);
	f.workspace->zoomTile(u"ses_b"_s);
	QTRY_COMPARE(f.workspace->zoomedId(), QString());
	QCOMPARE(f.workspace->visibleSessionIds(), (QStringList{u"ses_b"_s}));
}

void TestWorkspace::historyFiltersPersistAndCreatorFilterIsEphemeralAndAllImagesCollectsOrder()
{
	const QJsonArray messages{
		QJsonObject{{u"id"_s, u"m1"_s},
		            {u"role"_s, u"user"_s},
		            {u"time"_s, 1},
		            {u"parts"_s, QJsonArray{QJsonObject{{u"type"_s, u"image"_s},
		                                               {u"id"_s, u"img_u1"_s},
		                                               {u"mime"_s, u"image/png"_s},
		                                               {u"data"_s, QString()},
		                                               {u"size"_s, 128},
		                                               {u"name"_s, u"first.png"_s}}}}},
		QJsonObject{{u"id"_s, u"m2"_s},
		            {u"role"_s, u"assistant"_s},
		            {u"time"_s, 2},
		            {u"parts"_s, QJsonArray{QJsonObject{{u"type"_s, u"tool"_s},
		                                               {u"call_id"_s, u"c1"_s},
		                                               {u"name"_s, u"read"_s},
		                                               {u"input"_s, QJsonObject()},
		                                               {u"state"_s, u"done"_s},
		                                               {u"images"_s, QJsonArray{QJsonObject{{u"id"_s, u"img_t1"_s}, {u"mime"_s, u"image/png"_s}, {u"data"_s, QString()}, {u"size"_s, 256}}}}}}}},
	};
	Fixture f(refgui::proto::ProtocolVersion, false, messages, [](StubServer& stub) {
		stub.add(u"ses_lead"_s, 2, u"/other"_s);
		stub.addHistory(u"ses_hist_user"_s, 3, u"/work"_s);
		stub.addHistory(u"ses_hist_spawned"_s, 4, u"/other"_s, u"ses_lead"_s);
	});
	QTRY_COMPARE(f.workspace->historyLoaded(), true);
	QTRY_COMPARE(f.workspace->history()->rowCount(), 2);
	QCOMPARE(f.workspace->sessionChoices()->rowCount(), 4);

	// allImages collects both image block and tool block images in display order.
	const QVariantList imgs = f.workspace->current()->allImages();
	QCOMPARE(imgs.size(), 2);
	QCOMPARE(imgs.at(0).toMap().value(u"id"_s).toString(), u"img_u1"_s);
	QCOMPARE(imgs.at(1).toMap().value(u"id"_s).toString(), u"img_t1"_s);

	// Persisted user-created + currentDir filter updates settings and issues list_history with creator & cwd,
	// while keeping sessionChoices and allSessions() unfiltered.
	f.workspace->setHistoryFilter(u"user"_s, true);
	QCOMPARE(f.workspace->historyCreatorFilter(), u"user"_s);
	QVERIFY(f.workspace->historyCurrentDir());
	QCOMPARE(f.workspace->settings()->sidebarHistoryCreatorFilter(), u"user"_s);
	QVERIFY(f.workspace->settings()->sidebarHistoryCurrentDir());
	QTRY_VERIFY(f.stub.requests(u"list_history"_s).last().value(u"creator"_s).toString() == u"user"_s);
	QCOMPARE(f.stub.requests(u"list_history"_s).last().value(u"cwd"_s).toString(), u"/work"_s);
	QTRY_COMPARE(f.workspace->history()->rowCount(), 1);
	QCOMPARE(f.workspace->sessionChoices()->rowCount(), 4);
	QVERIFY(!f.workspace->info(u"ses_hist_spawned"_s).isEmpty());

	// Ephemeral By <creatorId> does NOT overwrite persisted sidebarHistoryCreatorFilter.
	f.workspace->filterHistoryByCreator(u"ses_lead"_s);
	QCOMPARE(f.workspace->historyCreatorFilter(), u"creator"_s);
	QCOMPARE(f.workspace->historyCreatorId(), u"ses_lead"_s);
	QCOMPARE(f.workspace->historyCreatorLabel(), u"Title ses_lead"_s);
	QCOMPARE(f.workspace->settings()->sidebarHistoryCreatorFilter(), u"user"_s);
	QTRY_COMPARE(f.stub.requests(u"list_history"_s).last().value(u"creator"_s).toString(), u"ses_lead"_s);
	QTRY_COMPARE(f.workspace->history()->rowCount(), 0); // currentDir is /work, spawned session is in /other
	QCOMPARE(f.workspace->sessionChoices()->rowCount(), 4);

	// Switching current session to /other re-queries list_history with new effectiveCwd when historyCurrentDir is true.
	f.workspace->openSession(u"ses_lead"_s);
	QTRY_COMPARE(f.workspace->effectiveCwd(), u"/other"_s);
	QTRY_COMPARE(f.stub.requests(u"list_history"_s).last().value(u"cwd"_s).toString(), u"/other"_s);
	QTRY_COMPARE(f.workspace->history()->rowCount(), 1);
	QCOMPARE(f.workspace->sessionChoices()->rowCount(), 4);
}
QTEST_MAIN(TestWorkspace)
#include "tst_workspace.moc"
