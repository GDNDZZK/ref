#pragma once

#include "notify/notification_center.h"

#include <QtQml/qqmlengine.h>
#include <QtQmlIntegration>

// Makes the notification center known to QML as the singleton `NotificationCenter`. The program
// creates the one instance and hands it over with setInstance().
struct NotificationCenterForeign
{
	Q_GADGET
	QML_FOREIGN(refgui::notify::NotificationCenter)
	QML_NAMED_ELEMENT(NotificationCenter)
	QML_SINGLETON

public:
	static refgui::notify::NotificationCenter* create(QQmlEngine*, QJSEngine*)
	{
		QJSEngine::setObjectOwnership(instance, QJSEngine::CppOwnership);
		return instance;
	}

	static inline refgui::notify::NotificationCenter* instance = nullptr;
};
