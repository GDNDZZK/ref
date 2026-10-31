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
	Q_PROPERTY(int windowWidth READ windowWidth WRITE setWindowWidth NOTIFY windowSizeChanged)
	Q_PROPERTY(int windowHeight READ windowHeight WRITE setWindowHeight NOTIFY windowSizeChanged)
	Q_PROPERTY(QString loadProblem READ loadProblem CONSTANT)

public:
	// The values of `theme` and `language` that mean "do what the system does".
	static const QString System;
	static const QString Light;
	static const QString Dark;
	static const QString English;
	static const QString ChineseSimplified;

	static constexpr int DefaultWindowWidth = 980;
	static constexpr int DefaultWindowHeight = 760;

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
	int windowWidth() const { return m_windowWidth; }
	int windowHeight() const { return m_windowHeight; }

	// Precondition for both: a known value (see the constants above).
	void setTheme(const QString& theme);
	void setLanguage(const QString& language);
	void setNotifyEnabled(bool enabled);
	void setNotifyPermission(bool enabled);
	void setNotifyTurnDone(bool enabled);
	void setNotifyTurnError(bool enabled);
	void setSidebarVisible(bool visible);
	void setWindowWidth(int width);
	void setWindowHeight(int height);

	// What went wrong reading the file, for the user. Empty when it was read or did not exist.
	QString loadProblem() const { return m_loadProblem; }

	// Writes the pending changes now. Failures are reported by saveFailed().
	void save();

signals:
	void themeChanged();
	void languageChanged();
	void notifyKindsChanged();
	void sidebarVisibleChanged();
	void windowSizeChanged();
	void saveFailed(const QString& reason);

private:
	void load();
	void readFields(const QJsonObject& root);
	void scheduleSave();
	// Marks `field` as changed to `value` and saves later; false when nothing changed.
	template<typename T>
	bool assign(T& field, const T& value);
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
	int m_windowWidth = DefaultWindowWidth;
	int m_windowHeight = DefaultWindowHeight;
	QString m_loadProblem;
	QTimer m_saveTimer;
	bool m_dirty = false;
};

} // namespace refgui::workspace
