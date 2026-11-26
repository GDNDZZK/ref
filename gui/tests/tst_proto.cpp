#include "proto/parse.h"
#include "proto/request.h"

#include <QDirIterator>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QtTest>

using namespace Qt::StringLiterals;
using namespace refgui::proto;

namespace
{

QJsonDocument readFixture(const QString& path)
{
	QFile file(path);
	[&] { QVERIFY2(file.open(QIODevice::ReadOnly), qPrintable(path)); }();
	QJsonParseError error;
	const QJsonDocument document = QJsonDocument::fromJson(file.readAll(), &error);
	[&] { QVERIFY2(error.error == QJsonParseError::NoError, qPrintable(path + u": "_s + error.errorString())); }();
	return document;
}

// One data row per fixture file in `kind`/, named after the file.
void addFixtureRows(const QString& kind)
{
	QTest::addColumn<QString>("path");
	const QString directory = QStringLiteral(REF_FIXTURES_DIR) + u'/' + kind;
	QDirIterator files(directory, {u"*.json"_s}, QDir::Files);
	int count = 0;
	while(files.hasNext())
	{
		const QString path = files.next();
		QTest::newRow(qPrintable(files.fileInfo().completeBaseName())) << path;
		++count;
	}
	QVERIFY2(count > 0, qPrintable(u"no fixtures in "_s + directory));
}

// A fixture whose root is a plain JSON value, such as a tool state ("running").
QJsonValue readFixtureValue(const QString& path)
{
	QFile file(path);
	[&] { QVERIFY2(file.open(QIODevice::ReadOnly), qPrintable(path)); }();
	const QJsonDocument wrapped = QJsonDocument::fromJson("{\"value\":" + file.readAll() + "}");
	return wrapped.object().value(u"value"_s);
}

QJsonObject fixtureObject(const QString& kind, const QString& name)
{
	return readFixture(QStringLiteral(REF_FIXTURES_DIR) + u'/' + kind + u'/' + name + u".json"_s).object();
}

template<typename T>
T parsedOrFail(const Parsed<T>& parsed)
{
	if(!parsed)
	{
		qFatal("%s", qPrintable(parsed.error().message));
	}
	return *parsed;
}

// What a successful response to the request the fixture is named after must parse as.
void checkResponseResult(const QString& name, const QJsonObject& result)
{
	if(name == u"response_hello")
	{
		QCOMPARE(parsedOrFail(parseHelloResult(result)).protocol, ProtocolVersion);
	}
	else if(name == u"response_create_session" || name == u"response_set_model")
	{
		QVERIFY(parseSessionResult(result));
	}
	else if(name == u"response_attach")
	{
		QVERIFY(parseAttachResult(result));
	}
	else if(name == u"response_list_sessions" || name == u"response_list_history" || name == u"response_watch_all")
	{
		QVERIFY(parseSessionsResult(result));
	}
	else if(name == u"response_prompt")
	{
		QVERIFY(parsePromptResult(result));
	}
	else if(name == u"response_messages")
	{
		QVERIFY(parseMessagesResult(result));
	}
	else if(name == u"response_get_config")
	{
		QVERIFY(parseConfigResult(result));
	}
	else if(name == u"response_reload_config")
	{
		QVERIFY(parseReloadConfigResult(result));
	}
	else if(name == u"response_get_config_files")
	{
		QVERIFY(parseConfigFilesResult(result));
	}
	else if(name == u"response_mcp_status")
	{
		QVERIFY(parseMcpResult(result));
	}
	else if(name == u"response_list_dirs")
	{
		QVERIFY(parseDirsResult(result));
	}
	else if(name == u"response_list_binaries")
	{
		QVERIFY(parseBinariesResult(result));
	}
	else if(name == u"response_complete_shell"_s)
	{
		QVERIFY(parseShellCompletionResult(result));
	}
	else if(name == u"response_peer_command" || name == u"response_expand_command")
	{
		QVERIFY(parseTextResult(result));
	}
	else if(name == u"response_export")
	{
		QVERIFY(parsePathResult(result));
	}
	else if(name == u"response_retry_now")
	{
		QVERIFY(parseRetryNowResult(result));
	}
	else if(name == u"response_stash_all")
	{
		QVERIFY(parseStashAllResult(result));
	}
	else if(name == u"response_create_group")
	{
		QVERIFY(parseCreateGroupResult(result));
	}
	else if(name == u"response_list_groups" || name == u"response_rename_group" || name == u"response_set_group_locked"
	        || name == u"response_delete_group" || name == u"response_add_to_group" || name == u"response_move_group_member"
	        || name == u"response_remove_from_group" || name == u"response_merge_groups")
	{
		QVERIFY(parseGroupsResult(result));
	}
	else if(name == u"response_restore_group")
	{
		QVERIFY(parseRestoreGroupResult(result));
	}
	else if(name == u"response_restore_group_member")
	{
		QVERIFY(parseRestoreGroupMemberResult(result));
	}
}
} // namespace

