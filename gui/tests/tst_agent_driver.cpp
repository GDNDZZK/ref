#include "fake_server.h"
#include "session/agent_driver.h"
#include "session/conversation_model.h"

#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QTemporaryDir>
#include <QTest>

#include <memory>

using namespace Qt::StringLiterals;
using refgui::session::AgentDriver;
using refgui::session::ConversationModel;

namespace
{

QByteArray textEvent(const QByteArray& text)
{
	return FakeServer::event(R"({"choices":[{"index":0,"delta":{"content":")"_ba + text + R"("}}]})"_ba);
}

const QByteArray StopEvent = FakeServer::event(R"({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]})"_ba);

// Renders every block as "kind:content".
QStringList renderBlocks(const ConversationModel& model)
{
	QStringList rendered;
	for(int row = 0; row < model.rowCount(); ++row)
	{
		const QModelIndex index = model.index(row);
		rendered.append(index.data(ConversationModel::KindRole).toString() + u':' + index.data(ConversationModel::ContentRole).toString());
	}
	return rendered;
}

// A configuration file for `server` in a temporary directory.
class Config
{
public:
	explicit Config(const FakeServer& server)
	{
		QFile file(path());
		if(!file.open(QIODevice::WriteOnly))
		{
			qFatal("cannot write %s", qPrintable(path()));
		}
		file.write(R"({"endpoint": {"baseUrl": ")"_ba + server.baseUrl().toEncoded() + R"(", "model": "m"}})"_ba);
	}

	QString path() const
	{
		return m_dir.filePath(u"config.jsonc"_s);
	}

private:
	QTemporaryDir m_dir;
};

// What the user does: the message goes into the model, then the driver replies.
void send(ConversationModel& model, AgentDriver& driver, const QString& text)
{
	model.appendUserMessage(text);
	driver.respond(text);
}

} // namespace

class TestAgentDriver : public QObject
{
	Q_OBJECT

private slots:
	void repliesIntoModel();
	void showsMissingConfiguration();
	void showsFailure();
	void stopKeepsPartialReplyInHistory();
	void destroyingMidReplyIsSafe();
};

void TestAgentDriver::repliesIntoModel()
{
	FakeServer server;
	server.response.chunks = {textEvent("Hel"), textEvent("lo"), StopEvent};
	const Config config(server);
	ConversationModel model;
	AgentDriver driver(model, config.path());

	send(model, driver, u"hi"_s);
	QVERIFY(driver.isRunning());
	QTRY_VERIFY(!driver.isRunning());

	QCOMPARE(renderBlocks(model), (QStringList{u"user:hi"_s, u"markdown:Hello"_s}));
}

void TestAgentDriver::showsMissingConfiguration()
{
	ConversationModel model;
	AgentDriver driver(model, u"/nonexistent/qutie/config.jsonc"_s);

	send(model, driver, u"hi"_s);

	QVERIFY(!driver.isRunning());
	QCOMPARE(model.rowCount(), 2);
	QCOMPARE(model.index(1).data(ConversationModel::KindRole).toString(), u"error"_s);
}

void TestAgentDriver::showsFailure()
{
	FakeServer server;
	server.response = {.chunks = {textEvent("partial")}, .ending = FakeServer::Ending::Drop};
	const Config config(server);
	ConversationModel model;
	AgentDriver driver(model, config.path());

	send(model, driver, u"hi"_s);
	QTRY_VERIFY(!driver.isRunning());

	const QStringList blocks = renderBlocks(model);
	QCOMPARE(blocks.size(), 3);
	QCOMPARE(blocks.at(1), u"markdown:partial"_s);
	QVERIFY(blocks.at(2).startsWith(u"error:"_s));
}

// What the user saw of a stopped reply is what the model sees next turn.
void TestAgentDriver::stopKeepsPartialReplyInHistory()
{
	FakeServer server;
	server.response = {.chunks = {textEvent("Hal")}, .ending = FakeServer::Ending::Hold};
	const Config config(server);
	ConversationModel model;
	AgentDriver driver(model, config.path());

	send(model, driver, u"one"_s);
	QTRY_COMPARE(model.rowCount(), 2);
	driver.stop();
	QVERIFY(!driver.isRunning());
	QCOMPARE(renderBlocks(model), (QStringList{u"user:one"_s, u"markdown:Hal"_s})); // no error block

	server.response = {.chunks = {StopEvent}};
	send(model, driver, u"two"_s);
	QTRY_VERIFY(!driver.isRunning());

	const QJsonArray messages = QJsonDocument::fromJson(server.lastBody()).object().value("messages"_L1).toArray();
	QCOMPARE(messages.size(), 3);
	QCOMPARE(messages.at(1).toObject().value("content"_L1).toString(), u"Hal"_s);
	QCOMPARE(messages.at(2).toObject().value("content"_L1).toString(), u"two"_s);
}

// Closing the window mid-reply: the reply must not resume into a destroyed driver.
// Run under REFGUI_SANITIZE to see a use after free.
void TestAgentDriver::destroyingMidReplyIsSafe()
{
	FakeServer server;
	server.response = {.chunks = {textEvent("Hal")}, .ending = FakeServer::Ending::Hold};
	const Config config(server);
	ConversationModel model;
	auto driver = std::make_unique<AgentDriver>(model, config.path());

	send(model, *driver, u"one"_s);
	QTRY_COMPARE(model.rowCount(), 2);
	driver.reset();

	QTest::qWait(50); // lets deferred deletes and late network events run
	QCOMPARE(model.rowCount(), 2);
}

QTEST_GUILESS_MAIN(TestAgentDriver)
#include "tst_agent_driver.moc"
