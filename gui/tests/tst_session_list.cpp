#include "client/backend.h"
#include "fake_backend.h"
#include "workspace/backend_config.h"
#include "workspace/session_directory.h"
#include "workspace/session_list_model.h"

#include <QJsonArray>
#include <QSignalSpy>
#include <QtTest>

using namespace Qt::StringLiterals;
using namespace refgui;
using namespace refgui::test;
using refgui::client::Backend;
using refgui::workspace::BackendConfig;
using refgui::workspace::SessionDirectory;
using refgui::workspace::SessionListModel;

namespace
{

proto::SessionInfo session(const QString& id, qint64 created, qint64 updated = 0)
{
	proto::SessionInfo info;
	info.id = id;
	info.cwd = u"/home/me/project"_s;
	info.live = true;
	info.createdAt = created;
	info.updatedAt = updated == 0 ? created : updated;
	info.model = {u"p"_s, u"m"_s, std::nullopt};
	return info;
}

proto::SessionInfo stored(const QString& id, qint64 created, qint64 updated = 0)
{
	proto::SessionInfo info = session(id, created, updated);
	info.live = false;
	return info;
}

QStringList ids(const SessionListModel& model)
{
	QStringList list;
	for(int row = 0; row < model.rowCount(); ++row)
	{
		list.append(model.data(model.index(row), SessionListModel::SessionIdRole).toString());
	}
	return list;
}

QJsonObject infoJson(const QString& id, qint64 created, bool live)
{
	return {{u"id"_s, id},
	        {u"name"_s, u""_s},
	        {u"title"_s, u"T "_s + id},
	        {u"agent"_s, u"build"_s},
	        {u"model"_s, QJsonObject{{u"provider"_s, u"p"_s}, {u"model"_s, u"m"_s}}},
	        {u"cwd"_s, u"/work"_s},
	        {u"created_at"_s, created},
	        {u"updated_at"_s, created},
	        {u"status"_s, QJsonObject{{u"state"_s, u"idle"_s}}},
	        {u"attached"_s, 0},
	        {u"live"_s, live}};
}

} // namespace

class TstSessionList : public QObject
{
	Q_OBJECT

private slots:
	void sortsLiveByCreationAndHistoryByUpdate();
	void keepsRowsOfSessionsThatStay();
	void movesInsteadOfRecreating();
	void filtersIgnoringCase();
	void derivesRoles();
	void historyRowsAreLabelledByTitle();
	void marksCurrentAndTiles();
	void directoryLoadsAndFollowsFrames();
	void directoryHistoryHidesLiveSessions();
	void configLoadsAgentsModelsAndMcp();
};

void TstSessionList::sortsLiveByCreationAndHistoryByUpdate()
{
	SessionListModel live;
	live.setSessions({session(u"a"_s, 1, 90), session(u"b"_s, 3, 10), session(u"c"_s, 2, 50)});
	QCOMPARE(ids(live), (QStringList{u"b"_s, u"c"_s, u"a"_s}));

	SessionListModel history;
	history.setSessions({stored(u"a"_s, 1, 90), stored(u"b"_s, 3, 10), stored(u"c"_s, 2, 50)});
	QCOMPARE(ids(history), (QStringList{u"a"_s, u"c"_s, u"b"_s}));

	// Together, the live ones come first.
	SessionListModel both;
	both.setSessions({stored(u"old"_s, 1, 99), session(u"live"_s, 2, 5)});
	QCOMPARE(ids(both), (QStringList{u"live"_s, u"old"_s}));
}

