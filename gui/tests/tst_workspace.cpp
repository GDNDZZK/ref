#include "app/workspace.h"
#include "fake_backend.h"

#include <QJSEngine>
#include <QJsonArray>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QtTest>

using namespace Qt::StringLiterals;
using namespace refgui;
using namespace refgui::test;
using refgui::client::Backend;

namespace
{

// A backend with a few sessions that answers like the real one for what the workspace asks.
class StubServer
{
public:
	explicit StubServer(FakeBackend& fake)
		: m_fake(fake)
	{
		fake.onRequest = [this](const QJsonObject& request) { answer(request); };
		fake.onHello = [this](const QJsonObject& hello) {
			m_fake.respond(hello, {{u"server_version"_s, u"0.0.0-test"_s}, {u"protocol"_s, m_protocol}});
		};
	}

	void add(const QString& id, qint64 created, const QString& cwd = u"/work"_s)
	{
		m_sessions.insert(id,
		                  {{u"id"_s, id},
		                   {u"name"_s, QString()},
		                   {u"title"_s, u"Title "_s + id},
		                   {u"agent"_s, u"build"_s},
		                   {u"model"_s, QJsonObject{{u"provider"_s, u"p"_s}, {u"model"_s, u"m"_s}}},
		                   {u"cwd"_s, cwd},
		                   {u"created_at"_s, created},
		                   {u"updated_at"_s, created},
		                   {u"status"_s, QJsonObject{{u"state"_s, u"idle"_s}}},
		                   {u"attached"_s, 0},
		                   {u"live"_s, true}});
	}

