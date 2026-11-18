#include "workspace/backend_config.h"

#include <QPointer>
#include <QVariantMap>

using namespace Qt::StringLiterals;

namespace refgui::workspace
{

namespace
{

QVariantMap toMap(const proto::AgentInfo& agent)
{
	return {
		{u"name"_s, agent.name},
		{u"description"_s, agent.description},
		{u"model"_s, agent.model ? agent.model->model : QString()},
		{u"color"_s, agent.color.value_or(QString())},
		{u"creatable"_s, agent.creatable},
		{u"canSpawn"_s, agent.canSpawn},
	};
}

QVariantMap toMap(const proto::ModelInfo& model)
{
	return {
		{u"provider"_s, model.provider},
		{u"providerName"_s, model.providerName},
		{u"id"_s, model.id},
		{u"name"_s, model.name},
		{u"context"_s, model.context},
		{u"images"_s, model.images},
		{u"reasoning"_s, model.reasoning},
		{u"variants"_s, model.variants},
	};
}

QVariantMap toMap(const proto::CommandInfo& command)
{
	return {
		{u"name"_s, command.name},
		{u"description"_s, command.description},
		{u"source"_s, command.source},
	};
}

QVariantMap toMap(const proto::McpServerStatus& server)
{
	return {
		{u"name"_s, server.name},
		{u"status"_s, server.status},
		{u"error"_s, server.error.value_or(QString())},
		{u"tools"_s, server.tools},
	};
}

template<typename T>
QVariantList toList(const QList<T>& items)
{
	QVariantList list;
	for(const T& item : items)
	{
		list.append(toMap(item));
	}
	return list;
}

} // namespace

BackendConfig::BackendConfig(Connection& connection, QObject* parent)
	: QObject(parent)
	, m_connection(connection)
	, m_backend(connection.backend())
{
	connect(&connection, &Connection::connected, this, &BackendConfig::refresh);
}

void BackendConfig::setCwd(const QString& cwd)
{
	if(cwd == m_cwd)
	{
		return;
	}
	m_cwd = cwd;
	if(m_connection.isConnected())
	{
		refresh();
	}
}

std::optional<QString> BackendConfig::optionalCwd() const
{
	return m_cwd.isEmpty() ? std::nullopt : std::optional<QString>(m_cwd);
}

QStringList BackendConfig::variantsOf(const QString& provider, const QString& modelId) const
{
	for(const proto::ModelInfo& model : m_config.models)
	{
		if(model.provider == provider && model.id == modelId)
		{
			return model.variants;
		}
	}
	return {};
}

void BackendConfig::refresh()
{
	load();
	loadMcp();
}

void BackendConfig::refreshMcp()
{
	loadMcp();
}

void BackendConfig::apply(proto::ServerConfig config)
{
	++m_configSerial;
	m_config = std::move(config);
	m_agents = toList(m_config.agents);
	m_models = toList(m_config.models);
	m_commands = toList(m_config.commands);
	emit changed();
}

QCoro::Task<client::Result<proto::ReloadConfigResult>> BackendConfig::reload(std::optional<QString> cwdOverride)
{
	const QPointer<BackendConfig> self(this);
	const std::optional<QString> targetCwd = cwdOverride ? std::move(cwdOverride) : optionalCwd();
	++m_reloadsInFlight;
	const auto result = co_await m_backend.client().reloadConfig(targetCwd);
	if(!self)
	{
		co_return result;
	}
	--m_reloadsInFlight;
	if(result)
	{
		apply(result->config);
		loadMcp();
	}
	co_return result;
}

QCoro::Task<> BackendConfig::load()
{
	const int serial = ++m_configSerial;
	const auto config = co_await m_backend.client().getConfig(optionalCwd());
	if(serial != m_configSerial)
	{
		co_return;
	}
	if(!config)
	{
		emit failed(config.error());
		co_return;
	}
	m_config = *config;
	m_agents = toList(m_config.agents);
	m_models = toList(m_config.models);
	m_commands = toList(m_config.commands);
	emit changed();
}

QCoro::Task<> BackendConfig::loadMcp()
{
	const int serial = ++m_mcpSerial;
	const auto servers = co_await m_backend.client().mcpStatus(optionalCwd());
	if(serial != m_mcpSerial)
	{
		co_return;
	}
	if(!servers)
	{
		emit failed(servers.error());
		co_return;
	}
	m_mcpServers = toList(*servers);
	emit mcpChanged();
}
} // namespace refgui::workspace
