#pragma once

#include "app/workspace.h"

#include <QtQmlIntegration>

// Makes the types of the workspace library known to QML.

struct SettingsType
{
	Q_GADGET
	QML_FOREIGN(refgui::workspace::Settings)
	QML_NAMED_ELEMENT(Settings)
	QML_UNCREATABLE("Owned by Workspace.")
};

struct BackendConfigType
{
	Q_GADGET
	QML_FOREIGN(refgui::workspace::BackendConfig)
	QML_NAMED_ELEMENT(BackendConfig)
	QML_UNCREATABLE("Owned by Workspace.")
};

// The one registry of the window, as the singleton `Commands`.
struct CommandsType
{
	Q_GADGET
	QML_FOREIGN(refgui::workspace::CommandRegistry)
	QML_NAMED_ELEMENT(Commands)
	QML_SINGLETON

public:
	static refgui::workspace::CommandRegistry* create(QQmlEngine*, QJSEngine*)
	{
		return Workspace::instance()->commands();
	}
};
