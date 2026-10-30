#include "workspace/settings.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QSaveFile>

using namespace Qt::StringLiterals;
using namespace std::chrono_literals;

namespace refgui::workspace
{
namespace
{

constexpr auto SaveDelay = 500ms;
// A window smaller than this in the file is a mistake, not a preference.
constexpr int SmallestWindowEdge = 200;

const QString ThemeKey = u"theme"_s;
const QString LanguageKey = u"language"_s;
const QString NotificationsKey = u"notifications"_s;
const QString EnabledKey = u"enabled"_s;
const QString PermissionKey = u"permission"_s;
const QString TurnDoneKey = u"turnDone"_s;
const QString TurnErrorKey = u"turnError"_s;
const QString SidebarKey = u"sidebar"_s;
const QString VisibleKey = u"visible"_s;
const QString WindowKey = u"window"_s;
const QString WidthKey = u"width"_s;
const QString HeightKey = u"height"_s;

const QString PermissionKind = u"permission"_s;
const QString TurnDoneKind = u"turn_done"_s;
const QString TurnErrorKind = u"turn_error"_s;

// Collects what was wrong in the file.
class Problems
{
public:
	void add(const QString& what) { m_list.append(what); }
	bool any() const { return !m_list.isEmpty(); }
	QString text() const { return m_list.join(u"; "_s); }

private:
	QStringList m_list;
};

// Reads `root[key]` as a string among `allowed`; leaves `value` alone when it is absent.
void readChoice(const QJsonObject& root, const QString& key, const QStringList& allowed, QString& value, Problems& problems)
{
	if(!root.contains(key))
	{
		return;
	}
	const QString read = root.value(key).toString();
	if(!allowed.contains(read))
	{
		problems.add(u"%1: unknown value"_s.arg(key));
		return;
	}
	value = read;
}

// `path` names the field in messages, e.g. "sidebar.visible".
void readFlag(const QJsonObject& root, const QString& key, const QString& path, bool& value, Problems& problems)
{
	if(!root.contains(key))
	{
		return;
	}
	if(!root.value(key).isBool())
	{
		problems.add(u"%1: expected true or false"_s.arg(path));
		return;
	}
	value = root.value(key).toBool();
}

void readEdge(const QJsonObject& root, const QString& key, const QString& path, int& value, Problems& problems)
{
	if(!root.contains(key))
	{
		return;
	}
	const int read = root.value(key).toInt(0);
	if(read < SmallestWindowEdge)
	{
		problems.add(u"%1: expected a size in pixels"_s.arg(path));
		return;
	}
	value = read;
}

// The object under `key`, or an empty one.
QJsonObject section(const QJsonObject& root, const QString& key)
{
	return root.value(key).toObject();
}

} // namespace

const QString Settings::System = u"system"_s;
const QString Settings::Light = u"light"_s;
const QString Settings::Dark = u"dark"_s;
const QString Settings::English = u"en"_s;
const QString Settings::ChineseSimplified = u"zh_CN"_s;

Settings::Settings(QString path, QObject* parent)
	: QObject(parent)
	, m_path(std::move(path))
	, m_theme(System)
	, m_language(System)
{
	m_saveTimer.setSingleShot(true);
	m_saveTimer.setInterval(SaveDelay);
	connect(&m_saveTimer, &QTimer::timeout, this, &Settings::save);
	load();
}

Settings::~Settings()
{
	save();
}

QStringList Settings::notifyKinds() const
{
	QStringList kinds;
	if(!m_notifyEnabled)
	{
		return kinds;
	}
	if(m_notifyPermission)
	{
		kinds.append(PermissionKind);
	}
	if(m_notifyTurnDone)
	{
		kinds.append(TurnDoneKind);
	}
	if(m_notifyTurnError)
	{
		kinds.append(TurnErrorKind);
	}
	return kinds;
}

template<typename T>
bool Settings::assign(T& field, const T& value)
{
	if(field == value)
	{
		return false;
	}
	field = value;
	scheduleSave();
	return true;
}

void Settings::setTheme(const QString& theme)
{
	Q_ASSERT(theme == System || theme == Light || theme == Dark);
	if(assign(m_theme, theme))
	{
		emit themeChanged();
	}
}

void Settings::setLanguage(const QString& language)
{
	Q_ASSERT(language == System || language == English || language == ChineseSimplified);
	if(assign(m_language, language))
	{
		emit languageChanged();
	}
}

void Settings::setNotifyEnabled(bool enabled)
{
	if(assign(m_notifyEnabled, enabled))
	{
		emit notifyKindsChanged();
	}
}

void Settings::setNotifyPermission(bool enabled)
{
	if(assign(m_notifyPermission, enabled))
	{
		emit notifyKindsChanged();
	}
}

void Settings::setNotifyTurnDone(bool enabled)
{
	if(assign(m_notifyTurnDone, enabled))
	{
		emit notifyKindsChanged();
	}
}

void Settings::setNotifyTurnError(bool enabled)
{
	if(assign(m_notifyTurnError, enabled))
	{
		emit notifyKindsChanged();
	}
}

void Settings::setSidebarVisible(bool visible)
{
	if(assign(m_sidebarVisible, visible))
	{
		emit sidebarVisibleChanged();
	}
}

void Settings::setWindowWidth(int width)
{
	if(assign(m_windowWidth, width))
	{
		emit windowSizeChanged();
	}
}

void Settings::setWindowHeight(int height)
{
	if(assign(m_windowHeight, height))
	{
		emit windowSizeChanged();
	}
}

void Settings::scheduleSave()
{
	m_dirty = true;
	m_saveTimer.start();
}

void Settings::load()
{
	QFile file(m_path);
	if(!file.exists())
	{
		return;
	}
	if(!file.open(QIODevice::ReadOnly))
	{
		m_loadProblem = tr("Could not read %1: %2").arg(m_path, file.errorString());
		return;
	}

	QJsonParseError error;
	const QJsonDocument document = QJsonDocument::fromJson(file.readAll(), &error);
	file.close();
	if(!document.isObject())
	{
		// Keep the broken file for the user and start over with the defaults.
		const QString kept = m_path + u".bad"_s;
		QFile::remove(kept);
		const bool renamed = QFile::rename(m_path, kept);
		const QString reason = error.error != QJsonParseError::NoError ? error.errorString() : tr("not a JSON object");
		m_loadProblem = renamed ? tr("Could not read %1 (%2). It was kept as %3 and the defaults are used.").arg(m_path, reason, kept)
		                        : tr("Could not read %1 (%2). The defaults are used.").arg(m_path, reason);
		return;
	}
	m_unknown = document.object();
	readFields(m_unknown);
}

void Settings::readFields(const QJsonObject& root)
{
	Problems problems;
	readChoice(root, ThemeKey, {System, Light, Dark}, m_theme, problems);
	readChoice(root, LanguageKey, {System, English, ChineseSimplified}, m_language, problems);

	const QJsonObject notifications = section(root, NotificationsKey);
	readFlag(notifications, EnabledKey, u"notifications.enabled"_s, m_notifyEnabled, problems);
	readFlag(notifications, PermissionKey, u"notifications.permission"_s, m_notifyPermission, problems);
	readFlag(notifications, TurnDoneKey, u"notifications.turnDone"_s, m_notifyTurnDone, problems);
	readFlag(notifications, TurnErrorKey, u"notifications.turnError"_s, m_notifyTurnError, problems);

	readFlag(section(root, SidebarKey), VisibleKey, u"sidebar.visible"_s, m_sidebarVisible, problems);

	const QJsonObject window = section(root, WindowKey);
	readEdge(window, WidthKey, u"window.width"_s, m_windowWidth, problems);
	readEdge(window, HeightKey, u"window.height"_s, m_windowHeight, problems);

	if(problems.any())
	{
		m_loadProblem = tr("Some settings in %1 were ignored: %2").arg(m_path, problems.text());
	}
}

QJsonObject Settings::toJson() const
{
	QJsonObject root = m_unknown;
	root.insert(ThemeKey, m_theme);
	root.insert(LanguageKey, m_language);

	QJsonObject notifications = section(root, NotificationsKey);
	notifications.insert(EnabledKey, m_notifyEnabled);
	notifications.insert(PermissionKey, m_notifyPermission);
	notifications.insert(TurnDoneKey, m_notifyTurnDone);
	notifications.insert(TurnErrorKey, m_notifyTurnError);
	root.insert(NotificationsKey, notifications);

	QJsonObject sidebar = section(root, SidebarKey);
	sidebar.insert(VisibleKey, m_sidebarVisible);
	root.insert(SidebarKey, sidebar);

	QJsonObject window = section(root, WindowKey);
	window.insert(WidthKey, m_windowWidth);
	window.insert(HeightKey, m_windowHeight);
	root.insert(WindowKey, window);
	return root;
}

void Settings::save()
{
	m_saveTimer.stop();
	if(!m_dirty)
	{
		return;
	}
	m_dirty = false;

	const QString directory = QFileInfo(m_path).absolutePath();
	if(!QDir().mkpath(directory))
	{
		emit saveFailed(tr("Could not create %1.").arg(directory));
		return;
	}
	QSaveFile file(m_path);
	if(!file.open(QIODevice::WriteOnly))
	{
		emit saveFailed(tr("Could not save %1: %2").arg(m_path, file.errorString()));
		return;
	}
	file.write(QJsonDocument(toJson()).toJson(QJsonDocument::Indented));
	if(!file.commit())
	{
		emit saveFailed(tr("Could not save %1: %2").arg(m_path, file.errorString()));
	}
}

} // namespace refgui::workspace
