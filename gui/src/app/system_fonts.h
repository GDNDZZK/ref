// Adapted from an early version of LightBlueCube/qutie
#pragma once

#include <QFont>
#include <QFontDatabase>
#include <QFontInfo>
#include <QObject>
#include <QStringList>
#include <QtQmlIntegration>

#include <algorithm>

// Platform fonts QML cannot query by itself. A family name such as "monospace"
// is a fontconfig alias and does not resolve on Windows or macOS.
class SystemFonts : public QObject
{
	Q_OBJECT
	QML_ELEMENT
	QML_SINGLETON
	Q_PROPERTY(QFont fixed READ fixed CONSTANT)

public:
	static QFont resolveFixedFont(const QFont& systemFixed, const QStringList& availableFamilies, const QString& matchedFamily = {})
	{
		// On Linux fontconfig often resolves "monospace" to Noto Sans Mono CJK (an OpenType CFF
		// .ttc collection), whose Bold face corrupts Qt 6 Quick SDF glyph atlases. Pick a
		// TrueType monospace family instead; Qt still falls back to Noto CJK for CJK glyphs.
		const auto isNotoCjk = [](const QString& family) {
			return family.startsWith(QStringLiteral("Noto Sans Mono CJK"), Qt::CaseInsensitive);
		};
		if(!isNotoCjk(systemFixed.family()) && !isNotoCjk(matchedFamily))
		{
			return systemFixed;
		}
		static const QStringList preferredFamilies = {
			QStringLiteral("DejaVu Sans Mono"),
			QStringLiteral("Liberation Mono"),
			QStringLiteral("Ubuntu Mono"),
			QStringLiteral("Fira Code"),
			QStringLiteral("JetBrains Mono"),
			QStringLiteral("Noto Mono"),
		};
		for(const QString& candidate : preferredFamilies)
		{
			const auto it = std::ranges::find_if(availableFamilies, [&](const QString& family) {
				return family.compare(candidate, Qt::CaseInsensitive) == 0;
			});
			if(it != availableFamilies.end())
			{
				QFont resolved = systemFixed;
				resolved.setFamily(*it);
				return resolved;
			}
		}
		return systemFixed;
	}

	QFont fixed() const
	{
		const QFont systemFixed = QFontDatabase::systemFont(QFontDatabase::FixedFont);
#if !defined(Q_OS_WIN) && !defined(Q_OS_MACOS)
		return resolveFixedFont(systemFixed, QFontDatabase::families(), QFontInfo(systemFixed).family());
#else
		return systemFixed;
#endif
	}
};
