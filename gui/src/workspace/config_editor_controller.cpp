#include "workspace/config_editor_controller.h"

#include "workspace/toast_text.h"

#include <QJsonArray>
#include <QJsonDocument>
#include <QPointer>
#include <QSet>

#include <algorithm>

using namespace Qt::StringLiterals;

namespace refgui::workspace
{
namespace
{

QJsonValue variantToJson(const QVariant& value)
{
	if(!value.isValid() || value.metaType().id() == QMetaType::Nullptr)
	{
		return QJsonValue(QJsonValue::Undefined);
	}
	switch(value.metaType().id())
	{
	case QMetaType::Bool:
		return QJsonValue(value.toBool());
	case QMetaType::Int:
	case QMetaType::LongLong:
	case QMetaType::UInt:
	case QMetaType::ULongLong:
	case QMetaType::Double:
	case QMetaType::Float:
		return QJsonValue(value.toDouble());
	case QMetaType::QString:
		return QJsonValue(value.toString());
	case QMetaType::QStringList:
		return QJsonArray::fromStringList(value.toStringList());
	case QMetaType::QVariantMap:
	{
		QJsonObject object;
		const QVariantMap map = value.toMap();
		for(auto it = map.cbegin(); it != map.cend(); ++it)
		{
			const QJsonValue child = variantToJson(it.value());
			if(!child.isUndefined())
			{
				object.insert(it.key(), child);
			}
		}
		return object;
	}
	case QMetaType::QVariantList:
	{
		QJsonArray array;
		const QVariantList list = value.toList();
		for(const QVariant& item : list)
		{
			const QJsonValue child = variantToJson(item);
			if(!child.isUndefined())
			{
				array.append(child);
			}
		}
		return array;
	}
	default:
		if(value.canConvert<QJsonValue>())
		{
			return value.value<QJsonValue>();
		}
		return QJsonValue::fromVariant(value);
	}
}

QVariantMap fileEntryToMap(const proto::ConfigFileEntry& entry)
{
	const QString label = entry.label.value_or(QString());
	const QString displayLabel = label.isEmpty()
		? u"%1 — %2"_s.arg(entry.file, entry.summary)
		: u"%1 — %2 (%3)"_s.arg(label, entry.summary, entry.file);
	return {
		{u"file"_s, entry.file},
		{u"active"_s, entry.active},
		{u"exists"_s, entry.exists},
		{u"label"_s, label},
		{u"summary"_s, entry.summary},
		{u"modifiedAt"_s, entry.modifiedAt},
		{u"size"_s, entry.size},
		{u"displayLabel"_s, displayLabel},
	};
}

} // namespace

ConfigEditorController::ConfigEditorController(Connection& connection, BackendConfig& backendConfig, QObject* parent)
	: QObject(parent)
	, m_connection(connection)
	, m_backendConfig(backendConfig)
{
}

std::optional<QString> ConfigEditorController::optionalCwd() const
{
	const QString effective = m_cwd.isEmpty() ? m_backendConfig.cwd() : m_cwd;
	return effective.isEmpty() ? std::nullopt : std::optional<QString>(effective);
}

void ConfigEditorController::open(const QString& targetCwd)
{
	m_cwd = targetCwd;
	setErrorText({});
	fetchTask(std::nullopt, true);
}

void ConfigEditorController::loadFile(const QString& fileName)
{
	if(fileName.trimmed().isEmpty())
	{
		return;
	}
	setErrorText({});
	fetchTask(fileName.trimmed(), false);
}

void ConfigEditorController::save()
{
	if(isBusy())
	{
		return;
	}
	setErrorText({});

	QString contentToSave;
	if(m_mode == "json"_L1)
	{
		const auto checked = ConfigJsonPatcher::parseJsonc(m_rawText);
		if(!checked)
		{
			setParseError(checked.error().message);
			setErrorText(checked.error().message);
			return;
		}
		setParseError({});
		m_patcher.setRoot(checked->root());
		contentToSave = m_rawText.endsWith(u'\n') ? m_rawText : m_rawText + u'\n';
	}
	else
	{
		contentToSave = m_patcher.toJson();
	}

	saveTask(m_selectedFile.isEmpty() ? u"ref.json"_s : m_selectedFile, std::move(contentToSave));
}

void ConfigEditorController::discard()
{
	m_rawText = m_baselineRawText;
	if(m_hasValidBaseline)
	{
		m_patcher.setRoot(m_baselineRoot);
		setParseError({});
	}
	else
	{
		m_patcher.setRoot({});
		const auto reparsed = ConfigJsonPatcher::parseJsonc(m_baselineRawText);
		setParseError(reparsed ? QString() : reparsed.error().message);
	}
	++m_revision;
	setErrorText({});
	updateDirty(false);
	emit parsedChanged();
	emit rawTextChanged();
}

void ConfigEditorController::createProfile(const QString& label, bool cloneCurrent)
{
	if(isBusy())
	{
		return;
	}
	setErrorText({});
	const QString trimmed = label.trimmed();
	createTask(trimmed.isEmpty() ? std::nullopt : std::optional<QString>(trimmed), cloneCurrent);
}

void ConfigEditorController::activateProfile(const QString& fileName)
{
	if(isBusy() || fileName.trimmed().isEmpty())
	{
		return;
	}
	setErrorText({});
	activateTask(fileName.trimmed());
}

void ConfigEditorController::deleteProfile(const QString& fileName)
{
	if(isBusy() || fileName.trimmed().isEmpty())
	{
		return;
	}
	setErrorText({});
	deleteTask(fileName.trimmed());
}

void ConfigEditorController::reloadFromDisk()
{
	if(isBusy())
	{
		return;
	}
	setErrorText({});
	reloadTask();
}

bool ConfigEditorController::setMode(const QString& targetMode)
{
	if(targetMode != "visual"_L1 && targetMode != "json"_L1)
	{
		return false;
	}
	if(targetMode == m_mode)
	{
		return true;
	}
	if(targetMode == "visual"_L1)
	{
		const auto checked = ConfigJsonPatcher::parseJsonc(m_rawText);
		if(!checked)
		{
			setParseError(checked.error().message);
			return false;
		}
		setParseError({});
		m_patcher.setRoot(checked->root());
		++m_revision;
		m_mode = u"visual"_s;
		emit parsedChanged();
		emit modeChanged();
		return true;
	}

	// visual -> json: keep original text untouched unless Visual mode changed the JSON tree.
	if(m_hasValidBaseline && m_patcher.root() != m_baselineRoot)
	{
		m_rawText = m_patcher.toJson();
		emit rawTextChanged();
	}
	m_mode = u"json"_s;
	emit modeChanged();
	return true;
}

void ConfigEditorController::setRawText(const QString& newText)
{
	if(newText == m_rawText)
	{
		return;
	}
	m_rawText = newText;
	setErrorText({});

	const auto checked = ConfigJsonPatcher::parseJsonc(m_rawText);
	if(checked)
	{
		setParseError({});
		m_patcher.setRoot(checked->root());
		++m_revision;
		emit parsedChanged();
	}
	else
	{
		setParseError(checked.error().message);
	}

	updateDirty(m_rawText != m_baselineRawText);
	emit rawTextChanged();
}

QVariant ConfigEditorController::valueAt(const QStringList& path) const
{
	const QJsonValue val = m_patcher.valueAt(path);
	if(val.isUndefined())
	{
		return {};
	}
	return val.toVariant();
}

bool ConfigEditorController::contains(const QStringList& path) const
{
	return m_patcher.contains(path);
}

QStringList ConfigEditorController::keysAt(const QStringList& path) const
{
	const QJsonValue val = path.isEmpty() ? QJsonValue(m_patcher.root()) : m_patcher.valueAt(path);
	if(!val.isObject())
	{
		return {};
	}
	return val.toObject().keys();
}

void ConfigEditorController::setValueAt(const QStringList& path, const QVariant& value)
{
	if(path.isEmpty())
	{
		return;
	}
	const QJsonValue jsonVal = variantToJson(value);
	if(jsonVal.isUndefined())
	{
		removeAt(path);
		return;
	}
	if(m_patcher.valueAt(path) == jsonVal)
	{
		return;
	}
	m_patcher.setValueAt(path, jsonVal);
	afterPatcherMutated();
}

void ConfigEditorController::setStringAt(const QStringList& path, const QString& text, bool removeWhenEmpty)
{
	if(path.isEmpty())
	{
		return;
	}
	if(removeWhenEmpty && text.isEmpty())
	{
		removeAt(path);
		return;
	}
	setValueAt(path, text);
}

void ConfigEditorController::setStringListAt(const QStringList& path, const QStringList& list, bool removeWhenEmpty)
{
	if(path.isEmpty())
	{
		return;
	}
	if(removeWhenEmpty && list.isEmpty())
	{
		removeAt(path);
		return;
	}
	const QJsonValue arr = QJsonArray::fromStringList(list);
	if(m_patcher.valueAt(path) == arr)
	{
		return;
	}
	m_patcher.setValueAt(path, arr);
	afterPatcherMutated();
}

QString ConfigEditorController::setSerializedJsonAt(const QStringList& path, const QString& jsonText, bool removeWhenEmptyObject)
{
	if(path.isEmpty())
	{
		return {};
	}
	const QString trimmed = jsonText.trimmed();
	if(trimmed.isEmpty())
	{
		removeAt(path);
		return {};
	}
	const auto wrapped = ConfigJsonPatcher::parseJsonc(u"{\"value\": %1}"_s.arg(trimmed));
	if(!wrapped)
	{
		return wrapped.error().message;
	}
	const QJsonValue val = wrapped->root().value(u"value"_s);
	if(removeWhenEmptyObject && val.isObject() && val.toObject().isEmpty())
	{
		removeAt(path);
		return {};
	}
	if(m_patcher.valueAt(path) == val)
	{
		return {};
	}
	m_patcher.setValueAt(path, val);
	afterPatcherMutated();
	return {};
}

QString ConfigEditorController::formatJsonAt(const QStringList& path) const
{
	const QJsonValue val = path.isEmpty() ? QJsonValue(m_patcher.root()) : m_patcher.valueAt(path);
	if(val.isUndefined() || val.isNull())
	{
		return {};
	}
	if(val.isObject())
	{
		return QString::fromUtf8(QJsonDocument(val.toObject()).toJson(QJsonDocument::Indented)).trimmed();
	}
	if(val.isArray())
	{
		return QString::fromUtf8(QJsonDocument(val.toArray()).toJson(QJsonDocument::Indented)).trimmed();
	}
	const QJsonObject wrapper{{u"v"_s, val}};
	const QString compact = QString::fromUtf8(QJsonDocument(wrapper).toJson(QJsonDocument::Compact));
	// Strip `{"v":` and trailing `}`
	return compact.mid(5, compact.size() - 6);
}

void ConfigEditorController::removeAt(const QStringList& path, bool pruneEmptyAncestors)
{
	if(path.isEmpty() || !m_patcher.contains(path))
	{
		return;
	}
	m_patcher.removeAt(path, pruneEmptyAncestors);
	afterPatcherMutated();
}

bool ConfigEditorController::renameKeyAt(const QStringList& parentPath, const QString& oldKey, const QString& newKey)
{
	const QString trimmed = newKey.trimmed();
	if(trimmed.isEmpty())
	{
		return false;
	}
	if(oldKey == trimmed)
	{
		return true;
	}
	if(!m_patcher.renameKeyAt(parentPath, oldKey, trimmed))
	{
		return false;
	}
	afterPatcherMutated();
	return true;
}

QString ConfigEditorController::uniqueKeyUnder(const QStringList& parentPath, const QString& baseName) const
{
	const QStringList existing = keysAt(parentPath);
	if(!existing.contains(baseName))
	{
		return baseName;
	}
	for(int i = 2; i < 1000; ++i)
	{
		const QString candidate = u"%1-%2"_s.arg(baseName).arg(i);
		if(!existing.contains(candidate))
		{
			return candidate;
		}
	}
	return {};
}

QString ConfigEditorController::addProviderPreset(const QString& presetId, const QString& customKey)
{
	const QStringList providerRoot{u"provider"_s};
	QString key;
	QJsonObject entry;

	if(presetId == "anthropic"_L1)
	{
		key = uniqueKeyUnder(providerRoot, u"anthropic"_s);
		entry = QJsonObject{
			{u"options"_s, QJsonObject{{u"apiKey"_s, u"{env:ANTHROPIC_API_KEY}"_s}}},
		};
	}
	else if(presetId == "openai"_L1)
	{
		key = uniqueKeyUnder(providerRoot, u"openai"_s);
		entry = QJsonObject{
			{u"options"_s, QJsonObject{{u"apiKey"_s, u"{env:OPENAI_API_KEY}"_s}}},
		};
	}
	else if(presetId == "deepseek"_L1)
	{
		key = uniqueKeyUnder(providerRoot, u"deepseek"_s);
		entry = QJsonObject{
			{u"options"_s, QJsonObject{{u"apiKey"_s, u"{env:DEEPSEEK_API_KEY}"_s}}},
		};
	}
	else if(presetId == "openrouter"_L1)
	{
		key = uniqueKeyUnder(providerRoot, u"openrouter"_s);
		entry = QJsonObject{
			{u"options"_s, QJsonObject{{u"apiKey"_s, u"{env:OPENROUTER_API_KEY}"_s}}},
		};
	}
	else if(presetId == "ollama"_L1)
	{
		key = uniqueKeyUnder(providerRoot, u"ollama"_s);
		entry = QJsonObject{
			{u"name"_s, u"Ollama / Local"_s},
			{u"options"_s, QJsonObject{{u"baseURL"_s, u"http://localhost:11434/v1"_s}}},
			{u"models"_s, QJsonObject{}},
		};
	}
	else
	{
		const QString requested = customKey.trimmed();
		if(!requested.isEmpty())
		{
			if(keysAt(providerRoot).contains(requested))
			{
				return {};
			}
			key = requested;
		}
		else
		{
			key = uniqueKeyUnder(providerRoot, u"custom"_s);
		}
		entry = QJsonObject{
			{u"options"_s, QJsonObject{{u"baseURL"_s, u"https://example.com/v1"_s}, {u"apiKey"_s, u"{env:API_KEY}"_s}}},
			{u"models"_s, QJsonObject{}},
		};
	}

	if(key.isEmpty())
	{
		return {};
	}
	m_patcher.setValueAt({u"provider"_s, key}, entry);
	afterPatcherMutated();
	return key;
}

QVariantList ConfigEditorController::modelOptions() const
{
	QVariantList options;
	QSet<QString> seen;

	const auto appendOption = [&](const QString& fullRef, const QString& label, const QString& description) {
		if(fullRef.isEmpty() || seen.contains(fullRef))
		{
			return;
		}
		seen.insert(fullRef);
		options.append(QVariantMap{
			{u"label"_s, label},
			{u"value"_s, fullRef},
			{u"description"_s, description},
		});
	};

	// Models defined in the edited JSON object first.
	const QJsonObject providers = m_patcher.valueAt({u"provider"_s}).toObject();
	for(auto pit = providers.begin(); pit != providers.end(); ++pit)
	{
		const QString providerId = pit.key();
		const QJsonObject providerObj = pit.value().toObject();
		const QString providerName = providerObj.value(u"name"_s).toString(providerId);
		const QJsonObject modelsObj = providerObj.value(u"models"_s).toObject();
		for(auto mit = modelsObj.begin(); mit != modelsObj.end(); ++mit)
		{
			const QJsonObject modelObj = mit.value().toObject();
			if(modelObj.value(u"disabled"_s).toBool(false))
			{
				continue;
			}
			const QString modelKey = mit.key();
			const QString fullRef = providerId + u'/' + modelKey;
			const QString displayName = modelObj.value(u"name"_s).toString();
			appendOption(fullRef, fullRef, displayName.isEmpty() ? providerName : displayName);
		}
	}

	// Plus models from the live BackendConfig (e.g. env-detected built-in providers).
	for(const proto::ModelInfo& model : m_backendConfig.config().models)
	{
		const QString fullRef = model.provider + u'/' + model.id;
		appendOption(fullRef, fullRef, model.name);
	}

	return options;
}

QVariantList ConfigEditorController::agentOptions() const
{
	QVariantList options;
	QSet<QString> seen;

	const auto appendAgent = [&](const QString& name, const QString& description) {
		if(name.isEmpty() || name == "compaction"_L1 || seen.contains(name))
		{
			return;
		}
		seen.insert(name);
		options.append(QVariantMap{
			{u"label"_s, name},
			{u"value"_s, name},
			{u"description"_s, description},
		});
	};

	const QJsonObject agentsObj = m_patcher.valueAt({u"agent"_s}).toObject();

	struct BuiltinDef
	{
		QString name;
		QString description;
	};
	const QList<BuiltinDef> builtins{
		{u"build"_s, tr("Default agent with full tool access")},
		{u"plan"_s, tr("Read-only analysis and planning")},
		{u"general"_s, tr("Multi-step tasks")},
		{u"explore"_s, tr("Fast read-only exploration")},
	};
	for(const BuiltinDef& builtin : builtins)
	{
		const QJsonObject overrideObj = agentsObj.value(builtin.name).toObject();
		if(overrideObj.value(u"disable"_s).toBool(false) || overrideObj.value(u"disabled"_s).toBool(false))
		{
			continue;
		}
		appendAgent(builtin.name, overrideObj.value(u"description"_s).toString(builtin.description));
	}

	for(auto it = agentsObj.begin(); it != agentsObj.end(); ++it)
	{
		const QJsonObject obj = it.value().toObject();
		if(obj.value(u"disable"_s).toBool(false) || obj.value(u"disabled"_s).toBool(false))
		{
			continue;
		}
		appendAgent(it.key(), obj.value(u"description"_s).toString());
	}

	for(const proto::AgentInfo& agent : m_backendConfig.config().agents)
	{
		appendAgent(agent.name, agent.description);
	}

	return options;
}

void ConfigEditorController::applyServerInfo(const proto::ConfigFilesInfo& info, bool resetModeOnValid)
{
	m_configDir = info.configDir;
	m_selectedFile = info.selectedFile;
	m_selectedFileActive = (info.selectedFile == "ref.json"_L1);
	m_selectedFileExists = false;

	QVariantList mappedFiles;
	mappedFiles.reserve(info.files.size());
	for(const proto::ConfigFileEntry& entry : info.files)
	{
		if(entry.file == info.selectedFile)
		{
			m_selectedFileActive = entry.active;
			m_selectedFileExists = entry.exists;
		}
		mappedFiles.append(fileEntryToMap(entry));
	}
	m_files = std::move(mappedFiles);

	m_hasJsoncOverride = info.hasJsoncOverride;
	m_envConfigOverride = info.envConfigOverride.value_or(QString());
	m_projectOverrides = info.projectOverrides;
	m_warnings = info.warnings;

	QVariantMap nextEnv;
	for(auto it = info.envStatus.cbegin(); it != info.envStatus.cend(); ++it)
	{
		nextEnv.insert(it.key(), it.value());
	}
	m_envStatus = std::move(nextEnv);

	m_baselineRawText = info.rawText;
	m_rawText = info.rawText;

	const bool validObject = !info.parseError.has_value() && info.parsed.isObject();
	m_hasValidBaseline = validObject;
	if(validObject)
	{
		m_baselineRoot = info.parsed.toObject();
		m_patcher.setRoot(m_baselineRoot);
		setParseError({});
		if(resetModeOnValid && m_mode != "visual"_L1)
		{
			m_mode = u"visual"_s;
			emit modeChanged();
		}
	}
	else
	{
		m_baselineRoot = {};
		m_patcher.setRoot({});
		setParseError(info.parseError.value_or(tr("Configuration root must be a JSON object.")));
		if(m_mode != "json"_L1)
		{
			m_mode = u"json"_s;
			emit modeChanged();
		}
	}

	++m_revision;
	updateDirty(false);
	emit filesChanged();
	emit envStatusChanged();
	emit parsedChanged();
	emit rawTextChanged();
}

void ConfigEditorController::afterPatcherMutated()
{
	++m_revision;
	m_rawText = m_patcher.toJson();
	setParseError({});
	setErrorText({});
	updateDirty(!m_hasValidBaseline || m_patcher.root() != m_baselineRoot);
	emit parsedChanged();
	emit rawTextChanged();
}

void ConfigEditorController::updateDirty(bool nextDirty)
{
	if(m_dirty == nextDirty)
	{
		return;
	}
	m_dirty = nextDirty;
	emit dirtyChanged();
}

void ConfigEditorController::setErrorText(const QString& message)
{
	if(m_errorText == message)
	{
		return;
	}
	m_errorText = message;
	emit errorTextChanged();
}

void ConfigEditorController::setParseError(const QString& message)
{
	if(m_parseError == message)
	{
		return;
	}
	m_parseError = message;
	emit parseErrorChanged();
}

void ConfigEditorController::setLoading(bool value)
{
	if(m_loading == value)
	{
		return;
	}
	m_loading = value;
	emit busyStateChanged();
}

void ConfigEditorController::setSaving(bool value)
{
	if(m_saving == value)
	{
		return;
	}
	m_saving = value;
	emit busyStateChanged();
}

QCoro::Task<> ConfigEditorController::fetchTask(std::optional<QString> fileName, bool resetModeOnValid)
{
	const int serial = ++m_fetchSerial;
	const QPointer<ConfigEditorController> self(this);
	setLoading(true);
	const auto result = co_await m_connection.backend().client().getConfigFiles(std::move(fileName), optionalCwd());
	if(!self || serial != m_fetchSerial)
	{
		co_return;
	}
	setLoading(false);
	if(!result)
	{
		setErrorText(result.error().message);
		co_return;
	}
	applyServerInfo(*result, resetModeOnValid);
}

QCoro::Task<> ConfigEditorController::saveTask(QString fileName, QString content)
{
	++m_fetchSerial;
	const QPointer<ConfigEditorController> self(this);
	setSaving(true);
	emit mutationStarted();
	const auto result = co_await m_connection.backend().client().saveConfigFile(fileName, std::move(content), optionalCwd());
	if(!self)
	{
		co_return;
	}
	emit mutationFinished();
	setSaving(false);
	if(!result)
	{
		setErrorText(result.error().message);
		co_return;
	}
	applyServerInfo(*result, false);
	emit saved();
	emit toastRequested(ToastLevel::Info, tr("Saved %1").arg(m_selectedFile));
	if(!result->warnings.isEmpty())
	{
		emit toastRequested(ToastLevel::Warn, toastText(result->warnings.join(u"; "_s)));
	}
}

QCoro::Task<> ConfigEditorController::createTask(std::optional<QString> label, bool cloneCurrent)
{
	++m_fetchSerial;
	const QPointer<ConfigEditorController> self(this);
	setSaving(true);
	emit mutationStarted();
	const auto result = co_await m_connection.backend().client().createConfigFile(std::move(label), cloneCurrent, std::nullopt, optionalCwd());
	if(!self)
	{
		co_return;
	}
	emit mutationFinished();
	setSaving(false);
	if(!result)
	{
		setErrorText(result.error().message);
		co_return;
	}
	applyServerInfo(*result, true);
	emit toastRequested(ToastLevel::Info, tr("Created new active configuration profile"));
	if(!result->warnings.isEmpty())
	{
		emit toastRequested(ToastLevel::Warn, toastText(result->warnings.join(u"; "_s)));
	}
}

QCoro::Task<> ConfigEditorController::activateTask(QString fileName)
{
	++m_fetchSerial;
	const QPointer<ConfigEditorController> self(this);
	setSaving(true);
	emit mutationStarted();
	const auto result = co_await m_connection.backend().client().selectConfigFile(fileName, optionalCwd());
	if(!self)
	{
		co_return;
	}
	emit mutationFinished();
	setSaving(false);
	if(!result)
	{
		setErrorText(result.error().message);
		co_return;
	}
	applyServerInfo(*result, false);
	emit toastRequested(ToastLevel::Info, tr("Activated configuration profile"));
	if(!result->warnings.isEmpty())
	{
		emit toastRequested(ToastLevel::Warn, toastText(result->warnings.join(u"; "_s)));
	}
}

QCoro::Task<> ConfigEditorController::deleteTask(QString fileName)
{
	++m_fetchSerial;
	const QPointer<ConfigEditorController> self(this);
	setSaving(true);
	emit mutationStarted();
	const auto result = co_await m_connection.backend().client().deleteConfigFile(fileName, optionalCwd());
	if(!self)
	{
		co_return;
	}
	emit mutationFinished();
	setSaving(false);
	if(!result)
	{
		setErrorText(result.error().message);
		co_return;
	}
	applyServerInfo(*result, false);
	emit toastRequested(ToastLevel::Info, tr("Deleted %1").arg(fileName));
	if(!result->warnings.isEmpty())
	{
		emit toastRequested(ToastLevel::Warn, toastText(result->warnings.join(u"; "_s)));
	}
}

QCoro::Task<> ConfigEditorController::reloadTask()
{
	const int serial = ++m_fetchSerial;
	const QPointer<ConfigEditorController> self(this);
	setLoading(true);
	emit mutationStarted();
	const auto reloaded = co_await m_backendConfig.reload(optionalCwd());
	if(!self)
	{
		co_return;
	}
	if(!reloaded)
	{
		emit mutationFinished();
		setLoading(false);
		setErrorText(reloaded.error().message);
		co_return;
	}
	const std::optional<QString> targetFile = m_selectedFile.isEmpty() ? std::nullopt : std::optional<QString>(m_selectedFile);
	const auto filesResult = co_await m_connection.backend().client().getConfigFiles(targetFile, optionalCwd());
	if(!self)
	{
		co_return;
	}
	emit mutationFinished();
	if(serial != m_fetchSerial)
	{
		co_return;
	}
	setLoading(false);
	if(!filesResult)
	{
		setErrorText(filesResult.error().message);
		co_return;
	}
	applyServerInfo(*filesResult, false);
	emit toastRequested(ToastLevel::Info, tr("Configuration reloaded"));
	if(!reloaded->warnings.isEmpty())
	{
		emit toastRequested(ToastLevel::Warn, toastText(reloaded->warnings.join(u"; "_s)));
	}
}

} // namespace refgui::workspace
