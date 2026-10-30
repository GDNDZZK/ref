#pragma once

#include "session/tool_content.h"

#include <QObject>
#include <QtQmlIntegration>

// What a tool call's name, input and output say about it, for the tool cards (see session/tool_content.h).
class ToolInfo : public QObject
{
	Q_OBJECT
	QML_ELEMENT
	QML_SINGLETON

public:
	Q_INVOKABLE QString icon(const QString& toolName) const { return refgui::session::tool_content::iconName(toolName); }
	Q_INVOKABLE QVariantMap parse(const QString& inputJson) const { return refgui::session::tool_content::parseInput(inputJson); }
	Q_INVOKABLE QVariantList todos(const QVariantMap& input) const { return refgui::session::tool_content::todos(input); }
	Q_INVOKABLE QString detail(const QString& toolName, const QVariantMap& input) const
	{
		return refgui::session::tool_content::detail(toolName, input);
	}
	Q_INVOKABLE QVariantList listing(const QString& output) const { return refgui::session::tool_content::listing(output); }
	Q_INVOKABLE QVariantList diffRows(const QString& diff) const { return refgui::session::tool_content::diffRows(diff); }
	Q_INVOKABLE QVariantMap splitPath(const QString& path) const { return refgui::session::tool_content::splitPath(path); }
	Q_INVOKABLE QString sizeText(qint64 bytes) const { return refgui::session::tool_content::sizeText(bytes); }
	Q_INVOKABLE QString firstLine(const QString& text) const { return refgui::session::tool_content::firstLine(text); }
	Q_INVOKABLE QString firstText(const QVariantMap& input, const QStringList& keys) const
	{
		return refgui::session::tool_content::firstText(input, keys);
	}
};
