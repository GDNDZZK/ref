// The input box of a session against a fake backend: what it sends and how it follows the answers.
#include "client/backend.h"
#include "compose/prompt_history.h"
#include "composer/composer_model.h"
#include "coro_test.h"
#include "fake_backend.h"

#include <QBuffer>
#include <QClipboard>
#include <QGuiApplication>
#include <QImage>
#include <QJsonArray>
#include <QMimeData>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QtTest>

#include <memory>

using namespace Qt::StringLiterals;
using namespace refgui;
using namespace refgui::test;
using refgui::client::Backend;
using refgui::composer::ComposerModel;
using refgui::session::SessionController;

namespace
{

QJsonObject infoJson()
{
	return {{u"id"_s, u"ses_1"_s},
	        {u"name"_s, u"one"_s},
	        {u"title"_s, u"A title"_s},
	        {u"agent"_s, u"plan"_s},
	        {u"model"_s, QJsonObject{{u"provider"_s, u"p"_s}, {u"model"_s, u"m"_s}, {u"variant"_s, u"high"_s}}},
	        {u"cwd"_s, u"/work"_s},
	        {u"created_at"_s, 1},
	        {u"updated_at"_s, 1},
	        {u"status"_s, QJsonObject{{u"state"_s, u"idle"_s}}},
	        {u"attached"_s, 1},
	        {u"live"_s, true}};
}

QJsonObject configJson()
{
	const QJsonObject model{{u"provider"_s, u"p"_s}, {u"model"_s, u"m"_s}};
	return {{u"config"_s,
	         QJsonObject{{u"agents"_s,
	                      QJsonArray{QJsonObject{{u"name"_s, u"build"_s}, {u"description"_s, u"Builds"_s}},
	                                 QJsonObject{{u"name"_s, u"plan"_s}, {u"description"_s, u"Plans"_s}, {u"color"_s, u"#112233"_s}}}},
	                     {u"default_agent"_s, u"build"_s},
	                     {u"models"_s,
	                      QJsonArray{QJsonObject{{u"provider"_s, u"p"_s},
	                                             {u"provider_name"_s, u"Provider"_s},
	                                             {u"id"_s, u"m"_s},
	                                             {u"name"_s, u"Model M"_s},
	                                             {u"variants"_s, QJsonArray{u"low"_s, u"high"_s}}}}},
	                     {u"commands"_s, QJsonArray()}}}};
}

QJsonObject queuedMessage(const QString& id, const QString& text)
{
	return {{u"id"_s, id},
	        {u"role"_s, u"user"_s},
	        {u"time"_s, 1},
	        {u"source"_s, QJsonObject{{u"kind"_s, u"human"_s}}},
	        {u"parts"_s, QJsonArray{QJsonObject{{u"type"_s, u"text"_s}, {u"text"_s, text}, {u"time"_s, 1}}}}};
}

struct Fixture
{
	Fixture()
		: backend(Backend::Source(Backend::Mode::Remote{{fake.url(), u"token"_s}}), u"/work"_s)
		, controller(backend)
		, composer(backend, controller, history)
	{
		fake.onRequest = [this](const QJsonObject& request) { answer(request); };
		backend.start();
		QTRY_COMPARE(backend.state(), Backend::State::Connected);
		attach();
	}

	~Fixture()
	{
		backend.shutdown();
	}

	void attach()
	{
		QVERIFY(await(controller.attach(u"ses_1"_s)));
		QTRY_COMPARE(composer.modelName(), u"Model M"_s); // the configuration arrived
	}

