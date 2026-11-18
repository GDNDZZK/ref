#include "workspace/config_editor_controller.h"

#include "coro_test.h"
#include "fake_backend.h"

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSignalSpy>
#include <QtTest>

using namespace Qt::StringLiterals;
using namespace refgui;
using namespace refgui::test;
using refgui::client::Backend;

namespace
{

class StubConfigServer
{
public:
	explicit StubConfigServer(FakeBackend& fake)
		: m_fake(fake)
	{
		m_activeRaw = u"{\n  // user comment\n  \"$name\": \"Work\",\n  \"model\": \"anthropic/claude-sonnet-4\",\n  \"provider\": {\n    \"anthropic\": {\n      \"options\": {\n        \"apiKey\": \"{env:ANTHROPIC_API_KEY}\",\n        \"extraBody\": {\"custom\": 42}\n      }\n    }\n  },\n  \"unknownFutureKey\": {\"keep\": true}\n}\n"_s;
		m_backupRaw = u"{\n  \"$name\": \"Experiment\",\n  \"model\": \"openai/gpt-5\"\n}\n"_s;

		fake.onHello = [this](const QJsonObject& hello) { m_fake.respondHello(hello, proto::ProtocolVersion); };
		fake.onRequest = [this](const QJsonObject& request) { answer(request); };
	}

	QString m_activeRaw;
	QString m_backupRaw;
	bool m_hasBackup = true;
	bool m_activeExists = true;
	QStringList m_warnings;
	QList<QJsonObject> m_requests;

private:
	QJsonObject buildFilesResponse(const QString& selected) const
	{
		const QString raw = selected == u"ref.json"_s ? m_activeRaw : m_backupRaw;
		const auto parsedPatcher = workspace::ConfigJsonPatcher::parseJsonc(raw);
		QJsonValue parsedVal = QJsonValue::Null;
		QString parseErr;
		if(parsedPatcher)
		{
			parsedVal = parsedPatcher->root();
		}
		else
		{
			parseErr = parsedPatcher.error().message;
		}

		QJsonArray files;
		files.append(QJsonObject{
			{u"file"_s, u"ref.json"_s},
			{u"active"_s, true},
			{u"exists"_s, m_activeExists},
			{u"label"_s, u"Work"_s},
			{u"summary"_s, u"1 provider · anthropic/claude-sonnet-4"_s},
			{u"modified_at"_s, 1760000060000LL},
			{u"size"_s, m_activeRaw.size()},
		});
		if(m_hasBackup)
		{
			files.append(QJsonObject{
				{u"file"_s, u"ref.json.1.bak"_s},
				{u"active"_s, false},
				{u"exists"_s, true},
				{u"label"_s, u"Experiment"_s},
				{u"summary"_s, u"openai/gpt-5"_s},
				{u"modified_at"_s, 1760000000000LL},
				{u"size"_s, m_backupRaw.size()},
			});
		}

		QJsonObject res{
			{u"config_dir"_s, u"/tmp/ref-cfg"_s},
			{u"files"_s, files},
			{u"selected_file"_s, selected},
			{u"raw_text"_s, raw},
			{u"parsed"_s, parsedVal},
			{u"env_status"_s, QJsonObject{{u"ANTHROPIC_API_KEY"_s, true}}},
			{u"has_jsonc_override"_s, true},
			{u"project_overrides"_s, QJsonArray{u"/work/.ref/ref.json"_s}},
			{u"warnings"_s, QJsonArray::fromStringList(m_warnings)},
		};
		if(!parseErr.isEmpty())
		{
			res.insert(u"parse_error"_s, parseErr);
		}
		return res;
	}

