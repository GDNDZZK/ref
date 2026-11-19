#pragma once

#include <QObject>
#include <QQuickTextDocument>
#include <QTextDocument>
#include <QtQmlIntegration>

// Disables QTextDocument's built-in undo/redo stack so ComposerModel owns all undo history.
class TextEditSupport : public QObject
{
	Q_OBJECT
	QML_ELEMENT
	QML_SINGLETON

public:
	Q_INVOKABLE void disableUndoRedo(QQuickTextDocument* document) const
	{
		if(document && document->textDocument())
		{
			document->textDocument()->setUndoRedoEnabled(false);
		}
	}
};
