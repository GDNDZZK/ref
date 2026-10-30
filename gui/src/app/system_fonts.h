// Adapted from an early version of qutie by LightBlueCube
#pragma once

#include <QFont>
#include <QFontDatabase>
#include <QObject>
#include <QtQmlIntegration>

// Platform fonts QML cannot query by itself. A family name such as "monospace"
// is a fontconfig alias and does not resolve on Windows or macOS.
class SystemFonts : public QObject
{
	Q_OBJECT
	QML_ELEMENT
	QML_SINGLETON
	Q_PROPERTY(QFont fixed READ fixed CONSTANT)

public:
	QFont fixed() const { return QFontDatabase::systemFont(QFontDatabase::FixedFont); }
};