class TestProto : public QObject
{
	Q_OBJECT

private slots:
	void serverFrames_data() { addFixtureRows(u"server_frame"_s); }
	void serverFrames()
	{
		QFETCH(QString, path);
		const QJsonObject frame = readFixture(path).object();
		const Parsed<ServerFrame> parsed = parseServerFrame(frame);
		QVERIFY2(parsed, parsed ? "" : qPrintable(parsed.error().message));

		const auto* response = std::get_if<ResponseFrame>(&*parsed);
		if(response != nullptr && response->ok)
		{
			checkResponseResult(QFileInfo(path).completeBaseName(), response->result);
		}
	}

	void sessionEvents_data() { addFixtureRows(u"session_event"_s); }
	void sessionEvents()
	{
		QFETCH(QString, path);
		const Parsed<SessionEvent> parsed = parseSessionEvent(readFixture(path).object());
		QVERIFY2(parsed, parsed ? "" : qPrintable(parsed.error().message));
	}

	void parts_data() { addFixtureRows(u"part"_s); }
	void parts()
	{
		QFETCH(QString, path);
		const Parsed<Part> parsed = parsePart(readFixture(path).object());
		QVERIFY2(parsed, parsed ? "" : qPrintable(parsed.error().message));
		QVERIFY(!std::holds_alternative<UnknownPart>(*parsed));
	}

	void deltas_data() { addFixtureRows(u"delta"_s); }
	void deltas()
	{
		QFETCH(QString, path);
		const Parsed<Delta> parsed = parseDelta(readFixture(path).object());
		QVERIFY2(parsed, parsed ? "" : qPrintable(parsed.error().message));
	}

	void toolStates_data() { addFixtureRows(u"tool_state"_s); }
	void toolStates()
	{
		QFETCH(QString, path);
		const Parsed<ToolState> parsed = parseToolState(readFixtureValue(path));
		QVERIFY2(parsed, parsed ? "" : qPrintable(parsed.error().message));
	}

	void requestFixturesAreOperations_data() { addFixtureRows(u"request"_s); }
	void requestFixturesAreOperations()
	{
		QFETCH(QString, path);
		const QJsonObject request = readFixture(path).object();
		const QString operation = request.value(u"op"_s).toString();
		QVERIFY(!operation.isEmpty());
		QVERIFY(QFileInfo(path).completeBaseName().startsWith(operation)); // e.g. prompt_with_image: prompt
	}

