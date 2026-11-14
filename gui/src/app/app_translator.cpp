#include "app/app_translator.h"

#include "workspace/settings.h"

#include <QCoreApplication>
#include <QLocale>
#include <QLoggingCategory>

using refgui::workspace::Settings;
using namespace Qt::StringLiterals;

Q_LOGGING_CATEGORY(lcTranslator, "refgui.app.translator")

AppTranslator::AppTranslator(QObject* parent)
	: QObject(parent)
{
}

AppTranslator::~AppTranslator()
{
	if(m_installed)
	{
		QCoreApplication::removeTranslator(&m_translator);
	}
}

void AppTranslator::setLanguage(const QString& language)
{
	if(m_installed)
	{
		QCoreApplication::removeTranslator(&m_translator);
		m_installed = false;
	}
	if(language != Settings::English)
	{
		const QLocale locale = language == Settings::System ? QLocale() : QLocale(language);
		if(install(locale))
		{
			return;
		}
	}
	// English, or a language without a file: the English text only needs its plural forms.
	if(!install(QLocale(QLocale::English)))
	{
		qCWarning(lcTranslator) << "cannot load the English translation; plural forms stay unresolved";
	}
}

bool AppTranslator::install(const QLocale& locale)
{
	if(!m_translator.load(locale, u"ref-gui"_s, u"_"_s, u":/i18n"_s))
	{
		return false;
	}
	QCoreApplication::installTranslator(&m_translator);
	m_installed = true;
	return true;
}