	void answer(const QJsonObject& request)
	{
		const QString op = request.value(u"op"_s).toString();
		if(op == u"attach"_s)
		{
			QJsonObject snapshot{{u"info"_s, infoJson()}, {u"messages"_s, QJsonArray()}, {u"more_before"_s, false}};
			fake.respond(request, {{u"snapshot"_s, snapshot}});
		}
		else if(op == u"get_config"_s)
		{
			fake.respond(request, configJson());
		}
		else if(op == u"prompt"_s)
		{
			if(promptError.isEmpty())
			{
				fake.respond(request, {{u"queued"_s, false}, {u"message_id"_s, u"u1"_s}});
			}
			else
			{
				fake.fail(request, promptError);
			}
		}
		else if(op == u"list_files"_s)
		{
			fake.respond(request, {{u"files"_s, QJsonArray::fromStringList(files)}});
		}
		else if(op == u"expand_command"_s)
		{
			fake.respond(request, {{u"text"_s, expansion.value_or(u"expanded "_s + request.value(u"args"_s).toString())}});
		}
		else if(op == u"remove_queued"_s)
		{
			fake.respond(request, removal);
		}
		else
		{
			fake.respond(request, {});
		}
	}

	QList<QJsonObject> requestsOf(const QString& op) const
	{
		QList<QJsonObject> result;
		for(const QJsonObject& request : fake.requests())
		{
			if(request.value(u"op"_s).toString() == op)
			{
				result.append(request);
			}
		}
		return result;
	}

	// The server announces the prompts waiting.
	void queue(const QJsonArray& messages)
	{
		fake.send({{u"kind"_s, u"event"_s},
		           {u"session_id"_s, u"ses_1"_s},
		           {u"event"_s, QJsonObject{{u"type"_s, u"queue"_s}, {u"messages"_s, messages}}}});
	}

	// Types `text` the way the text area reports it.
	void type(const QString& text)
	{
		composer.edit(text, static_cast<int>(text.size()));
	}

	FakeBackend fake;
	Backend backend;
	SessionController controller;
	compose::PromptHistory history;
	ComposerModel composer;
	QStringList files;
	QString promptError;
	std::optional<QString> expansion;
	QJsonObject removal;
};

} // namespace

class TestComposerModel : public QObject
{
	Q_OBJECT

private slots:
	void labelsFollowTheSessionAndTheConfiguration();
	void sendsThePromptAndEmptiesTheDraft();
	void failedPromptComesBack();
	void bangStartsShellMode();
	void shellLineIsSentAsShell();
	void historyWalksBackAndForth();
	void historyKeepsShellLines();
	void mentionListFollowsTheQueryAndDropsStaleAnswers();
	void acceptingAFileReplacesTheMention();
	void escapeClosesTheListFirst();
	void doubleEscapeInterrupts();
	void secondEscapeComesTooLate();
	void withdrawingFillsTheDraft();
	void withdrawingASentPromptTellsSo();
	void customCommandSendsTheExpandedText();
	void emptyExpansionSendsNothing();
	void imageFilesAreAttached();
	void clipboardImageIsAttached();
	void clipboardImagePathIsAttachedAndOtherTextIsLeftAlone();
	void clearDraftKeepsLongDraftsInTheHistory();
	void queueFollowsTheServer();
	void answersAfterTheModelIsGoneAreIgnored();
};

void TestComposerModel::labelsFollowTheSessionAndTheConfiguration()
{
	Fixture f;
	QCOMPARE(f.composer.agentName(), u"plan"_s);
	QCOMPARE(f.composer.agentColor(), u"#112233"_s);
	QCOMPARE(f.composer.agentIndex(), 1);
	QCOMPARE(f.composer.modelName(), u"Model M"_s);
	QCOMPARE(f.composer.variant(), u"high"_s);
	QVERIFY(f.composer.variantAvailable());
	QVERIFY(f.composer.isReady());
}

void TestComposerModel::sendsThePromptAndEmptiesTheDraft()
{
	Fixture f;
	QSignalSpy sent(&f.composer, &ComposerModel::sent);
	f.type(u"Do it  "_s);
	QVERIFY(f.composer.hasContent());

	QVERIFY(f.composer.submit());

	QVERIFY(!f.composer.hasContent());
	QTRY_COMPARE(sent.count(), 1);
	const auto prompts = f.requestsOf(u"prompt"_s);
	QCOMPARE(prompts.size(), 1);
	QCOMPARE(prompts.first().value(u"text"_s).toString(), u"Do it"_s);
	QCOMPARE(prompts.first().value(u"session_id"_s).toString(), u"ses_1"_s);
	QCOMPARE(f.history.items(), (QStringList{u"Do it"_s}));
}