	void requestBuildersMatchFixtures()
	{
		const auto withoutId = [](const QString& name) {
			QJsonObject request = fixtureObject(u"request"_s, name);
			request.remove(u"id"_s);
			return request;
		};

		QCOMPARE(request::attach(u"ses_a1b2c3d4"_s, true), [&] {
			QJsonObject expected = withoutId(u"attach"_s);
			expected.remove(u"limit"_s); // the client does not set a page size
			return expected;
		}());
		QCOMPARE(request::detach(u"ses_a1b2c3d4"_s), withoutId(u"detach"_s));
		QCOMPARE(request::listSessions(), withoutId(u"list_sessions"_s));
		QCOMPARE(request::listHistory(u"/home/user/project"_s, 50), withoutId(u"list_history"_s));
		const QJsonObject filteredHistory = request::listHistory(u"/home/user/project"_s, 50, u"user"_s, u"parser"_s);
		QCOMPARE(filteredHistory.value(u"creator"_s).toString(), u"user"_s);
		QCOMPARE(filteredHistory.value(u"query"_s).toString(), u"parser"_s);
		QVERIFY(!request::listHistory(u"/home/user/project"_s, 50, QString(), QString()).contains(u"creator"_s));
		QVERIFY(!request::listHistory(u"/home/user/project"_s, 50, QString(), QString()).contains(u"query"_s));
		QCOMPARE(request::abort(u"ses_a1b2c3d4"_s), withoutId(u"abort"_s));
		QCOMPARE(request::permissionReply(u"ses_a1b2c3d4"_s, u"call_9x8y7z"_s, true, false), withoutId(u"permission_reply"_s));
		QCOMPARE(request::prompt(u"ses_a1b2c3d4"_s, u"Fix the failing parser test"_s), withoutId(u"prompt"_s));
		QCOMPARE(request::questionReply(u"ses_a1b2c3d4"_s, u"call_9x8y7z"_s, QList<QStringList>{{u"Exclusive range"_s}, {u"Unit tests"_s, u"Clippy"_s}}),
		         withoutId(u"question_reply"_s));
		QCOMPARE(request::questionReply(u"ses_a1b2c3d4"_s, u"call_9x8y7z"_s, std::nullopt), withoutId(u"question_reply_dismiss"_s));

		QCOMPARE(request::getConfig(u"/home/user/project"_s), withoutId(u"get_config"_s));
		QVERIFY(!request::getConfig().contains(u"cwd"_s));
		QCOMPARE(request::watchAll(true), withoutId(u"watch_all"_s));
		QCOMPARE(request::setAgent(u"ses_a1b2c3d4"_s, u"plan"_s), withoutId(u"set_agent"_s));
		QCOMPARE(request::setVariant(u"ses_a1b2c3d4"_s, u"low"_s), withoutId(u"set_variant"_s));
		QCOMPARE(request::rename(u"ses_a1b2c3d4"_s, u"parser-fix-2"_s), withoutId(u"rename"_s));
		QCOMPARE(request::setCwd(u"ses_a1b2c3d4"_s, u"~/project/crates/parser"_s), withoutId(u"set_cwd"_s));
		QCOMPARE(request::compact(u"ses_a1b2c3d4"_s), withoutId(u"compact"_s));
		QCOMPARE(request::close(u"ses_a1b2c3d4"_s), withoutId(u"close"_s));
		QCOMPARE(request::deleteSession(u"ses_a1b2c3d4"_s), withoutId(u"delete"_s));
		QCOMPARE(request::exportTranscript(u"ses_a1b2c3d4"_s, u"/tmp/parser-fix.md"_s), withoutId(u"export"_s));
		QCOMPARE(request::peerCommand(u"ses_a1b2c3d4"_s, u"peers"_s, QString()), withoutId(u"peer_command"_s));
		QCOMPARE(request::mcpStatus(u"/home/user/project"_s), withoutId(u"mcp_status"_s));
		QCOMPARE(request::listDirs(u"~/pro"_s, u"/home/user/project"_s, true), withoutId(u"list_dirs"_s));
		QVERIFY(!request::listDirs(u"~/pro"_s).contains(u"cwd"_s));
		QCOMPARE(request::listDirs(u"~/pro"_s).value(u"include_files"_s).toBool(), false);
		QCOMPARE(request::reloadConfig(u"/home/user/project"_s), withoutId(u"reload_config"_s));
		QVERIFY(!request::reloadConfig().contains(u"cwd"_s));
		QCOMPARE(request::getConfigFiles(std::nullopt, u"/home/user/project"_s), withoutId(u"get_config_files"_s));
		QVERIFY(!request::getConfigFiles().contains(u"file"_s));
		QVERIFY(!request::getConfigFiles().contains(u"cwd"_s));
		QCOMPARE(request::saveConfigFile(u"ref.json"_s, u"{\n  \"$name\": \"Work\",\n  \"model\": \"anthropic/claude-sonnet-4\"\n}\n"_s, u"/home/user/project"_s),
		         withoutId(u"save_config_file"_s));
		QCOMPARE(request::createConfigFile(u"Experiment"_s, true, std::nullopt, u"/home/user/project"_s), withoutId(u"create_config_file"_s));
		QCOMPARE(request::selectConfigFile(u"ref.json.1.bak"_s, u"/home/user/project"_s), withoutId(u"select_config_file"_s));
		QCOMPARE(request::deleteConfigFile(u"ref.json.1.bak"_s, u"/home/user/project"_s), withoutId(u"delete_config_file"_s));
		QCOMPARE(request::listBinaries(u"car"_s, 20), withoutId(u"list_binaries"_s));
		QVERIFY(!request::listBinaries(u"car"_s).contains(u"limit"_s));
		QCOMPARE(request::completeShell(u"/home/user/project"_s, u"git che"_s, 7, 50), withoutId(u"complete_shell"_s));
		const QJsonObject minimalCompleteReq = request::completeShell(u"/work"_s, u"git "_s);
		QVERIFY(!minimalCompleteReq.contains(u"cursor"_s));
		QVERIFY(!minimalCompleteReq.contains(u"limit"_s));
		QCOMPARE(request::shutdown(), withoutId(u"shutdown"_s));
		QCOMPARE(request::ping(), withoutId(u"ping"_s));
		QCOMPARE(request::setModel(u"ses_a1b2c3d4"_s, {.provider = u"anthropic"_s, .model = u"claude-sonnet-4"_s, .variant = u"high"_s}),
		         withoutId(u"set_model"_s));
		QVERIFY(!request::setVariant(u"ses_a1b2c3d4"_s, std::nullopt).contains(u"variant"_s)); // rotates

		QJsonObject createSession = withoutId(u"create_session"_s);
		createSession.remove(u"name"_s); // the GUI leaves naming to the backend
		const ModelRef model{.provider = u"anthropic"_s, .model = u"claude-sonnet-4"_s, .variant = u"high"_s};
		QCOMPARE(request::createSession(u"/home/user/project"_s, u"build"_s, model), createSession);

		QJsonObject hello = withoutId(u"hello"_s);
		hello.insert(u"client"_s, u"ref-gui"_s);
		hello.insert(u"notify"_s, QJsonArray());
		QCOMPARE(request::hello(u"ref-gui"_s, u"/home/user/project"_s, u"3f9c1d2e"_s), hello);

		QCOMPARE(request::messages(u"ses_a1b2c3d4"_s, u"msg_user_01"_s, 100), withoutId(u"messages"_s));
		QCOMPARE(request::messagesRange(u"ses_a1b2c3d4"_s, 0, 50, 50), withoutId(u"messages_range"_s));
		QCOMPARE(request::retryNow(u"ses_a1b2c3d4"_s), withoutId(u"retry_now"_s));
		QCOMPARE(request::listGroups(), withoutId(u"list_groups"_s));
		QCOMPARE(request::stashAll(u"Parser investigation"_s), withoutId(u"stash_all"_s));
		QVERIFY(!request::stashAll().contains(u"name"_s));
		QCOMPARE(request::createGroup(u"Parser investigation"_s, {u"ses_a1b2c3d4"_s}, true), withoutId(u"create_group"_s));
		QCOMPARE(request::renameGroup(u"grp_a1b2c3d4"_s, u"Parser fix"_s), withoutId(u"rename_group"_s));
		QCOMPARE(request::setGroupLocked(u"grp_a1b2c3d4"_s, true), withoutId(u"set_group_locked"_s));
		QCOMPARE(request::deleteGroup(u"grp_a1b2c3d4"_s), withoutId(u"delete_group"_s));
		QCOMPARE(request::addToGroup(u"grp_a1b2c3d4"_s, u"ses_a1b2c3d4"_s, 0U, true), withoutId(u"add_to_group"_s));
		QVERIFY(!request::addToGroup(u"grp_a1b2c3d4"_s, u"ses_a1b2c3d4"_s).contains(u"position"_s));
		QCOMPARE(request::moveGroupMember(u"grp_a1b2c3d4"_s, u"grp_e5f6a7b8"_s, u"ses_a1b2c3d4"_s, 0U, true),
		         withoutId(u"move_group_member"_s));
		QVERIFY(!request::moveGroupMember(std::nullopt, u"grp_e5f6a7b8"_s, u"ses_a1b2c3d4"_s, 0U, false).contains(u"from_group_id"_s));
		QCOMPARE(request::removeFromGroup(u"grp_a1b2c3d4"_s, u"ses_a1b2c3d4"_s), withoutId(u"remove_from_group"_s));
		QCOMPARE(request::mergeGroups(u"grp_e5f6a7b8"_s, u"grp_a1b2c3d4"_s), withoutId(u"merge_groups"_s));
		QCOMPARE(request::restoreGroup(u"grp_a1b2c3d4"_s), withoutId(u"restore_group"_s));
		QCOMPARE(request::restoreGroupMember(u"grp_a1b2c3d4"_s, u"ses_a1b2c3d4"_s), withoutId(u"restore_group_member"_s));
	}
	void configResult()
	{
		const QJsonObject frame = fixtureObject(u"server_frame"_s, u"response_get_config"_s);
		const ServerConfig config = parsedOrFail(parseConfigResult(frame.value(u"result"_s).toObject()));
		QCOMPARE(config.defaultAgent, u"build"_s);
		QCOMPARE(config.agents.size(), 2);
		QVERIFY(config.agents.at(0).canSpawn);
		QCOMPARE(config.agents.at(0).color, u"#3b82f6"_s);
		QVERIFY(!config.agents.at(1).color.has_value());
		QCOMPARE(config.models.first().variants, (QStringList{u"low"_s, u"medium"_s, u"high"_s}));
		QCOMPARE(config.commands.first().source, u"config"_s);
		QVERIFY(!config.mcp.isEmpty());
		QVERIFY(config.warnings.isEmpty());
	}

