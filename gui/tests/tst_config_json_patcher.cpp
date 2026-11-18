#include "workspace/config_json_patcher.h"

#include <QJsonArray>
#include <QJsonObject>
#include <QtTest>

using namespace Qt::StringLiterals;
using refgui::workspace::ConfigJsonPatcher;

class TestConfigJsonPatcher : public QObject
{
	Q_OBJECT

private slots:
	void parsesJsoncCommentsAndTrailingCommasPreservingTokens();
	void rejectsMalformedJsoncAndNonObjectRoots();
	void mutatesNestedPathsLosslesslyPreservingSiblingsAndSpecialKeys();
	void renamesKeysLosslesslyAndRejectsCollisions();
	void scansEnvReferencesAndDetectsInterpolations();
	void preservesExactInsertionOrderAndDetectsKeyReordering();
	void preserves64BitIntegersShortestFloatsSurrogatesAndStrictSyntax();
};

void TestConfigJsonPatcher::parsesJsoncCommentsAndTrailingCommasPreservingTokens()
{
	const QString raw = u"{\n"
	                    "  // Global profile name and comments inside strings: \"http://example.com/*\"\n"
	                    "  \"$name\": \"Work /* keep */ // keep\",\n"
	                    "  \"model\": \"anthropic/claude-sonnet-4-5\",\n"
	                    "  /* Block comment\n"
	                    "     spanning lines */\n"
	                    "  \"provider\": {\n"
	                    "    \"work\": {\n"
	                    "      \"options\": {\n"
	                    "        \"baseURL\": \"https://api.example.com/v1\",\n"
	                    "        \"apiKey\": \"{env:WORK_API_KEY}\",\n"
	                    "        \"headers\": {\n"
	                    "          \"X-Team\": \"{file:~/.config/ref/team-id}\",\n"
	                    "        },\n"
	                    "        \"extraBody\": {\n"
	                    "          \"vendor_flag\": true,\n"
	                    "        },\n"
	                    "      },\n"
	                    "      \"models\": {\n"
	                    "        \"anthropic/claude-sonnet-4.5\": {\n"
	                    "          \"limit\": { \"context\": 200000, },\n"
	                    "        },\n"
	                    "      },\n"
	                    "    },\n"
	                    "  },\n"
	                    "}\n"_s;

	const auto parsed = ConfigJsonPatcher::parseJsonc(raw);
	QVERIFY2(parsed.has_value(), parsed ? "" : qPrintable(parsed.error().message));
	ConfigJsonPatcher patcher = *parsed;

	QCOMPARE(patcher.valueAt({u"$name"_s}).toString(), u"Work /* keep */ // keep"_s);
	QCOMPARE(patcher.valueAt({u"provider"_s, u"work"_s, u"options"_s, u"apiKey"_s}).toString(), u"{env:WORK_API_KEY}"_s);
	QCOMPARE(patcher.valueAt({u"provider"_s, u"work"_s, u"options"_s, u"headers"_s, u"X-Team"_s}).toString(),
	         u"{file:~/.config/ref/team-id}"_s);
	QCOMPARE(patcher.valueAt({u"provider"_s, u"work"_s, u"models"_s, u"anthropic/claude-sonnet-4.5"_s, u"limit"_s, u"context"_s}).toInt(),
	         200000);
}

void TestConfigJsonPatcher::rejectsMalformedJsoncAndNonObjectRoots()
{
	QVERIFY(!ConfigJsonPatcher::parseJsonc(u"[1, 2, 3]"_s).has_value());
	QVERIFY(!ConfigJsonPatcher::parseJsonc(u"\"just a string\""_s).has_value());
	QVERIFY(!ConfigJsonPatcher::parseJsonc(u"{ /* unclosed"_s).has_value());
	QVERIFY(!ConfigJsonPatcher::parseJsonc(u"{ \"a\": }"_s).has_value());
}