void TestComposerModel::failedPromptComesBack()
{
	Fixture f;
	f.promptError = u"no way"_s;
	QSignalSpy failed(&f.composer, &ComposerModel::failed);
	f.type(u"Do it"_s);

	QVERIFY(f.composer.submit());

	QTRY_COMPARE(failed.count(), 1);
	QCOMPARE(f.composer.text(), u"Do it"_s);
}

void TestComposerModel::bangStartsShellMode()
{
	Fixture f;
	f.type(u"!"_s);
	QVERIFY(f.composer.shellMode());
	QCOMPARE(f.composer.text(), QString());

	f.composer.leaveShell();
	QVERIFY(!f.composer.shellMode());

	f.type(u"a!"_s);
	QVERIFY(!f.composer.shellMode());
}

void TestComposerModel::shellLineIsSentAsShell()
{
	Fixture f;
	f.type(u"!"_s);
	f.type(u"ls -l"_s);

	QVERIFY(f.composer.submit());

	QTRY_COMPARE(f.requestsOf(u"shell"_s).size(), 1);
	QCOMPARE(f.requestsOf(u"shell"_s).first().value(u"command"_s).toString(), u"ls -l"_s);
	QVERIFY(!f.composer.shellMode());
	QCOMPARE(f.history.items(), (QStringList{u"!ls -l"_s}));
	QVERIFY(f.requestsOf(u"prompt"_s).isEmpty());
}

void TestComposerModel::historyWalksBackAndForth()
{
	Fixture f;
	f.history.push(u"first"_s);
	f.history.push(u"second"_s);
	f.type(u"draft"_s);

	// Up needs an empty text, the cursor at the start, or a walk under way.
	QVERIFY(!f.composer.historyPrevious());
	f.composer.edit(u"draft"_s, 0);
	QVERIFY(f.composer.historyPrevious());
	QCOMPARE(f.composer.text(), u"second"_s);
	QVERIFY(f.composer.historyPrevious());
	QCOMPARE(f.composer.text(), u"first"_s);
	QVERIFY(f.composer.historyNext());
	QCOMPARE(f.composer.text(), u"second"_s);
	QVERIFY(f.composer.historyNext());
	QCOMPARE(f.composer.text(), u"draft"_s);
	QVERIFY(!f.composer.historyNext());
}

void TestComposerModel::historyKeepsShellLines()
{
	Fixture f;
	f.history.push(u"!make"_s);
	QVERIFY(f.composer.historyPrevious());
	QVERIFY(f.composer.shellMode());
	QCOMPARE(f.composer.text(), u"make"_s);
	QVERIFY(f.composer.historyNext());
	QVERIFY(!f.composer.shellMode());
}

void TestComposerModel::mentionListFollowsTheQueryAndDropsStaleAnswers()
{
	Fixture f;
	f.files = {u"src/main.rs"_s, u"src/"_s};
	QCOMPARE(f.composer.completionKind(), u"none"_s);

	f.type(u"look at @sr"_s);

	QCOMPARE(f.composer.completionKind(), u"file"_s);
	QCOMPARE(f.composer.completionQuery(), u"sr"_s);
	QTRY_COMPARE(f.composer.files()->rowCount(), 2);
	const auto requests = f.requestsOf(u"list_files"_s);
	QCOMPARE(requests.size(), 1);
	QCOMPARE(requests.first().value(u"query"_s).toString(), u"sr"_s);
	QCOMPARE(requests.first().value(u"cwd"_s).toString(), u"/work"_s);
	QCOMPARE(f.composer.files()->index(1, 0).data(composer::FileModel::IsDirectoryRole).toBool(), true);
	QCOMPARE(f.composer.files()->index(0, 0).data(composer::FileModel::NameRole).toString(), u"main.rs"_s);

	// Typing on within the delay asks once; leaving the mention clears the list.
	f.type(u"look at @src"_s);
	f.type(u"look at @src/"_s);
	f.type(u"look at"_s);
	QCOMPARE(f.composer.completionKind(), u"none"_s);
	QCOMPARE(f.composer.files()->rowCount(), 0);
	QTest::qWait(200);
	QCOMPARE(f.requestsOf(u"list_files"_s).size(), 1);
}