	void v5ConfigAndCompletionFixtures()
	{
		const auto changedFrame = parsedOrFail(parseServerFrame(fixtureObject(u"server_frame"_s, u"config_changed"_s)));
		const auto& changed = std::get<ConfigChangedFrame>(changedFrame);
		QCOMPARE(changed.warnings, (QStringList{u"default_agent \"custom\" not found"_s}));

		const QJsonObject reloadFrame = fixtureObject(u"server_frame"_s, u"response_reload_config"_s);
		const ReloadConfigResult reload = parsedOrFail(parseReloadConfigResult(reloadFrame.value(u"result"_s).toObject()));
		QCOMPARE(reload.config.defaultAgent, u"build"_s);
		QVERIFY(reload.warnings.isEmpty());

		const QJsonObject filesFrame = fixtureObject(u"server_frame"_s, u"response_get_config_files"_s);
		const ConfigFilesInfo files = parsedOrFail(parseConfigFilesResult(filesFrame.value(u"result"_s).toObject()));
		QCOMPARE(files.configDir, u"/home/user/.config/ref"_s);
		QCOMPARE(files.selectedFile, u"ref.json"_s);
		QCOMPARE(files.files.size(), 2);
		QVERIFY(files.files.at(0).active);
		QVERIFY(files.files.at(0).exists);
		QCOMPARE(files.files.at(0).label, u"Work"_s);
		QVERIFY(!files.files.at(1).active);
		QVERIFY(!files.files.at(1).label.has_value());
		QVERIFY(files.parsed.isObject());
		QVERIFY(!files.parseError.has_value());
		QCOMPARE(files.envStatus.value(u"ANTHROPIC_API_KEY"_s), true);
		QVERIFY(!files.hasJsoncOverride);
		QVERIFY(!files.envConfigOverride.has_value());
		QCOMPARE(files.projectOverrides, (QStringList{u"/home/user/project/.ref/ref.json"_s}));
		QVERIFY(files.warnings.isEmpty());

		const QJsonObject dirsFrame = fixtureObject(u"server_frame"_s, u"response_list_dirs"_s);
		const DirListing dirs = parsedOrFail(parseDirsResult(dirsFrame.value(u"result"_s).toObject()));
		QCOMPARE(dirs.path, u"/home/user/project"_s);
		QCOMPARE(dirs.dirs, (QStringList{u"benches"_s, u"crates"_s, u"docs"_s}));
		QCOMPARE(dirs.files, (QStringList{u"Cargo.toml"_s, u"README.md"_s}));

		const QJsonObject binsFrame = fixtureObject(u"server_frame"_s, u"response_list_binaries"_s);
		const QStringList bins = parsedOrFail(parseBinariesResult(binsFrame.value(u"result"_s).toObject()));
		QCOMPARE(bins, (QStringList{u"cargo"_s, u"cargo-clippy"_s, u"cargo-fmt"_s}));

		const QJsonObject shellCompFrame = fixtureObject(u"server_frame"_s, u"response_complete_shell"_s);
		const QList<ShellCompletionItem> fixtureShellItems = parsedOrFail(parseShellCompletionResult(shellCompFrame.value(u"result"_s).toObject()));
		QCOMPARE(fixtureShellItems.size(), 2);
		QCOMPARE(fixtureShellItems.at(0).value, u"checkout"_s);
		QVERIFY(!fixtureShellItems.at(0).description.isEmpty());

		const QJsonObject shellCompResult{{u"items"_s,
		                                   QJsonArray{QJsonObject{{u"value"_s, u"checkout"_s}, {u"description"_s, u"Switch branches"_s}},
		                                              QJsonObject{{u"value"_s, u"--format="_s}},
		                                              QJsonObject{{u"value"_s, u"status"_s}, {u"description"_s, QJsonValue::Null}}}}};
		const QList<ShellCompletionItem> shellItems = parsedOrFail(parseShellCompletionResult(shellCompResult));
		QCOMPARE(shellItems.size(), 3);
		QCOMPARE(shellItems.at(0), (ShellCompletionItem{.value = u"checkout"_s, .description = u"Switch branches"_s}));
		QCOMPARE(shellItems.at(1), (ShellCompletionItem{.value = u"--format="_s, .description = QString()}));
		QCOMPARE(shellItems.at(2), (ShellCompletionItem{.value = u"status"_s, .description = QString()}));
		QVERIFY(!parseShellCompletionResult({{u"items"_s, QJsonArray{QJsonObject{{u"description"_s, u"missing value"_s}}}}}));
		QVERIFY(!parseShellCompletionResult({{u"items"_s, u"not an array"_s}}));
	}

