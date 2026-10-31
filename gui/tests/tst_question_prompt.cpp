#include "session/question_prompt.h"

#include <QSignalSpy>
#include <QtTest>

using namespace Qt::StringLiterals;
using namespace refgui::proto;
using refgui::session::QuestionPrompt;

namespace
{

QuestionOption option(const QString& label, bool isDefault = false)
{
	return {.label = label, .description = label + u" description"_s, .isDefault = isDefault};
}

// The two questions of the mock backend's "question" script: a single choice and a multiple choice.
QuestionRequest twoQuestions(const QString& callId = u"call_1"_s)
{
	QuestionRequest request;
	request.sessionId = u"ses_1"_s;
	request.callId = callId;
	request.questions.append({.question = u"How?"_s,
	                          .header = u"Loop"_s,
	                          .options = {option(u"Range"_s, true), option(u"Iterator"_s), option(u"Guard"_s)},
	                          .multiple = false,
	                          .custom = true});
	request.questions.append({.question = u"Checks?"_s,
	                          .header = u"Checks"_s,
	                          .options = {option(u"Tests"_s, true), option(u"Clippy"_s, true), option(u"Format"_s)},
	                          .multiple = true,
	                          .custom = false});
	return request;
}

QuestionRequest oneQuestion(bool custom = true)
{
	QuestionRequest request;
	request.callId = u"call_one"_s;
	request.questions.append({.question = u"Which?"_s,
	                          .header = u"File"_s,
	                          .options = {option(u"a.rs"_s), option(u"b.rs"_s, true)},
	                          .multiple = false,
	                          .custom = custom});
	return request;
}

QVariantMap row(const QuestionPrompt& prompt, int index)
{
	return prompt.rows().at(index).toMap();
}

} // namespace

class TestQuestionPrompt : public QObject
{
	Q_OBJECT

private slots:
	void nothingUntilARequestComes()
	{
		QuestionPrompt prompt;
		QVERIFY(!prompt.isActive());
		QVERIFY(prompt.rows().isEmpty());
		QVERIFY(prompt.answers().isEmpty());
		QVERIFY(!prompt.handleKey(Qt::Key_Down, Qt::NoModifier));
	}

	void oneSingleChoiceIsAPlainList()
	{
		QuestionPrompt prompt;
		prompt.setRequest(oneQuestion());

		QVERIFY(prompt.isActive());
		QVERIFY(prompt.isSimple());
		QVERIFY(prompt.tabs().isEmpty());
		QCOMPARE(prompt.header(), u"File"_s);
		QCOMPARE(prompt.text(), u"Which?"_s);
		QCOMPARE(prompt.rows().size(), 3); // two options and the custom row
		QVERIFY(row(prompt, 2).value(u"custom"_s).toBool());
		QCOMPARE(row(prompt, 2).value(u"number"_s).toInt(), 3);
	}

	void theDefaultIsPickedAndHighlighted()
	{
		QuestionPrompt prompt;
		prompt.setRequest(oneQuestion());

		QVERIFY(!row(prompt, 0).value(u"checked"_s).toBool());
		QVERIFY(row(prompt, 1).value(u"checked"_s).toBool());
		QVERIFY(row(prompt, 1).value(u"isDefault"_s).toBool());
		QVERIFY(row(prompt, 1).value(u"highlighted"_s).toBool());
	}

	void pickingAnOptionAnswersASingleQuestion()
	{
		QuestionPrompt prompt;
		prompt.setRequest(oneQuestion());
		QSignalSpy submitted(&prompt, &QuestionPrompt::submitRequested);

		prompt.activateRow(0);

		QCOMPARE(submitted.count(), 1);
		QCOMPARE(submitted.first().at(0).toString(), u"call_one"_s);
		QCOMPARE(submitted.first().at(1).value<QList<QStringList>>(), (QList<QStringList>{{u"a.rs"_s}}));
		QVERIFY(prompt.isBusy());
	}

	void enterTakesTheHighlightedRow()
	{
		QuestionPrompt prompt;
		prompt.setRequest(oneQuestion());
		QSignalSpy submitted(&prompt, &QuestionPrompt::submitRequested);

		QVERIFY(prompt.handleKey(Qt::Key_Up, Qt::NoModifier));
		QVERIFY(prompt.handleKey(Qt::Key_Return, Qt::NoModifier));

		QCOMPARE(submitted.first().at(1).value<QList<QStringList>>(), (QList<QStringList>{{u"a.rs"_s}}));
	}