void TestComposerModel::acceptingAFileReplacesTheMention()
{
	Fixture f;
	f.files = {u"src/main.rs"_s};
	f.type(u"see @ma and more"_s);
	f.composer.edit(u"see @ma and more"_s, 7);
	QTRY_COMPARE(f.composer.files()->rowCount(), 1);

	f.composer.acceptFile(0);

	QCOMPARE(f.composer.text(), u"see @src/main.rs and more"_s);
	QCOMPARE(f.composer.cursor(), 17);
	QCOMPARE(f.composer.completionKind(), u"none"_s);
}

void TestComposerModel::escapeClosesTheListFirst()
{
	Fixture f;
	f.type(u"/he"_s);
	QCOMPARE(f.composer.completionKind(), u"command"_s);
	QVERIFY(f.composer.escape());
	QCOMPARE(f.composer.completionKind(), u"none"_s);
	f.type(u"/hel"_s);
	QCOMPARE(f.composer.completionKind(), u"command"_s); // the text changed

	f.composer.clearDraft();
	f.type(u"!"_s);
	f.type(u"ls"_s);
	QVERIFY(f.composer.shellMode());
	QVERIFY(f.composer.escape());
	QVERIFY(!f.composer.shellMode());
	QVERIFY(!f.composer.escape()); // nothing running, nothing to do
}

void TestComposerModel::doubleEscapeInterrupts()
{
	Fixture f;
	QSignalSpy interrupts(&f.composer, &ComposerModel::interruptRequested);
	f.fake.send({{u"kind"_s, u"event"_s},
	             {u"session_id"_s, u"ses_1"_s},
	             {u"event"_s, QJsonObject{{u"type"_s, u"info"_s}, {u"info"_s, [] {
		                                                           QJsonObject info = infoJson();
		                                                           info[u"status"_s] = QJsonObject{{u"state"_s, u"busy"_s}};
		                                                           return info;
	                                                           }()}}}});
	QTRY_VERIFY(f.controller.isTurnInProgress());

	QVERIFY(f.composer.escape());
	QVERIFY(f.composer.isInterruptArmed());
	QCOMPARE(interrupts.count(), 0);
	QVERIFY(f.composer.escape());
	QCOMPARE(interrupts.count(), 1);
	QVERIFY(!f.composer.isInterruptArmed());
}

void TestComposerModel::secondEscapeComesTooLate()
{
	Fixture f;
	f.composer.setInterruptWindow(30);
	QSignalSpy interrupts(&f.composer, &ComposerModel::interruptRequested);
	f.fake.send({{u"kind"_s, u"event"_s},
	             {u"session_id"_s, u"ses_1"_s},
	             {u"event"_s, QJsonObject{{u"type"_s, u"info"_s}, {u"info"_s, [] {
		                                                           QJsonObject info = infoJson();
		                                                           info[u"status"_s] = QJsonObject{{u"state"_s, u"busy"_s}};
		                                                           return info;
	                                                           }()}}}});
	QTRY_VERIFY(f.controller.isTurnInProgress());

	QVERIFY(f.composer.escape());
	QTRY_VERIFY(!f.composer.isInterruptArmed());
	QVERIFY(f.composer.escape()); // arms again
	QCOMPARE(interrupts.count(), 0);
}

