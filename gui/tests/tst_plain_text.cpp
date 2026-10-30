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

const QStringList Files = {u"Composer.qml"_s,
                           u"ComposerChip.qml"_s,
                           u"ComposerTags.qml"_s,
                           u"AttachmentStrip.qml"_s,
                           u"QueueStrip.qml"_s,
                           u"CompletionPopup.qml"_s,
                           u"NotificationSettingsSection.qml"_s,
                           u"PlainToolTip.qml"_s};

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
};

void TestPlainText::textItemsArePlain()
{
	const QRegularExpression textItem(uR"(\bText \{)"_s);
	for(const QString& name : Files)
	{
		const QString source = read(name);
		QVERIFY2(!source.isEmpty(), qPrintable(name));
		for(const QRegularExpressionMatch& match : textItem.globalMatch(source))
		{
			const QString body = block(source, match.capturedEnd() - 1);
			QVERIFY2(body.contains(u"textFormat: Text.PlainText"_s), qPrintable(u"%1: a Text without textFormat: Text.PlainText near\n%2"_s.arg(name, body.left(120))));
		}
	}
}

void TestPlainText::tooltipsArePlain()
{
	for(const QString& name : Files)
	{
		QVERIFY2(!read(name).contains(u"ToolTip.text"_s), qPrintable(name + u": use PlainToolTip"_s));
	}
}

void TestPlainText::noRichFormats()
{
	for(const QString& name : Files)
	{
		const QString source = read(name);
		QVERIFY2(!source.contains(u"StyledText"_s) && !source.contains(u"RichText"_s) && !source.contains(u"MarkdownText"_s), qPrintable(name));
	}
}

QTEST_GUILESS_MAIN(TestPlainText)
#include "tst_plain_text.moc"