	void theHighlightStopsAtTheEnds()
	{
		QuestionPrompt prompt;
		prompt.setRequest(oneQuestion());

		for(int i = 0; i < 5; ++i)
		{
			prompt.handleKey(Qt::Key_Down, Qt::NoModifier);
		}
		QVERIFY(row(prompt, 2).value(u"highlighted"_s).toBool());
		for(int i = 0; i < 5; ++i)
		{
			prompt.handleKey(Qt::Key_K, Qt::NoModifier);
		}
		QVERIFY(row(prompt, 0).value(u"highlighted"_s).toBool());
		prompt.handleKey(Qt::Key_J, Qt::NoModifier);
		QVERIFY(row(prompt, 1).value(u"highlighted"_s).toBool());
	}

	void aNumberPicksThatRow()
	{
		QuestionPrompt prompt;
		prompt.setRequest(oneQuestion());
		QSignalSpy submitted(&prompt, &QuestionPrompt::submitRequested);

		QVERIFY(prompt.handleKey(Qt::Key_9, Qt::NoModifier)); // no such row: the key is the card's, and nothing happens
		QVERIFY(submitted.isEmpty());
		QVERIFY(prompt.handleKey(Qt::Key_1, Qt::NoModifier));

		QCOMPARE(submitted.first().at(1).value<QList<QStringList>>(), (QList<QStringList>{{u"a.rs"_s}}));
	}

	void escapeDismisses()
	{
		QuestionPrompt prompt;
		prompt.setRequest(oneQuestion());
		QSignalSpy dismissed(&prompt, &QuestionPrompt::dismissRequested);

		QVERIFY(prompt.handleKey(Qt::Key_Escape, Qt::NoModifier));

		QCOMPARE(dismissed.count(), 1);
		QCOMPARE(dismissed.first().at(0).toString(), u"call_one"_s);
	}

	void shortcutsAreNotTheCards()
	{
		QuestionPrompt prompt;
		prompt.setRequest(oneQuestion());
		QSignalSpy submitted(&prompt, &QuestionPrompt::submitRequested);

		QVERIFY(!prompt.handleKey(Qt::Key_1, Qt::ControlModifier));
		QVERIFY(!prompt.handleKey(Qt::Key_Return, Qt::AltModifier));
		QVERIFY(submitted.isEmpty());
	}

	void aBusyCardIgnoresTheUser()
	{
		QuestionPrompt prompt;
		prompt.setRequest(oneQuestion());
		QSignalSpy submitted(&prompt, &QuestionPrompt::submitRequested);
		QSignalSpy dismissed(&prompt, &QuestionPrompt::dismissRequested);
		prompt.activateRow(0);

		prompt.activateRow(1);
		prompt.dismiss();

		QCOMPARE(submitted.count(), 1);
		QVERIFY(dismissed.isEmpty());
		prompt.setBusy(false); // the answer failed: the card can be used again
		prompt.activateRow(1);
		QCOMPARE(submitted.count(), 2);
	}

	void severalQuestionsHaveATabEachAndAConfirmation()
	{
		QuestionPrompt prompt;
		prompt.setRequest(twoQuestions());

		QVERIFY(!prompt.isSimple());
		const QVariantList tabs = prompt.tabs();
		QCOMPARE(tabs.size(), 3);
		QCOMPARE(tabs.at(0).toMap().value(u"title"_s).toString(), u"Loop"_s);
		QVERIFY(tabs.at(0).toMap().value(u"current"_s).toBool());
		QVERIFY(tabs.at(2).toMap().value(u"confirm"_s).toBool());
		QCOMPARE(prompt.page(), 0);
		QVERIFY(!prompt.isConfirming());
	}

	void pickingOnASingleChoicePageMovesOn()
	{
		QuestionPrompt prompt;
		prompt.setRequest(twoQuestions());
		QSignalSpy submitted(&prompt, &QuestionPrompt::submitRequested);

		prompt.activateRow(1);

		QCOMPARE(prompt.page(), 1);
		QVERIFY(submitted.isEmpty());
		QCOMPARE(prompt.answers().first(), (QStringList{u"Iterator"_s}));
	}

