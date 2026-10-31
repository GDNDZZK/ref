#pragma once

#include "session/question_prompt.h"

#include <QtQmlIntegration>

// Makes the card model for the model's questions known to QML.
struct QuestionPromptForeign
{
	Q_GADGET
	QML_FOREIGN(refgui::session::QuestionPrompt)
	QML_NAMED_ELEMENT(QuestionPrompt)
	QML_UNCREATABLE("Created by ChatSession.")
};