void TstSessionList::keepsRowsOfSessionsThatStay()
{
	SessionListModel model;
	model.setSessions({session(u"a"_s, 1), session(u"b"_s, 2)});
	QSignalSpy inserted(&model, &QAbstractItemModel::rowsInserted);
	QSignalSpy removed(&model, &QAbstractItemModel::rowsRemoved);
	QSignalSpy changed(&model, &QAbstractItemModel::dataChanged);
	QSignalSpy countChanged(&model, &SessionListModel::countChanged);

	proto::SessionInfo renamed = session(u"b"_s, 2);
	renamed.name = u"Renamed"_s;
	model.setSessions({session(u"a"_s, 1), renamed, session(u"c"_s, 3)});
	QCOMPARE(inserted.count(), 1);
	QCOMPARE(removed.count(), 0);
	QCOMPARE(changed.count(), 1);
	QCOMPARE(countChanged.count(), 1);
	QCOMPARE(ids(model), (QStringList{u"c"_s, u"b"_s, u"a"_s}));

	model.setSessions({session(u"c"_s, 3)});
	QCOMPARE(removed.count(), 2);

	// The same list again changes nothing.
	changed.clear();
	inserted.clear();
	model.setSessions({session(u"c"_s, 3)});
	QCOMPARE(changed.count(), 0);
	QCOMPARE(inserted.count(), 0);
}

void TstSessionList::movesInsteadOfRecreating()
{
	SessionListModel model;
	model.setSessions({stored(u"a"_s, 1, 30), stored(u"b"_s, 2, 20), stored(u"c"_s, 3, 10)});
	QSignalSpy moved(&model, &QAbstractItemModel::rowsMoved);
	QSignalSpy inserted(&model, &QAbstractItemModel::rowsInserted);
	QSignalSpy removed(&model, &QAbstractItemModel::rowsRemoved);

	model.setSessions({stored(u"a"_s, 1, 30), stored(u"b"_s, 2, 20), stored(u"c"_s, 3, 40)});
	QCOMPARE(ids(model), (QStringList{u"c"_s, u"a"_s, u"b"_s}));
	QCOMPARE(moved.count(), 1);
	QCOMPARE(inserted.count(), 0);
	QCOMPARE(removed.count(), 0);
}

void TstSessionList::filtersIgnoringCase()
{
	SessionListModel model;
	proto::SessionInfo a = session(u"ses_aaa"_s, 1);
	a.title = u"Fix the Parser"_s;
	proto::SessionInfo b = session(u"ses_bbb"_s, 2);
	b.name = u"docs"_s;
	b.cwd = u"/srv/Website"_s;
	model.setSessions({a, b});

	model.setFilter(u"PARSER"_s);
	QCOMPARE(ids(model), (QStringList{u"ses_aaa"_s}));
	model.setFilter(u"website"_s);
	QCOMPARE(ids(model), (QStringList{u"ses_bbb"_s}));
	model.setFilter(u"ses_"_s);
	QCOMPARE(model.count(), 2);
	model.setFilter(u"nothing"_s);
	QCOMPARE(model.count(), 0);
	QVERIFY(model.contains(u"ses_aaa"_s)); // filtered out, not gone
	model.setFilter({});
	QCOMPARE(model.count(), 2);
}

void TstSessionList::derivesRoles()
{
	SessionListModel model;
	proto::SessionInfo info = session(u"a"_s, 1);
	info.status.state = proto::SessionStatus::State::WaitingPermission;
	info.model.variant = u"high"_s;
	info.todos = {{u"x"_s, proto::TodoStatus::Completed}, {u"y"_s, proto::TodoStatus::Pending}};
	info.lastError = u"boom"_s;
	info.busySince = 77;
	model.setSessions({info});

	const QModelIndex row = model.index(0);
	QCOMPARE(row.data(SessionListModel::DisplayTitleRole).toString(), u"project"_s);
	QCOMPARE(row.data(SessionListModel::DirNameRole).toString(), u"project"_s);
	QCOMPARE(row.data(SessionListModel::ModelLabelRole).toString(), u"m · high"_s);
	QCOMPARE(row.data(SessionListModel::StateRole).toString(), u"waiting_permission"_s);
	QVERIFY(row.data(SessionListModel::HasPermissionRole).toBool());
	QCOMPARE(row.data(SessionListModel::TodoDoneRole).toInt(), 1);
	QCOMPARE(row.data(SessionListModel::TodoTotalRole).toInt(), 2);
	QVERIFY(!row.data(SessionListModel::TodoAllDoneRole).toBool());
	QVERIFY(row.data(SessionListModel::HasErrorRole).toBool());
	QCOMPARE(row.data(SessionListModel::BusySinceRole).toLongLong(), 77);
	QVERIFY(row.data(SessionListModel::IsLiveRole).toBool());

	info.title = u"A title"_s;
	model.setSessions({info});
	QCOMPARE(model.index(0).data(SessionListModel::DisplayTitleRole).toString(), u"A title"_s);
	info.name = u"Mine"_s;
	model.setSessions({info});
	QCOMPARE(model.index(0).data(SessionListModel::DisplayTitleRole).toString(), u"Mine"_s);
}