	void defaultsAreTickedOnAMultipleChoice()
	{
		QuestionPrompt prompt;
		prompt.setRequest(twoQuestions());
		prompt.goToPage(1);

		QVERIFY(prompt.isMultiple());
		QVERIFY(row(prompt, 0).value(u"checked"_s).toBool());
		QVERIFY(row(prompt, 1).value(u"checked"_s).toBool());
		QVERIFY(!row(prompt, 2).value(u"checked"_s).toBool());
		QCOMPARE(prompt.rows().size(), 3); // no custom row: the backend does not take one
		QCOMPARE(prompt.answers().at(1), (QStringList{u"Tests"_s, u"Clippy"_s}));
	}

	void aMultipleChoiceTogglesAndKeepsThePage()
	{
		QuestionPrompt prompt;
		prompt.setRequest(twoQuestions());
		prompt.goToPage(1);

		prompt.activateRow(1);
		prompt.activateRow(2);

		QCOMPARE(prompt.page(), 1);
		QCOMPARE(prompt.answers().at(1), (QStringList{u"Tests"_s, u"Format"_s}));
	}

	void theConfirmationListsAnswersAndMarksTheMissingOnes()
	{
		QuestionPrompt prompt;
		prompt.setRequest(twoQuestions());
		prompt.goToPage(1);
		prompt.activateRow(0);
		prompt.activateRow(1); // nothing left ticked
		prompt.nextPage();

		QVERIFY(prompt.isConfirming());
		QCOMPARE(prompt.unansweredCount(), 1); // the first question has its default
		const QVariantList summary = prompt.summary();
		QCOMPARE(summary.size(), 2);
		QCOMPARE(summary.at(0).toMap().value(u"header"_s).toString(), u"Loop"_s);
		QCOMPARE(summary.at(0).toMap().value(u"answer"_s).toString(), u"Range"_s);
		QVERIFY(summary.at(0).toMap().value(u"answered"_s).toBool());
		QVERIFY(!summary.at(1).toMap().value(u"answered"_s).toBool());
		QCOMPARE(summary.at(1).toMap().value(u"answer"_s).toString(), u"Unanswered"_s);
		QVERIFY(!prompt.tabs().last().toMap().value(u"answered"_s).toBool());
	}

	void submittingSendsOneListPerQuestion()
	{
		QuestionPrompt prompt;
		prompt.setRequest(twoQuestions());
		QSignalSpy submitted(&prompt, &QuestionPrompt::submitRequested);
		prompt.activateRow(0);
		prompt.goToPage(2);

		prompt.submit();

		QCOMPARE(submitted.count(), 1);
		QCOMPARE(submitted.first().at(1).value<QList<QStringList>>(),
		         (QList<QStringList>{{u"Range"_s}, {u"Tests"_s, u"Clippy"_s}}));
	}

	void submittingBeforeTheConfirmationDoesNothing()
	{
		QuestionPrompt prompt;
		prompt.setRequest(twoQuestions());
		QSignalSpy submitted(&prompt, &QuestionPrompt::submitRequested);

		prompt.submit();

		QVERIFY(submitted.isEmpty());
	}

	void tabsGoRound()
	{
		QuestionPrompt prompt;
		prompt.setRequest(twoQuestions());

		prompt.previousPage();
		QCOMPARE(prompt.page(), 2);
		prompt.nextPage();
		QCOMPARE(prompt.page(), 0);
		QVERIFY(prompt.handleKey(Qt::Key_Tab, Qt::NoModifier));
		QCOMPARE(prompt.page(), 1);
		QVERIFY(prompt.handleKey(Qt::Key_Backtab, Qt::ShiftModifier));
		QCOMPARE(prompt.page(), 0);
		QVERIFY(prompt.handleKey(Qt::Key_Left, Qt::NoModifier));
		QCOMPARE(prompt.page(), 2);
	}

	void enterOnTheConfirmationSubmits()
	{
		QuestionPrompt prompt;
		prompt.setRequest(twoQuestions());
		QSignalSpy submitted(&prompt, &QuestionPrompt::submitRequested);
		prompt.goToPage(2);

		QVERIFY(prompt.handleKey(Qt::Key_Space, Qt::NoModifier)); // only Enter submits
		QVERIFY(submitted.isEmpty());
		QVERIFY(prompt.handleKey(Qt::Key_Return, Qt::NoModifier));

		QCOMPARE(submitted.count(), 1);
	}

