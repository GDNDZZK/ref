// Rendering text from a model must not reach the network: Markdown images and HTML tags would make
// Qt fetch their addresses. A local server counts what arrives.
#include "app/local_only_network.h"
#include "session/markdown_text.h"

#include <QQmlComponent>
#include <QQmlEngine>
#include <QQuickItem>
#include <QQuickWindow>
#include <QTcpServer>
#include <QtTest>

using namespace Qt::StringLiterals;

class TestNetworkPolicy : public QObject
{
	Q_OBJECT

private slots:
	void initTestCase();
	void rawTextReachesTheNetworkWithoutThePolicy();
	void thePolicyKeepsTextFromTheNetwork();
	void sanitisedMarkdownKeepsTextFromTheNetwork();
	void servesOnlyTheProgramsOwnUrls();
	void opensOnlyWebAndMailLinks();

private:
	// Shows `qml` for a moment and returns how many connections the server got.
	int connectionsWhileShowing(const QString& qml, bool withPolicy);
	QString payload(const QString& body) const;

	QTcpServer m_server;
	int m_connections = 0;
};

void TestNetworkPolicy::initTestCase()
{
	connect(&m_server, &QTcpServer::newConnection, this, [this] {
		++m_connections;
		while(QTcpSocket* socket = m_server.nextPendingConnection())
		{
			socket->close();
		}
	});
	QVERIFY(m_server.listen(QHostAddress::LocalHost));
}

QString TestNetworkPolicy::payload(const QString& body) const
{
	const QString address = u"http://127.0.0.1:%1/x.png"_s.arg(m_server.serverPort());
	return QString(body).replace(u"%URL%"_s, address);
}

int TestNetworkPolicy::connectionsWhileShowing(const QString& qml, bool withPolicy)
{
	LocalOnlyNetworkAccessManagerFactory factory;
	QQmlEngine engine;
	if(withPolicy)
	{
		engine.setNetworkAccessManagerFactory(&factory);
	}
	QQmlComponent component(&engine);
	component.setData(qml.toUtf8(), QUrl());
	std::unique_ptr<QObject> created(component.create());
	if(!created)
	{
		qWarning() << component.errorString();
		return -1;
	}
	QQuickWindow window;
	window.resize(400, 300);
	qobject_cast<QQuickItem*>(created.get())->setParentItem(window.contentItem());
	window.show();
	m_connections = 0;
	QTest::qWait(700);
	return m_connections;
}

static const QString MarkdownImage = u"import QtQuick\\nTextEdit { width: 300; textFormat: TextEdit.MarkdownText; text: '![x](%URL%)' }"_s;
static const QString HtmlImage = u"import QtQuick\\nText { width: 300; textFormat: Text.RichText; text: '<img src=\"%URL%\">' }"_s;
static const QString AutoImage = u"import QtQuick\\nText { width: 300; text: '<img src=\"%URL%\">' }"_s;
static const QString MarkdownHtml = u"import QtQuick\\nTextEdit { width: 300; textFormat: TextEdit.MarkdownText; text: 'a <img src=\"%URL%\"> b' }"_s;
static const QString RemoteImage = u"import QtQuick\\nImage { source: '%URL%' }"_s;

void TestNetworkPolicy::rawTextReachesTheNetworkWithoutThePolicy()
{
	// The control: the payloads do fetch when nothing stops them, so the other tests mean something.
	QVERIFY(connectionsWhileShowing(payload(RemoteImage).replace(u"\\n"_s, u"\n"_s), false) > 0);
	QVERIFY(connectionsWhileShowing(payload(MarkdownImage).replace(u"\\n"_s, u"\n"_s), false) > 0);
}

void TestNetworkPolicy::thePolicyKeepsTextFromTheNetwork()
{
	for(const QString& qml : {RemoteImage, MarkdownImage, HtmlImage, AutoImage, MarkdownHtml})
	{
		QCOMPARE(connectionsWhileShowing(payload(qml).replace(u"\\n"_s, u"\n"_s), true), 0);
	}
}

void TestNetworkPolicy::sanitisedMarkdownKeepsTextFromTheNetwork()
{
	const QString safe = refgui::session::markdownForDisplay(u"![x](%URL%) a <img src=\"%URL%\"> b"_s);
	const QString qml = u"import QtQuick\nTextEdit { width: 300; textFormat: TextEdit.MarkdownText; text: '"_s + QString(safe).replace(u"'"_s, u"\\'"_s) + u"' }"_s;
	QCOMPARE(connectionsWhileShowing(payload(qml), false), 0);
}

void TestNetworkPolicy::servesOnlyTheProgramsOwnUrls()
{
	QVERIFY(LocalOnlyNetworkAccessManager::isAllowed(QUrl(u"qrc:/qt/qml/RefGui/Main.qml"_s)));
	QVERIFY(LocalOnlyNetworkAccessManager::isAllowed(QUrl(u"data:text/plain;base64,AA=="_s)));
	QVERIFY(!LocalOnlyNetworkAccessManager::isAllowed(QUrl(u"http://example.com/a.png"_s)));
	QVERIFY(!LocalOnlyNetworkAccessManager::isAllowed(QUrl(u"HTTPS://example.com/a.png"_s)));
	QVERIFY(!LocalOnlyNetworkAccessManager::isAllowed(QUrl(u"ftp://example.com/a"_s)));
	QVERIFY(!LocalOnlyNetworkAccessManager::isAllowed(QUrl()));
}

void TestNetworkPolicy::opensOnlyWebAndMailLinks()
{
	QQmlEngine engine;
	QQmlComponent component(&engine, QUrl::fromLocalFile(QStringLiteral(REF_QML_DIR) + u"/Links.qml"_s));
	std::unique_ptr<QObject> links(component.create());
	QVERIFY2(links, qPrintable(component.errorString()));
	const auto allowed = [&links](const QString& link) {
		QVariant result;
		QMetaObject::invokeMethod(links.get(), "isAllowed", Q_RETURN_ARG(QVariant, result), Q_ARG(QVariant, link));
		return result.toBool();
	};
	QVERIFY(allowed(u"http://example.com"_s));
	QVERIFY(allowed(u"HTTPS://example.com/a?b=c"_s));
	QVERIFY(allowed(u"mailto:someone@example.com"_s));
	QVERIFY(!allowed(u"file:///etc/passwd"_s));
	QVERIFY(!allowed(u"javascript:alert(1)"_s));
	QVERIFY(!allowed(u"x-terminal:run"_s));
	QVERIFY(!allowed(u"example.com"_s));
	QVERIFY(!allowed(QString()));
}

QTEST_MAIN(TestNetworkPolicy)
#include "tst_network_policy.moc"
