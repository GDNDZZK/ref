#include "app/app_translator.h"
#include "workspace/settings.h"

#include <QtTest>

using namespace Qt::StringLiterals;
using refgui::workspace::Settings;

namespace
{

// The chip of a grep result, as GrepBody builds it: the matches in the sentence, the files as its argument.
QString grepChip(int matches, int files)
{
	return QCoreApplication::translate("GrepBody", "%n match(es) in %1", "", matches)
	    .arg(QCoreApplication::translate("GrepBody", "%n file(s)", "", files));
}

QString moreLines(int lines)
{
	return QCoreApplication::translate("LineView", "Show %n more line(s)", "", lines);
}

} // namespace

class TestAppTranslator : public QObject
{
	Q_OBJECT

private slots:
	void englishHasItsPluralForms()
	{
		AppTranslator translator;
		translator.setLanguage(Settings::English);

		QCOMPARE(moreLines(1), u"Show 1 more line"_s);
		QCOMPARE(moreLines(2), u"Show 2 more lines"_s);
		QCOMPARE(grepChip(1, 1), u"1 match in 1 file"_s);
		QCOMPARE(grepChip(2, 1), u"2 matches in 1 file"_s);
		QCOMPARE(grepChip(1, 3), u"1 match in 3 files"_s);
		QCOMPARE(QCoreApplication::translate("FindBody", "%n result(s)", "", 0), u"0 results"_s);
		QCOMPARE(QCoreApplication::translate("BashBody", "%n earlier line(s)", "", 1), u"1 earlier line"_s);
	}

	void aLanguageWithoutAFileShowsEnglishPlurals()
	{
		AppTranslator translator;
		translator.setLanguage(u"de_DE"_s);

		QCOMPARE(grepChip(2, 2), u"2 matches in 2 files"_s);
	}

	void theSystemLanguageDecides()
	{
		const QLocale before = QLocale();
		QLocale::setDefault(QLocale(u"fr_FR"_s));
		AppTranslator translator;
		translator.setLanguage(Settings::System);

		QCOMPARE(moreLines(1), u"Show 1 more line"_s);
		QLocale::setDefault(before);
	}

	void chineseKeepsItsWordOrder()
	{
		AppTranslator translator;
		translator.setLanguage(u"zh_CN"_s);

		QCOMPARE(grepChip(1, 1), u"1 个文件中有 1 处匹配"_s);
		QCOMPARE(grepChip(5, 2), u"2 个文件中有 5 处匹配"_s);
		QCOMPARE(moreLines(3), u"再显示 3 行"_s);
	}

	void changingTheLanguageTakesEffectAtOnce()
	{
		AppTranslator translator;
		translator.setLanguage(u"zh_CN"_s);
		translator.setLanguage(Settings::English);

		QCOMPARE(moreLines(2), u"Show 2 more lines"_s);
	}
};

QTEST_GUILESS_MAIN(TestAppTranslator)
#include "tst_app_translator.moc"
