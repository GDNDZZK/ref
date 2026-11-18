// The input box of a session against a fake backend: what it sends and how it follows the answers.
#include "client/backend.h"
#include "compose/prompt_history.h"
#include "composer/composer_model.h"
#include "coro_test.h"
#include "workspace/session_directory.h"
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

constexpr int SettleMs = 100;       // time for the messages in flight to be handled
constexpr int SearchSettleMs = 200; // well past ComposerModel::FileSearchDelay and the answer

QJsonObject infoJson()
{
	return sessionInfoJson(u"ses_1"_s,
	                       {.name = u"one"_s,
	                        .title = u"A title"_s,
	                        .agent = u"plan"_s,
	                        .model = QJsonObject{{u"provider"_s, u"p"_s}, {u"model"_s, u"m"_s}, {u"variant"_s, u"high"_s}},
	                        .attached = 1});
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
		: connection(Backend::Source(Backend::Mode::Remote{{fake.url(), u"token"_s}}), u"/work"_s)
		, backend(connection.backend())
		, config(connection)
		, controller(backend)
		, directory(connection)
		, composer(config, controller, history)
	{
		fake.onRequest = [this](const QJsonObject& request) { answer(request); };
		composer.setSessionDirectory(&directory);
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
		else if(op == u"shell"_s)
		{
			if(shellError.isEmpty())
			{
				fake.respond(request, {});
			}
			else
			{
				fake.fail(request, shellError);
			}
		}
		else if(op == u"list_files"_s && !filesError.isEmpty())
		{
			fake.fail(request, filesError);
		}
		else if(op == u"list_files"_s)
		{
			fake.respond(request, {{u"files"_s, QJsonArray::fromStringList(files)}});
		}
		else if(op == u"list_dirs"_s)
		{
			const QString reqPath = request.value(u"path"_s).toString();
			if(!dirsError.isEmpty())
			{
				fake.fail(request, dirsError, u"dir_not_found"_s);
			}
			else if(dirMap.contains(reqPath))
			{
				fake.respond(request, dirMap.value(reqPath));
			}
			else
			{
				fake.respond(request, {{u"path"_s, u"/work"_s}, {u"dirs"_s, QJsonArray::fromStringList(dirs)}, {u"files"_s, QJsonArray::fromStringList(dirFiles)}});
			}
		}
		else if(op == u"list_binaries"_s)
		{
			fake.respond(request, {{u"binaries"_s, QJsonArray::fromStringList(binaries)}});
		}
		else if(op == u"list_sessions"_s)
		{
			fake.respond(request, {{u"sessions"_s, liveSessions}});
		}
		else if(op == u"list_history"_s)
		{
			fake.respond(request, {{u"sessions"_s, historySessions}});
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
	workspace::Connection connection;
	Backend& backend;
	workspace::BackendConfig config;
	SessionController controller;
	workspace::SessionDirectory directory;
	compose::PromptHistory history;
	ComposerModel composer;
	QStringList files;
	QString filesError; // when set, list_files is refused with it
	QStringList dirs;
	QStringList dirFiles;
	QHash<QString, QJsonObject> dirMap;
	QString dirsError;
	QStringList binaries;
	QJsonArray liveSessions;
	QJsonArray historySessions;
	QString promptError;
	QString shellError;
	std::optional<QString> expansion;
	QJsonObject removal;
};

} // namespace

class TestComposerModel : public QObject
{
	Q_OBJECT

private slots:
	void labelsFollowTheSessionAndTheConfiguration();
	void agentMissingFromTheConfigurationHasNoIndex();
	void sendsThePromptAndEmptiesTheDraft();
	void failedPromptComesBack();
	void bangStartsShellMode();
	void failedShellLineComesBackAsShellLine();
	void shellLineIsSentAsShell();
	void historyWalksBackAndForth();
	void historyKeepsShellLines();
	void mentionListFollowsTheQueryAndDropsStaleAnswers();
	void failedFileSearchEmptiesTheList();
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
	void unknownSingleSlashTokenWarnsAndKeepsDraft();
	void slashArgumentCompletionsCoverPathsModelsAgentsVariantsAndSessions();
	void shellModeCompletionsAndHistoryFilterWork();
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

void TestComposerModel::agentMissingFromTheConfigurationHasNoIndex()
{
	Fixture f;
	QJsonObject info = infoJson();
	info.insert(u"agent"_s, u"ghost"_s);

	f.fake.send({{u"kind"_s, u"event"_s}, {u"session_id"_s, u"ses_1"_s}, {u"event"_s, QJsonObject{{u"type"_s, u"info"_s}, {u"info"_s, info}}}});

	QTRY_COMPARE(f.composer.agentName(), u"ghost"_s);
	QCOMPARE(f.composer.agentIndex(), ComposerModel::NoAgentIndex);
	QCOMPARE(f.composer.agentColor(), QString());
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

void TestComposerModel::failedShellLineComesBackAsShellLine()
{
	Fixture f;
	f.shellError = u"no shell"_s;
	QSignalSpy failed(&f.composer, &ComposerModel::failed);
	f.type(u"!"_s);
	f.type(u"ls -l"_s);

	QVERIFY(f.composer.submit());

	QTRY_COMPARE(failed.count(), 1);
	QVERIFY(f.composer.shellMode());
	QCOMPARE(f.composer.text(), u"ls -l"_s);
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
	QTest::qWait(SearchSettleMs);
	QCOMPARE(f.requestsOf(u"list_files"_s).size(), 1);
}

void TestComposerModel::failedFileSearchEmptiesTheList()
{
	Fixture f;
	f.files = {u"src/main.rs"_s};
	f.type(u"look at @sr"_s);
	QTRY_COMPARE(f.composer.files()->rowCount(), 1);

	f.filesError = u"no such directory"_s;
	QTest::ignoreMessage(QtWarningMsg, "file search failed: \"no such directory\"");
	f.type(u"look at @src"_s);

	QTRY_COMPARE(f.requestsOf(u"list_files"_s).size(), 2);
	QTRY_COMPARE(f.composer.files()->rowCount(), 0);
	QCOMPARE(f.composer.completionKind(), u"file"_s);
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
	QFile notes(dir.filePath(u"notes.txt"_s));
	QVERIFY(notes.open(QIODevice::WriteOnly));
	notes.write("hello");
	notes.close();
	f.type(u"look: "_s);

	QSignalSpy toasts(&f.composer, &ComposerModel::toastRequested);
	const QList<QUrl> urls{QUrl::fromLocalFile(file.fileName()), QUrl::fromLocalFile(notes.fileName()), QUrl(u"file:///nowhere/readme.txt"_s)};
	QCOMPARE(f.composer.attachUrls(urls), 2);
	QCOMPARE(toasts.count(), 1); // missing local file emits a warning toast

	QCOMPARE(f.composer.attachments()->rowCount(), 1);
	QVERIFY(f.composer.text().contains(u"[Image 1]"_s));
	QVERIFY(f.composer.text().contains(notes.fileName()));
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

	QFile notes(dir.filePath(u"notes.txt"_s));
	QVERIFY(notes.open(QIODevice::WriteOnly));
	notes.write("hello");
	notes.close();
	auto* mime = new QMimeData;
	mime->setUrls({QUrl::fromLocalFile(notes.fileName())});
	QGuiApplication::clipboard()->setMimeData(mime);
	QVERIFY(f.composer.pasteFromClipboard());
	QVERIFY(f.composer.text().contains(notes.fileName()));
	QCOMPARE(f.composer.attachments()->rowCount(), 1);

	auto* webMime = new QMimeData;
	webMime->setUrls({QUrl(u"https://example.com"_s)});
	QGuiApplication::clipboard()->setMimeData(webMime);
	QVERIFY(!f.composer.pasteFromClipboard());

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
	auto gone = std::make_unique<ComposerModel>(f.config, f.controller, f.history);
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

	QTest::qWait(SettleMs); // the answers are handled
	QVERIFY(f.fake.socket()->isValid());
}

void TestComposerModel::unknownSingleSlashTokenWarnsAndKeepsDraft()
{
	Fixture f;
	QSignalSpy toasts(&f.composer, &ComposerModel::toastRequested);

	f.type(u"/modles"_s);
	QVERIFY(!f.composer.submit());
	QCOMPARE(f.composer.text(), u"/modles"_s);
	QCOMPARE(toasts.count(), 1);
	QCOMPARE(toasts.first().at(0).toString(), u"warn"_s);
	QVERIFY(toasts.first().at(1).toString().contains(u"Unknown command /modles"_s));
	QVERIFY(toasts.first().at(1).toString().contains(u"/models"_s));
	QVERIFY(f.requestsOf(u"prompt"_s).isEmpty());

	toasts.clear();
	f.type(u"/zzzzzz"_s);
	QVERIFY(!f.composer.submit());
	QCOMPARE(f.composer.text(), u"/zzzzzz"_s);
	QCOMPARE(toasts.count(), 1);
	QCOMPARE(toasts.first().at(1).toString(), u"Unknown command /zzzzzz"_s);

	// Multi-segment file paths and multi-word lines are still sent as prompts.
	f.type(u"/etc/hosts"_s);
	QVERIFY(f.composer.submit());
	QTRY_COMPARE(f.requestsOf(u"prompt"_s).size(), 1);
	QCOMPARE(f.requestsOf(u"prompt"_s).first().value(u"text"_s).toString(), u"/etc/hosts"_s);
}

void TestComposerModel::slashArgumentCompletionsCoverPathsModelsAgentsVariantsAndSessions()
{
	Fixture f;
	f.dirMap.insert(u"."_s, QJsonObject{{u"path"_s, u"/work"_s}, {u"dirs"_s, QJsonArray{u"docs"_s, u"src"_s}}, {u"files"_s, QJsonArray{u"README.md"_s}}});
	f.dirMap.insert(u"src/"_s, QJsonObject{{u"path"_s, u"/work/src"_s}, {u"dirs"_s, QJsonArray{u"app"_s}}, {u"files"_s, QJsonArray{u"main.cpp"_s}}});

	// /cd: lists directories only, caches parent dir with 0 extra RPCs while typing, and drills into subdirectories on Tab.
	f.type(u"/cd "_s);
	QCOMPARE(f.composer.completionKind(), u"argument"_s);
	QCOMPARE(f.composer.completionCommand(), u"cd"_s);
	QTRY_COMPARE(f.composer.completions()->rowCount(), 2);
	const int listDirsAfterRoot = int(f.requestsOf(u"list_dirs"_s).size());
	QCOMPARE(f.requestsOf(u"list_dirs"_s).last().value(u"include_files"_s).toBool(), false);
	QCOMPARE(f.requestsOf(u"list_dirs"_s).last().value(u"cwd"_s).toString(), u"/work"_s);

	f.type(u"/cd s"_s);
	f.type(u"/cd sr"_s);
	QCOMPARE(f.composer.completions()->rowCount(), 1); // synchronous parent-dir cache hit!
	QTest::qWait(SearchSettleMs);
	QCOMPARE(int(f.requestsOf(u"list_dirs"_s).size()), listDirsAfterRoot);

	QVERIFY(!f.composer.acceptCompletionItem(0, false)); // directory never auto-submits
	QCOMPARE(f.composer.text(), u"/cd src/"_s);
	QTRY_COMPARE(f.composer.completions()->rowCount(), 1);
	QCOMPARE(f.composer.completions()->index(0, 0).data(composer::CompletionItemModel::ValueRole).toString(), u"src/app/"_s);

	// /export: offers default ref-session-<id>.md first, plus directories and files.
	f.dirMap.clear();
	f.dirs = {u"docs"_s};
	f.dirFiles = {u"old.md"_s};
	f.type(u"/export "_s);
	QTRY_COMPARE(f.composer.completions()->rowCount(), 3);
	QCOMPARE(f.composer.completions()->index(0, 0).data(composer::CompletionItemModel::ValueRole).toString(), u"ref-session-ses_1.md"_s);
	QCOMPARE(f.requestsOf(u"list_dirs"_s).last().value(u"include_files"_s).toBool(), true);

	// /variant, /models, /agents.
	f.type(u"/variant "_s);
	QCOMPARE(f.composer.completions()->rowCount(), 3); // default, low, high
	QVERIFY(f.composer.acceptCompletionItem(1, true));  // Enter on leaf slash arg requests submit
	QCOMPARE(f.composer.text(), u"/variant low"_s);

	f.type(u"/model p/m#hi"_s);
	QCOMPARE(f.composer.completions()->rowCount(), 1);
	QCOMPARE(f.composer.completions()->index(0, 0).data(composer::CompletionItemModel::ValueRole).toString(), u"p/m#high"_s);

	f.type(u"/agent pl"_s);
	QCOMPARE(f.composer.completions()->rowCount(), 1);
	QCOMPARE(f.composer.completions()->index(0, 0).data(composer::CompletionItemModel::ValueRole).toString(), u"plan"_s);

	// /sessions and multi-token /close-session.
	proto::SessionInfo s1;
	s1.id = u"ses_1"_s;
	s1.name = u"alpha"_s;
	s1.title = u"First"_s;
	s1.cwd = u"/work"_s;
	s1.live = true;
	f.directory.update(s1);
	proto::SessionInfo s2;
	s2.id = u"ses_2"_s;
	s2.name = u"beta"_s;
	s2.title = u"Second"_s;
	s2.cwd = u"/work"_s;
	s2.live = true;
	f.directory.update(s2);
	f.type(u"/close-session al"_s);
	QCOMPARE(f.composer.completions()->rowCount(), 1);
	QVERIFY(!f.composer.acceptCompletionItem(0, false)); // Tab appends trailing space and keeps completing next token
	QCOMPARE(f.composer.text(), u"/close-session alpha "_s);
	QCOMPARE(f.composer.completions()->rowCount(), 1); // "alpha" is excluded from second token!
	QCOMPARE(f.composer.completions()->index(0, 0).data(composer::CompletionItemModel::ValueRole).toString(), u"beta"_s);
	QVERIFY(f.composer.acceptCompletionItem(0, true)); // Enter completes without extra space and submits
	QCOMPARE(f.composer.text(), u"/close-session alpha beta"_s);

	// Custom command @file completion.
	f.files = {u"src/main.rs"_s};
	f.type(u"/review please check @sr"_s);
	QCOMPARE(f.composer.completionKind(), u"file"_s);
	QTRY_COMPARE(f.composer.files()->rowCount(), 1);
	f.composer.acceptFile(0);
	QCOMPARE(f.composer.text(), u"/review please check @src/main.rs "_s);
}

void TestComposerModel::shellModeCompletionsAndHistoryFilterWork()
{
	Fixture f;
	f.history.push(u"normal prompt"_s);
	f.history.push(u"!cargo check --workspace"_s);
	f.history.push(u"another prompt"_s);
	f.binaries = {u"cargo"_s, u"cat"_s};
	f.dirs = {u"src"_s};
	f.dirFiles = {u"Cargo.toml"_s};

	f.type(u"!"_s);
	QVERIFY(f.composer.shellMode());
	QCOMPARE(f.composer.completionKind(), u"none"_s); // quiet until typing or Tab

	// Up/Down in shell mode visits only `!`-prefixed history entries and restores shell mode at end.
	QVERIFY(f.composer.historyPrevious());
	QVERIFY(f.composer.shellMode());
	QCOMPARE(f.composer.text(), u"cargo check --workspace"_s);
	QVERIFY(f.composer.historyPrevious()); // stays browsing at oldest shell entry, skipping "normal prompt"
	QVERIFY(f.composer.shellMode());
	QCOMPARE(f.composer.text(), u"cargo check --workspace"_s);
	QVERIFY(f.composer.historyNext());
	QVERIFY(f.composer.shellMode());
	QCOMPARE(f.composer.text(), QString());

	// Tab on empty `!` triggers ShellCommand completion (!history + binaries).
	f.composer.triggerCompletion();
	QCOMPARE(f.composer.completionKind(), u"shellCommand"_s);
	QTRY_COMPARE(f.composer.completions()->rowCount(), 3); // !cargo check --workspace, cargo, cat
	QCOMPARE(f.composer.completions()->index(0, 0).data(composer::CompletionItemModel::ValueRole).toString(), u"cargo check --workspace"_s);
	QCOMPARE(f.composer.completions()->index(1, 0).data(composer::CompletionItemModel::ValueRole).toString(), u"cargo"_s);
	QCOMPARE(f.requestsOf(u"list_binaries"_s).last().value(u"limit"_s).toInt(), 50);

	// Accepting a binary in shell mode NEVER auto-submits, appends space, and transitions to ShellPath.
	QVERIFY(!f.composer.acceptCompletionItem(2, true));
	QCOMPARE(f.composer.text(), u"cat "_s);
	QCOMPARE(f.composer.completionKind(), u"shellPath"_s);
	QTRY_COMPARE(f.composer.completions()->rowCount(), 2); // src/, Cargo.toml
	QVERIFY(!f.composer.acceptCompletionItem(1, true)); // accepting Cargo.toml never auto-submits shell!
	QCOMPARE(f.composer.text(), u"cat Cargo.toml "_s);
	QVERIFY(f.requestsOf(u"shell"_s).isEmpty());
}

QTEST_MAIN(TestComposerModel)
#include "tst_composer_model.moc"
