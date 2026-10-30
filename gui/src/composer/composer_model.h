#pragma once

#include "client/backend.h"
#include "compose/completion_target.h"
#include "compose/prompt_draft.h"
#include "compose/prompt_history.h"
#include "composer/attachment_model.h"
#include "composer/file_model.h"
#include "composer/queue_model.h"
#include "session/session_controller.h"

#include <QCoroTask>
#include <QObject>
#include <QTimer>
#include <QUrl>

#include <chrono>

namespace refgui::composer
{

// The input box of one session, apart from how it is drawn: the prompt being written with its
// images, the `/` and `@` lists, the history behind Up and Down, the queued prompts, the labels
// for agent, model and variant, and the actions Enter and Esc stand for.
//
// The text area owns the editing; it tells this object what the text and the cursor are
// (edit(), moveCursor()), and shows `text` and `cursor` again when draftReplaced() says they
// were changed from here (history, a restored prompt, a new attachment).
//
// Slash commands are the window's: the view asks the command registry first and calls
// submit() for what is not a command.
class ComposerModel : public QObject
{
	Q_OBJECT
	Q_PROPERTY(QString text READ text NOTIFY draftChanged)
	Q_PROPERTY(int cursor READ cursor NOTIFY draftChanged)
	Q_PROPERTY(bool hasContent READ hasContent NOTIFY draftChanged)
	Q_PROPERTY(bool shellMode READ shellMode NOTIFY shellModeChanged)
	Q_PROPERTY(bool ready READ isReady NOTIFY readyChanged)
	Q_PROPERTY(QAbstractItemModel* attachments READ attachments CONSTANT)
	Q_PROPERTY(QAbstractItemModel* queue READ queue CONSTANT)
	Q_PROPERTY(QAbstractItemModel* files READ files CONSTANT)
	Q_PROPERTY(QString completionKind READ completionKind NOTIFY completionChanged)
	Q_PROPERTY(QString completionQuery READ completionQuery NOTIFY completionChanged)
	Q_PROPERTY(bool interruptArmed READ isInterruptArmed NOTIFY interruptArmedChanged)
	Q_PROPERTY(int interruptWindow READ interruptWindow WRITE setInterruptWindow NOTIFY interruptWindowChanged)
	Q_PROPERTY(QString agentName READ agentName NOTIFY labelsChanged)
	Q_PROPERTY(QString agentColor READ agentColor NOTIFY labelsChanged)
	Q_PROPERTY(int agentIndex READ agentIndex NOTIFY labelsChanged)
	Q_PROPERTY(QString modelName READ modelName NOTIFY labelsChanged)
	Q_PROPERTY(QString modelDescription READ modelDescription NOTIFY labelsChanged)
	Q_PROPERTY(QString variant READ variant NOTIFY labelsChanged)
	Q_PROPERTY(bool variantAvailable READ variantAvailable NOTIFY labelsChanged)

public:
	// How long the typed `@` query rests before the files are asked for.
	static constexpr std::chrono::milliseconds FileSearchDelay{80};
	static constexpr int FileLimit = 50;
	// A draft this long goes to the history when it is thrown away with Ctrl+C.
	static constexpr qsizetype DiscardKeepLength = 20;
	static constexpr int DefaultInterruptWindow = 1500;

	// All of them outlive the model.
	ComposerModel(client::Backend& backend, session::SessionController& controller, compose::PromptHistory& history, QObject* parent = nullptr);

	const QString& text() const { return m_draft.text(); }
	int cursor() const { return static_cast<int>(m_draft.cursor()); }
	bool hasContent() const { return !m_draft.isEmpty(); }
	bool shellMode() const { return m_shellMode; }
	// Prompts can be sent: the backend is reachable and the session is attached.
	bool isReady() const;
	QAbstractItemModel* attachments() { return &m_attachments; }
	QAbstractItemModel* queue() { return &m_queue; }
	QAbstractItemModel* files() { return &m_files; }
	// "none", "command" or "file": what the list under the input box is about.
	QString completionKind() const;
	QString completionQuery() const { return m_target.query; }
	bool isInterruptArmed() const { return m_interruptTimer.isActive(); }
	int interruptWindow() const { return static_cast<int>(m_interruptTimer.interval()); }
	void setInterruptWindow(int milliseconds);
	QString agentName() const;
	// The color the agent's configuration names, or empty; with the agent's place in the list
	// the view picks one from the palette.
	QString agentColor() const;
	int agentIndex() const;
	QString modelName() const;
	// Provider and model id, for a tooltip.
	QString modelDescription() const;
	// The variant in use, or empty.
	QString variant() const;
	// The model has variants to choose from.
	bool variantAvailable() const;