	void setProtocol(int protocol) { m_protocol = protocol; }

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
			array.append(info);
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
			add(created, 100 + m_created, request.value(u"cwd"_s).toString());
			m_fake.respond(request, {{u"session"_s, m_sessions.value(created)}});
			broadcast();
		}
		else if(op == u"attach"_s)
		{
			if(!m_sessions.contains(id))
			{
				m_fake.fail(request, u"no such session"_s);
				return;
			}
			QJsonObject info = m_sessions.value(id);
			info[u"attached"_s] = request.value(u"foreground"_s).toBool() ? 1 : 0;
			m_sessions[id] = info;
			m_fake.respond(request, {{u"snapshot"_s, QJsonObject{{u"info"_s, info}, {u"messages"_s, QJsonArray()}, {u"more_before"_s, false}}}});
			broadcast();
		}
		else if(op == u"detach"_s)
		{
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
		else if(op == u"list_sessions"_s || op == u"list_history"_s || op == u"watch_all"_s)
		{
			m_fake.respond(request, {{u"sessions"_s, list()}});
		}
		else if(op == u"list_dirs"_s)
		{
			m_fake.respond(request, {{u"path"_s, u"/work"_s}, {u"dirs"_s, QJsonArray{u"a"_s, u"b"_s}}});
		}
		else if(op == u"get_config"_s)
		{
			m_fake.respond(request,
			               {{u"config"_s,
			                 QJsonObject{{u"default_agent"_s, u"build"_s},
			                             {u"agents"_s, QJsonArray{QJsonObject{{u"name"_s, u"build"_s}, {u"description"_s, u""_s}}, QJsonObject{{u"name"_s, u"plan"_s}, {u"description"_s, u""_s}}}},
			                             {u"models"_s, QJsonArray()}}}});
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
	int m_created = 0;
	int m_protocol = 2;
};

struct Fixture
{
	explicit Fixture(int protocol = 2, bool withSecond = false)
	{
		stub.setProtocol(protocol);
		stub.add(u"ses_a"_s, 1);
		if(withSecond)
		{
			stub.add(u"ses_b"_s, 2);
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
	void newSessionBecomesCurrentAndLetsGoOfTheOld();
	void openingAnotherSessionSwitches();
	void closingTheCurrentSessionOpensAnother();
	void closingTheLastSessionStartsOneInTheSameDirectory();
	void renameReportsToTheCallback();
	void failedRequestsBecomeToasts();
	void slashCommandsRunTheirHandlers();
	void cyclesAgents();
	void serverToastsArePassedOn();
	void splitViewWatchesAndComesBack();
	void zoomUpgradesTheTileToTheForeground();
	void oldBackendOffersARestart();
	void commandsFollowTheState();
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

void TestWorkspace::closingTheLastSessionStartsOneInTheSameDirectory()
{
	Fixture f;
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 1);

	f.workspace->closeSession(u"ses_a"_s);

	QTRY_COMPARE(f.workspace->currentId(), u"ses_new1"_s);
	QCOMPARE(f.stub.requests(u"create_session"_s).last().value(u"cwd"_s).toString(), u"/work"_s);
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

	QVERIFY(commands->run(u"variant"_s));
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
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 2);
	QTRY_COMPARE(f.stub.requests(u"watch_all"_s).size(), 1);
	QVERIFY(f.stub.requests(u"watch_all"_s).first().value(u"enabled"_s).toBool());
	// The current session gave up the foreground; the other one is attached as a watcher.
	QCOMPARE(f.stub.requests(u"detach"_s).first().value(u"session_id"_s).toString(), u"ses_a"_s);
	QTRY_VERIFY(f.workspace->session(u"ses_b"_s)->isAttached());
	QCOMPARE(f.stub.requests(u"attach"_s).last().value(u"session_id"_s).toString(), u"ses_b"_s);
	QVERIFY(!f.stub.requests(u"attach"_s).last().value(u"foreground"_s).toBool());
	QCOMPARE(f.workspace->focusedTileId(), u"ses_a"_s);
	QCOMPARE(f.workspace->visibleSessionIds().size(), 2);

	f.workspace->toggleSplit();

	QTRY_VERIFY(!f.workspace->isSplitView());
	QCOMPARE(f.workspace->tiles()->rowCount(), 0);
	QTRY_COMPARE(f.stub.requests(u"watch_all"_s).size(), 2);
	QVERIFY(!f.stub.requests(u"watch_all"_s).last().value(u"enabled"_s).toBool());
	QTRY_VERIFY(f.stub.requests(u"attach"_s).last().value(u"foreground"_s).toBool());
	QCOMPARE(f.stub.requests(u"attach"_s).last().value(u"session_id"_s).toString(), u"ses_a"_s);
	QTRY_VERIFY(!f.workspace->session(u"ses_b"_s)->isAttached());
	QCOMPARE(f.workspace->visibleSessionIds(), (QStringList{u"ses_a"_s}));
}

void TestWorkspace::zoomUpgradesTheTileToTheForeground()
{
	Fixture f(2, true);
	QTRY_COMPARE(f.workspace->sessions()->rowCount(), 2);
	f.workspace->toggleSplit();
	QTRY_COMPARE(f.workspace->tiles()->rowCount(), 2);
	QTRY_VERIFY(f.workspace->session(u"ses_b"_s)->isAttached());

	f.workspace->zoomTile(u"ses_b"_s);

	QCOMPARE(f.workspace->zoomedId(), u"ses_b"_s);
	QCOMPARE(f.workspace->visibleSessionIds(), (QStringList{u"ses_b"_s}));
	QTRY_VERIFY(f.stub.requests(u"attach"_s).last().value(u"foreground"_s).toBool());
	QCOMPARE(f.stub.requests(u"attach"_s).last().value(u"session_id"_s).toString(), u"ses_b"_s);

	f.workspace->unzoom();
	QCOMPARE(f.workspace->zoomedId(), QString());
	QTRY_COMPARE(f.stub.requests(u"detach"_s).last().value(u"session_id"_s).toString(), u"ses_b"_s);
	QCOMPARE(f.workspace->visibleSessionIds().size(), 2);
}

void TestWorkspace::oldBackendOffersARestart()
{
	Fixture f(1);

	QVERIFY(f.workspace->isProtocolOutdated());
	QCOMPARE(f.workspace->protocolVersion(), 1);
	QCOMPARE(f.workspace->backendVersion(), u"0.0.0-test"_s);
	QTRY_VERIFY(f.workspace->canRestartBackend());

	f.workspace->restartBackend();
	QTRY_COMPARE(f.stub.requests(u"shutdown"_s).size(), 1);
}

void TestWorkspace::commandsFollowTheState()
{
	Fixture f;
	const auto enabled = [&f](const QString& id) {
		for(const QVariant& entry : f.workspace->commands()->match({}, false))
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

QTEST_MAIN(TestWorkspace)
#include "tst_workspace.moc"
