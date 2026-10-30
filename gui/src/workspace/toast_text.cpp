#include "workspace/toast_text.h"

#include <QChar>
#include <algorithm>
#include <string>

namespace refgui::workspace
{

QString oneLine(const QString& text)
{
	std::u32string kept;
	bool space = false;
	for(const char32_t c : text.toStdU32String())
	{
		if(QChar::isSpace(c))
		{
			space = true;
			continue;
		}
		if(QChar::category(c) == QChar::Other_Control)
		{
			continue;
		}
		if(space && !kept.empty())
		{
			kept.push_back(U' ');
		}
		space = false;
		kept.push_back(c);
	}
	return QString::fromStdU32String(kept);
}

QString truncated(const QString& text, int maxChars)
{
	const std::u32string chars = text.toStdU32String();
	if(chars.size() <= size_t(std::max(maxChars, 0)))
	{
		return text;
	}
	if(maxChars <= 0)
	{
		return {};
	}
	return QString::fromStdU32String(chars.substr(0, size_t(maxChars - 1))) + QChar(0x2026);
}

} // namespace refgui::workspace
