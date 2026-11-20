#pragma once

#include "client/backend.h"
#include "workspace/backend_config.h"
#include "workspace/config_json_patcher.h"
#include "workspace/connection.h"

#include <QCoroTask>
#include <QObject>
#include <QString>
#include <QStringList>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>

#include <optional>

namespace refgui::workspace
{

// State and backend operations for ConfigDialog (`ref.json` and `ref.json.<n>.bak` profiles):
// lossless Visual <-> JSONC sync via ConfigJsonPatcher, dirty tracking, and profile CRUD.
class ConfigEditorController : public QObject
{
	Q_OBJECT
	Q_PROPERTY(bool loading READ isLoading NOTIFY busyStateChanged)
	Q_PROPERTY(bool saving READ isSaving NOTIFY busyStateChanged)
	Q_PROPERTY(bool busy READ isBusy NOTIFY busyStateChanged)
	Q_PROPERTY(QString errorText READ errorText NOTIFY errorTextChanged)
	Q_PROPERTY(QString configDir READ configDir NOTIFY filesChanged)
	Q_PROPERTY(QVariantList files READ files NOTIFY filesChanged)
	Q_PROPERTY(QString selectedFile READ selectedFile NOTIFY filesChanged)
	Q_PROPERTY(bool selectedFileActive READ isSelectedFileActive NOTIFY filesChanged)
	Q_PROPERTY(bool selectedFileExists READ selectedFileExists NOTIFY filesChanged)
	Q_PROPERTY(bool hasJsoncOverride READ hasJsoncOverride NOTIFY filesChanged)
	Q_PROPERTY(QString envConfigOverride READ envConfigOverride NOTIFY filesChanged)
	Q_PROPERTY(QStringList projectOverrides READ projectOverrides NOTIFY filesChanged)
	Q_PROPERTY(QStringList warnings READ warnings NOTIFY filesChanged)
	Q_PROPERTY(QVariantMap envStatus READ envStatus NOTIFY envStatusChanged)
	Q_PROPERTY(QString mode READ mode NOTIFY modeChanged)
	Q_PROPERTY(QString rawText READ rawText NOTIFY rawTextChanged)
	Q_PROPERTY(QVariantMap parsed READ parsed NOTIFY parsedChanged)
	Q_PROPERTY(QString parseError READ parseError NOTIFY parseErrorChanged)
	Q_PROPERTY(bool dirty READ isDirty NOTIFY dirtyChanged)
	Q_PROPERTY(int revision READ revision NOTIFY parsedChanged)

public:
	ConfigEditorController(Connection& connection, BackendConfig& backendConfig, QObject* parent = nullptr);

	bool isLoading() const { return m_loading; }
	bool isSaving() const { return m_saving; }
	bool isBusy() const { return m_loading || m_saving; }
	const QString& errorText() const { return m_errorText; }
	const QString& configDir() const { return m_configDir; }
	QVariantList files() const { return m_files; }
	const QString& selectedFile() const { return m_selectedFile; }
	bool isSelectedFileActive() const { return m_selectedFileActive; }
	bool selectedFileExists() const { return m_selectedFileExists; }
	bool hasJsoncOverride() const { return m_hasJsoncOverride; }
	const QString& envConfigOverride() const { return m_envConfigOverride; }
	const QStringList& projectOverrides() const { return m_projectOverrides; }
	const QStringList& warnings() const { return m_warnings; }
	QVariantMap envStatus() const { return m_envStatus; }
	const QString& mode() const { return m_mode; }
	const QString& rawText() const { return m_rawText; }
	QVariantMap parsed() const { return m_patcher.orderedRoot().toVariantMap(); }
	const QString& parseError() const { return m_parseError; }
	bool isDirty() const { return m_dirty; }
	int revision() const { return m_revision; }

	const ConfigJsonPatcher& patcher() const { return m_patcher; }

	// Opens the editor for `cwd` (uses BackendConfig::cwd when empty) and loads the active profile.
	Q_INVOKABLE void open(const QString& targetCwd = {});
	// Loads a specific profile (`"ref.json"` or `"ref.json.<n>.bak"`) without swapping it into active.
	Q_INVOKABLE void loadFile(const QString& fileName);
	// Saves the currently selected profile via atomic copy-on-write on the backend.
	Q_INVOKABLE void save();
	// Reverts unsaved Visual / JSON edits back to the last loaded or saved state.
	Q_INVOKABLE void discard();
	// Creates a new active `ref.json` (backing up the current one when it exists).
	Q_INVOKABLE void createProfile(const QString& label, bool cloneCurrent);
	// Swaps `fileName` (`ref.json.<n>.bak`) into active `ref.json`.
	Q_INVOKABLE void activateProfile(const QString& fileName);
	// Deletes `fileName` (promoting newest backup when `fileName == "ref.json"`).
	Q_INVOKABLE void deleteProfile(const QString& fileName);
	// Reloads runtime config across live sessions/MCP and re-reads the selected profile from disk.
	Q_INVOKABLE void reloadFromDisk();