void TstSessionList::historyRowsAreLabelledByTitle()
{
	SessionListModel model;
	proto::SessionInfo titled = stored(u"titled"_s, 3);
	titled.name = u"parser-fix"_s;
	titled.title = u"Fix  parser\npanic "_s;
	proto::SessionInfo named = stored(u"named"_s, 2);
	named.name = u"bench-ideas"_s;
	proto::SessionInfo bare = stored(u"bare"_s, 1);
	proto::SessionInfo live = session(u"live"_s, 4);
	live.name = u"live-name"_s;
	live.title = u"A title"_s;
	model.setSessions({titled, named, bare, live});

	const auto label = [&](const QString& id) {
		const QStringList order = ids(model);
		return model.index(int(order.indexOf(id))).data(SessionListModel::DisplayTitleRole).toString();
	};
	QCOMPARE(label(u"titled"_s), u"Fix parser panic"_s); // the title on one line, not the name
	QCOMPARE(label(u"named"_s), u"bench-ideas"_s); // no title: the name
	QCOMPARE(label(u"bare"_s), u"project"_s); // neither: the directory
	QCOMPARE(label(u"live"_s), u"live-name"_s); // live sessions go by name
}

void TstSessionList::marksCurrentAndTiles()
{
	SessionListModel model;
	model.setSessions({session(u"a"_s, 1), session(u"b"_s, 2)});
	QSignalSpy changed(&model, &QAbstractItemModel::dataChanged);

	model.setCurrentId(u"a"_s);
	QCOMPARE(changed.count(), 1);
	QVERIFY(model.index(1).data(SessionListModel::IsCurrentRole).toBool());
	QVERIFY(!model.index(0).data(SessionListModel::IsCurrentRole).toBool());

	model.setTileIds({u"b"_s});
	QVERIFY(model.index(0).data(SessionListModel::IsTileRole).toBool());
	model.setTileIds({});
	QVERIFY(!model.index(0).data(SessionListModel::IsTileRole).toBool());

	// The flags survive a new list.
	model.setSessions({session(u"a"_s, 1), session(u"b"_s, 2)});
	QVERIFY(model.index(1).data(SessionListModel::IsCurrentRole).toBool());
}

void TstSessionList::directoryLoadsAndFollowsFrames()
{
	FakeBackend fake;
	fake.onRequest = [&fake](const QJsonObject& request) {
		const QString op = request.value(u"op"_s).toString();
		if(op == u"list_sessions"_s)
		{
			fake.respond(request, {{u"sessions"_s, QJsonArray{infoJson(u"s1"_s, 1, true)}}});
		}
		else if(op == u"list_history"_s)
		{
			fake.respond(request, {{u"sessions"_s, QJsonArray{infoJson(u"s1"_s, 1, true), infoJson(u"old"_s, 0, false)}}});
		}
	};
	Backend backend(Backend::Source(Backend::Mode::Remote{{fake.url(), u"token"_s}}), u"/work"_s);
	SessionDirectory directory(backend);
	backend.start();

	QTRY_COMPARE(directory.live().count(), 1);
	QTRY_COMPARE(directory.history().count(), 1);
	QCOMPARE(ids(directory.history()), (QStringList{u"old"_s}));
	QVERIFY(directory.find(u"s1"_s) != nullptr);

	fake.send({{u"kind"_s, u"sessions"_s}, {u"sessions"_s, QJsonArray{infoJson(u"s1"_s, 1, true), infoJson(u"s2"_s, 2, true)}}});
	QTRY_COMPARE(directory.live().count(), 2);
	QCOMPARE(ids(directory.live()), (QStringList{u"s2"_s, u"s1"_s}));
	QCOMPARE(directory.liveSessions().size(), 2);
}

