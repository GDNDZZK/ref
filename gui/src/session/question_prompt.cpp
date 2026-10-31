#include "session/question_prompt.h"

#include <QVariantMap>

#include <algorithm>

using namespace Qt::StringLiterals;

namespace refgui::session
{

QuestionPrompt::QuestionPrompt(QObject* parent)
	: QObject(parent)
{
}

void QuestionPrompt::setRequest(const std::optional<proto::QuestionRequest>& request)
{
	if(!request || request->questions.isEmpty())
	{
		if(!m_active)
		{
			return;
		}
		m_request = {};
		m_state.clear();
		m_active = false;
		m_editing = false;
		m_busy = false;
		m_page = 0;
		emit changed();
		return;
	}
	if(m_active && request->callId == m_request.callId)
	{
		return;
	}

	m_request = *request;
	m_state.clear();
	for(const proto::Question& question : m_request.questions)
	{
		PageState state;
		for(const proto::QuestionOption& option : question.options)
		{
			state.checked.append(option.isDefault);
		}
		const auto firstDefault = std::ranges::find_if(question.options, [](const proto::QuestionOption& option) { return option.isDefault; });
		state.highlighted = firstDefault == question.options.end() ? 0 : static_cast<int>(firstDefault - question.options.begin());
		m_state.append(state);
	}
	m_active = true;
	m_editing = false;
	m_busy = false;
	m_page = 0;
	emit changed();
}

bool QuestionPrompt::isSimple() const
{
	return m_active && questionCount() == 1 && !m_request.questions.first().multiple;
}

QVariantList QuestionPrompt::tabs() const
{
	QVariantList list;
	if(!m_active || isSimple())
	{
		return list;
	}
	for(int i = 0; i < questionCount(); ++i)
	{
		list.append(QVariantMap{{u"title"_s, m_request.questions.at(i).header},
		                        {u"answered"_s, hasAnswer(i)},
		                        {u"current"_s, m_page == i},
		                        {u"confirm"_s, false}});
	}
	list.append(QVariantMap{{u"title"_s, tr("Confirm")}, {u"answered"_s, unansweredCount() == 0}, {u"current"_s, isConfirming()}, {u"confirm"_s, true}});
	return list;
}

QString QuestionPrompt::header() const
{
	if(!m_active)
	{
		return {};
	}
	return onQuestion() ? currentQuestion().header : tr("Confirm");
}

QString QuestionPrompt::text() const
{
	if(!m_active)
	{
		return {};
	}
	return onQuestion() ? currentQuestion().question : tr("Check your answers");
}

bool QuestionPrompt::isMultiple() const
{
	return onQuestion() && currentQuestion().multiple;
}

int QuestionPrompt::customRow() const
{
	return static_cast<int>(currentQuestion().options.size());
}

int QuestionPrompt::rowCount() const
{
	if(!onQuestion())
	{
		return 0;
	}
	return customRow() + (currentQuestion().custom ? 1 : 0);
}

QVariantList QuestionPrompt::rows() const
{
	QVariantList list;
	if(!onQuestion())
	{
		return list;
	}
	const proto::Question& question = currentQuestion();
	const PageState& state = m_state.at(m_page);
	for(int i = 0; i < question.options.size(); ++i)
	{
		const proto::QuestionOption& option = question.options.at(i);
		list.append(QVariantMap{{u"label"_s, option.label},
		                        {u"description"_s, option.description},
		                        {u"isDefault"_s, option.isDefault},
		                        {u"checked"_s, state.checked.at(i)},
		                        {u"highlighted"_s, state.highlighted == i},
		                        {u"number"_s, i + 1},
		                        {u"custom"_s, false}});
	}
	if(question.custom)
	{
		const int row = customRow();
		list.append(QVariantMap{{u"label"_s, state.customText.isEmpty() ? tr("Type your own answer") : state.customText},
		                        {u"description"_s, QString()},
		                        {u"isDefault"_s, false},
		                        {u"checked"_s, !state.customText.isEmpty()},
		                        {u"highlighted"_s, state.highlighted == row},
		                        {u"number"_s, row + 1},
		                        {u"custom"_s, true}});
	}
	return list;
}

QVariantList QuestionPrompt::summary() const
{
	QVariantList list;
	if(!isConfirming())
	{
		return list;
	}
	for(int i = 0; i < questionCount(); ++i)
	{
		const bool answered = hasAnswer(i);
		list.append(QVariantMap{{u"header"_s, m_request.questions.at(i).header},
		                        {u"answer"_s, answered ? answerOf(i).join(u", "_s) : tr("Unanswered")},
		                        {u"answered"_s, answered}});
	}
	return list;
}

QString QuestionPrompt::customText() const
{
	return onQuestion() ? m_state.at(m_page).customText : QString();
}

void QuestionPrompt::setBusy(bool busy)
{
	if(busy != m_busy)
	{
		m_busy = busy;
		emit changed();
	}
}

bool QuestionPrompt::hasAnswer(int question) const
{
	const PageState& state = m_state.at(question);
	return !state.customText.isEmpty() || std::ranges::any_of(state.checked, [](bool checked) { return checked; });
}

int QuestionPrompt::unansweredCount() const
{
	int count = 0;
	for(int i = 0; m_active && i < questionCount(); ++i)
	{
		count += hasAnswer(i) ? 0 : 1;
	}
	return count;
}

QStringList QuestionPrompt::answerOf(int question) const
{
	const PageState& state = m_state.at(question);
	QStringList answer;
	for(int i = 0; i < state.checked.size(); ++i)
	{
		if(state.checked.at(i))
		{
			answer.append(m_request.questions.at(question).options.at(i).label);
		}
	}
	if(!state.customText.isEmpty())
	{
		answer.append(state.customText);
	}
	return answer;
}

QList<QStringList> QuestionPrompt::answers() const
{
	QList<QStringList> list;
	for(int i = 0; m_active && i < questionCount(); ++i)
	{
		list.append(answerOf(i));
	}
	return list;
}

void QuestionPrompt::setHighlight(int row)
{
	if(!onQuestion() || m_busy || row < 0 || row >= rowCount() || m_state.at(m_page).highlighted == row)
	{
		return;
	}
	m_state[m_page].highlighted = row;
	emit changed();
}

void QuestionPrompt::moveHighlight(int delta)
{
	const int count = rowCount();
	if(count == 0 || m_busy)
	{
		return;
	}
	setHighlight(std::clamp(m_state.at(m_page).highlighted + delta, 0, count - 1));
}

void QuestionPrompt::activateRow(int row)
{
	if(!onQuestion() || m_busy || row < 0 || row >= rowCount())
	{
		return;
	}
	m_state[m_page].highlighted = row;
	if(isMultiple())
	{
		toggleRow(row);
	}
	else
	{
		pickRow(row);
	}
}

void QuestionPrompt::toggleRow(int row)
{
	PageState& state = m_state[m_page];
	if(row == customRow() && currentQuestion().custom)
	{
		if(state.customText.isEmpty())
		{
			startCustom();
			return;
		}
		state.customText.clear();
	}
	else
	{
		state.checked[row] = !state.checked.at(row);
	}
	emit changed();
}

void QuestionPrompt::pickRow(int row)
{
	if(row == customRow() && currentQuestion().custom)
	{
		startCustom();
		return;
	}
	PageState& state = m_state[m_page];
	std::ranges::fill(state.checked, false);
	state.checked[row] = true;
	state.customText.clear();
	if(isSimple())
	{
		submitNow({answerOf(m_page)});
		return;
	}
	emit changed();
	goToPage(m_page + 1);
}

void QuestionPrompt::startCustom()
{
	m_editing = true;
	emit changed();
}

void QuestionPrompt::commitCustom(const QString& text)
{
	if(!m_editing || !onQuestion())
	{
		return;
	}
	m_editing = false;
	const QString trimmed = text.trimmed();
	if(trimmed.isEmpty())
	{
		emit changed();
		return;
	}
	PageState& state = m_state[m_page];
	if(!isMultiple())
	{
		std::ranges::fill(state.checked, false);
	}
	state.customText = trimmed;
	if(isSimple())
	{
		submitNow({answerOf(m_page)});
		return;
	}
	emit changed();
	if(!isMultiple())
	{
		goToPage(m_page + 1);
	}
}

void QuestionPrompt::cancelCustom()
{
	if(m_editing)
	{
		m_editing = false;
		emit changed();
	}
}

void QuestionPrompt::goToPage(int page)
{
	if(!m_active || isSimple() || m_busy)
	{
		return;
	}
	const int target = std::clamp(page, 0, questionCount());
	if(target == m_page && !m_editing)
	{
		return;
	}
	m_page = target;
	m_editing = false;
	emit changed();
}

void QuestionPrompt::nextPage()
{
	if(m_active)
	{
		goToPage((m_page + 1) % (questionCount() + 1));
	}
}

void QuestionPrompt::previousPage()
{
	if(m_active)
	{
		goToPage((m_page + questionCount()) % (questionCount() + 1));
	}
}

void QuestionPrompt::submit()
{
	if(!isConfirming() || m_busy)
	{
		return;
	}
	submitNow(answers());
}

void QuestionPrompt::submitNow(const QList<QStringList>& answers)
{
	m_busy = true;
	m_editing = false;
	emit changed();
	emit submitRequested(m_request.callId, answers);
}

void QuestionPrompt::dismiss()
{
	if(!m_active || m_busy)
	{
		return;
	}
	m_busy = true;
	m_editing = false;
	emit changed();
	emit dismissRequested(m_request.callId);
}

bool QuestionPrompt::handleKey(int key, int modifiers)
{
	if(!m_active || m_editing)
	{
		return false;
	}
	const Qt::KeyboardModifiers held(modifiers);
	if(held.testAnyFlags(Qt::ControlModifier | Qt::AltModifier | Qt::MetaModifier))
	{
		return false; // shortcuts stay shortcuts
	}
	const int highlighted = onQuestion() ? m_state.at(m_page).highlighted : 0;
	switch(key)
	{
	case Qt::Key_Escape:
		dismiss();
		return true;
	case Qt::Key_K:
		if(held.testFlag(Qt::ShiftModifier))
		{
			return false;
		}
		[[fallthrough]];
	case Qt::Key_Up:
		moveHighlight(-1);
		return true;
	case Qt::Key_J:
		if(held.testFlag(Qt::ShiftModifier))
		{
			return false;
		}
		[[fallthrough]];
	case Qt::Key_Down:
		moveHighlight(1);
		return true;
	case Qt::Key_Left:
	case Qt::Key_Backtab:
		previousPage();
		return true;
	case Qt::Key_Right:
		nextPage();
		return true;
	case Qt::Key_Tab:
		held.testFlag(Qt::ShiftModifier) ? previousPage() : nextPage();
		return true;
	case Qt::Key_Return:
	case Qt::Key_Enter:
		isConfirming() ? submit() : activateRow(highlighted);
		return true;
	case Qt::Key_Space:
		activateRow(highlighted);
		return true;
	default:
		break;
	}
	if(key >= Qt::Key_1 && key <= Qt::Key_9 && !held.testFlag(Qt::ShiftModifier))
	{
		activateRow(key - Qt::Key_1);
		return true;
	}
	return false;
}

} // namespace refgui::session