	void malformedConfigIsAnError()
	{
		QVERIFY(!parseConfigResult({{u"config"_s, QJsonObject{{u"agents"_s, QJsonArray{1}}}}}));
		QVERIFY(!parseMcpResult({{u"servers"_s, QJsonArray{QJsonObject{{u"name"_s, u"x"_s}}}}})); // no status
	}

	void toolMessageContent()
	{
		const Message message = parsedOrFail(parseMessage(fixtureObject(u"session_event"_s, u"message"_s).value(u"message"_s).toObject()));
		QCOMPARE(message.role, Role::Assistant);
		QCOMPARE(message.parts.size(), 3);
		QVERIFY(std::holds_alternative<ReasoningPart>(message.parts.at(0)));
		QVERIFY(std::get<ReasoningPart>(message.parts.at(0)).timeEnd.has_value());
		const auto& tool = std::get<ToolPart>(message.parts.at(2));
		QCOMPARE(tool.name, u"edit"_s);
		QVERIFY(tool.input.isObject());
		QVERIFY(!tool.rawInput.isEmpty());
	}

	void peerMessageSource()
	{
		const Message message = parsedOrFail(parseMessage(fixtureObject(u"session_event"_s, u"message_peer"_s).value(u"message"_s).toObject()));
		QCOMPARE(message.source.kind, MessageSource::Kind::Peer);
		QVERIFY(!message.source.fromName.isEmpty());
		QVERIFY(!message.source.msgId.isEmpty());
	}