void TstSessionList::directoryHistoryHidesLiveSessions()
{
	FakeBackend fake;
	QJsonArray history{infoJson(u"s1"_s, 1, false), infoJson(u"old"_s, 0, false)};
	fake.onRequest = [&](const QJsonObject& request) {
		const QString op = request.value(u"op"_s).toString();
		if(op == u"list_sessions"_s)
		{
			fake.respond(request, {{u"sessions"_s, QJsonArray()}});
		}
		else if(op == u"list_history"_s)
		{
			fake.respond(request, {{u"sessions"_s, history}});
		}
	};
	Backend backend(Backend::Source(Backend::Mode::Remote{{fake.url(), u"token"_s}}), u"/work"_s);
	SessionDirectory directory(backend);
	backend.start();
	QTRY_COMPARE(directory.history().count(), 2);

	// A session going live leaves the history at once; going back shows after the refetch.
	fake.send({{u"kind"_s, u"sessions"_s}, {u"sessions"_s, QJsonArray{infoJson(u"s1"_s, 1, true)}}});
	QTRY_COMPARE(directory.history().count(), 1);
	fake.send({{u"kind"_s, u"sessions"_s}, {u"sessions"_s, QJsonArray()}});
	QTRY_COMPARE_WITH_TIMEOUT(directory.history().count(), 2, 3000);
}

void TstSessionList::configLoadsAgentsModelsAndMcp()
{
	FakeBackend fake;
	fake.onRequest = [&fake](const QJsonObject& request) {
		const QString op = request.value(u"op"_s).toString();
		if(op == u"get_config"_s)
		{
			const QJsonObject config{
				{u"default_agent"_s, u"build"_s},
				{u"agents"_s, QJsonArray{QJsonObject{{u"name"_s, u"build"_s}, {u"description"_s, u"Builds"_s}}}},
				{u"models"_s,
			     QJsonArray{QJsonObject{{u"provider"_s, u"p"_s},
			                            {u"provider_name"_s, u"P"_s},
			                            {u"id"_s, u"m"_s},
			                            {u"name"_s, u"M"_s},
			                            {u"variants"_s, QJsonArray{u"low"_s, u"high"_s}}}}},
				{u"commands"_s, QJsonArray{QJsonObject{{u"name"_s, u"review"_s}, {u"description"_s, u"Review"_s}, {u"source"_s, u"config"_s}}}},
			};
			fake.respond(request, {{u"config"_s, config}});
		}
		else if(op == u"mcp_status"_s)
		{
			fake.respond(request,
			             {{u"servers"_s,
			               QJsonArray{QJsonObject{{u"name"_s, u"fs"_s}, {u"status"_s, u"connected"_s}, {u"tools"_s, QJsonArray{u"fs_read"_s}}}}}});
		}
	};
	Backend backend(Backend::Source(Backend::Mode::Remote{{fake.url(), u"token"_s}}), u"/work"_s);
	BackendConfig config(backend);
	backend.start();

	QTRY_COMPARE(config.agents().size(), 1);
	QCOMPARE(config.defaultAgent(), u"build"_s);
	QCOMPARE(config.models().first().toMap().value(u"id"_s).toString(), u"m"_s);
	QCOMPARE(config.commands().first().toMap().value(u"source"_s).toString(), u"config"_s);
	QCOMPARE(config.variantsOf(u"p"_s, u"m"_s), (QStringList{u"low"_s, u"high"_s}));
	QVERIFY(config.variantsOf(u"p"_s, u"unknown"_s).isEmpty());
	QTRY_COMPARE(config.mcpServers().size(), 1);
	QCOMPARE(config.mcpServers().first().toMap().value(u"status"_s).toString(), u"connected"_s);
}

QTEST_MAIN(TstSessionList)
#include "tst_session_list.moc"
