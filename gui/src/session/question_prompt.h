#pragma once

#include "proto/types.h"

#include <QList>
#include <QObject>
#include <QString>
#include <QStringList>
#include <QVariantList>

#include <optional>

namespace refgui::session
{

// The card for the model's questions: the page the user is on, what is ticked, the text of a
// custom answer. The model asks, this keeps the answers and says when they are ready to send. It
// knows nothing of the backend; the owner sends `submitRequested` and `dismissRequested` on.
//
// One question with a single choice is a plain list: picking a row answers. Anything else has a
// page per question and a last page that lists the answers to confirm.
class QuestionPrompt : public QObject
{
	Q_OBJECT
	Q_PROPERTY(bool active READ isActive NOTIFY changed)
	Q_PROPERTY(QString callId READ callId NOTIFY changed)
	Q_PROPERTY(bool simple READ isSimple NOTIFY changed)
	Q_PROPERTY(QVariantList tabs READ tabs NOTIFY changed)
	Q_PROPERTY(int page READ page NOTIFY changed)
	Q_PROPERTY(bool confirming READ isConfirming NOTIFY changed)
	Q_PROPERTY(QString header READ header NOTIFY changed)
	Q_PROPERTY(QString text READ text NOTIFY changed)
	Q_PROPERTY(bool multiple READ isMultiple NOTIFY changed)
	Q_PROPERTY(QVariantList rows READ rows NOTIFY changed)
	Q_PROPERTY(QVariantList summary READ summary NOTIFY changed)
	Q_PROPERTY(bool editing READ isEditing NOTIFY changed)
	Q_PROPERTY(QString customText READ customText NOTIFY changed)
	Q_PROPERTY(bool busy READ isBusy WRITE setBusy NOTIFY changed)
	Q_PROPERTY(int unansweredCount READ unansweredCount NOTIFY changed)

public:
	explicit QuestionPrompt(QObject* parent = nullptr);

	// Shows `request`, or nothing. The same request again changes nothing, so what the user
	// did stays.
	void setRequest(const std::optional<proto::QuestionRequest>& request);

	bool isActive() const { return m_active; }
	const QString& callId() const { return m_request.callId; }
	// One question, one choice: no tabs, no confirmation.
	bool isSimple() const;
	// A tab per question and a last one for the confirmation. Empty when simple.
	// Entries: title, answered, current, confirm.
	QVariantList tabs() const;
	int page() const { return m_page; }
	bool isConfirming() const { return m_active && !isSimple() && m_page == questionCount(); }
	QString header() const;
	QString text() const;
	bool isMultiple() const;
	// The rows of the page: an option each, then the custom answer. Entries: label, description,
	// isDefault, checked, highlighted, number, custom.
	QVariantList rows() const;
	// On the confirming page, an entry per question: header, answer, answered.
	QVariantList summary() const;
	// Typing a custom answer; the card has a text field then and the keys are the field's.
	bool isEditing() const { return m_editing; }
	// What the custom answer was before, to start the field with.
	QString customText() const;
	// The answers are on their way; the card ignores the user until they are answered.
	bool isBusy() const { return m_busy; }
	void setBusy(bool busy);
	int unansweredCount() const;

	// The answers so far: per question, the ticked labels in order and the custom text.
	QList<QStringList> answers() const;

	Q_INVOKABLE void setHighlight(int row);
	Q_INVOKABLE void moveHighlight(int delta);
	// A click or Enter on a row: answers (simple), picks and moves on (single choice), or ticks
	// (multiple choice). The custom row starts the text field.
	Q_INVOKABLE void activateRow(int row);
	Q_INVOKABLE void goToPage(int page);
	// The pages go round, the confirming page being the last.
	Q_INVOKABLE void nextPage();
	Q_INVOKABLE void previousPage();
	// Takes the answers as they are (on the confirming page).
	Q_INVOKABLE void submit();
	Q_INVOKABLE void dismiss();
	// The text field: Enter takes the text, Esc leaves.
	Q_INVOKABLE void commitCustom(const QString& text);
	Q_INVOKABLE void cancelCustom();

	// The keys of the card when it has them: arrows, j and k, Tab, Enter, Space, 1 to 9, Esc. False for a key
	// it has no use for. Not while a text field is being typed in.
	bool handleKey(int key, int modifiers);

signals:
	void changed();
	// The answers are ready; one list per question (empty: not answered).
	void submitRequested(const QString& callId, const QList<QStringList>& answers);
	void dismissRequested(const QString& callId);

private:
	struct PageState
	{
		QList<bool> checked; // per option
		QString customText;
		int highlighted = 0;
	};

	int questionCount() const { return static_cast<int>(m_request.questions.size()); }
	const proto::Question& currentQuestion() const { return m_request.questions.at(m_page); }
	bool onQuestion() const { return m_active && m_page < questionCount(); }
	int rowCount() const;
	int customRow() const;
	bool hasAnswer(int question) const;
	QStringList answerOf(int question) const;
	void toggleRow(int row);
	void pickRow(int row);
	void startCustom();
	void submitNow(const QList<QStringList>& answers);

	proto::QuestionRequest m_request;
	QList<PageState> m_state;
	bool m_active = false;
	bool m_editing = false;
	bool m_busy = false;
	int m_page = 0;
};

} // namespace refgui::session