void TestComposerModel::withdrawingFillsTheDraft()
{
	Fixture f;
	f.queue({queuedMessage(u"q1"_s, u"first"_s)});
	f.removal = {{u"removed"_s, true}, {u"text"_s, u"first"_s}, {u"images"_s, QJsonArray()}};
	QTRY_COMPARE(f.composer.queue()->rowCount(), 1);

	f.composer.withdraw(0);

	QTRY_COMPARE(f.composer.text(), u"first"_s);
	QCOMPARE(f.requestsOf(u"remove_queued"_s).first().value(u"message_id"_s).toString(), u"q1"_s);
}

void TestComposerModel::withdrawingASentPromptTellsSo()
{
	Fixture f;
	f.queue({queuedMessage(u"q1"_s, u"first"_s)});
	f.removal = {{u"removed"_s, false}, {u"text"_s, u""_s}, {u"images"_s, QJsonArray()}};
	QSignalSpy toasts(&f.composer, &ComposerModel::toastRequested);
	QTRY_COMPARE(f.composer.queue()->rowCount(), 1);

	f.composer.withdraw(0);

	QTRY_COMPARE(toasts.count(), 1);
	QCOMPARE(f.composer.text(), QString());
	QCOMPARE(f.composer.queue()->index(0, 0).data(composer::QueueModel::RemovingRole).toBool(), false);
}

void TestComposerModel::customCommandSendsTheExpandedText()
{
	Fixture f;
	f.composer.runCustomCommand(u"review"_s, u"main.rs"_s);

	QTRY_COMPARE(f.requestsOf(u"prompt"_s).size(), 1);
	QCOMPARE(f.requestsOf(u"expand_command"_s).first().value(u"name"_s).toString(), u"review"_s);
	QCOMPARE(f.requestsOf(u"prompt"_s).first().value(u"text"_s).toString(), u"expanded main.rs"_s);
}

void TestComposerModel::emptyExpansionSendsNothing()
{
	Fixture f;
	f.expansion = u"  "_s;
	QSignalSpy toasts(&f.composer, &ComposerModel::toastRequested);

	f.composer.runCustomCommand(u"empty"_s, {});

	QTRY_COMPARE(toasts.count(), 1);
	QVERIFY(f.requestsOf(u"prompt"_s).isEmpty());
}

void TestComposerModel::imageFilesAreAttached()
{
	Fixture f;
	QTemporaryDir dir;
	QFile file(dir.filePath(u"shot.png"_s));
	QVERIFY(file.open(QIODevice::WriteOnly));
	file.write("abc");
	file.close();
	f.type(u"look: "_s);

	QSignalSpy toasts(&f.composer, &ComposerModel::toastRequested);
	QCOMPARE(f.composer.attachUrls({QUrl::fromLocalFile(file.fileName()), QUrl(u"file:///nowhere/readme.txt"_s)}), 1);
	QCOMPARE(toasts.count(), 1); // the text file is not an image

	QCOMPARE(f.composer.attachments()->rowCount(), 1);
	QVERIFY(f.composer.text().contains(u"[Image 1]"_s));
	QCOMPARE(f.composer.attachments()->index(0, 0).data(composer::AttachmentModel::NameRole).toString(), u"shot.png"_s);

	QVERIFY(f.composer.submit());
	QTRY_COMPARE(f.requestsOf(u"prompt"_s).size(), 1);
	const QJsonArray images = f.requestsOf(u"prompt"_s).first().value(u"images"_s).toArray();
	QCOMPARE(images.size(), 1);
	QCOMPARE(images.first().toObject().value(u"data"_s).toString(), u"YWJj"_s);
	QCOMPARE(f.composer.attachments()->rowCount(), 0);
}

void TestComposerModel::clipboardImageIsAttached()
{
	Fixture f;
	QImage image(4, 4, QImage::Format_RGB32);
	image.fill(Qt::red);
	QGuiApplication::clipboard()->setImage(image);

	QVERIFY(f.composer.pasteFromClipboard());

	QCOMPARE(f.composer.attachments()->rowCount(), 1);
	QVERIFY(f.composer.text().contains(u"[Image 1]"_s));
	QVERIFY(f.composer.attachments()->index(0, 0).data(composer::AttachmentModel::SourceRole).toString().startsWith(u"data:image/png;base64,"_s));
}

