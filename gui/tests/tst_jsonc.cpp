#include "infra/jsonc.h"

#include <QTest>

using refgui::jsonc::parse;

class TestJsonc : public QObject
{
	Q_OBJECT

private slots:
	void acceptsExtensions_data();
	void acceptsExtensions();

	void rejectsInvalid_data();
	void rejectsInvalid();

	void reportsSourcePosition_data();
	void reportsSourcePosition();
};

void TestJsonc::acceptsExtensions_data()
{
	QTest::addColumn<QByteArray>("source");
	QTest::addColumn<QByteArray>("equivalentJson");

	QTest::newRow("plain json") << QByteArray(R"({"a": [1, 2]})") << QByteArray(R"({"a": [1, 2]})");
	QTest::newRow("line comment") << QByteArray("{\"a\": 1 // note\n}") << QByteArray(R"({"a": 1})");
	QTest::newRow("block comment across lines") << QByteArray("{/* x\n y */\"a\": 1}") << QByteArray(R"({"a": 1})");
	QTest::newRow("comment markers inside string") << QByteArray(R"({"u": "http://x/*y*/"})") << QByteArray(R"({"u": "http://x/*y*/"})");
	QTest::newRow("escaped quote inside string") << QByteArray(R"({"a": "\"//"})") << QByteArray(R"({"a": "\"//"})");
	QTest::newRow("escaped backslash ends string") << QByteArray("{\"a\": \"x\\\\\", // c\n}") << QByteArray(R"({"a": "x\\"})");
	QTest::newRow("trailing comma in object") << QByteArray(R"({"a": 1,})") << QByteArray(R"({"a": 1})");
	QTest::newRow("trailing comma in array") << QByteArray(R"([1, 2, ])") << QByteArray(R"([1, 2])");
	QTest::newRow("trailing comma before comment") << QByteArray("[1, // c\n]") << QByteArray(R"([1])");
	QTest::newRow("comma and bracket inside string") << QByteArray(R"({"a": ",]"})") << QByteArray(R"({"a": ",]"})");
	QTest::newRow("nested trailing commas") << QByteArray(R"({"a": [1,], "b": {"c": 2,},})") << QByteArray(R"({"a": [1], "b": {"c": 2}})");
}

void TestJsonc::acceptsExtensions()
{
	QFETCH(QByteArray, source);
	QFETCH(QByteArray, equivalentJson);

	const auto result = parse(source);
	if(!result)
	{
		QFAIL(qPrintable(result.error().message));
	}
	QCOMPARE(*result, QJsonDocument::fromJson(equivalentJson));
}

void TestJsonc::rejectsInvalid_data()
{
	QTest::addColumn<QByteArray>("source");

	QTest::newRow("unterminated block comment") << QByteArray(R"({"a": 1 /* oops)");
	QTest::newRow("lone comma") << QByteArray(R"([,])");
	QTest::newRow("double comma") << QByteArray(R"([1,,])");
	QTest::newRow("comma after opening brace") << QByteArray(R"({,})");
	QTest::newRow("single slash") << QByteArray(R"({"a": 1 / 2})");
	QTest::newRow("scalar top level") << QByteArray(R"(42)");
}

void TestJsonc::rejectsInvalid()
{
	QFETCH(QByteArray, source);

	QVERIFY(!parse(source).has_value());
}

void TestJsonc::reportsSourcePosition_data()
{
	QTest::addColumn<QByteArray>("source");
	QTest::addColumn<int>("line");
	QTest::addColumn<int>("column");

	QTest::newRow("unterminated block comment") << QByteArray("{\n  /* x\n}") << 2 << 3;
	// "é" is 2 UTF-8 bytes but 1 UTF-16 code unit, so the comment starts at column 9.
	QTest::newRow("column counts characters") << QByteArray("{\"\xc3\xa9\": 1 /*") << 1 << 9;
}

void TestJsonc::reportsSourcePosition()
{
	QFETCH(QByteArray, source);
	QFETCH(int, line);
	QFETCH(int, column);

	const auto result = parse(source);
	QVERIFY(!result.has_value());
	QCOMPARE(result.error().line, line);
	QCOMPARE(result.error().column, column);
}

QTEST_GUILESS_MAIN(TestJsonc)
#include "tst_jsonc.moc"
