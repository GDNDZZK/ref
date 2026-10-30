#pragma once

#include <QString>

namespace refgui::workspace
{

// Text for a toast, as the terminal front end writes it: whitespace runs become one space,
// control characters go, and the text is cut to `maxChars` Unicode characters with "…".
inline constexpr int MaxToastChars = 200;

QString oneLine(const QString& text);
QString truncated(const QString& text, int maxChars);

inline QString toastText(const QString& text)
{
	return truncated(oneLine(text), MaxToastChars);
}

} // namespace refgui::workspace