	void answer(const QJsonObject& request)
	{
		m_requests.append(request);
		const QString op = request.value(u"op"_s).toString();
		if(op == u"get_config"_s)
		{
			m_fake.respond(request, {{u"config"_s, QJsonObject{
				{u"default_agent"_s, u"build"_s},
				{u"agents"_s, QJsonArray{QJsonObject{{u"name"_s, u"build"_s}, {u"description"_s, u"Builds"_s}}}},
				{u"models"_s, QJsonArray{QJsonObject{
					{u"provider"_s, u"anthropic"_s},
					{u"provider_name"_s, u"Anthropic"_s},
					{u"id"_s, u"claude-sonnet-4"_s},
					{u"name"_s, u"Claude Sonnet 4"_s},
				}}},
			}}});
		}
		else if(op == u"mcp_status"_s)
		{
			m_fake.respond(request, {{u"servers"_s, QJsonArray()}});
		}
		else if(op == u"get_config_files"_s)
		{
			const QString file = request.value(u"file"_s).toString(u"ref.json"_s);
			m_fake.respond(request, buildFilesResponse(file));
		}
		else if(op == u"save_config_file"_s)
		{
			const QString file = request.value(u"file"_s).toString();
			const QString content = request.value(u"content"_s).toString();
			if(file == u"ref.json"_s)
			{
				m_activeRaw = content;
				m_activeExists = true;
			}
			else
			{
				m_backupRaw = content;
			}
			m_fake.respond(request, buildFilesResponse(file));
		}
		else if(op == u"create_config_file"_s)
		{
			m_backupRaw = m_activeRaw;
			m_hasBackup = true;
			const bool cloneCurrent = request.value(u"clone_current"_s).toBool();
			const QString label = request.value(u"label"_s).toString();
			QJsonObject obj;
			if(cloneCurrent)
			{
				if(const auto p = workspace::ConfigJsonPatcher::parseJsonc(m_activeRaw))
				{
					obj = p->root();
				}
			}
			if(!label.isEmpty())
			{
				obj.insert(u"$name"_s, label);
			}
			m_activeRaw = QString::fromUtf8(QJsonDocument(obj).toJson(QJsonDocument::Indented));
			m_fake.respond(request, buildFilesResponse(u"ref.json"_s));
		}
		else if(op == u"select_config_file"_s)
		{
			std::swap(m_activeRaw, m_backupRaw);
			m_fake.respond(request, buildFilesResponse(u"ref.json"_s));
		}
		else if(op == u"delete_config_file"_s)
		{
			const QString file = request.value(u"file"_s).toString();
			if(file == u"ref.json.1.bak"_s)
			{
				m_hasBackup = false;
			}
			m_fake.respond(request, buildFilesResponse(u"ref.json"_s));
		}
		else if(op == u"reload_config"_s)
		{
			m_fake.respond(request, {{u"config"_s, QJsonObject{
				{u"default_agent"_s, u"build"_s},
				{u"agents"_s, QJsonArray()},
				{u"models"_s, QJsonArray()},
			}}, {u"warnings"_s, QJsonArray::fromStringList(m_warnings)}});
		}
	}

	FakeBackend& m_fake;
};

struct EditorFixture
{
	FakeBackend fake;
	StubConfigServer stub{fake};
	workspace::Connection connection{Backend::Source(Backend::Mode::Remote{{fake.url(), u"token"_s}}), u"/work"_s};
	workspace::BackendConfig backendConfig{connection};
	workspace::ConfigEditorController editor{connection, backendConfig};

	EditorFixture()
	{
		backendConfig.setCwd(u"/work"_s);
		connection.start();
	}
};

} // namespace

class TestConfigEditor : public QObject
{
	Q_OBJECT

private slots:
	void openLoadsProfilesAndPreservesUnknownKeysAndInterpolations()
	{
		EditorFixture f;
		QTRY_VERIFY(f.connection.isConnected());

		f.editor.open(u"/work"_s);
		QTRY_VERIFY(!f.editor.isLoading());
		QCOMPARE(f.editor.selectedFile(), u"ref.json"_s);
		QVERIFY(f.editor.isSelectedFileActive());
		QVERIFY(f.editor.selectedFileExists());
		QCOMPARE(f.editor.files().size(), 2);
		QVERIFY(f.editor.hasJsoncOverride());
		QCOMPARE(f.editor.projectOverrides(), (QStringList{u"/work/.ref/ref.json"_s}));
		QCOMPARE(f.editor.envStatus().value(u"ANTHROPIC_API_KEY"_s).toBool(), true);
		QCOMPARE(f.editor.mode(), u"visual"_s);
		QVERIFY(!f.editor.isDirty());

		// Visual mutation keeps sibling/unknown keys and {env:...} intact.
		f.editor.setValueAt({u"compaction"_s, u"auto"_s}, false);
		QVERIFY(f.editor.isDirty());
		QCOMPARE(f.editor.valueAt({u"provider"_s, u"anthropic"_s, u"options"_s, u"apiKey"_s}).toString(),
		         u"{env:ANTHROPIC_API_KEY}"_s);
		QCOMPARE(f.editor.valueAt({u"provider"_s, u"anthropic"_s, u"options"_s, u"extraBody"_s, u"custom"_s}).toInt(), 42);
		QCOMPARE(f.editor.valueAt({u"unknownFutureKey"_s, u"keep"_s}).toBool(), true);

		// Discard restores clean baseline.
		f.editor.discard();
		QVERIFY(!f.editor.isDirty());
		QVERIFY(!f.editor.contains({u"compaction"_s, u"auto"_s}));
	}