	// Switches between `"visual"` and `"json"`. Returns false when `"json" -> "visual"` is blocked
	// by a JSONC syntax error (leaving `parseError` populated).
	Q_INVOKABLE bool setMode(const QString& targetMode);
	// Updates the raw JSONC text in `"json"` mode and validates syntax live.
	Q_INVOKABLE void setRawText(const QString& newText);

	// Lossless ConfigJsonPatcher operations for Visual mode.
	Q_INVOKABLE QVariant valueAt(const QStringList& path) const;
	Q_INVOKABLE bool contains(const QStringList& path) const;
	Q_INVOKABLE QStringList keysAt(const QStringList& path) const;
	Q_INVOKABLE QVariantList orderedEntriesAt(const QStringList& path) const;
	Q_INVOKABLE void setOrderedObjectAt(const QStringList& path, const QVariantList& entries, bool removeWhenEmpty = true, bool pruneEmptyAncestors = false);
	Q_INVOKABLE QVariant permissionAt(const QStringList& path) const;
	Q_INVOKABLE void setPermissionAt(const QStringList& path, const QVariant& permission, bool pruneEmptyAncestors = true);
	Q_INVOKABLE void setValueAt(const QStringList& path, const QVariant& value);
	Q_INVOKABLE void setStringAt(const QStringList& path, const QString& text, bool removeWhenEmpty = true);
	Q_INVOKABLE void setStringListAt(const QStringList& path, const QStringList& list, bool removeWhenEmpty = true);
	// Parses `jsonText` as JSON/JSONC and writes it at `path` (empty `jsonText` removes `path`).
	// Returns an error string on invalid JSON, or empty string on success.
	Q_INVOKABLE QString setSerializedJsonAt(const QStringList& path, const QString& jsonText, bool removeWhenEmptyObject = false);
	Q_INVOKABLE QString formatJsonAt(const QStringList& path) const;
	Q_INVOKABLE void removeAt(const QStringList& path, bool pruneEmptyAncestors = true);
	Q_INVOKABLE bool renameKeyAt(const QStringList& parentPath, const QString& oldKey, const QString& newKey);

	// Adds a provider entry from a preset (`"anthropic"`, `"openai"`, `"deepseek"`, `"openrouter"`,
	// `"ollama"`, or `"custom"`), returning the created provider key (or empty on conflict).
	Q_INVOKABLE QString addProviderPreset(const QString& presetId, const QString& customKey = {});

	// Combined `{label, value, description}` options from both the edited JSON and live BackendConfig.
	Q_INVOKABLE QVariantList modelOptions() const;
	Q_INVOKABLE QVariantList agentOptions() const;
	Q_INVOKABLE QVariantList variantOptions(const QString& modelRef) const;

signals:
	void busyStateChanged();
	void errorTextChanged();
	void filesChanged();
	void envStatusChanged();
	void modeChanged();
	void rawTextChanged();
	void parsedChanged();
	void parseErrorChanged();
	void dirtyChanged();
	void saved();
	void mutationStarted();
	void mutationFinished();
	void toastRequested(const QString& level, const QString& text);

private:
	std::optional<QString> optionalCwd() const;
	void applyServerInfo(const proto::ConfigFilesInfo& info, bool resetModeOnValid);
	void afterPatcherMutated();
	void updateDirty(bool nextDirty);
	void setErrorText(const QString& message);
	void setParseError(const QString& message);
	void setLoading(bool value);
	void setSaving(bool value);
	QString uniqueKeyUnder(const QStringList& parentPath, const QString& baseName) const;

	QCoro::Task<> fetchTask(std::optional<QString> fileName, bool resetModeOnValid);
	QCoro::Task<> saveTask(QString fileName, QString content);
	QCoro::Task<> createTask(std::optional<QString> label, bool cloneCurrent);
	QCoro::Task<> activateTask(QString fileName);
	QCoro::Task<> deleteTask(QString fileName);
	QCoro::Task<> reloadTask();

	Connection& m_connection;
	BackendConfig& m_backendConfig;
	QString m_cwd;
	ConfigJsonPatcher m_patcher;
	OrderedJsonObject m_baselineRoot;
	QString m_baselineRawText;
	bool m_hasValidBaseline = true;

	bool m_loading = false;
	bool m_saving = false;
	QString m_errorText;
	QString m_configDir;
	QVariantList m_files;
	QString m_selectedFile{QStringLiteral("ref.json")};
	bool m_selectedFileActive = true;
	bool m_selectedFileExists = false;
	bool m_hasJsoncOverride = false;
	QString m_envConfigOverride;
	QStringList m_projectOverrides;
	QStringList m_warnings;
	QVariantMap m_envStatus;
	QString m_mode{QStringLiteral("visual")};
	QString m_rawText{QStringLiteral("{}\n")};
	QString m_parseError;
	bool m_dirty = false;
	int m_revision = 0;
	int m_fetchSerial = 0;
};

} // namespace refgui::workspace
