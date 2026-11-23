#include "workspace/settings.h"

#include "proto/notify_kind.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QSaveFile>
#include <QtMath>
#include <algorithm>
#include <QJsonArray>
#include <QSet>
#include <cmath>
#include <limits>
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
const QString TodoExpandedKey = u"todoExpanded"_s;
const QString RunningExpandedKey = u"runningExpanded"_s;
const QString HistoryExpandedKey = u"historyExpanded"_s;
const QString RunningOrderKey = u"runningOrder"_s;
const QString TodoHeightKey = u"todoHeight"_s;
const QString RunningHeightKey = u"runningHeight"_s;
const QString HistoryCreatorFilterKey = u"historyCreatorFilter"_s;
const QString HistoryCurrentDirKey = u"historyCurrentDir"_s;
const QString WindowKey = u"window"_s;
const QString WidthKey = u"width"_s;
const QString HeightKey = u"height"_s;
const QString UiScaleKey = u"uiScale"_s;

using NotifyKind = proto::NotifyFrame::Kind;

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

// The object under `key`, or an empty one.
QJsonObject section(const QJsonObject& root, const QString& key)
{
	return root.value(key).toObject();
}

// How messages name `key` of the section `sectionKey`, e.g. "sidebar.visible".
QString fieldPath(const QString& sectionKey, const QString& key)
{
	return sectionKey + u'.' + key;
}

void readSidebarChoice(const QJsonObject& root, const QString& sectionKey, const QString& key, const QStringList& allowed, QString& value, Problems& problems)
{
	const QJsonObject fields = section(root, sectionKey);
	if(!fields.contains(key))
	{
		return;
	}
	const QJsonValue raw = fields.value(key);
	const QString read = raw.isString() ? raw.toString() : QString();
	if(!raw.isString() || !allowed.contains(read))
	{
		problems.add(u"%1: unknown value"_s.arg(fieldPath(sectionKey, key)));
		return;
	}
	value = read;
}
// Reads `root[sectionKey][key]` as a flag; leaves `value` alone when it is absent.
void readFlag(const QJsonObject& root, const QString& sectionKey, const QString& key, bool& value, Problems& problems)
{
	const QJsonObject fields = section(root, sectionKey);
	if(!fields.contains(key))
	{
		return;
	}
	if(!fields.value(key).isBool())
	{
		problems.add(u"%1: expected true or false"_s.arg(fieldPath(sectionKey, key)));
		return;
	}
	value = fields.value(key).toBool();
}

// Reads `root[sectionKey][key]` as a window edge in pixels; leaves `value` alone when it is absent.
void readEdge(const QJsonObject& root, const QString& sectionKey, const QString& key, int& value, Problems& problems)
{
	const QJsonObject fields = section(root, sectionKey);
	if(!fields.contains(key))
	{
		return;
	}
	const int read = fields.value(key).toInt(0);
	if(read < SmallestWindowEdge)
	{
		problems.add(u"%1: expected a size in pixels"_s.arg(fieldPath(sectionKey, key)));
		return;
	}
	value = read;
}

void readSidebarWidth(const QJsonObject& root, const QString& sectionKey, const QString& key, int& value, Problems& problems)
{
	const QJsonObject fields = section(root, sectionKey);
	if(!fields.contains(key))
	{
		return;
	}
	if(!fields.value(key).isDouble())
	{
		problems.add(u"%1: expected a size in pixels"_s.arg(fieldPath(sectionKey, key)));
		return;
	}
	const int read = qRound(fields.value(key).toDouble());
	if(read <= 0)
	{
		problems.add(u"%1: expected a size in pixels"_s.arg(fieldPath(sectionKey, key)));
		return;
	}
	value = std::clamp(read, Settings::MinSidebarWidth, Settings::MaxSidebarWidth);
}

void readSidebarHeight(const QJsonObject& root, const QString& sectionKey, const QString& key, int& value, Problems& problems)
{
	const QJsonObject fields = section(root, sectionKey);
	if(!fields.contains(key))
	{
		return;
	}
	const QJsonValue raw = fields.value(key);
	if(!raw.isDouble())
	{
		problems.add(u"%1: expected a non-negative number in pixels"_s.arg(fieldPath(sectionKey, key)));
		return;
	}
	const double read = raw.toDouble();
	if(!std::isfinite(read) || read < 0.0 || read > static_cast<double>(std::numeric_limits<int>::max()))
	{
		problems.add(u"%1: expected a non-negative number in pixels"_s.arg(fieldPath(sectionKey, key)));
		return;
	}
	value = qRound(read);
}

QStringList deduplicateIds(const QStringList& ids)
{
	QStringList cleaned;
	QSet<QString> seen;
	for(const QString& id : ids)
	{
		if(!id.isEmpty() && !seen.contains(id))
		{
			seen.insert(id);
			cleaned.append(id);
		}
	}
	return cleaned;
}

