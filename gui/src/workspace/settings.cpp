#include "workspace/settings.h"

#include "proto/notify_kind.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QSaveFile>
#include <QtMath>
#include \u003cQUrl\u003e
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
const QString BackendUrlKey = u\"backendUrl\"_s;
const QString BackendTokenKey = u\"backendToken\"_s;
const QString FloatingBallKey = u\"floatingBall\"_s;
const QString ScreenNameKey = u\"screenName\"_s;
const QString XKey = u\"x\"_s;
const QString YKey = u\"y\"_s;

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

bool isValidBackendUrl(const QString\u0026 rawUrl)
{
\tconst QUrl parsed(rawUrl);
\treturn parsed.isValid() \u0026\u0026 (parsed.scheme() == u\"ws\"_s || parsed.scheme() == u\"wss\"_s) \u0026\u0026 !parsed.host().isEmpty();
}

void readBackendSettings(const QJsonObject\u0026 root, QString\u0026 backendUrl, QString\u0026 backendToken, Problems\u0026 problems)
{
\tif(root.contains(BackendUrlKey))
\t{
\t\tconst QJsonValue rawUrl = root.value(BackendUrlKey);
\t\tif(!rawUrl.isString())
\t\t{
\t\t\tproblems.add(u\"%1: expected a WebSocket URL\"_s.arg(BackendUrlKey));
\t\t}
\t\telse
\t\t{
\t\t\tconst QString trimmed = rawUrl.toString().trimmed();
\t\t\tif(!trimmed.isEmpty() \u0026\u0026 !isValidBackendUrl(trimmed))
\t\t\t{
\t\t\t\tproblems.add(u\"%1: expected ws:// or wss:// URL\"_s.arg(BackendUrlKey));
\t\t\t}
\t\t\telse
\t\t\t{
\t\t\t\tbackendUrl = trimmed;
\t\t\t}
\t\t}
\t}
\tif(root.contains(BackendTokenKey))
\t{
\t\tconst QJsonValue rawToken = root.value(BackendTokenKey);
\t\tif(!rawToken.isString())
\t\t{
\t\t\tproblems.add(u\"%1: expected a string\"_s.arg(BackendTokenKey));
\t\t}
\t\telse if(!backendUrl.isEmpty())
\t\t{
\t\t\tbackendToken = rawToken.toString();
\t\t}
\t}
}

void readFloatingBall(const QJsonObject\u0026 root, QString\u0026 screenName, int\u0026 x, int\u0026 y, bool\u0026 hasPos, Problems\u0026 problems)
{
\tif(!root.contains(FloatingBallKey))
\t{
\t\treturn;
\t}
\tconst QJsonValue raw = root.value(FloatingBallKey);
\tif(!raw.isObject())
\t{
\t\tproblems.add(u\"%1: expected an object\"_s.arg(FloatingBallKey));
\t\treturn;
\t}
\tconst QJsonObject obj = raw.toObject();
\tQString parsedScreen;
\tif(obj.contains(ScreenNameKey))
\t{
\t\tif(!obj.value(ScreenNameKey).isString())
\t\t{
\t\t\tproblems.add(u\"%1: expected a string\"_s.arg(fieldPath(FloatingBallKey, ScreenNameKey)));
\t\t}
\t\telse
\t\t{
\t\t\tparsedScreen = obj.value(ScreenNameKey).toString();
\t\t}
\t}
\tconst QJsonValue rawX = obj.value(XKey);
\tconst QJsonValue rawY = obj.value(YKey);
\tif(obj.contains(XKey) || obj.contains(YKey))
\t{
\t\tif(!rawX.isDouble() || !rawY.isDouble() || !std::isfinite(rawX.toDouble()) || !std::isfinite(rawY.toDouble()))
\t\t{
\t\t\tproblems.add(u\"%1: expected numeric x and y\"_s.arg(FloatingBallKey));
\t\t\treturn;
\t\t}
\t\tscreenName = parsedScreen;
\t\tx = qRound(rawX.toDouble());
\t\ty = qRound(rawY.toDouble());
\t\thasPos = true;
\t}
\telse if(!parsedScreen.isEmpty())
\t{
\t\tscreenName = parsedScreen;
\t}
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

void Settings::setBackend(const QString\u0026 url, const QString\u0026 token)
{
\tconst QString trimmedUrl = url.trimmed();
\tconst QString effectiveToken = trimmedUrl.isEmpty() ? QString() : token;
\tif(m_backendUrl == trimmedUrl \u0026\u0026 m_backendToken == effectiveToken)
\t{
\t\treturn;
\t}
\tm_backendUrl = trimmedUrl;
\tm_backendToken = effectiveToken;
\tm_dirty = true;
\tsave();
\temit backendChanged();
}

void Settings::setFloatingBallPosition(const QString\u0026 screenName, int x, int y)
{
\tif(m_hasFloatingBallPosition \u0026\u0026 m_floatingBallScreen == screenName \u0026\u0026 m_floatingBallX == x \u0026\u0026 m_floatingBallY == y)
\t{
\t\treturn;
\t}
\tm_floatingBallScreen = screenName;
\tm_floatingBallX = x;
\tm_floatingBallY = y;
\tm_hasFloatingBallPosition = true;
\tscheduleSave();
\temit floatingBallChanged();
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
\treadBackendSettings(root, m_backendUrl, m_backendToken, problems);
\treadFloatingBall(root, m_floatingBallScreen, m_floatingBallX, m_floatingBallY, m_hasFloatingBallPosition, problems);
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
\tif(!m_backendUrl.isEmpty())
\t{
\t\troot.insert(BackendUrlKey, m_backendUrl);
\t\tif(!m_backendToken.isEmpty())
\t\t{
\t\t\troot.insert(BackendTokenKey, m_backendToken);
\t\t}
\t\telse
\t\t{
\t\t\troot.remove(BackendTokenKey);
\t\t}
\t}
\telse
\t{
\t\troot.remove(BackendUrlKey);
\t\troot.remove(BackendTokenKey);
\t}
\tif(m_hasFloatingBallPosition)
\t{
\t\tQJsonObject floatingBall = section(root, FloatingBallKey);
\t\tfloatingBall.insert(ScreenNameKey, m_floatingBallScreen);
\t\tfloatingBall.insert(XKey, m_floatingBallX);
\t\tfloatingBall.insert(YKey, m_floatingBallY);
\t\troot.insert(FloatingBallKey, floatingBall);
\t}
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
#ifndef Q_OS_WIN
\tfile.setPermissions(QFileDevice::ReadOwner | QFileDevice::WriteOwner);
#endif
\tfile.write(QJsonDocument(toJson()).toJson(QJsonDocument::Indented));
\tif(!file.commit())
\t{
\t\temit saveFailed(tr(\"Could not save %1: %2\").arg(m_path, file.errorString()));
\t\treturn;
\t}
#ifndef Q_OS_WIN
\tQFile::setPermissions(m_path, QFileDevice::ReadOwner | QFileDevice::WriteOwner);
#endif
}

} // namespace refgui::workspace