	void retryingStatus()
	{
		const auto event = parsedOrFail(parseSessionEvent(fixtureObject(u"session_event"_s, u"info_retrying"_s)));
		const SessionStatus& status = std::get<InfoEvent>(event).info.status;
		QCOMPARE(status.state, SessionStatus::State::Retrying);
		QCOMPARE(status.attempt, 2U);
		QVERIFY(!status.error.isEmpty());
	}

	void snapshotKeepsImageStubs()
	{
		const auto snapshot = parsedOrFail(parseAttachResult(fixtureObject(u"server_frame"_s, u"response_attach"_s).value(u"result"_s).toObject()));
		QVERIFY(snapshot.moreBefore);
		QVERIFY(!snapshot.pendingPermissions.isEmpty());
		const auto& image = std::get<ImagePart>(snapshot.messages.at(0).parts.at(1));
		QVERIFY(image.image.data.isEmpty());
		QVERIFY(image.image.id.has_value());
	}

	void snapshotCarriesPendingQuestions()
	{
		const auto snapshot = parsedOrFail(parseAttachResult(fixtureObject(u"server_frame"_s, u"response_attach"_s).value(u"result"_s).toObject()));
		QCOMPARE(snapshot.pendingQuestions.size(), 1);
		const QuestionRequest& request = snapshot.pendingQuestions.first();
		QCOMPARE(request.callId, u"call_9x8y7z"_s);
		QCOMPARE(request.questions.size(), 2);
	}

	void questionEventKeepsOptionsAndFlags()
	{
		const auto event = parsedOrFail(parseSessionEvent(fixtureObject(u"session_event"_s, u"question"_s)));
		const QuestionRequest& request = std::get<QuestionEvent>(event).request;
		QCOMPARE(request.sessionId, u"ses_a1b2c3d4"_s);
		QCOMPARE(request.messageId, u"msg_0f1e2d3c"_s);
		QCOMPARE(request.questions.size(), 2);
		const Question& single = request.questions.at(0);
		QCOMPARE(single.header, u"Loop bound"_s);
		QVERIFY(!single.multiple);
		QVERIFY(single.custom);
		QCOMPARE(single.options.size(), 2);
		QCOMPARE(single.options.at(0).label, u"Exclusive range"_s);
		QCOMPARE(single.options.at(0).description, u"Use `0..items.len()`"_s);
		QVERIFY(single.options.at(0).isDefault);
		QVERIFY(!single.options.at(1).isDefault);
		const Question& several = request.questions.at(1);
		QVERIFY(several.multiple);
		QVERIFY(!several.custom);
	}

	void questionFlagsHaveDefaults()
	{
		QJsonObject event = fixtureObject(u"session_event"_s, u"question"_s);
		QJsonObject request = event.value(u"request"_s).toObject();
		QJsonObject question = request.value(u"questions"_s).toArray().first().toObject();
		question.remove(u"multiple"_s);
		question.remove(u"custom"_s);
		QJsonObject option = question.value(u"options"_s).toArray().first().toObject();
		option.remove(u"default"_s);
		question.insert(u"options"_s, QJsonArray{option});
		request.insert(u"questions"_s, QJsonArray{question});
		event.insert(u"request"_s, request);

		const Question parsed = std::get<QuestionEvent>(parsedOrFail(parseSessionEvent(event))).request.questions.first();
		QVERIFY(!parsed.multiple);
		QVERIFY(parsed.custom); // custom answers are allowed unless the backend says no
		QVERIFY(!parsed.options.first().isDefault);
	}

	void questionNotifyCarriesTheFirstQuestion()
	{
		const auto frame = parsedOrFail(parseServerFrame(fixtureObject(u"server_frame"_s, u"notify_question"_s)));
		const auto& notify = std::get<NotifyFrame>(frame);
		QCOMPARE(notify.kind, NotifyFrame::Kind::Question);
		QCOMPARE(notify.detail.value_or(QString()), u"How should the loop bound be fixed?"_s);
	}

