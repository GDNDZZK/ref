#pragma once

#include <QObject>
#include <QString>
#include <QTranslator>

// Loads the interface translation for the language the user chose, and can change it while
// the program runs. Interface text is English in the sources; a language without a
// translation shows English.
class AppTranslator : public QObject
{
	Q_OBJECT

public:
	explicit AppTranslator(QObject* parent = nullptr);
	~AppTranslator() override;

	// `language` is a value of Settings::language: system, en or a locale name like zh_CN.
	void setLanguage(const QString& language);

private:
	QTranslator m_translator;
	bool m_installed = false;
};