void TestConfigJsonPatcher::mutatesNestedPathsLosslesslyPreservingSiblingsAndSpecialKeys()
{
	const auto parsed = ConfigJsonPatcher::parseJsonc(uR"({
		"provider": {
			"openai": {
				"options": {
					"apiKey": "{env:OPENAI_API_KEY}",
					"extraBody": { "store": false, "service_tier": "flex" }
				}
			}
		},
		"permission": {
			"edit": { "*.lock": "deny" }
		},
		"unknownTopLevel": { "custom": 42 }
	})"_s);
	QVERIFY(parsed.has_value());
	ConfigJsonPatcher patcher = *parsed;

	patcher.setValueAt({u"provider"_s, u"openai"_s, u"options"_s, u"chunkTimeout"_s}, 120000);
	patcher.setValueAt({u"permission"_s, u"edit"_s, u"src/**"_s}, u"allow"_s);
	patcher.setValueAt({u"retry"_s, u"max_delay"_s}, 300);

	// Untouched siblings, `{env:...}` strings, `extraBody`, and unknown keys survive intact.
	QCOMPARE(patcher.valueAt({u"provider"_s, u"openai"_s, u"options"_s, u"apiKey"_s}).toString(), u"{env:OPENAI_API_KEY}"_s);
	QCOMPARE(patcher.valueAt({u"provider"_s, u"openai"_s, u"options"_s, u"extraBody"_s, u"service_tier"_s}).toString(), u"flex"_s);
	QCOMPARE(patcher.valueAt({u"provider"_s, u"openai"_s, u"options"_s, u"chunkTimeout"_s}).toInt(), 120000);
	QCOMPARE(patcher.valueAt({u"permission"_s, u"edit"_s, u"*.lock"_s}).toString(), u"deny"_s);
	QCOMPARE(patcher.valueAt({u"permission"_s, u"edit"_s, u"src/**"_s}).toString(), u"allow"_s);
	QCOMPARE(patcher.valueAt({u"unknownTopLevel"_s, u"custom"_s}).toInt(), 42);

	// Removing the only key under `retry` prunes the empty `retry` object; removing one sibling keeps the rest.
	patcher.removeAt({u"retry"_s, u"max_delay"_s});
	QVERIFY(!patcher.contains({u"retry"_s}));

	patcher.removeAt({u"permission"_s, u"edit"_s, u"src/**"_s});
	QVERIFY(patcher.contains({u"permission"_s, u"edit"_s}));
	QCOMPARE(patcher.valueAt({u"permission"_s, u"edit"_s, u"*.lock"_s}).toString(), u"deny"_s);

	// Round-tripping through toJson() preserves everything.
	const QString serialized = patcher.toJson();
	const auto reparsed = ConfigJsonPatcher::parseJsonc(serialized);
	QVERIFY(reparsed.has_value());
	QCOMPARE(reparsed->root(), patcher.root());
}

void TestConfigJsonPatcher::renamesKeysLosslesslyAndRejectsCollisions()
{
	ConfigJsonPatcher patcher;
	patcher.setValueAt({u"mcp"_s, u"old-srv"_s, u"url"_s}, u"https://mcp.example.com"_s);
	patcher.setValueAt({u"mcp"_s, u"existing"_s, u"enabled"_s}, false);

	QVERIFY(!patcher.renameKeyAt({u"mcp"_s}, u"old-srv"_s, u"existing"_s));
	QVERIFY(!patcher.renameKeyAt({u"mcp"_s}, u"missing"_s, u"new-srv"_s));
	QVERIFY(patcher.renameKeyAt({u"mcp"_s}, u"old-srv"_s, u"new-srv"_s));
	QVERIFY(!patcher.contains({u"mcp"_s, u"old-srv"_s}));
	QCOMPARE(patcher.valueAt({u"mcp"_s, u"new-srv"_s, u"url"_s}).toString(), u"https://mcp.example.com"_s);
}