	void waitingQuestionIsAStatus()
	{
		const auto event = parsedOrFail(parseSessionEvent(fixtureObject(u"session_event"_s, u"info_waiting_question"_s)));
		QCOMPARE(std::get<InfoEvent>(event).info.status.state, SessionStatus::State::WaitingQuestion);
	}

	void questionResolvedNamesTheCall()
	{
		const auto event = parsedOrFail(parseSessionEvent(fixtureObject(u"session_event"_s, u"question_resolved"_s)));
		QCOMPARE(std::get<QuestionResolvedEvent>(event).callId, u"call_9x8y7z"_s);
	}

	void toolStatesAreFinalOnlyAtTheEnd()
	{
		QVERIFY(!isFinal(ToolState::Streaming));
		QVERIFY(!isFinal(ToolState::AwaitingPermission));
		QVERIFY(!isFinal(ToolState::Delayed));
		QVERIFY(!isFinal(ToolState::Running));
		QVERIFY(isFinal(ToolState::Done));
		QVERIFY(isFinal(ToolState::Error));
		QVERIFY(isFinal(ToolState::Denied));
		QVERIFY(isFinal(ToolState::Cancelled));
	}

	void unknownFrameKindIsReportedAsUnknown()
	{
		const Parsed<ServerFrame> parsed = parseServerFrame({{u"kind"_s, u"from_the_future"_s}});
		QVERIFY(!parsed);
		QVERIFY(parsed.error().unknown);
	}

	void unknownEventTypeIsReportedAsUnknown()
	{
		const Parsed<SessionEvent> parsed = parseSessionEvent({{u"type"_s, u"from_the_future"_s}});
		QVERIFY(!parsed);
		QVERIFY(parsed.error().unknown);
	}

	void unknownPartTypeKeepsItsSlot()
	{
		const Part part = parsedOrFail(parsePart({{u"type"_s, u"from_the_future"_s}}));
		QCOMPARE(std::get<UnknownPart>(part).type, u"from_the_future"_s);
	}

	void missingRequiredFieldNamesThePath()
	{
		QJsonObject message = fixtureObject(u"session_event"_s, u"message"_s).value(u"message"_s).toObject();
		QJsonArray parts = message.value(u"parts"_s).toArray();
		QJsonObject tool = parts.at(2).toObject();
		tool.remove(u"state"_s);
		parts.replace(2, tool);
		message.insert(u"parts"_s, parts);

		const Parsed<Message> parsed = parseMessage(message);
		QVERIFY(!parsed);
		QVERIFY(!parsed.error().unknown);
		QVERIFY2(parsed.error().message.contains(u"message.parts[2].state"_s), qPrintable(parsed.error().message));
	}

	void failedResponseAlwaysSaysWhy()
	{
		const ServerFrame withoutOk = parsedOrFail(parseServerFrame({{u"kind"_s, u"response"_s}, {u"id"_s, 1}}));
		QCOMPARE(std::get<ResponseFrame>(withoutOk).error, u"the backend sent a response without ok"_s);

		const ServerFrame withoutReason = parsedOrFail(parseServerFrame({{u"kind"_s, u"response"_s}, {u"id"_s, 1}, {u"ok"_s, false}}));
		QVERIFY(!std::get<ResponseFrame>(withoutReason).ok);
		QCOMPARE(std::get<ResponseFrame>(withoutReason).error, u"the backend gave no reason"_s);
	}

	void errorResponseHasNoResult()
	{
		const ServerFrame frame = parsedOrFail(parseServerFrame(fixtureObject(u"server_frame"_s, u"response_error"_s)));
		const auto& response = std::get<ResponseFrame>(frame);
		QVERIFY(!response.ok);
		QVERIFY(response.result.isEmpty());
		QVERIFY(!response.error.isEmpty());
		QCOMPARE(response.code, u"not_found"_s);
	}
	void permissionPreviewParsesAndFallsBackWhenAbsent()
	{
		const auto event = parsedOrFail(parseSessionEvent(fixtureObject(u"session_event"_s, u"permission"_s)));
		const PermissionRequest& request = std::get<PermissionEvent>(event).request;
		QVERIFY(request.preview.has_value());
		QCOMPARE(request.preview->added, 1U);
		QCOMPARE(request.preview->removed, 1U);
		QVERIFY(!request.preview->truncated);
		QVERIFY(request.preview->diff.contains(u"+    for i in 0..items.len() {"_s));

		QJsonObject withoutPreview = fixtureObject(u"session_event"_s, u"permission"_s);
		QJsonObject req = withoutPreview.value(u"request"_s).toObject();
		req.remove(u"preview"_s);
		withoutPreview.insert(u"request"_s, req);
		const auto fallback = parsedOrFail(parseSessionEvent(withoutPreview));
		QVERIFY(!std::get<PermissionEvent>(fallback).request.preview.has_value());
	}

