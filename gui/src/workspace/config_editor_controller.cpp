#include "workspace/config_editor_controller.h"

#include "workspace/toast_text.h"

#include <QJsonValue>
#include <QPointer>
#include <QSet>

#include <algorithm>
#include <cmath>
#include <limits>

using namespace Qt::StringLiterals;

namespace refgui::workspace
{
namespace
{

constexpr double MaxSafeJsIntegerDouble = 9007199254740991.0;

OrderedJsonValue variantToOrderedJson(const QVariant& value)
{
	if(!value.isValid() || value.metaType().id() == QMetaType::Nullptr)
	{
		return {};
	}
	switch(value.metaType().id())
	{
	case QMetaType::Bool:
		return OrderedJsonValue(value.toBool());
	case QMetaType::Int:
	case QMetaType::LongLong:
		return OrderedJsonValue(value.toLongLong());
	case QMetaType::UInt:
	case QMetaType::ULongLong:
	{
		const qulonglong u = value.toULongLong();
		if(u <= static_cast<qulonglong>(std::numeric_limits<qint64>::max()))
		{
			return OrderedJsonValue(static_cast<qint64>(u));
		}
		return OrderedJsonValue(static_cast<double>(u));
	}
	case QMetaType::Double:
	case QMetaType::Float:
	{
		const double d = value.toDouble();
		if(std::isfinite(d) && std::trunc(d) == d && d >= -MaxSafeJsIntegerDouble && d <= MaxSafeJsIntegerDouble)
		{
			return OrderedJsonValue(static_cast<qint64>(d));
		}
		return OrderedJsonValue(d);
	}
	case QMetaType::QString:
		return OrderedJsonValue(value.toString());
	case QMetaType::QStringList:
	{
		OrderedJsonValue::Array arr;
		const QStringList list = value.toStringList();
		arr.reserve(list.size());
		for(const QString& item : list)
		{
			arr.append(OrderedJsonValue(item));
		}
		return OrderedJsonValue(std::move(arr));
	}
	case QMetaType::QVariantMap:
	{
		OrderedJsonObject object;
		const QVariantMap map = value.toMap();
		for(auto it = map.cbegin(); it != map.cend(); ++it)
		{
			const OrderedJsonValue child = variantToOrderedJson(it.value());
			if(!child.isUndefined())
			{
				object.set(it.key(), child);
			}
		}
		return OrderedJsonValue(std::move(object));
	}
	case QMetaType::QVariantList:
	{
		OrderedJsonValue::Array array;
		const QVariantList list = value.toList();
		for(const QVariant& item : list)
		{
			const OrderedJsonValue child = variantToOrderedJson(item);
			if(!child.isUndefined())
			{
				array.append(child);
			}
		}
		return OrderedJsonValue(std::move(array));
	}
	default:
		if(value.canConvert<QJsonValue>())
		{
			return OrderedJsonValue::fromJsonValue(value.value<QJsonValue>());
		}
		return OrderedJsonValue::fromJsonValue(QJsonValue::fromVariant(value));
	}
}

QString normalizePermissionAction(const QVariant& raw)
{
	if(raw.metaType().id() == QMetaType::Bool)
	{
		return raw.toBool() ? u"allow"_s : u"deny"_s;
	}
	const QString text = raw.toString().trimmed();
	if(text == "deny"_L1 || text == "reject"_L1 || text == "no"_L1 || text == "false"_L1)
	{
		return u"deny"_s;
	}
	if(text == "ask"_L1 || text == "confirm"_L1 || text == "prompt"_L1)
	{
		return u"ask"_s;
	}
	return u"allow"_s;
}

void appendNestedPermissionRules(const QString& toolKey, const OrderedJsonObject& nested, QVariantList& out)
{
	for(const OrderedJsonObject::Entry& sub : nested.entries())
	{
		out.append(QVariantMap{
			{u"tool"_s, toolKey},
			{u"pattern"_s, sub.key},
			{u"action"_s, normalizePermissionAction(sub.value.toVariant())},
		});
	}
}

OrderedJsonObject buildPermissionObjectFromRules(const QVariantList& rules)
{
	OrderedJsonObject out;
	for(const QVariant& item : rules)
	{
		const QVariantMap rule = item.toMap();
		const QString tool = rule.value(u"tool"_s).toString().trimmed();
		if(tool.isEmpty())
		{
			continue;
		}
		const QString pattern = rule.value(u"pattern"_s).toString().trimmed();
		const QString action = normalizePermissionAction(rule.value(u"action"_s));
		if(pattern.isEmpty() && !out.contains(tool))
		{
			out.set(tool, OrderedJsonValue(action));
			continue;
		}
		const QString effectivePattern = pattern.isEmpty() ? u"*"_s : pattern;
		OrderedJsonObject nested;
		const OrderedJsonValue existing = out.value(tool);
		if(existing.isObject())
		{
			nested = existing.toObject();
		}
		else if(existing.isString())
		{
			nested.set(u"*"_s, existing);
		}
		nested.set(effectivePattern, OrderedJsonValue(action));
		out.set(tool, OrderedJsonValue(std::move(nested)));
	}
	return out;
}

OrderedJsonObject singleKeyOptionsProvider(const QString& envToken)
{
	OrderedJsonObject options;
	options.set(u"apiKey"_s, OrderedJsonValue(envToken));
	OrderedJsonObject entry;
	entry.set(u"options"_s, OrderedJsonValue(std::move(options)));
	return entry;
}

OrderedJsonObject parseBaselineObject(const proto::ConfigFilesInfo& info)
{
	if(!info.rawText.trimmed().isEmpty())
	{
		if(const auto parsedOrdered = ConfigJsonPatcher::parseJsonc(info.rawText))
		{
			return parsedOrdered->orderedRoot();
		}
	}
	return OrderedJsonObject::fromJsonObject(info.parsed.toObject());
}

void collectProviderModels(
	const QString& providerId,
	const OrderedJsonObject& providerObj,
	QSet<QString>& seen,
	QVariantList& options)
{
	const QString providerName = providerObj.value(u"name"_s).toString(providerId);
	const OrderedJsonObject modelsObj = providerObj.value(u"models"_s).toObject();
	for(const OrderedJsonObject::Entry& modelEntry : modelsObj.entries())
	{
		const OrderedJsonObject modelObj = modelEntry.value.toObject();
		if(modelObj.value(u"disabled"_s).toBool(false))
		{
			continue;
		}
		const QString fullRef = providerId + u'/' + modelEntry.key;
		if(fullRef.isEmpty() || seen.contains(fullRef))
		{
			continue;
		}
		seen.insert(fullRef);
		const QString displayName = modelObj.value(u"name"_s).toString();
		options.append(QVariantMap{
			{u"label"_s, fullRef},
			{u"value"_s, fullRef},
			{u"description"_s, displayName.isEmpty() ? providerName : displayName},
		});
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
		m_patcher.setOrderedRoot(checked->orderedRoot());
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
		m_patcher.setOrderedRoot(m_baselineRoot);
		setParseError({});
	}
	else
	{
		m_patcher.setOrderedRoot({});
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
		m_patcher.setOrderedRoot(checked->orderedRoot());
		++m_revision;
		m_mode = u"visual"_s;
		emit parsedChanged();
		emit modeChanged();
		return true;
	}

	// visual -> json: keep original text untouched unless Visual mode changed the JSON tree.
	if(m_hasValidBaseline && m_patcher.orderedRoot() != m_baselineRoot)
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
		m_patcher.setOrderedRoot(checked->orderedRoot());
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
	const OrderedJsonValue val = m_patcher.orderedValueAt(path);
	return val.isUndefined() ? QVariant() : val.toVariant();
}

bool ConfigEditorController::contains(const QStringList& path) const
{
	return m_patcher.contains(path);
}

QStringList ConfigEditorController::keysAt(const QStringList& path) const
{
	if(path.isEmpty())
	{
		return m_patcher.orderedRoot().keys();
	}
	const OrderedJsonValue val = m_patcher.orderedValueAt(path);
	if(!val.isObject())
	{
		return {};
	}
	return val.toObject().keys();
}

QVariantList ConfigEditorController::orderedEntriesAt(const QStringList& path) const
{
	OrderedJsonObject object;
	if(path.isEmpty())
	{
		object = m_patcher.orderedRoot();
	}
	else
	{
		const OrderedJsonValue val = m_patcher.orderedValueAt(path);
		if(!val.isObject())
		{
			return {};
		}
		object = val.toObject();
	}

	QVariantList entries;
	entries.reserve(object.size());
	for(const OrderedJsonObject::Entry& entry : object.entries())
	{
		entries.append(QVariantMap{
			{u"key"_s, entry.key},
			{u"value"_s, entry.value.toVariant()},
			{u"json"_s, entry.value.toCompactJson()},
		});
	}
	return entries;
}

void ConfigEditorController::setOrderedObjectAt(
	const QStringList& path,
	const QVariantList& entries,
	bool removeWhenEmpty,
	bool pruneEmptyAncestors)
{
	if(path.isEmpty())
	{
		return;
	}
	OrderedJsonObject nextObj;
	for(const QVariant& item : entries)
	{
		const QVariantMap map = item.toMap();
		const QString key = map.value(u"key"_s).toString().trimmed();
		if(key.isEmpty())
		{
			continue;
		}
		if(map.contains(u"json"_s))
		{
			if(const auto parsedVal = ConfigJsonPatcher::parseJsoncValue(map.value(u"json"_s).toString()))
			{
				nextObj.set(key, *parsedVal);
				continue;
			}
		}
		const OrderedJsonValue val = variantToOrderedJson(map.value(u"value"_s));
		if(!val.isUndefined())
		{
			nextObj.set(key, val);
		}
	}

	if(removeWhenEmpty && nextObj.isEmpty())
	{
		removeAt(path, pruneEmptyAncestors);
		return;
	}
	const OrderedJsonValue nextVal(std::move(nextObj));
	if(m_patcher.orderedValueAt(path) == nextVal)
	{
		return;
	}
	m_patcher.setOrderedValueAt(path, nextVal);
	afterPatcherMutated();
}

QVariant ConfigEditorController::permissionAt(const QStringList& path) const
{
	const OrderedJsonValue val = m_patcher.orderedValueAt(path);
	if(val.isUndefined() || val.isNull())
	{
		return {};
	}
	if(val.isBool() || val.isString())
	{
		return val.toVariant();
	}
	if(!val.isObject())
	{
		return {};
	}

	QVariantList rules;
	const OrderedJsonObject permObject = val.toObject();
	for(const OrderedJsonObject::Entry& entry : permObject.entries())
	{
		if(entry.value.isObject())
		{
			appendNestedPermissionRules(entry.key, entry.value.toObject(), rules);
		}
		else
		{
			rules.append(QVariantMap{
				{u"tool"_s, entry.key},
				{u"pattern"_s, QString()},
				{u"action"_s, normalizePermissionAction(entry.value.toVariant())},
			});
		}
	}
	return rules;
}

void ConfigEditorController::setPermissionAt(const QStringList& path, const QVariant& permission, bool pruneEmptyAncestors)
{
	if(path.isEmpty())
	{
		return;
	}
	if(!permission.isValid() || permission.isNull() || permission.metaType().id() == QMetaType::Nullptr)
	{
		removeAt(path, pruneEmptyAncestors);
		return;
	}
	if(permission.metaType().id() == QMetaType::QString || permission.metaType().id() == QMetaType::Bool)
	{
		const QString action = permission.metaType().id() == QMetaType::Bool
			? normalizePermissionAction(permission)
			: permission.toString().trimmed();
		if(action.isEmpty())
		{
			removeAt(path, pruneEmptyAncestors);
			return;
		}
		const OrderedJsonValue nextVal(action);
		if(m_patcher.orderedValueAt(path) == nextVal)
		{
			return;
		}
		m_patcher.setOrderedValueAt(path, nextVal);
		afterPatcherMutated();
		return;
	}
	if(permission.metaType().id() == QMetaType::QVariantList)
	{
		OrderedJsonObject built = buildPermissionObjectFromRules(permission.toList());
		if(built.isEmpty())
		{
			removeAt(path, pruneEmptyAncestors);
			return;
		}
		const OrderedJsonValue nextVal(std::move(built));
		if(m_patcher.orderedValueAt(path) == nextVal)
		{
			return;
		}
		m_patcher.setOrderedValueAt(path, nextVal);
		afterPatcherMutated();
		return;
	}
	const OrderedJsonValue fallback = variantToOrderedJson(permission);
	if(!fallback.isObject() || fallback.toObject().isEmpty())
	{
		removeAt(path, pruneEmptyAncestors);
		return;
	}
	if(m_patcher.orderedValueAt(path) == fallback)
	{
		return;
	}
	m_patcher.setOrderedValueAt(path, fallback);
	afterPatcherMutated();
}

void ConfigEditorController::setValueAt(const QStringList& path, const QVariant& value)
{
	if(path.isEmpty())
	{
		return;
	}
	const OrderedJsonValue jsonVal = variantToOrderedJson(value);
	if(jsonVal.isUndefined())
	{
		removeAt(path);
		return;
	}
	if(m_patcher.orderedValueAt(path) == jsonVal)
	{
		return;
	}
	m_patcher.setOrderedValueAt(path, jsonVal);
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
	OrderedJsonValue::Array arr;
	arr.reserve(list.size());
	for(const QString& item : list)
	{
		arr.append(OrderedJsonValue(item));
	}
	const OrderedJsonValue nextVal(std::move(arr));
	if(m_patcher.orderedValueAt(path) == nextVal)
	{
		return;
	}
	m_patcher.setOrderedValueAt(path, nextVal);
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
	const auto parsed = ConfigJsonPatcher::parseJsoncValue(trimmed);
	if(!parsed)
	{
		return parsed.error().message;
	}
	if(removeWhenEmptyObject && parsed->isObject() && parsed->toObject().isEmpty())
	{
		removeAt(path);
		return {};
	}
	if(m_patcher.orderedValueAt(path) == *parsed)
	{
		return {};
	}
	m_patcher.setOrderedValueAt(path, *parsed);
	afterPatcherMutated();
	return {};
}

QString ConfigEditorController::formatJsonAt(const QStringList& path) const
{
	const OrderedJsonValue val = path.isEmpty()
		? OrderedJsonValue(m_patcher.orderedRoot())
		: m_patcher.orderedValueAt(path);
	if(val.isUndefined() || val.isNull())
	{
		return {};
	}
	if(val.isObject() || val.isArray())
	{
		return val.toIndentedJson();
	}
	return val.toCompactJson();
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
	OrderedJsonObject entry;

	if(presetId == "anthropic"_L1)
	{
		key = uniqueKeyUnder(providerRoot, u"anthropic"_s);
		entry = singleKeyOptionsProvider(u"{env:ANTHROPIC_API_KEY}"_s);
	}
	else if(presetId == "openai"_L1)
	{
		key = uniqueKeyUnder(providerRoot, u"openai"_s);
		entry = singleKeyOptionsProvider(u"{env:OPENAI_API_KEY}"_s);
	}
	else if(presetId == "deepseek"_L1)
	{
		key = uniqueKeyUnder(providerRoot, u"deepseek"_s);
		entry = singleKeyOptionsProvider(u"{env:DEEPSEEK_API_KEY}"_s);
	}
	else if(presetId == "openrouter"_L1)
	{
		key = uniqueKeyUnder(providerRoot, u"openrouter"_s);
		entry = singleKeyOptionsProvider(u"{env:OPENROUTER_API_KEY}"_s);
	}
	else if(presetId == "ollama"_L1)
	{
		key = uniqueKeyUnder(providerRoot, u"ollama"_s);
		OrderedJsonObject options;
		options.set(u"baseURL"_s, OrderedJsonValue(u"http://localhost:11434/v1"_s));
		entry.set(u"name"_s, OrderedJsonValue(u"Ollama / Local"_s));
		entry.set(u"options"_s, OrderedJsonValue(std::move(options)));
		entry.set(u"models"_s, OrderedJsonValue(OrderedJsonObject{}));
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
		OrderedJsonObject options;
		options.set(u"baseURL"_s, OrderedJsonValue(u"https://example.com/v1"_s));
		options.set(u"apiKey"_s, OrderedJsonValue(u"{env:API_KEY}"_s));
		entry.set(u"options"_s, OrderedJsonValue(std::move(options)));
		entry.set(u"models"_s, OrderedJsonValue(OrderedJsonObject{}));
	}

	if(key.isEmpty())
	{
		return {};
	}
	m_patcher.setOrderedValueAt({u"provider"_s, key}, OrderedJsonValue(std::move(entry)));
	afterPatcherMutated();
	return key;
}

QVariantList ConfigEditorController::modelOptions() const
{
	QVariantList options;
	QSet<QString> seen;

	// Models defined in the edited JSON object first (preserving provider & model key order).
	const OrderedJsonObject providers = m_patcher.orderedValueAt({u"provider"_s}).toObject();
	for(const OrderedJsonObject::Entry& providerEntry : providers.entries())
	{
		collectProviderModels(providerEntry.key, providerEntry.value.toObject(), seen, options);
	}

	// Plus models from the live BackendConfig (e.g. env-detected built-in providers).
	for(const proto::ModelInfo& model : m_backendConfig.config().models)
	{
		const QString fullRef = model.provider + u'/' + model.id;
		if(fullRef.isEmpty() || seen.contains(fullRef))
		{
			continue;
		}
		seen.insert(fullRef);
		options.append(QVariantMap{
			{u"label"_s, fullRef},
			{u"value"_s, fullRef},
			{u"description"_s, model.name},
		});
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

	const OrderedJsonObject agentsObj = m_patcher.orderedValueAt({u"agent"_s}).toObject();

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
		const OrderedJsonObject overrideObj = agentsObj.value(builtin.name).toObject();
		if(overrideObj.value(u"disable"_s).toBool(false) || overrideObj.value(u"disabled"_s).toBool(false))
		{
			continue;
		}
		appendAgent(builtin.name, overrideObj.value(u"description"_s).toString(builtin.description));
	}

	for(const OrderedJsonObject::Entry& entry : agentsObj.entries())
	{
		const OrderedJsonObject obj = entry.value.toObject();
		if(obj.value(u"disable"_s).toBool(false) || obj.value(u"disabled"_s).toBool(false))
		{
			continue;
		}
		appendAgent(entry.key, obj.value(u"description"_s).toString());
	}

	for(const proto::AgentInfo& agent : m_backendConfig.config().agents)
	{
		appendAgent(agent.name, agent.description);
	}

	return options;
}

QVariantList ConfigEditorController::variantOptions(const QString& modelRef) const
{
	QString effectiveModel = modelRef.trimmed();
	if(effectiveModel.isEmpty())
	{
		effectiveModel = valueAt({u"model"_s}).toString().trimmed();
	}
	const qsizetype hash = effectiveModel.indexOf(u'#');
	if(hash >= 0)
	{
		effectiveModel = effectiveModel.left(hash).trimmed();
	}
	const qsizetype slash = effectiveModel.indexOf(u'/');
	if(slash <= 0 || slash + 1 >= effectiveModel.size())
	{
		return {};
	}
	const QString providerId = effectiveModel.left(slash);
	const QString modelId = effectiveModel.mid(slash + 1);

	QVariantList options;
	QSet<QString> seen;
	const auto appendVariants = [&](const QStringList& variants) {
		for(const QString& variant : variants)
		{
			if(variant.isEmpty() || seen.contains(variant))
			{
				continue;
			}
			seen.insert(variant);
			options.append(QVariantMap{
				{u"label"_s, variant},
				{u"value"_s, variant},
			});
		}
	};

	appendVariants(keysAt({u"provider"_s, providerId, u"models"_s, modelId, u"variants"_s}));
	appendVariants(m_backendConfig.variantsOf(providerId, modelId));
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
		m_baselineRoot = parseBaselineObject(info);
		m_patcher.setOrderedRoot(m_baselineRoot);
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
		m_patcher.setOrderedRoot({});
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
	updateDirty(!m_hasValidBaseline || m_patcher.orderedRoot() != m_baselineRoot);
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
	const auto reloaded = co_await m_backendConfig.reload(optionalCwd());
	if(!self || serial != m_fetchSerial)
	{
		co_return;
	}
	if(!reloaded)
	{
		setLoading(false);
		setErrorText(reloaded.error().message);
		co_return;
	}
	const std::optional<QString> targetFile = m_selectedFile.isEmpty() ? std::nullopt : std::optional<QString>(m_selectedFile);
	const auto filesResult = co_await m_connection.backend().client().getConfigFiles(targetFile, optionalCwd());
	if(!self || serial != m_fetchSerial)
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

void ConfigEditorController::reset()
{
\t++m_fetchSerial;
\tsetLoading(false);
\tsetSaving(false);
\tsetErrorText({});
\tsetParseError({});
\tm_cwd.clear();
\tm_configDir.clear();
\tm_files.clear();
\tm_selectedFile = u\"ref.json\"_s;
\tm_selectedFileActive = true;
\tm_selectedFileExists = false;
\tm_hasJsoncOverride = false;
\tm_envConfigOverride.clear();
\tm_projectOverrides.clear();
\tm_warnings.clear();
\tm_envStatus.clear();
\tm_patcher = ConfigJsonPatcher();
\tm_baselineRoot = OrderedJsonObject();
\tm_baselineRawText = u\"{}\\n\"_s;
\tm_hasValidBaseline = true;
\tm_rawText = m_baselineRawText;
\t++m_revision;
\tupdateDirty(false);
\temit filesChanged();
\temit envStatusChanged();
\temit rawTextChanged();
\temit parsedChanged();
}

} // namespace refgui::workspace