	void aCustomAnswerIsTyped()
	{
		QuestionPrompt prompt;
		prompt.setRequest(oneQuestion());
		QSignalSpy submitted(&prompt, &QuestionPrompt::submitRequested);

		prompt.activateRow(2);
		QVERIFY(prompt.isEditing());
		QVERIFY(!prompt.handleKey(Qt::Key_Down, Qt::NoModifier)); // the text field has the keys
		prompt.commitCustom(u"  my file.rs \n"_s);

		QVERIFY(!prompt.isEditing());
		QCOMPARE(submitted.first().at(1).value<QList<QStringList>>(), (QList<QStringList>{{u"my file.rs"_s}}));
	}

	void anEmptyCustomAnswerIsNoAnswer()
	{
		QuestionPrompt prompt;
		prompt.setRequest(oneQuestion());
		QSignalSpy submitted(&prompt, &QuestionPrompt::submitRequested);

		prompt.activateRow(2);
		prompt.commitCustom(u"   "_s);

		QVERIFY(!prompt.isEditing());
		QVERIFY(submitted.isEmpty());
		QVERIFY(prompt.isActive());
	}

	void leavingTheTextFieldKeepsTheCard()
	{
		QuestionPrompt prompt;
		prompt.setRequest(oneQuestion());
		QSignalSpy dismissed(&prompt, &QuestionPrompt::dismissRequested);

		prompt.activateRow(2);
		prompt.cancelCustom();

		QVERIFY(!prompt.isEditing());
		QVERIFY(dismissed.isEmpty());
		QVERIFY(prompt.handleKey(Qt::Key_Up, Qt::NoModifier)); // the card has the keys again
	}

	void aCustomAnswerReplacesTheTickedOptionOfASingleChoice()
	{
		QuestionPrompt prompt;
		prompt.setRequest(twoQuestions());

		prompt.activateRow(3);
		prompt.commitCustom(u"mine"_s);

		QCOMPARE(prompt.page(), 1);
		QCOMPARE(prompt.answers().first(), (QStringList{u"mine"_s}));
	}

	void aCustomAnswerJoinsTheTicksOfAMultipleChoice()
	{
		QuestionRequest request = twoQuestions();
		request.questions[1].custom = true;
		QuestionPrompt prompt;
		prompt.setRequest(request);
		prompt.goToPage(1);

		prompt.activateRow(3);
		prompt.commitCustom(u"extra"_s);

		QCOMPARE(prompt.page(), 1);
		QCOMPARE(prompt.answers().at(1), (QStringList{u"Tests"_s, u"Clippy"_s, u"extra"_s}));
		prompt.activateRow(3); // the custom row toggles: the typed answer is taken back
		QVERIFY(!prompt.isEditing());
		QCOMPARE(prompt.answers().at(1), (QStringList{u"Tests"_s, u"Clippy"_s}));
	}

	void theSameRequestAgainKeepsTheUsersWork()
	{
		QuestionPrompt prompt;
		prompt.setRequest(twoQuestions());
		prompt.activateRow(2);
		QSignalSpy changed(&prompt, &QuestionPrompt::changed);

		prompt.setRequest(twoQuestions());

		QVERIFY(changed.isEmpty());
		QCOMPARE(prompt.page(), 1);
		QCOMPARE(prompt.answers().first(), (QStringList{u"Guard"_s}));
	}

	void aNewRequestStartsOver()
	{
		QuestionPrompt prompt;
		prompt.setRequest(twoQuestions());
		prompt.activateRow(2);

		prompt.setRequest(twoQuestions(u"call_2"_s));

		QCOMPARE(prompt.callId(), u"call_2"_s);
		QCOMPARE(prompt.page(), 0);
		QCOMPARE(prompt.answers().first(), (QStringList{u"Range"_s}));
		QVERIFY(!prompt.isBusy());
	}

	void noRequestClosesTheCard()
	{
		QuestionPrompt prompt;
		prompt.setRequest(oneQuestion());
		prompt.activateRow(2);

		prompt.setRequest(std::nullopt);

		QVERIFY(!prompt.isActive());
		QVERIFY(!prompt.isEditing());
		QVERIFY(prompt.rows().isEmpty());
	}

	void aQuestionWithoutCustomAnswersHasNoCustomRow()
	{
		QuestionPrompt prompt;
		prompt.setRequest(oneQuestion(false));

		QSignalSpy submitted(&prompt, &QuestionPrompt::submitRequested);

		QCOMPARE(prompt.rows().size(), 2);
		prompt.handleKey(Qt::Key_3, Qt::NoModifier);
		QVERIFY(!prompt.isEditing());
		QVERIFY(submitted.isEmpty());
	}
};

QTEST_GUILESS_MAIN(TestQuestionPrompt)
#include "tst_question_prompt.moc"
