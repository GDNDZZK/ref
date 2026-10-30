#include "compose/image_file.h"
#include "compose/prompt_history.h"

#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QTemporaryDir>
#include <QTest>

using namespace Qt::StringLiterals;
using namespace refgui::compose;

class TestPromptHistory : public QObject
{
	Q_OBJECT

private slots:
	void remembersAndSkipsRepeatsAndBlanks();
	void savesLinesTheTuiCanRead();
	void readsWhatTheTuiWrote();
	void keepsTheNewestFiveHundred();
	void browsingWalksBackAndForth();
	void browsingReturnsTheDraftAfterTheNewest();
	void browsingNeedsHistory();
	void imagePathsAreRecognised();
	void otherPathsAreText();
	void unusableImageFilesAreErrors();
};

void TestPromptHistory::remembersAndSkipsRepeatsAndBlanks()
{
	PromptHistory history;
	history.push(u"one  \n"_s);
	history.push(u"one"_s);
	history.push(u"   "_s);
	history.push(u"two"_s);
	history.push(u"one"_s);
	QCOMPARE(history.items(), (QStringList{u"one"_s, u"two"_s, u"one"_s}));
}

void TestPromptHistory::savesLinesTheTuiCanRead()
{
	QTemporaryDir dir;
	const QString path = dir.filePath(u"state/prompt-history.jsonl"_s);
	{
		PromptHistory history(path);
		history.push(u"first \"quoted\""_s);
		history.push(u"!ls -l"_s);
		history.push(u"multi\nline"_s);
	}

	QFile file(path);
	QVERIFY(file.open(QIODevice::ReadOnly));
	const QList<QByteArray> lines = file.readAll().split('\n');
	QCOMPARE(lines.size(), 4); // the last one is empty
	QCOMPARE(QJsonDocument::fromJson(lines[0]).object().value(u"text"_s).toString(), u"first \"quoted\""_s);
	QCOMPARE(QJsonDocument::fromJson(lines[2]).object().value(u"text"_s).toString(), u"multi\nline"_s);

	const PromptHistory reloaded(path);
	QCOMPARE(reloaded.items(), (QStringList{u"first \"quoted\""_s, u"!ls -l"_s, u"multi\nline"_s}));
}

void TestPromptHistory::readsWhatTheTuiWrote()
{
	QTemporaryDir dir;
	const QString path = dir.filePath(u"prompt-history.jsonl"_s);
	QFile file(path);
	QVERIFY(file.open(QIODevice::WriteOnly));
	file.write("{\"text\":\"from the tui\"}\nnot json\n{\"other\":1}\n{\"text\":\"\\u4f60\\u597d\"}\n");
	file.close();

	const PromptHistory history(path);
	QCOMPARE(history.items(), (QStringList{u"from the tui"_s, u"\u4f60\u597d"_s}));
}

void TestPromptHistory::keepsTheNewestFiveHundred()
{
	QTemporaryDir dir;
	const QString path = dir.filePath(u"prompt-history.jsonl"_s);
	{
		PromptHistory history(path);
		for(int i = 0; i < PromptHistory::MaxItems + 3; ++i)
		{
			history.push(u"prompt %1"_s.arg(i));
		}
		QCOMPARE(history.items().size(), PromptHistory::MaxItems);
	}
	const PromptHistory reloaded(path);
	QCOMPARE(reloaded.items().size(), PromptHistory::MaxItems);
	QCOMPARE(reloaded.items().first(), u"prompt 3"_s);
	QCOMPARE(reloaded.items().last(), u"prompt 502"_s);
}

void TestPromptHistory::browsingWalksBackAndForth()
{
	PromptHistory history;
	history.push(u"a"_s);
	history.push(u"b"_s);
	history.push(u"c"_s);

	HistoryBrowser browser(history);
	QVERIFY(!browser.browsing());
	QCOMPARE(browser.previous(u"typed"_s), u"c"_s);
	QVERIFY(browser.browsing());
	QCOMPARE(browser.previous(u"c"_s), u"b"_s);
	QCOMPARE(browser.previous(u"b"_s), u"a"_s);
	QVERIFY(!browser.previous(u"a"_s)); // already at the oldest
	QCOMPARE(browser.next(), u"b"_s);
}

void TestPromptHistory::browsingReturnsTheDraftAfterTheNewest()
{
	PromptHistory history;
	history.push(u"a"_s);

	HistoryBrowser browser(history);
	QCOMPARE(browser.previous(u"half written"_s), u"a"_s);
	QCOMPARE(browser.next(), u"half written"_s);
	QVERIFY(!browser.browsing());
	QVERIFY(!browser.next());

	browser.previous(u"x"_s);
	browser.reset();
	QVERIFY(!browser.browsing());
}

void TestPromptHistory::browsingNeedsHistory()
{
	PromptHistory history;
	HistoryBrowser browser(history);
	QVERIFY(!browser.previous(u"x"_s));
	QVERIFY(!browser.browsing());
}

void TestPromptHistory::imagePathsAreRecognised()
{
	QTemporaryDir dir;
	const QString path = dir.filePath(u"my shot.PNG"_s);
	QFile file(path);
	QVERIFY(file.open(QIODevice::WriteOnly));
	file.write("abc");
	file.close();

	const auto plain = imageFromPath(path);
	QVERIFY(plain && plain->has_value());
	QCOMPARE((*plain)->mime, u"image/png"_s);
	QCOMPARE((*plain)->data, u"YWJj"_s);
	QCOMPARE((*plain)->name, u"my shot.PNG"_s);

	const QString escaped = QString(path).replace(u' ', u"\\ "_s);
	QVERIFY(imageFromPath(escaped)->has_value());
	QVERIFY(imageFromPath(u"'"_s + path + u"'"_s)->has_value());
	QVERIFY(imageFromPath(u"file://"_s + QString(path).replace(u' ', u"%20"_s))->has_value());
}

void TestPromptHistory::otherPathsAreText()
{
	QTemporaryDir dir;
	QVERIFY(!imageFromPath(dir.filePath(u"missing.png"_s))->has_value());
	QVERIFY(!imageFromPath(u"just some words"_s)->has_value());
	QVERIFY(!imageFromPath(u"line one\nline two.png"_s)->has_value());
	QVERIFY(!imageFromPath(QString())->has_value());

	QFile file(dir.filePath(u"notes.txt"_s));
	QVERIFY(file.open(QIODevice::WriteOnly));
	file.close();
	QVERIFY(!imageFromPath(file.fileName())->has_value());
}

void TestPromptHistory::unusableImageFilesAreErrors()
{
	QTemporaryDir dir;
	QFile file(dir.filePath(u"huge.png"_s));
	QVERIFY(file.open(QIODevice::WriteOnly));
	QVERIFY(file.resize(20 * 1024 * 1024 + 1));
	file.close();

	const auto result = imageFromPath(file.fileName());
	QVERIFY(!result);
	QVERIFY(result.error().contains(u"huge.png"_s));
}

QTEST_GUILESS_MAIN(TestPromptHistory)
#include "tst_prompt_history.moc"
