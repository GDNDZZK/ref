// Text that comes from outside (file names, model names, queued messages, service errors, shell
// output) must be shown as it is: with the default format a Text reads <img src="http://..."> and
// fetches it. This scans every QML file of the app for text items and tooltips that would not be
// plain. It looks at the properties an item sets itself, so a nested item that says PlainText
// does not excuse the item around it.
#include <QDir>
#include <QFile>
#include <QRegularExpression>
#include <QtTest>

using namespace Qt::StringLiterals;

namespace
{

// Files that render Markdown on purpose. Their text has been through markdownForDisplay().
const QStringList MarkdownFiles = {u"MarkdownBlock.qml"_s, u"ReasoningBlock.qml"_s, u"UserBlock.qml"_s};

// Letters of fixed labels laid out as rich text, each escaped.
const QStringList RichTextFiles = {u"ShimmerText.qml"_s};

// The only file that may declare a ToolTip item.
const QString PlainToolTipFile = u"PlainToolTip.qml"_s;

// Items that show text and read a format of their own. TextInput and TextField have none: they show plain text.
const QRegularExpression TextItem(uR"(\b(Text|Label|TextEdit|TextArea)\s*\{)"_s);
const QRegularExpression ToolTipItem(uR"(\bToolTip\s*\{)"_s);
const QRegularExpression TextFormat(uR"(\btextFormat\s*:([^\n;]*))"_s);

QString readFile(const QString& path)
{
	QFile file(path);
	return file.open(QIODevice::ReadOnly) ? QString::fromUtf8(file.readAll()) : QString();
}

QStringList qmlFiles()
{
	return QDir(QStringLiteral(REF_QML_DIR)).entryList({u"*.qml"_s}, QDir::Files, QDir::Name);
}

// Strings and comments out of the way, so that braces and words in them are not mistaken for code.
QString withoutStringsAndComments(const QString& source)
{
	static const QRegularExpression strings(uR"("(?:[^"\\\n]|\\.)*")"_s);
	static const QRegularExpression lineComments(uR"(//[^\n]*)"_s);
	static const QRegularExpression blockComments(uR"(/\*.*?\*/)"_s, QRegularExpression::DotMatchesEverythingOption);
	QString result = source;
	result.replace(strings, u"\"\""_s);
	result.replace(lineComments, QString());
	result.replace(blockComments, QString());
	return result;
}

// The braces that open at `open`, with the whole text between them.
QString block(const QString& text, qsizetype open)
{
	int depth = 0;
	for(qsizetype i = open; i < text.size(); ++i)
	{
		depth += text[i] == u'{' ? 1 : text[i] == u'}' ? -1 : 0;
		if(depth == 0)
		{
			return text.mid(open, i - open + 1);
		}
	}
	return text.mid(open);
}

// What an item sets itself: its block without the blocks nested in it.
QString ownProperties(const QString& itemBlock)
{
	QString result;
	int depth = 0;
	for(const QChar c : itemBlock)
	{
		if(c == u'{')
		{
			++depth;
			continue;
		}
		if(c == u'}')
		{
			--depth;
			continue;
		}
		if(depth == 1)
		{
			result.append(c);
		}
	}
	return result;
}

// What is wrong with the text items and tooltips of one file.
QStringList problems(const QString& name, const QString& rawSource)
{
	QStringList found;
	const QString source = withoutStringsAndComments(rawSource);
	const bool markdownAllowed = MarkdownFiles.contains(name);
	const bool richAllowed = RichTextFiles.contains(name);

	if(!richAllowed)
	{
		for(const QRegularExpressionMatch& match : TextItem.globalMatch(source))
		{
			const QString own = ownProperties(block(source, match.capturedEnd() - 1));
			const QRegularExpressionMatch format = TextFormat.match(own);
			if(!format.hasMatch())
			{
				found.append(u"%1: %2 without textFormat of its own"_s.arg(name, match.captured(1)));
			}
			else if(!format.captured(1).contains(u"PlainText"_s) && !(markdownAllowed && format.captured(1).contains(u"MarkdownText"_s)))
			{
				found.append(u"%1: %2 with textFormat:%3"_s.arg(name, match.captured(1), format.captured(1)));
			}
		}
		if(source.contains(u"StyledText"_s) || source.contains(u"RichText"_s) || (!markdownAllowed && source.contains(u"MarkdownText"_s)))
		{
			found.append(name + u": a rich text format"_s);
		}
	}
	// The default tooltip reads rich text.
	if(source.contains(u"ToolTip.text"_s))
	{
		found.append(name + u": ToolTip.text (use PlainToolTip)"_s);
	}
	if(name != PlainToolTipFile && source.contains(ToolTipItem))
	{
		found.append(name + u": a ToolTip item (use PlainToolTip)"_s);
	}
	return found;
}

} // namespace

class TestPlainText : public QObject
{
	Q_OBJECT

private slots:
	void everyQmlFileShowsOutsideTextAsItIs();
	void theScanSeesWhatItShould();
	void theScanLooksAtTheItemsOwnProperties();
	void theScanCoversTheFilesOfTheApp();
};

void TestPlainText::everyQmlFileShowsOutsideTextAsItIs()
{
	QStringList found;
	for(const QString& name : qmlFiles())
	{
		const QString source = readFile(QStringLiteral(REF_QML_DIR) + u"/"_s + name);
		QVERIFY2(!source.isEmpty(), qPrintable(name));
		found.append(problems(name, source));
	}
	QVERIFY2(found.isEmpty(), qPrintable(u"\n"_s + found.join(u'\n')));
}

// The checks themselves, on QML written for the purpose.
void TestPlainText::theScanSeesWhatItShould()
{
	QVERIFY(problems(u"A.qml"_s, u"Item { Text { text: x } }"_s).size() == 1);
	QVERIFY(problems(u"A.qml"_s, u"Item { Text { textFormat: Text.PlainText } }"_s).isEmpty());
	QVERIFY(problems(u"A.qml"_s, u"Item { TextArea { text: x } }"_s).size() == 1);
	QVERIFY(problems(u"A.qml"_s, u"Item { TextEdit { textFormat: TextEdit.PlainText } }"_s).isEmpty());
	QVERIFY(problems(u"A.qml"_s, u"Item { component Heading: Text { } }"_s).size() == 1);
	QVERIFY(problems(u"A.qml"_s, u"Item { Text { textFormat: Text.RichText } }"_s).size() >= 1);
	QVERIFY(problems(u"A.qml"_s, u"Item { Text { textFormat: Text.StyledText } }"_s).size() >= 1);
	QVERIFY(problems(u"A.qml"_s, u"Item { TextEdit { textFormat: TextEdit.MarkdownText } }"_s).size() >= 1);
	QVERIFY(problems(u"UserBlock.qml"_s, u"Item { TextEdit { textFormat: TextEdit.MarkdownText } }"_s).isEmpty());
	QVERIFY(problems(u"ShimmerText.qml"_s, u"Text { textFormat: Text.RichText }"_s).isEmpty());
	// A tooltip is not plain unless PlainToolTip makes it so.
	QVERIFY(problems(u"A.qml"_s, u"Item { ToolTip.text: qsTr(\"x\") }"_s).size() == 1);
	QVERIFY(problems(u"A.qml"_s, u"Item { ToolTip { text: x } }"_s).size() == 1);
	QVERIFY(problems(u"A.qml"_s, u"Item { PlainToolTip { text: x } }"_s).isEmpty());
	QVERIFY(problems(u"PlainToolTip.qml"_s, u"ToolTip { contentItem: Text { textFormat: Text.PlainText } }"_s).isEmpty());
	// Words in strings and comments are no code.
	QVERIFY(problems(u"A.qml"_s, u"Item { // a Text { here\n Text { textFormat: Text.PlainText; text: \"ToolTip.text {\" } }"_s).isEmpty());
}

// A nested item saying PlainText must not excuse the item around it.
void TestPlainText::theScanLooksAtTheItemsOwnProperties()
{
	const QString outerLacksIt = u"Text { Behavior on x { NumberAnimation {} } Item { Text { textFormat: Text.PlainText } } }"_s;
	QCOMPARE(problems(u"A.qml"_s, outerLacksIt).size(), 1);
	const QString innerLacksIt = u"Item { Text { textFormat: Text.PlainText; Item { Text { text: x } } } }"_s;
	QCOMPARE(problems(u"A.qml"_s, innerLacksIt).size(), 1);
	const QString both = u"Text { textFormat: Text.PlainText; Item { Text { textFormat: Text.PlainText } } }"_s;
	QVERIFY(problems(u"A.qml"_s, both).isEmpty());
}

// A scan that finds no files would pass; make sure it has plenty and the known ones.
void TestPlainText::theScanCoversTheFilesOfTheApp()
{
	const QStringList files = qmlFiles();
	QVERIFY(files.size() > 50);
	for(const QString& known : {u"Composer.qml"_s, u"CompletionPopup.qml"_s, u"BackendBanner.qml"_s, u"PlainToolTip.qml"_s, u"ShimmerText.qml"_s})
	{
		QVERIFY2(files.contains(known), qPrintable(known));
	}
}

QTEST_GUILESS_MAIN(TestPlainText)
#include "tst_plain_text.moc"
