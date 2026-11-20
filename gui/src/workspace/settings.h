#pragma once

#include <QJsonObject>
#include <QObject>
#include <QString>
#include <QStringList>
#include <QTimer>

#include <chrono>

namespace refgui::workspace
{

// The user's settings, kept in `gui.json` in the state directory. Every change is saved
// shortly after it (changes in a burst, like a window being resized, make one write), and
// what is pending is written when the object is destroyed.
//
// Fields this version does not know are kept when the file is written, so a newer or older
// ref-gui sharing the file does not lose its settings.
class Settings : public QObject
{
	Q_OBJECT
	Q_PROPERTY(QString theme READ theme WRITE setTheme NOTIFY themeChanged)
	Q_PROPERTY(QString language READ language WRITE setLanguage NOTIFY languageChanged)
	Q_PROPERTY(bool notifyEnabled READ notifyEnabled WRITE setNotifyEnabled NOTIFY notifyKindsChanged)
	Q_PROPERTY(bool notifyPermission READ notifyPermission WRITE setNotifyPermission NOTIFY notifyKindsChanged)
	Q_PROPERTY(bool notifyTurnDone READ notifyTurnDone WRITE setNotifyTurnDone NOTIFY notifyKindsChanged)
	Q_PROPERTY(bool notifyTurnError READ notifyTurnError WRITE setNotifyTurnError NOTIFY notifyKindsChanged)
	Q_PROPERTY(QStringList notifyKinds READ notifyKinds NOTIFY notifyKindsChanged)
	Q_PROPERTY(bool sidebarVisible READ sidebarVisible WRITE setSidebarVisible NOTIFY sidebarVisibleChanged)
	Q_PROPERTY(bool sidebarTodoExpanded READ sidebarTodoExpanded WRITE setSidebarTodoExpanded NOTIFY sidebarTodoExpandedChanged)
	Q_PROPERTY(bool sidebarRunningExpanded READ sidebarRunningExpanded WRITE setSidebarRunningExpanded NOTIFY sidebarRunningExpandedChanged)
	Q_PROPERTY(bool sidebarHistoryExpanded READ sidebarHistoryExpanded WRITE setSidebarHistoryExpanded NOTIFY sidebarHistoryExpandedChanged)
	Q_PROPERTY(int sidebarWidth READ sidebarWidth WRITE setSidebarWidth NOTIFY sidebarWidthChanged)
	Q_PROPERTY(int sidebarTodoHeight READ sidebarTodoHeight WRITE setSidebarTodoHeight NOTIFY sidebarTodoHeightChanged)
	Q_PROPERTY(int sidebarRunningHeight READ sidebarRunningHeight WRITE setSidebarRunningHeight NOTIFY sidebarRunningHeightChanged)
	Q_PROPERTY(QStringList sidebarRunningOrder READ sidebarRunningOrder WRITE setSidebarRunningOrder NOTIFY sidebarRunningOrderChanged)
	Q_PROPERTY(int windowWidth READ windowWidth WRITE setWindowWidth NOTIFY windowSizeChanged)
	Q_PROPERTY(int windowHeight READ windowHeight WRITE setWindowHeight NOTIFY windowSizeChanged)
	Q_PROPERTY(double uiScale READ uiScale WRITE setUiScale NOTIFY uiScaleChanged)
	Q_PROPERTY(double minUiScale READ minUiScale CONSTANT)
	Q_PROPERTY(double maxUiScale READ maxUiScale CONSTANT)
	Q_PROPERTY(double uiScaleStep READ uiScaleStep CONSTANT)
	Q_PROPERTY(QString loadProblem READ loadProblem CONSTANT)

public:
	// The values of `theme` and `language` that mean "do what the system does".
	static const QString System;
	static const QString Light;
	static const QString Dark;
	static const QString English;
	static const QString ChineseSimplified;

	static constexpr int DefaultSidebarWidth = 264;
	static constexpr int MinSidebarWidth = 210;
	static constexpr int MaxSidebarWidth = 480;
	static constexpr int DefaultWindowWidth = 980;
	static constexpr int DefaultWindowHeight = 760;
	static constexpr double DefaultUiScale = 1.0;
	static constexpr double MinUiScale = 0.5;
	static constexpr double MaxUiScale = 4.0;
	static constexpr double UiScaleStep = 0.05;
	// Read-only startup helper: returns normalized `uiScale`, or `DefaultUiScale` if unreadable/invalid (Settings reports errors later).
	static double readUiScaleOnly(const QString& path);
	// Reads `path`. A file that cannot be read keeps the defaults; see loadProblem().
	explicit Settings(QString path, QObject* parent = nullptr);
	~Settings() override;