void TestConfigJsonPatcher::scansEnvReferencesAndDetectsInterpolations()
{
	const auto parsed = ConfigJsonPatcher::parseJsonc(uR"({
		"provider": {
			"a": {
				"options": {
					"apiKey": "{env:ALPHA_KEY}",
					"headers": {
						"Authorization": "Bearer {env:BETA_TOKEN}-{env:ALPHA_KEY}",
						"X-Cert": "{file:~/.certs/client.pem}"
					}
				}
			}
		},
		"mcp": {
			"{env:IGNORED_KEY_NAME}": {
				"command": ["node", "{env:GAMMA_SCRIPT}"]
			}
		}
	})"_s);
	QVERIFY(parsed.has_value());
	QCOMPARE(parsed->envReferences(), (QStringList{u"ALPHA_KEY"_s, u"BETA_TOKEN"_s, u"GAMMA_SCRIPT"_s}));

	QVERIFY(ConfigJsonPatcher::hasInterpolation(u"{env:OPENAI_API_KEY}"_s));
	QVERIFY(ConfigJsonPatcher::hasInterpolation(u"Bearer {file:./token.txt}"_s));
	QVERIFY(!ConfigJsonPatcher::hasInterpolation(u"sk-proj-literal-secret"_s));
	QVERIFY(!ConfigJsonPatcher::hasInterpolation(u"{env:}"_s));
	QVERIFY(!ConfigJsonPatcher::hasInterpolation(u"{env:UNCLOSED"_s));
}

void TestConfigJsonPatcher::preservesExactInsertionOrderAndDetectsKeyReordering()
{
	const auto parsed = ConfigJsonPatcher::parseJsonc(uR"({
		"zebra": 1,
		"permission": {
			"edit": {
				"*.env": "deny",
				"src/**": "allow",
				"*": "ask"
			},
			"bash": "ask"
		},
		"alpha": 2
	})"_s);
	QVERIFY2(parsed.has_value(), parsed ? "" : qPrintable(parsed.error().message));
	ConfigJsonPatcher patcher = *parsed;

	QCOMPARE(patcher.orderedRoot().keys(), (QStringList{u"zebra"_s, u"permission"_s, u"alpha"_s}));
	QCOMPARE(patcher.orderedValueAt({u"permission"_s}).toObject().keys(), (QStringList{u"edit"_s, u"bash"_s}));
	QCOMPARE(patcher.orderedValueAt({u"permission"_s, u"edit"_s}).toObject().keys(),
	         (QStringList{u"*.env"_s, u"src/**"_s, u"*"_s}));

	// Updating an existing key keeps its position; adding a new key appends at end.
	patcher.setValueAt({u"permission"_s, u"edit"_s, u"src/**"_s}, u"deny"_s);
	QCOMPARE(patcher.orderedValueAt({u"permission"_s, u"edit"_s}).toObject().keys(),
	         (QStringList{u"*.env"_s, u"src/**"_s, u"*"_s}));
	patcher.setValueAt({u"middle"_s}, true);
	QCOMPARE(patcher.orderedRoot().keys(), (QStringList{u"zebra"_s, u"permission"_s, u"alpha"_s, u"middle"_s}));

	// Renaming mutates the key in place without moving it to the end.
	QVERIFY(patcher.renameKeyAt({}, u"zebra"_s, u"omega"_s));
	QCOMPARE(patcher.orderedRoot().keys(), (QStringList{u"omega"_s, u"permission"_s, u"alpha"_s, u"middle"_s}));
	patcher.removeAt({u"alpha"_s});
	QCOMPARE(patcher.orderedRoot().keys(), (QStringList{u"omega"_s, u"permission"_s, u"middle"_s}));

	const QString serialized = patcher.toJson();
	QVERIFY(serialized.indexOf(u"\"*.env\""_s) < serialized.indexOf(u"\"src/**\""_s));
	QVERIFY(serialized.indexOf(u"\"src/**\""_s) < serialized.indexOf(u"\"*\""_s));
	QVERIFY(serialized.indexOf(u"\"omega\""_s) < serialized.indexOf(u"\"permission\""_s));
	QVERIFY(serialized.indexOf(u"\"permission\""_s) < serialized.indexOf(u"\"middle\""_s));

	// OrderedJsonObject::operator== compares exact key order as well as values.
	const auto firstOrder = ConfigJsonPatcher::parseJsonc(uR"({"a": 1, "b": 2})"_s);
	const auto secondOrder = ConfigJsonPatcher::parseJsonc(uR"({"b": 2, "a": 1})"_s);
	QVERIFY(firstOrder.has_value() && secondOrder.has_value());
	QVERIFY(firstOrder->orderedRoot() != secondOrder->orderedRoot());
}

