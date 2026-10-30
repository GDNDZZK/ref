// Text that comes from outside (file names, model names, queued messages, service errors) must be
// shown as it is: with the default format QML reads <img src="http://..."> and fetches it. This
// checks the QML files of the input box and the notification settings for Text items and tooltips
// that would not be plain.
#include <QFile>
#include <QRegularExpression>
#include <QtTest>

using namespace Qt::StringLiterals;

namespace
{

// Files of the conversation area.
const QStringList ConversationFiles = {u"Block.qml"_s,        u"ChatView.qml"_s,       u"UserBlock.qml"_s,     u"ImageBlock.qml"_s,
                                       u"ErrorBlock.qml"_s,   u"MarkdownBlock.qml"_s,  u"ReasoningBlock.qml"_s, u"ToolBlock.qml"_s,
                                       u"ToolBody.qml"_s,     u"ToolBodyBase.qml"_s,   u"GenericBody.qml"_s,   u"DiffBody.qml"_s,
                                       u"BashBody.qml"_s,     u"ReadBody.qml"_s,       u"GrepBody.qml"_s,      u"FindBody.qml"_s,
                                       u"WebfetchBody.qml"_s, u"SessionBody.qml"_s,    u"TodoBody.qml"_s,      u"CommandBox.qml"_s,
                                       u"LineView.qml"_s,     u"OutputText.qml"_s,     u"OutputSection.qml"_s, u"ToolSection.qml"_s,
                                       u"ToolChip.qml"_s,     u"PermissionSection.qml"_s, u"PermissionButton.qml"_s, u"CopyButton.qml"_s,
                                       u"MetaBlock.qml"_s,    u"MetaText.qml"_s,       u"MetaAction.qml"_s,    u"StatusLine.qml"_s,
                                       u"OlderPill.qml"_s,    u"Thumbnail.qml"_s,      u"ImageStrip.qml"_s,    u"Lightbox.qml"_s,
                                       u"ToolStateSlot.qml"_s, u"CodeBlock.qml"_s,     u"ProseText.qml"_s,     u"SessionPane.qml"_s};

// Files that render Markdown on purpose. Their text has been through markdownForDisplay().
const QStringList MarkdownFiles = {u"MarkdownBlock.qml"_s, u"ReasoningBlock.qml"_s, u"UserBlock.qml"_s};

// Letters of fixed labels laid out as rich text, escaped one by one.
const QStringList RichTextFiles = {u"ShimmerText.qml"_s};

const QStringList Files = {u"Composer.qml"_s,
                           u"ComposerChip.qml"_s,
                           u"ComposerTags.qml"_s,
                           u"AttachmentStrip.qml"_s,
                           u"QueueStrip.qml"_s,
                           u"CompletionPopup.qml"_s,
                           u"NotificationSettingsSection.qml"_s,
                           u"PlainToolTip.qml"_s};

QStringList allFiles()
{
	return Files + ConversationFiles + MarkdownFiles + RichTextFiles;
}

QString read(const QString& name)
{
	QFile file(QStringLiteral(REF_QML_DIR) + u"/"_s + name);
	return file.open(QIODevice::ReadOnly) ? QString::fromUtf8(file.readAll()) : QString();
}

// The text between the braces that open at `open`.
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

} // namespace

class TestPlainText : public QObject
{
	Q_OBJECT

private slots:
	void textItemsArePlain();
	void tooltipsArePlain();
	void noRichFormats();
	void textEditsSayWhatTheyAre();
};

void TestPlainText::textItemsArePlain()
{
	const QRegularExpression textItem(uR"(\bText \{)"_s);
	for(const QString& name : allFiles())
	{
		const QString source = read(name);
		QVERIFY2(!source.isEmpty(), qPrintable(name));
		if(RichTextFiles.contains(name))
		{
			continue;
		}
		for(const QRegularExpressionMatch& match : textItem.globalMatch(source))
		{
			const QString body = block(source, match.capturedEnd() - 1);
			QVERIFY2(body.contains(u"textFormat: Text.PlainText"_s), qPrintable(u"%1: a Text without textFormat: Text.PlainText near\n%2"_s.arg(name, body.left(120))));
		}
	}
}

void TestPlainText::tooltipsArePlain()
{
	for(const QString& name : allFiles())
	{
		QVERIFY2(!read(name).contains(u"ToolTip.text"_s), qPrintable(name + u": use PlainToolTip"_s));
	}
}

void TestPlainText::noRichFormats()
{
	for(const QString& name : Files + ConversationFiles)
	{
		if(MarkdownFiles.contains(name))
		{
			continue;
		}
		const QString source = read(name);
		QVERIFY2(!source.contains(u"StyledText"_s) && !source.contains(u"RichText"_s) && !source.contains(u"MarkdownText"_s), qPrintable(name));
	}
}

// TextEdit and ProseText read AutoText unless told: markup in their text would be rendered.
// ProseText.qml itself sets plain text; items that want Markdown say so.
void TestPlainText::textEditsSayWhatTheyAre()
{
	const QRegularExpression editItem(uR"(\bTextEdit \{)"_s);
	for(const QString& name : ConversationFiles)
	{
		const QString source = read(name);
		for(const QRegularExpressionMatch& match : editItem.globalMatch(source))
		{
			const QString body = block(source, match.capturedEnd() - 1);
			QVERIFY2(body.contains(u"textFormat:"_s), qPrintable(u"%1: a TextEdit without textFormat near\n%2"_s.arg(name, body.left(120))));
		}
	}
}

QTEST_GUILESS_MAIN(TestPlainText)
#include "tst_plain_text.moc"