	// The text area's text and cursor after the user changed them.
	Q_INVOKABLE void edit(const QString& text, int cursor);
	Q_INVOKABLE void moveCursor(int cursor);

	// Sends the prompt (or runs the shell line in shell mode). False when there is nothing to
	// send or the session is not ready.
	Q_INVOKABLE bool submit();
	// The view ran the draft as a slash command: remember it and empty the draft.
	Q_INVOKABLE void finishCommand();
	// Sends the prompt a user-defined command expands to.
	Q_INVOKABLE void runCustomCommand(const QString& name, const QString& args);

	// Up and Down in the prompt history. They return false when the key should do its normal
	// thing: move the cursor.
	Q_INVOKABLE bool historyPrevious();
	Q_INVOKABLE bool historyNext();

	// Takes a queued prompt back into the draft.
	Q_INVOKABLE void withdraw(int row);
	// Ctrl+C: empties the draft; a long one is kept in the history.
	Q_INVOKABLE void clearDraft();
	// Esc. Returns true when it did something: closed the list, left shell mode, or took part
	// in the double Esc that interrupts the running reply.
	Q_INVOKABLE bool escape();
	Q_INVOKABLE void leaveShell();

	// The `@` list entry `row` replaces the mention being typed; the command replaces the text.
	Q_INVOKABLE void acceptFile(int row);
	Q_INVOKABLE void completeCommand(const QString& name);
	// Closes the list until the text changes.
	Q_INVOKABLE void dismissCompletion();

	// Ctrl+V: attaches an image from the clipboard, or the image file whose path or URL is
	// there. False when the clipboard holds something the text area should paste itself.
	Q_INVOKABLE bool pasteFromClipboard();
	// Dropped files. Returns how many were attached; others are left to the caller.
	Q_INVOKABLE int attachUrls(const QList<QUrl>& urls);
	Q_INVOKABLE void removeAttachment(int row);

	// Puts a prompt that was taken back (undo) into the draft.
	void restoreDraft(const QString& text, const QList<proto::NamedImage>& images);

signals:
	// The text, the cursor or the attachments changed, by whoever.
	void draftChanged();
	// The text or the cursor was changed from this side; the text area shows them again.
	void draftReplaced();
	void shellModeChanged();
	void readyChanged();
	void completionChanged();
	void interruptArmedChanged();
	void interruptWindowChanged();
	void labelsChanged();
	// The user asked to stop the running reply (the second Esc).
	void interruptRequested();
	// A prompt or shell line was handed to the backend.
	void sent();
	// Something the user asked for failed.
	void failed(const QString& text);
	// A remark for the window's toast; `level` is info, warn or error.
	void toastRequested(const QString& level, const QString& text);

private:
	void updateCompletion();
	// The draft was changed from here: the text area must show it.
	void changeDraft();
	// The draft was edited in the text area, which already shows it.
	void draftEdited();
	void setShellMode(bool shellMode);
	void fillFromHistory(const QString& entry);
	// Attaches the image files among `urls`; returns how many. Other files are reported if asked.
	int attachImageUrls(const QList<QUrl>& urls, bool reportOthers);
	void attachImage(proto::NamedImage image);
	void disarmInterrupt();
	QCoro::Task<> loadConfig();
	// With `restoreOnFailure` a prompt that was not accepted goes back into the draft.
	QCoro::Task<> sendPrompt(QString text, QList<proto::NamedImage> images, bool restoreOnFailure);
	QCoro::Task<> sendShell(QString command);
	QCoro::Task<> expandAndSend(QString name, QString args);
	QCoro::Task<> removeQueued(QString messageId);
	QCoro::Task<> findFiles(QString query, quint64 serial);
	const proto::AgentInfo* agentInfo() const;
	const proto::ModelInfo* modelInfo() const;

	client::Backend& m_backend;
	session::SessionController& m_controller;
	compose::PromptHistory& m_history;
	compose::HistoryBrowser m_browser;
	compose::PromptDraft m_draft;
	bool m_shellMode = false;
	compose::CompletionTarget m_target;
	bool m_completionDismissed = false;
	AttachmentModel m_attachments;
	QueueModel m_queue;
	FileModel m_files;
	QTimer m_fileTimer;
	QString m_fileQuery;
	quint64 m_fileSerial = 0;
	QTimer m_interruptTimer;
	proto::ServerConfig m_config;
};

} // namespace refgui::composer
