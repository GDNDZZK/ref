#pragma once

#include "composer/composer_model.h"

#include <QtQmlIntegration>

// Makes the input box model of the composer library known to QML.
struct ComposerModelForeign
{
	Q_GADGET
	QML_FOREIGN(refgui::composer::ComposerModel)
	QML_NAMED_ELEMENT(ComposerModel)
	QML_UNCREATABLE("Created by ChatSession.")
};