	void modeSwitchingValidatesJsoncAndBlocksBrokenSyntax()
	{
		EditorFixture f;
		QTRY_VERIFY(f.connection.isConnected());
		f.editor.open();
		QTRY_VERIFY(!f.editor.isLoading());

		// Unmodified visual -> json keeps raw comments intact.
		QVERIFY(f.editor.setMode(u"json"_s));
		QVERIFY(f.editor.rawText().contains(u"// user comment"_s));

		// Broken JSON blocks switching back to visual and populates parseError.
		f.editor.setRawText(u"{ broken_json: "_s);
		QVERIFY(f.editor.isDirty());
		QVERIFY(!f.editor.parseError().isEmpty());
		QVERIFY(!f.editor.setMode(u"visual"_s));
		QCOMPARE(f.editor.mode(), u"json"_s);

		// Valid JSONC with comments & trailing comma switches cleanly to visual.
		f.editor.setRawText(u"{\n  // ok\n  \"model\": \"openai/gpt-5\",\n}\n"_s);
		QVERIFY(f.editor.parseError().isEmpty());
		QVERIFY(f.editor.setMode(u"visual"_s));
		QCOMPARE(f.editor.mode(), u"visual"_s);
		QCOMPARE(f.editor.valueAt({u"model"_s}).toString(), u"openai/gpt-5"_s);
	}

	void saveCreateSelectAndDeleteProfiles()
	{
		EditorFixture f;
		QTRY_VERIFY(f.connection.isConnected());
		f.editor.open(u"/work"_s);
		QTRY_VERIFY(!f.editor.isLoading());

		QSignalSpy savedSpy(&f.editor, &workspace::ConfigEditorController::saved);
		f.editor.setStringAt({u"default_agent"_s}, u"plan"_s);
		QVERIFY(f.editor.isDirty());

		f.editor.save();
		QTRY_COMPARE(savedSpy.count(), 1);
		QVERIFY(!f.editor.isDirty());
		QVERIFY(f.stub.m_activeRaw.contains(u"\"default_agent\": \"plan\""_s));

		// Load backup without activating.
		f.editor.loadFile(u"ref.json.1.bak"_s);
		QTRY_COMPARE(f.editor.selectedFile(), u"ref.json.1.bak"_s);
		QVERIFY(!f.editor.isSelectedFileActive());
		QCOMPARE(f.editor.valueAt({u"model"_s}).toString(), u"openai/gpt-5"_s);

		// Activate backup -> swaps into ref.json.
		f.editor.activateProfile(u"ref.json.1.bak"_s);
		QTRY_COMPARE(f.editor.selectedFile(), u"ref.json"_s);
		QVERIFY(f.editor.isSelectedFileActive());
		QCOMPARE(f.editor.valueAt({u"model"_s}).toString(), u"openai/gpt-5"_s);

		// Create new profile cloning current.
		f.editor.createProfile(u"Fresh Clone"_s, true);
		QTRY_COMPARE(f.editor.valueAt({u"$name"_s}).toString(), u"Fresh Clone"_s);
		QCOMPARE(f.editor.valueAt({u"model"_s}).toString(), u"openai/gpt-5"_s);

		// Delete backup profile.
		f.editor.deleteProfile(u"ref.json.1.bak"_s);
		QTRY_COMPARE(f.editor.files().size(), 1);
	}

	void brokenDiskFileOpensDirectlyInJsonMode()
	{
		EditorFixture f;
		f.stub.m_activeRaw = u"{ invalid "_s;
		QTRY_VERIFY(f.connection.isConnected());

		f.editor.open();
		QTRY_VERIFY(!f.editor.isLoading());
		QCOMPARE(f.editor.mode(), u"json"_s);
		QVERIFY(!f.editor.parseError().isEmpty());
	}

	void providerPresetsAndSerializedJsonHelpersWork()
	{
		EditorFixture f;
		QTRY_VERIFY(f.connection.isConnected());
		f.editor.open();
		QTRY_VERIFY(!f.editor.isLoading());

		const QString createdKey = f.editor.addProviderPreset(u"ollama"_s);
		QCOMPARE(createdKey, u"ollama"_s);
		QCOMPARE(f.editor.valueAt({u"provider"_s, u"ollama"_s, u"options"_s, u"baseURL"_s}).toString(),
		         u"http://localhost:11434/v1"_s);

		QVERIFY(f.editor.renameKeyAt({u"provider"_s}, u"ollama"_s, u"local-vllm"_s));
		QVERIFY(f.editor.contains({u"provider"_s, u"local-vllm"_s}));

		const QString err = f.editor.setSerializedJsonAt({u"permission"_s}, u"{\"bash\": {\"*\": \"ask\"}}"_s);
		QVERIFY(err.isEmpty());
		QCOMPARE(f.editor.valueAt({u"permission"_s, u"bash"_s, u"*"_s}).toString(), u"ask"_s);
	}
};

QTEST_GUILESS_MAIN(TestConfigEditor)
#include "tst_config_editor.moc"