	QString theme() const { return m_theme; }
	QString language() const { return m_language; }
	bool notifyEnabled() const { return m_notifyEnabled; }
	bool notifyPermission() const { return m_notifyPermission; }
	bool notifyTurnDone() const { return m_notifyTurnDone; }
	bool notifyTurnError() const { return m_notifyTurnError; }
	// The wire names of the notification kinds to ask the backend for: permission, question,
	// turn_done, turn_error. Empty when notifications are off.
	QStringList notifyKinds() const;
	bool sidebarVisible() const { return m_sidebarVisible; }
	bool sidebarTodoExpanded() const { return m_sidebarTodoExpanded; }
	bool sidebarRunningExpanded() const { return m_sidebarRunningExpanded; }
	bool sidebarHistoryExpanded() const { return m_sidebarHistoryExpanded; }
	int sidebarWidth() const { return m_sidebarWidth; }
	int sidebarTodoHeight() const { return m_sidebarTodoHeight; }
	int sidebarRunningHeight() const { return m_sidebarRunningHeight; }
	QStringList sidebarRunningOrder() const { return m_sidebarRunningOrder; }
	int windowWidth() const { return m_windowWidth; }
	int windowHeight() const { return m_windowHeight; }
	double uiScale() const { return m_uiScale; }
	double minUiScale() const { return MinUiScale; }
	double maxUiScale() const { return MaxUiScale; }
	double uiScaleStep() const { return UiScaleStep; }

	// Precondition for both: a known value (see the constants above).
	void setTheme(const QString& theme);
	void setLanguage(const QString& language);
	void setNotifyEnabled(bool enabled);
	void setNotifyPermission(bool enabled);
	void setNotifyTurnDone(bool enabled);
	void setNotifyTurnError(bool enabled);
	void setSidebarVisible(bool visible);
	void setSidebarTodoExpanded(bool expanded);
	void setSidebarRunningExpanded(bool expanded);
	void setSidebarHistoryExpanded(bool expanded);
	void setSidebarWidth(int width);
	void setSidebarTodoHeight(int height);
	void setSidebarRunningHeight(int height);
	void setSidebarRunningOrder(const QStringList& order);
	void setWindowWidth(int width);
	void setWindowHeight(int height);
	void setUiScale(double scale);

	// What went wrong reading the file, for the user. Empty when it was read or did not exist.
	QString loadProblem() const { return m_loadProblem; }

	// Writes the pending changes now. Failures are reported by saveFailed().
	void save();

signals:
	void themeChanged();
	void languageChanged();
	void notifyKindsChanged();
	void sidebarVisibleChanged();
	void sidebarTodoExpandedChanged();
	void sidebarRunningExpandedChanged();
	void sidebarHistoryExpandedChanged();
	void sidebarWidthChanged();
	void sidebarTodoHeightChanged();
	void sidebarRunningHeightChanged();
	void sidebarRunningOrderChanged();
	void windowSizeChanged();
	void uiScaleChanged();
	void saveFailed(const QString& reason);

private:
	void load();
	void readFields(const QJsonObject& root);
	void scheduleSave();
	// Changes `field` to `value`, saves later and emits `changed`; nothing when it is unchanged.
	template<typename T>
	void set(T& field, const T& value, void (Settings::*changed)());
	QJsonObject toJson() const;

	QString m_path;
	QJsonObject m_unknown; // the file's content, to carry over what is not ours
	QString m_theme;
	QString m_language;
	bool m_notifyEnabled = true;
	bool m_notifyPermission = true;
	bool m_notifyTurnDone = true;
	bool m_notifyTurnError = true;
	bool m_sidebarVisible = true;
	bool m_sidebarTodoExpanded = true;
	bool m_sidebarRunningExpanded = true;
	bool m_sidebarHistoryExpanded = true;
	int m_sidebarWidth = DefaultSidebarWidth;
	int m_sidebarTodoHeight = 0;
	int m_sidebarRunningHeight = 0;
	QStringList m_sidebarRunningOrder;
	int m_windowWidth = DefaultWindowWidth;
	int m_windowHeight = DefaultWindowHeight;
	double m_uiScale = DefaultUiScale;
	QString m_loadProblem;
	QTimer m_saveTimer;
	bool m_dirty = false;
};

} // namespace refgui::workspace
