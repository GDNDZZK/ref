#include "app/system_fonts.h"

#include <QFont>
#include <QtTest>

using namespace Qt::StringLiterals;

class TestSystemFonts : public QObject
{
	Q_OBJECT

private slots:
	void replacesNotoMonoCjkWithFirstPreferredTrueTypeFamily();
	void replacesFontconfigAliasMatchedToNotoMonoCjk();
	void preservesFontMetricsAndStyleHint();
	void leavesNonNotoCjkFontsUntouched();
	void fallsBackToOriginalWhenNoPreferredFamilyInstalled();
};

void TestSystemFonts::replacesNotoMonoCjkWithFirstPreferredTrueTypeFamily()
{
	QFont base(u"Noto Sans Mono CJK SC"_s);
	const QStringList allInstalled{u"Noto Sans"_s, u"Noto Mono"_s, u"Liberation Mono"_s, u"DejaVu Sans Mono"_s};
	QCOMPARE(SystemFonts::resolveFixedFont(base, allInstalled).family(), u"DejaVu Sans Mono"_s);

	const QStringList onlyJetBrains{u"noto sans mono cjk sc"_s, u"JetBrains Mono"_s, u"Noto Mono"_s};
	QFont lowerCaseBase(u"noto sans mono cjk jp"_s);
	QCOMPARE(SystemFonts::resolveFixedFont(lowerCaseBase, onlyJetBrains).family(), u"JetBrains Mono"_s);
}

void TestSystemFonts::replacesFontconfigAliasMatchedToNotoMonoCjk()
{
	const QFont alias(u"monospace"_s, 11);
	const QStringList installed{u"Noto Sans Mono CJK SC"_s, u"Liberation Mono"_s};
	const QFont resolved = SystemFonts::resolveFixedFont(alias, installed, u"Noto Sans Mono CJK SC"_s);
	QCOMPARE(resolved.family(), u"Liberation Mono"_s);
	QCOMPARE(resolved.pointSize(), 11);

	const QFont safeAlias = SystemFonts::resolveFixedFont(alias, installed, u"Liberation Mono"_s);
	QCOMPARE(safeAlias.family(), u"monospace"_s);
}

void TestSystemFonts::preservesFontMetricsAndStyleHint()
{
	QFont pointSized(u"Noto Sans Mono CJK TC"_s, 13);
	pointSized.setStyleHint(QFont::Monospace);
	const QFont resolvedPoint = SystemFonts::resolveFixedFont(pointSized, {u"Ubuntu Mono"_s});
	QCOMPARE(resolvedPoint.family(), u"Ubuntu Mono"_s);
	QCOMPARE(resolvedPoint.pointSize(), 13);
	QCOMPARE(resolvedPoint.styleHint(), QFont::Monospace);

	QFont pixelSized(u"Noto Sans Mono CJK KR"_s);
	pixelSized.setPixelSize(16);
	pixelSized.setStyleHint(QFont::TypeWriter);
	const QFont resolvedPixel = SystemFonts::resolveFixedFont(pixelSized, {u"Fira Code"_s});
	QCOMPARE(resolvedPixel.family(), u"Fira Code"_s);
	QCOMPARE(resolvedPixel.pixelSize(), 16);
	QCOMPARE(resolvedPixel.styleHint(), QFont::TypeWriter);
}

void TestSystemFonts::leavesNonNotoCjkFontsUntouched()
{
	const QFont custom(u"Iosevka"_s, 11);
	const QFont resolved = SystemFonts::resolveFixedFont(custom, {u"DejaVu Sans Mono"_s, u"Liberation Mono"_s});
	QCOMPARE(resolved.family(), u"Iosevka"_s);
	QCOMPARE(resolved.pointSize(), 11);
}

void TestSystemFonts::fallsBackToOriginalWhenNoPreferredFamilyInstalled()
{
	const QFont base(u"Noto Sans Mono CJK SC"_s, 12);
	const QFont resolved = SystemFonts::resolveFixedFont(base, {u"Noto Sans Mono CJK SC"_s, u"Some Random Sans"_s});
	QCOMPARE(resolved.family(), u"Noto Sans Mono CJK SC"_s);
	QCOMPARE(resolved.pointSize(), 12);
}

QTEST_MAIN(TestSystemFonts)
#include "tst_system_fonts.moc"