void TestConfigJsonPatcher::preserves64BitIntegersShortestFloatsSurrogatesAndStrictSyntax()
{
	constexpr qint64 ExactIntBeyondDouble = 9007199254740993LL; // 2^53 + 1
	constexpr int ExcessiveJsonDepth = 130;
	constexpr int AllowedJsonDepth = 64;

	const auto parsed = ConfigJsonPatcher::parseJsonc(uR"({
		"context": 200000,
		"largeId": 9007199254740993,
		"negLargeId": -9007199254740993,
		"threshold": 0.6,
		"emoji": "\uD83D\uDE80"
	})"_s);
	QVERIFY2(parsed.has_value(), parsed ? "" : qPrintable(parsed.error().message));
	const ConfigJsonPatcher& patcher = *parsed;

	QVERIFY(patcher.orderedValueAt({u"context"_s}).isInt());
	QCOMPARE(patcher.orderedValueAt({u"context"_s}).toInt(), 200000);
	QVERIFY(patcher.orderedValueAt({u"largeId"_s}).isInt());
	QCOMPARE(patcher.orderedValueAt({u"largeId"_s}).toInt64(), ExactIntBeyondDouble);
	QCOMPARE(patcher.orderedValueAt({u"negLargeId"_s}).toInt64(), -ExactIntBeyondDouble);
	QVERIFY(patcher.orderedValueAt({u"threshold"_s}).isDouble());
	QCOMPARE(patcher.orderedValueAt({u"threshold"_s}).toDouble(), 0.6);
	QCOMPARE(patcher.orderedValueAt({u"emoji"_s}).toString(), u"🚀"_s);

	const QString serialized = patcher.toJson();
	QVERIFY(serialized.contains(u"\"context\": 200000"_s));
	QVERIFY(serialized.contains(u"\"largeId\": 9007199254740993"_s));
	QVERIFY(serialized.contains(u"\"negLargeId\": -9007199254740993"_s));
	QVERIFY(serialized.contains(u"\"threshold\": 0.6"_s));

	// Leading commas, lone commas, double commas, and unpaired UTF-16 surrogates are rejected.
	QVERIFY(!ConfigJsonPatcher::parseJsoncValue(u"[,1]"_s).has_value());
	QVERIFY(!ConfigJsonPatcher::parseJsoncValue(u"[ , ]"_s).has_value());
	QVERIFY(!ConfigJsonPatcher::parseJsonc(u"{,\"a\": 1}"_s).has_value());
	QVERIFY(!ConfigJsonPatcher::parseJsonc(u"{ , }"_s).has_value());
	QVERIFY(!ConfigJsonPatcher::parseJsonc(u"{\"a\": 1,,}"_s).has_value());
	QVERIFY(!ConfigJsonPatcher::parseJsonc(u"{\"bad\": \"\\uD800\"}"_s).has_value());
	QVERIFY(!ConfigJsonPatcher::parseJsonc(u"{\"bad\": \"\\uDC00\"}"_s).has_value());
	QVERIFY(!ConfigJsonPatcher::parseJsonc(u"{\"bad\": \"\\uD800x\"}"_s).has_value());

	const QString okDepth = u"{\"x\": "_s + QString(AllowedJsonDepth, u'[') + u'1' + QString(AllowedJsonDepth, u']') + u'}';
	QVERIFY(ConfigJsonPatcher::parseJsonc(okDepth).has_value());
	const QString tooDeep = u"{\"x\": "_s + QString(ExcessiveJsonDepth, u'[') + u'1' + QString(ExcessiveJsonDepth, u']') + u'}';
	QVERIFY(!ConfigJsonPatcher::parseJsonc(tooDeep).has_value());
}

QTEST_GUILESS_MAIN(TestConfigJsonPatcher)
#include "tst_config_json_patcher.moc"
