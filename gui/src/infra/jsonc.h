#pragma once

#include <QByteArrayView>
#include <QJsonDocument>
#include <QString>

#include <expected>

namespace refgui::jsonc
{

struct ParseError
{
	int line;   // 1-based
	int column; // 1-based, counted in UTF-16 code units
	QString message;
};

// Parses JSON extended with `//` and `/* */` comments and trailing commas.
// The top-level value must be an object or an array (QJsonDocument restriction).
std::expected<QJsonDocument, ParseError> parse(QByteArrayView source);

} // namespace refgui::jsonc
