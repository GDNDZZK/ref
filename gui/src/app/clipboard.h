#pragma once

#include <QClipboard>
#include <QGuiApplication>
#include <QObject>
#include <QString>
#include <QtQmlIntegration>

// The system clipboard, for QML components that copy text.
class Clipboard : public QObject
{
	Q_OBJECT
	QML_ELEMENT
	QML_SINGLETON

public:
	Q_INVOKABLE void setText(const QString& text) const { QGuiApplication::clipboard()->setText(text); }
};
