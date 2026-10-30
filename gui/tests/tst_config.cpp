#include "llm/config.h"

#include <QTest>

using namespace Qt::StringLiterals;

class TestConfig : public QObject
{
	Q_OBJECT

private slots:
	void parsesEndpoint();
	void apiKeyIsOptional();
	void rejects_data();
	void rejects();
	void reportsMissingFile();
};

void TestConfig::parsesEndpoint()
{
	const auto endpoint = qutie::llm::parseEndpoint(R"({
		// local server
		"endpoint": {
			"baseUrl": "http://localhost:8080/v1",
			"model": "qwen3",
			"apiKey": "sk-test",
		},
	})"_ba);

	QVERIFY2(endpoint.has_value(), endpoint.has_value() ? "" : qPrintable(endpoint.error()));
	QCOMPARE(endpoint->baseUrl, QUrl(u"http://localhost:8080/v1"_s));
	QCOMPARE(endpoint->model, u"qwen3"_s);
	QCOMPARE(endpoint->apiKey, u"sk-test"_s);
}

void TestConfig::apiKeyIsOptional()
{
	const auto endpoint = qutie::llm::parseEndpoint(R"({"endpoint": {"baseUrl": "https://api.example.com/v1", "model": "m"}})"_ba);

	QVERIFY(endpoint.has_value());
	QVERIFY(endpoint->apiKey.isEmpty());
}

void TestConfig::rejects_data()
{
	QTest::addColumn<QByteArray>("source");
	QTest::addColumn<QString>("mentions");

	QTest::newRow("syntax error") << R"({"endpoint": })"_ba << u"line 1"_s;
	QTest::newRow("no endpoint") << R"({})"_ba << u"endpoint.baseUrl"_s;
	QTest::newRow("no model") << R"({"endpoint": {"baseUrl": "http://x/v1"}})"_ba << u"endpoint.model"_s;
	QTest::newRow("not http") << R"({"endpoint": {"baseUrl": "ftp://x/v1", "model": "m"}})"_ba << u"ftp://x/v1"_s;
}

void TestConfig::rejects()
{
	QFETCH(QByteArray, source);
	QFETCH(QString, mentions);

	const auto endpoint = qutie::llm::parseEndpoint(source);

	QVERIFY(!endpoint.has_value());
	QVERIFY2(endpoint.error().contains(mentions), qPrintable(endpoint.error()));
}

void TestConfig::reportsMissingFile()
{
	const QString path = u"/nonexistent/qutie/config.jsonc"_s;
	const auto endpoint = qutie::llm::loadEndpoint(path);

	QVERIFY(!endpoint.has_value());
	QVERIFY(endpoint.error().contains(path));
	QVERIFY(endpoint.error().contains(u"\"baseUrl\""_s)); // shows an example
}

QTEST_GUILESS_MAIN(TestConfig)
#include "tst_config.moc"
