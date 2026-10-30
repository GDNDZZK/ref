#include "app/app_translator.h"

#include "workspace/settings.h"

#include <QCoreApplication>
#include <QLocale>

using refgui::workspace::Settings;
using namespace Qt::StringLiterals;

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
	if(language == Settings::English)
	{
		return;
	}
	const QLocale locale = language == Settings::System ? QLocale() : QLocale(language);
	if(m_translator.load(locale, u"ref-gui"_s, u"_"_s, u":/i18n"_s))
	{
		QCoreApplication::installTranslator(&m_translator);
		m_installed = true;
	}
}
