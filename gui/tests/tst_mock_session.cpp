// Runs the session layer against the real `ref-server --mock`. Needs the binary: see REF_SERVER in CMake.
#include "client/backend.h"
#include "coro_test.h"
#include "session/session_controller.h"
#include "session_test_data.h"

#include <QDir>
#include <QFileInfo>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QtTest>

using namespace Qt::StringLiterals;
using namespace refgui;
using namespace refgui::test;
using refgui::client::Backend;
using refgui::session::ConversationModel;
using refgui::session::SessionController;

namespace
{

constexpr int ReplyTimeoutMs = 20000;

}

class TestMockSession : public QObject
{
	Q_OBJECT

private slots:
	void initTestCase();
	void streamsMarkdownAndCode();
	void toolCallsFinish();
	void permissionWaitsForTheUser();
	void deniedPermissionShowsAsFailed();
	void undoTakesThePromptBack();
	void mockDirectoryIsRemovedWhenTheBackendIsDestroyed();

private:
	// A connected backend with a session, and the controller on it.
	struct Session
	{
		Session()
			: backend(Backend::Source(Backend::Mode::Mock{}), QDir::currentPath())
			, controller(backend)
		{
			backend.start();
			QTRY_COMPARE_WITH_TIMEOUT(backend.state(), Backend::State::Connected, ReplyTimeoutMs);
			const auto created = await(controller.create(QDir::currentPath(), std::nullopt, std::nullopt));
			QVERIFY(created && created->has_value());
		}

		~Session()
		{
			backend.shutdown();
		}

		void say(const QString& script)
		{
			const auto sent = await(controller.send(script));
			QVERIFY(sent && sent->has_value());
		}

		// Blocks of the conversation, e.g. "tool:bash ls".
		QStringList blocks() const
		{
			return render(controller.model());
		}

		bool hasBlock(const QString& kind) const
		{
			return !blocks().filter(kind + u':').isEmpty();
		}

		Backend backend;
		SessionController controller;
	};

	QTemporaryDir m_directory;
};

void TestMockSession::initTestCase()
{
	const QFileInfo server(QStringLiteral(REF_SERVER_BINARY));
	QVERIFY2(server.isExecutable(), qPrintable(server.filePath()));
	QVERIFY(m_directory.isValid());

	// The program finds ref-server on PATH; keep the user's own state and data out of this.
	qputenv("PATH", (server.absolutePath() + QDir::listSeparator()).toLocal8Bit() + qgetenv("PATH"));
	qputenv("REF_STATE_DIR", QDir(m_directory.path()).filePath(u"state"_s).toLocal8Bit());
	qputenv("REF_DATA_DIR", QDir(m_directory.path()).filePath(u"data"_s).toLocal8Bit());
}

void TestMockSession::streamsMarkdownAndCode()
{
	Session s;
	s.say(u"markdown"_s);

	QTRY_VERIFY_WITH_TIMEOUT(s.controller.model().rowCount() > 1, ReplyTimeoutMs);
	QTRY_VERIFY_WITH_TIMEOUT(!s.controller.isTurnInProgress(), ReplyTimeoutMs);
	QVERIFY(s.hasBlock(u"user"_s));
	QVERIFY(s.hasBlock(u"markdown"_s));
	QVERIFY(s.hasBlock(u"code"_s));
}

// The script reads, searches and lists, then asks for permission to edit.
void TestMockSession::toolCallsFinish()
{
	Session s;
	s.say(u"tools"_s);
	const ConversationModel& model = s.controller.model();
	const auto states = [&model] {
		QStringList result;
		for(int row = 0; row < model.rowCount(); ++row)
		{
			if(roleOf(model, row, ConversationModel::KindRole).toString() == u"tool")
			{
				result.append(roleOf(model, row, ConversationModel::ToolStateRole).toString());
			}
		}
		return result;
	};

	QTRY_VERIFY_WITH_TIMEOUT(states().contains(u"awaiting_permission"_s), ReplyTimeoutMs);
	QVERIFY(states().contains(u"done"_s));
	QVERIFY(states().contains(u"error"_s));
	QVERIFY(s.controller.isBusy());
	for(int row = 0; row < model.rowCount(); ++row)
	{
		const bool isFinal = !roleOf(model, row, ConversationModel::StreamingRole).toBool();
		const bool isTool = roleOf(model, row, ConversationModel::KindRole).toString() == u"tool";
		const bool waits = roleOf(model, row, ConversationModel::ToolStateRole).toString() == u"awaiting_permission";
		QVERIFY(!isTool || waits || isFinal);
	}
}