void readRunningOrder(const QJsonObject& root, const QString& sectionKey, const QString& key, QStringList& value, Problems& problems)
{
	const QJsonObject fields = section(root, sectionKey);
	if(!fields.contains(key))
	{
		return;
	}
	const QJsonValue raw = fields.value(key);
	if(!raw.isArray())
	{
		problems.add(u"%1: expected an array of session ids"_s.arg(fieldPath(sectionKey, key)));
		return;
	}
	QStringList ids;
	const QJsonArray arr = raw.toArray();
	ids.reserve(arr.size());
	for(const QJsonValue& item : arr)
	{
		if(!item.isString() || item.toString().isEmpty())
		{
			problems.add(u"%1: expected session ids"_s.arg(fieldPath(sectionKey, key)));
			return;
		}
		ids.append(item.toString());
	}
	value = deduplicateIds(ids);
}

double normalizeUiScale(double scale)
{
	const double rounded = static_cast<double>(qRound(scale * 100.0)) / 100.0;
	return std::clamp(rounded, Settings::MinUiScale, Settings::MaxUiScale);
}

void readUiScale(const QJsonObject& root, const QString& key, double& value, Problems& problems)
{
	if(!root.contains(key))
	{
		return;
	}
	const QJsonValue raw = root.value(key);
	if(!raw.isDouble())
	{
		problems.add(u"%1: expected a positive number"_s.arg(key));
		return;
	}
	const double read = raw.toDouble();
	if(!std::isfinite(read) || read <= 0.0)
	{
		problems.add(u"%1: expected a positive number"_s.arg(key));
		return;
	}
	value = normalizeUiScale(read);
}
} // namespace

const QString Settings::System = u"system"_s;
const QString Settings::Light = u"light"_s;
const QString Settings::Dark = u"dark"_s;
const QString Settings::English = u"en"_s;
const QString Settings::ChineseSimplified = u"zh_CN"_s;
const QString Settings::HistoryCreatorAll = u"all"_s;
const QString Settings::HistoryCreatorUser = u"user"_s;
double Settings::readUiScaleOnly(const QString& path)
{
	QFile file(path);
	if(!file.exists() || !file.open(QIODevice::ReadOnly))
	{
		return DefaultUiScale;
	}
	QJsonParseError error;
	const QJsonDocument document = QJsonDocument::fromJson(file.readAll(), &error);
	if(error.error != QJsonParseError::NoError || !document.isObject())
	{
		return DefaultUiScale;
	}
	const QJsonValue raw = document.object().value(UiScaleKey);
	if(!raw.isDouble())
	{
		return DefaultUiScale;
	}
	const double read = raw.toDouble();
	if(!std::isfinite(read) || read <= 0.0)
	{
		return DefaultUiScale;
	}
	return normalizeUiScale(read);
}

Settings::Settings(QString path, QObject* parent)
	: QObject(parent)
	, m_path(std::move(path))
	, m_theme(System)
	, m_language(System)
	, m_sidebarHistoryCreatorFilter(HistoryCreatorAll)
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
		kinds.append(proto::notifyKindName(NotifyKind::Permission));
		kinds.append(proto::notifyKindName(NotifyKind::Question)); // the user is needed in both cases, so one switch
	}
	if(m_notifyTurnDone)
	{
		kinds.append(proto::notifyKindName(NotifyKind::TurnDone));
	}
	if(m_notifyTurnError)
	{
		kinds.append(proto::notifyKindName(NotifyKind::TurnError));
	}
	return kinds;
}

template<typename T>
void Settings::set(T& field, const T& value, void (Settings::*changed)())
{
	if(field == value)
	{
		return;
	}
	field = value;
	scheduleSave();
	emit (this->*changed)();
}

void Settings::setTheme(const QString& theme)
{
	Q_ASSERT(theme == System || theme == Light || theme == Dark);
	set(m_theme, theme, &Settings::themeChanged);
}

void Settings::setLanguage(const QString& language)
{
	Q_ASSERT(language == System || language == English || language == ChineseSimplified);
	set(m_language, language, &Settings::languageChanged);
}

void Settings::setNotifyEnabled(bool enabled)
{
	set(m_notifyEnabled, enabled, &Settings::notifyKindsChanged);
}

void Settings::setNotifyPermission(bool enabled)
{
	set(m_notifyPermission, enabled, &Settings::notifyKindsChanged);
}

void Settings::setNotifyTurnDone(bool enabled)
{
	set(m_notifyTurnDone, enabled, &Settings::notifyKindsChanged);
}

void Settings::setNotifyTurnError(bool enabled)
{
	set(m_notifyTurnError, enabled, &Settings::notifyKindsChanged);
}

void Settings::setSidebarVisible(bool visible)
{
	set(m_sidebarVisible, visible, &Settings::sidebarVisibleChanged);
}

void Settings::setSidebarTodoExpanded(bool expanded)
{
	set(m_sidebarTodoExpanded, expanded, &Settings::sidebarTodoExpandedChanged);
}

void Settings::setSidebarRunningExpanded(bool expanded)
{
	set(m_sidebarRunningExpanded, expanded, &Settings::sidebarRunningExpandedChanged);
}

