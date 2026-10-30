// Runs the input box against the real `ref-server --mock`. Needs the binary: see REF_SERVER in CMake.
#include "client/backend.h"
#include "compose/prompt_history.h"
#include "composer/composer_model.h"
#include "coro_test.h"
#include "session/session_controller.h"
#include "session_test_data.h"

#include <QDir>
#include <QFileInfo>
#include <QTemporaryDir>
#include <QtTest>

using namespace Qt::StringLiterals;
using namespace refgui;
using namespace refgui::test;
using refgui::client::Backend;
using refgui::composer::ComposerModel;
using refgui::session::SessionController;

namespace
{

constexpr int ReplyTimeoutMs = 20000;

}

class TestMockComposer : public QObject
{
	Q_OBJECT

private slots:
	void initTestCase();
	void sendingShowsTheUserMessage();
	void promptSentWhileBusyWaitsAndCanBeWithdrawn();
	void shellLineRuns();
	void mentionsListTheFilesOfTheWorkspace();

private:
	struct Session
	{
		explicit Session(const QString& cwd)
			: backend(Backend::Source(Backend::Mode::Mock{}), cwd)
			, controller(backend)
			, composer(backend, controller, history)
		{
			backend.start();
			QTRY_COMPARE_WITH_TIMEOUT(backend.state(), Backend::State::Connected, ReplyTimeoutMs);
			const auto created = await(controller.create(cwd, std::nullopt, std::nullopt));
			QVERIFY(created && created->has_value());
			QTRY_VERIFY_WITH_TIMEOUT(composer.agentName() != QString() && composer.modelName() != QString(), ReplyTimeoutMs);
		}

		~Session()
		{
			backend.shutdown();
		}

		void type(const QString& text)
		{
			composer.edit(text, static_cast<int>(text.size()));
		}

		Backend backend;
		SessionController controller;
		compose::PromptHistory history;
		ComposerModel composer;
	};

	QTemporaryDir m_directory;
	QString m_work;
};

void TestMockComposer::initTestCase()
{
	const QFileInfo server(QStringLiteral(REF_SERVER_BINARY));
	QVERIFY2(server.isExecutable(), qPrintable(server.filePath()));
	QVERIFY(m_directory.isValid());
	qputenv("PATH", (server.absolutePath() + QDir::listSeparator()).toLocal8Bit() + qgetenv("PATH"));
	qputenv("REF_STATE_DIR", QDir(m_directory.path()).filePath(u"state"_s).toLocal8Bit());
	qputenv("REF_DATA_DIR", QDir(m_directory.path()).filePath(u"data"_s).toLocal8Bit());

	m_work = QDir(m_directory.path()).filePath(u"work"_s);
	QVERIFY(QDir().mkpath(m_work));
}

void TestMockComposer::sendingShowsTheUserMessage()
{
	Session s(m_work);
	s.type(u"markdown"_s);

	QVERIFY(s.composer.submit());

	QVERIFY(!s.composer.hasContent());
	QTRY_VERIFY_WITH_TIMEOUT(render(s.controller.model()).contains(u"user:markdown"_s), ReplyTimeoutMs);
	QTRY_VERIFY_WITH_TIMEOUT(!s.controller.isTurnInProgress(), ReplyTimeoutMs);
	QCOMPARE(s.history.items(), (QStringList{u"markdown"_s}));
}

// The "tools" script stops at a permission question, so the session stays busy.
void TestMockComposer::promptSentWhileBusyWaitsAndCanBeWithdrawn()
{
	Session s(m_work);
	s.type(u"tools"_s);
	QVERIFY(s.composer.submit());
	QTRY_VERIFY_WITH_TIMEOUT(s.controller.isBusy(), ReplyTimeoutMs);

	s.type(u"markdown"_s);
	QVERIFY(s.composer.submit());
	QTRY_COMPARE_WITH_TIMEOUT(s.composer.queue()->rowCount(), 1, ReplyTimeoutMs);
	QCOMPARE(s.composer.queue()->index(0, 0).data(composer::QueueModel::TextRole).toString(), u"markdown"_s);

	s.composer.withdraw(0);

	QTRY_COMPARE_WITH_TIMEOUT(s.composer.text(), u"markdown"_s, ReplyTimeoutMs);
	QTRY_COMPARE_WITH_TIMEOUT(s.composer.queue()->rowCount(), 0, ReplyTimeoutMs);
}

void TestMockComposer::shellLineRuns()
{
	Session s(m_work);
	s.type(u"!"_s);
	QVERIFY(s.composer.shellMode());
	s.type(u"echo from-the-shell"_s);

	QVERIFY(s.composer.submit());

	QTRY_VERIFY_WITH_TIMEOUT(!render(s.controller.model()).filter(u"from-the-shell"_s).isEmpty(), ReplyTimeoutMs);
	QCOMPARE(s.history.items(), (QStringList{u"!echo from-the-shell"_s}));
}

// The mock backend lists a workspace of its own, not the session directory.
void TestMockComposer::mentionsListTheFilesOfTheWorkspace()
{
	Session s(m_work);
	s.type(u"read @parser"_s);

	QCOMPARE(s.composer.completionKind(), u"file"_s);
	QTRY_VERIFY_WITH_TIMEOUT(s.composer.files()->rowCount() > 0, ReplyTimeoutMs);
	QCOMPARE(s.composer.files()->index(0, 0).data(composer::FileModel::PathRole).toString(), u"src/parser.rs"_s);
	s.composer.acceptFile(0);
	QCOMPARE(s.composer.text(), u"read @src/parser.rs "_s);
}

QTEST_MAIN(TestMockComposer)
#include "tst_mock_composer.moc"