void TestMockSession::permissionWaitsForTheUser()
{
	Session s;
	s.say(u"permission"_s);

	const ConversationModel& model = s.controller.model();
	const auto awaitingRow = [&model] {
		for(int row = 0; row < model.rowCount(); ++row)
		{
			if(roleOf(model, row, ConversationModel::ToolStateRole).toString() == u"awaiting_permission")
			{
				return row;
			}
		}
		return -1;
	};
	QTRY_VERIFY_WITH_TIMEOUT(awaitingRow() >= 0, ReplyTimeoutMs);
	QVERIFY(s.controller.isBusy());

	const QString callId = roleOf(model, awaitingRow(), ConversationModel::CallIdRole).toString();
	const auto replied = await(s.controller.permissionReply(callId, true, false));
	QVERIFY(replied && replied->has_value());

	QTRY_VERIFY_WITH_TIMEOUT(!s.controller.isTurnInProgress(), ReplyTimeoutMs);
	QCOMPARE(awaitingRow(), -1);
}

void TestMockSession::undoTakesThePromptBack()
{
	Session s;
	s.say(u"hello"_s);
	QTRY_VERIFY_WITH_TIMEOUT(!s.controller.isTurnInProgress() && s.hasBlock(u"markdown"_s), ReplyTimeoutMs);
	s.say(u"markdown"_s);
	QTRY_VERIFY_WITH_TIMEOUT(!s.controller.isTurnInProgress(), ReplyTimeoutMs);

	// The last prompt goes, with its reply; the first stays.
	const auto undone = await(s.controller.undo(std::nullopt));
	QVERIFY(undone && undone->has_value());
	QCOMPARE((*undone)->text, u"markdown"_s);
	QVERIFY((*undone)->removed);
	QTRY_VERIFY_WITH_TIMEOUT(s.blocks().filter(u"user:"_s).size() == 1, ReplyTimeoutMs);
	QVERIFY(s.blocks().filter(u"user:hello"_s).size() == 1);

	// By message id: the only prompt left.
	const QString firstId = s.controller.model().firstMessageId();
	const auto all = await(s.controller.undo(firstId));
	QVERIFY(all && all->has_value());
	QCOMPARE((*all)->text, u"hello"_s);
	QTRY_VERIFY_WITH_TIMEOUT(s.controller.model().rowCount() == 0, ReplyTimeoutMs);
}

void TestMockSession::deniedPermissionShowsAsFailed()
{
	Session s;
	s.say(u"permission"_s);
	const ConversationModel& model = s.controller.model();
	const auto toolRow = [&model](const QString& state) {
		for(int row = 0; row < model.rowCount(); ++row)
		{
			if(roleOf(model, row, ConversationModel::ToolStateRole).toString() == state)
			{
				return row;
			}
		}
		return -1;
	};
	QTRY_VERIFY_WITH_TIMEOUT(toolRow(u"awaiting_permission"_s) >= 0, ReplyTimeoutMs);

	const QString callId = roleOf(model, toolRow(u"awaiting_permission"_s), ConversationModel::CallIdRole).toString();
	const auto replied = await(s.controller.permissionReply(callId, false, false));
	QVERIFY(replied && replied->has_value());

	QTRY_VERIFY_WITH_TIMEOUT(toolRow(u"denied"_s) >= 0, ReplyTimeoutMs);
	QVERIFY(roleOf(model, toolRow(u"denied"_s), ConversationModel::FailedRole).toBool());
}

// The mock backend keeps its workspace under the temporary directory and removes it when it exits
// on its own. Killing it leaves the directory behind, so the backend has to be given time to leave.
void TestMockSession::mockDirectoryIsRemovedWhenTheBackendIsDestroyed()
{
	QTemporaryDir temporary;
	QVERIFY(temporary.isValid());
	const QByteArray before = qgetenv("TMPDIR");
	qputenv("TMPDIR", temporary.path().toLocal8Bit());
	{
		Session s;
		QVERIFY(!QDir(temporary.path()).entryList({u"ref-mock-*"_s}, QDir::Dirs | QDir::NoDotAndDotDot).isEmpty());
	}
	if(before.isNull())
	{
		qunsetenv("TMPDIR");
	}
	else
	{
		qputenv("TMPDIR", before);
	}
	QCOMPARE(QDir(temporary.path()).entryList({u"ref-mock-*"_s}, QDir::Dirs | QDir::NoDotAndDotDot), QStringList());
}

QTEST_GUILESS_MAIN(TestMockSession)
#include "tst_mock_session.moc"