	void v6LayoutMetasRangeRetryAndStashFixtures()
	{
		const auto snapshot = parsedOrFail(parseAttachResult(fixtureObject(u"server_frame"_s, u"response_attach"_s).value(u"result"_s).toObject()));
		QCOMPARE(snapshot.totalMessages, quint64(3));
		QCOMPARE(snapshot.startSeq, quint64(1));
		QCOMPARE(snapshot.layoutMetas.size(), 1);
		QCOMPARE(snapshot.layoutMetas.at(0).seq, quint64(0));
		QCOMPARE(snapshot.layoutMetas.at(0).id, u"msg_older_00"_s);
		QCOMPARE(snapshot.layoutMetas.at(0).role, Role::User);
		QCOMPARE(snapshot.layoutMetas.at(0).sourceKind, u"human"_s);
		QCOMPARE(snapshot.layoutMetas.at(0).parts.size(), 1);
		QCOMPARE(snapshot.layoutMetas.at(0).parts.at(0).kind, u"text"_s);
		QCOMPARE(snapshot.layoutMetas.at(0).parts.at(0).chars, quint32(42));
		QCOMPARE(snapshot.layoutMetas.at(0).parts.at(0).lines, quint32(2));

		const auto resetEvent = parsedOrFail(parseSessionEvent(fixtureObject(u"session_event"_s, u"reset"_s)));
		const auto& reset = std::get<ResetEvent>(resetEvent);
		QCOMPARE(reset.totalMessages, quint64(3));
		QCOMPARE(reset.startSeq, quint64(1));
		QCOMPARE(reset.layoutMetas.size(), 1);

		const auto page = parsedOrFail(parseMessagesResult(fixtureObject(u"server_frame"_s, u"response_messages"_s).value(u"result"_s).toObject()));
		QCOMPARE(page.startSeq, quint64(0));
		QCOMPARE(page.endSeq, quint64(1));
		QVERIFY(!page.moreBefore);

		const auto retry = parsedOrFail(parseRetryNowResult(fixtureObject(u"server_frame"_s, u"response_retry_now"_s).value(u"result"_s).toObject()));
		QVERIFY(retry.retried);

		const auto groupsFrame = parsedOrFail(parseServerFrame(fixtureObject(u"server_frame"_s, u"groups_changed"_s)));
		const auto& groupsChanged = std::get<GroupsChangedFrame>(groupsFrame);
		QCOMPARE(groupsChanged.groups.size(), 1);
		QCOMPARE(groupsChanged.groups.at(0).id, u"grp_a1b2c3d4"_s);
		QCOMPARE(groupsChanged.groups.at(0).members.size(), 1);
		QCOMPARE(groupsChanged.groups.at(0).members.at(0).sessionId, u"ses_a1b2c3d4"_s);
	}

	void pinningInfoRequestAndHistoryChangedFrame()
	{
		QJsonObject infoJson = fixtureObject(u"server_frame"_s, u"response_create_session"_s)
		                           .value(u"result"_s)
		                           .toObject()
		                           .value(u"session"_s)
		                           .toObject();
		QVERIFY(!parsedOrFail(parseSessionInfo(infoJson)).isPinned());

		infoJson.insert(u"pinned_at"_s, QJsonValue::Null);
		QVERIFY(!parsedOrFail(parseSessionInfo(infoJson)).isPinned());

		infoJson.insert(u"pinned_at"_s, qint64(1700000123456));
		const SessionInfo pinned = parsedOrFail(parseSessionInfo(infoJson));
		QVERIFY(pinned.isPinned());
		QCOMPARE(pinned.pinnedAt, qint64(1700000123456));

		const QJsonObject pinReq = request::setPinned(u"ses_a1b2c3d4"_s, true);
		QCOMPARE(pinReq, (QJsonObject{{u"op"_s, u"set_pinned"_s}, {u"session_id"_s, u"ses_a1b2c3d4"_s}, {u"pinned"_s, true}}));
		const QJsonObject unpinReq = request::setPinned(u"ses_a1b2c3d4"_s, false);
		QCOMPARE(unpinReq, (QJsonObject{{u"op"_s, u"set_pinned"_s}, {u"session_id"_s, u"ses_a1b2c3d4"_s}, {u"pinned"_s, false}}));

		const auto historyChanged = parsedOrFail(parseServerFrame({{u"kind"_s, u"history_changed"_s}}));
		QVERIFY(std::holds_alternative<HistoryChangedFrame>(historyChanged));
	}
};
QTEST_GUILESS_MAIN(TestProto)
#include "tst_proto.moc"