void TestComposerModel::clipboardImagePathIsAttachedAndOtherTextIsLeftAlone()
{
	Fixture f;
	QTemporaryDir dir;
	QFile file(dir.filePath(u"shot.png"_s));
	QVERIFY(file.open(QIODevice::WriteOnly));
	file.write("abc");
	file.close();

	QGuiApplication::clipboard()->setText(file.fileName());
	QVERIFY(f.composer.pasteFromClipboard());
	QCOMPARE(f.composer.attachments()->rowCount(), 1);

	QGuiApplication::clipboard()->setText(u"just words"_s);
	QVERIFY(!f.composer.pasteFromClipboard()); // the text area pastes it
	QCOMPARE(f.composer.attachments()->rowCount(), 1);
}

void TestComposerModel::clearDraftKeepsLongDraftsInTheHistory()
{
	Fixture f;
	f.type(u"short"_s);
	f.composer.clearDraft();
	QVERIFY(f.history.items().isEmpty());

	f.type(u"a draft long enough to keep"_s);
	f.composer.clearDraft();
	QCOMPARE(f.history.items(), (QStringList{u"a draft long enough to keep"_s}));
	QVERIFY(!f.composer.hasContent());
}

void TestComposerModel::queueFollowsTheServer()
{
	Fixture f;
	QCOMPARE(f.composer.queue()->rowCount(), 0);
	f.queue({queuedMessage(u"q1"_s, u"later"_s)});

	QTRY_COMPARE(f.composer.queue()->rowCount(), 1);
	const QModelIndex row = f.composer.queue()->index(0, 0);
	QCOMPARE(row.data(composer::QueueModel::TextRole).toString(), u"later"_s);
	QVERIFY(row.data(composer::QueueModel::RemovableRole).toBool());
}

// Every request the model makes is held back, the model is destroyed, and then the answers arrive.
// The test passes without a sanitizer too; an address sanitizer build reports the access to the freed model.
void TestComposerModel::answersAfterTheModelIsGoneAreIgnored()
{
	Fixture f;
	const QStringList held{u"prompt"_s, u"shell"_s, u"expand_command"_s, u"list_files"_s, u"remove_queued"_s};
	QList<QJsonObject> pending;
	f.fake.onRequest = [&](const QJsonObject& request) {
		if(held.contains(request.value(u"op"_s).toString()))
		{
			pending.append(request);
			return;
		}
		f.answer(request);
	};
	f.queue({queuedMessage(u"q1"_s, u"first"_s)});
	auto gone = std::make_unique<ComposerModel>(f.backend, f.controller, f.history);
	QTRY_COMPARE(gone->queue()->rowCount(), 1);
	gone->edit(u"look at @sr"_s, 11);
	QTRY_COMPARE(pending.size(), 1); // the file lookup waits a moment before it asks
	gone->edit(u"Do it"_s, 5);
	QVERIFY(gone->submit());
	gone->edit(u"!"_s, 1);
	gone->edit(u"ls"_s, 2);
	QVERIFY(gone->submit());
	gone->runCustomCommand(u"review"_s, u"x"_s);
	gone->withdraw(0);
	QTRY_COMPARE(pending.size(), 5);

	gone.reset();
	// One answer that fits every held request.
	const QJsonObject answer{{u"queued"_s, false},
	                         {u"message_id"_s, u"u9"_s},
	                         {u"text"_s, u"x"_s},
	                         {u"files"_s, QJsonArray()},
	                         {u"removed"_s, true},
	                         {u"images"_s, QJsonArray()}};
	for(const QJsonObject& request : std::as_const(pending))
	{
		f.fake.respond(request, answer);
	}

	QTest::qWait(100); // the answers are handled
	QVERIFY(f.fake.socket()->isValid());
}

QTEST_MAIN(TestComposerModel)
#include "tst_composer_model.moc"