void Settings::setSidebarHistoryExpanded(bool expanded)
{
	set(m_sidebarHistoryExpanded, expanded, &Settings::sidebarHistoryExpandedChanged);
}

void Settings::setSidebarWidth(int width)
{
	set(m_sidebarWidth, std::clamp(width, MinSidebarWidth, MaxSidebarWidth), &Settings::sidebarWidthChanged);
}

void Settings::setSidebarTodoHeight(int height)
{
	set(m_sidebarTodoHeight, std::max(0, height), &Settings::sidebarTodoHeightChanged);
}

void Settings::setSidebarRunningHeight(int height)
{
	set(m_sidebarRunningHeight, std::max(0, height), &Settings::sidebarRunningHeightChanged);
}

void Settings::setSidebarRunningOrder(const QStringList& order)
{
	set(m_sidebarRunningOrder, deduplicateIds(order), &Settings::sidebarRunningOrderChanged);
}

void Settings::setSidebarHistoryCreatorFilter(const QString& filter)
{
	Q_ASSERT(filter == HistoryCreatorAll || filter == HistoryCreatorUser);
	set(m_sidebarHistoryCreatorFilter, filter, &Settings::sidebarHistoryCreatorFilterChanged);
}

void Settings::setSidebarHistoryCurrentDir(bool currentDir)
{
	set(m_sidebarHistoryCurrentDir, currentDir, &Settings::sidebarHistoryCurrentDirChanged);
}
void Settings::setWindowWidth(int width)
{
	set(m_windowWidth, width, &Settings::windowSizeChanged);
}

void Settings::setWindowHeight(int height)
{
	set(m_windowHeight, height, &Settings::windowSizeChanged);
}

void Settings::setUiScale(double scale)
{
	if(!std::isfinite(scale) || scale <= 0.0)
	{
		return;
	}
	const double normalized = normalizeUiScale(scale);
	if(qAbs(m_uiScale - normalized) < 1e-9)
	{
		return;
	}
	m_uiScale = normalized;
	scheduleSave();
	emit uiScaleChanged();
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

	readFlag(root, NotificationsKey, EnabledKey, m_notifyEnabled, problems);
	readFlag(root, NotificationsKey, PermissionKey, m_notifyPermission, problems);
	readFlag(root, NotificationsKey, TurnDoneKey, m_notifyTurnDone, problems);
	readFlag(root, NotificationsKey, TurnErrorKey, m_notifyTurnError, problems);

	readFlag(root, SidebarKey, VisibleKey, m_sidebarVisible, problems);
	readFlag(root, SidebarKey, TodoExpandedKey, m_sidebarTodoExpanded, problems);
	readFlag(root, SidebarKey, RunningExpandedKey, m_sidebarRunningExpanded, problems);
	readFlag(root, SidebarKey, HistoryExpandedKey, m_sidebarHistoryExpanded, problems);
	readSidebarWidth(root, SidebarKey, WidthKey, m_sidebarWidth, problems);
	readSidebarHeight(root, SidebarKey, TodoHeightKey, m_sidebarTodoHeight, problems);
	readSidebarHeight(root, SidebarKey, RunningHeightKey, m_sidebarRunningHeight, problems);
	readRunningOrder(root, SidebarKey, RunningOrderKey, m_sidebarRunningOrder, problems);
	readSidebarChoice(root, SidebarKey, HistoryCreatorFilterKey, {HistoryCreatorAll, HistoryCreatorUser}, m_sidebarHistoryCreatorFilter, problems);
	readFlag(root, SidebarKey, HistoryCurrentDirKey, m_sidebarHistoryCurrentDir, problems);
	readEdge(root, WindowKey, WidthKey, m_windowWidth, problems);
	readEdge(root, WindowKey, HeightKey, m_windowHeight, problems);
	readUiScale(root, UiScaleKey, m_uiScale, problems);
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
	sidebar.insert(TodoExpandedKey, m_sidebarTodoExpanded);
	sidebar.insert(RunningExpandedKey, m_sidebarRunningExpanded);
	sidebar.insert(HistoryExpandedKey, m_sidebarHistoryExpanded);
	sidebar.insert(WidthKey, m_sidebarWidth);
	sidebar.insert(TodoHeightKey, m_sidebarTodoHeight);
	sidebar.insert(RunningHeightKey, m_sidebarRunningHeight);
	sidebar.insert(RunningOrderKey, QJsonArray::fromStringList(m_sidebarRunningOrder));
	sidebar.insert(HistoryCreatorFilterKey, m_sidebarHistoryCreatorFilter);
	sidebar.insert(HistoryCurrentDirKey, m_sidebarHistoryCurrentDir);
	root.insert(SidebarKey, sidebar);

	QJsonObject window = section(root, WindowKey);
	window.insert(WidthKey, m_windowWidth);
	window.insert(HeightKey, m_windowHeight);
	root.insert(WindowKey, window);
	root.insert(UiScaleKey, m_uiScale);
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
