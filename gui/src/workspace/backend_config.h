#pragma once

#include "client/backend.h"
#include "workspace/connection.h"

#include <QCoroTask>
#include <QObject>
#include <QVariantList>

namespace refgui::workspace
{

// What the backend offers to choose from: agents, models, user-defined commands, and the
// status of the MCP servers. Loaded after each (re)connection.
class BackendConfig : public QObject
{
	Q_OBJECT
	Q_PROPERTY(QVariantList agents READ agents NOTIFY changed)
	Q_PROPERTY(QVariantList models READ models NOTIFY changed)
	Q_PROPERTY(QVariantList commands READ commands NOTIFY changed)
	Q_PROPERTY(QString defaultAgent READ defaultAgent NOTIFY changed)
	Q_PROPERTY(QStringList warnings READ warnings NOTIFY changed)
	Q_PROPERTY(QVariantList mcpServers READ mcpServers NOTIFY mcpChanged)
public:
	explicit BackendConfig(Connection& connection, QObject* parent = nullptr);

	// Agents as maps: name, description, model (the id of the agent's own, or empty), color
	// (empty for none), creatable, canSpawn.
	QVariantList agents() const { return m_agents; }
	// Models as maps: provider, providerName, id, name, context, images, reasoning, variants.
	QVariantList models() const { return m_models; }
	// User-defined commands as maps: name, description, source.
	QVariantList commands() const { return m_commands; }
	QString defaultAgent() const { return m_config.defaultAgent; }
	QStringList warnings() const { return m_config.warnings; }
	const proto::ServerConfig& config() const { return m_config; }
	const QString& cwd() const { return m_cwd; }
	void setCwd(const QString& cwd);
	// MCP servers as maps: name, status, error (empty for none), tools.
	QVariantList mcpServers() const { return m_mcpServers; }

	// The variants of a model, empty when it has none or is unknown.
	Q_INVOKABLE QStringList variantsOf(const QString& provider, const QString& modelId) const;
	// Reloads effective config and MCP status from the backend for the active cwd.
	Q_INVOKABLE void refresh();
	// Asks the backend for the status of the MCP servers.
	Q_INVOKABLE void refreshMcp();
	// Replaces the cached config directly (e.g. from ReloadConfig's response) and refreshes MCP.
	void apply(proto::ServerConfig config);
\t// Clears cached config and MCP status (e.g. on backend switch).
\tvoid reset();
	// True while a ReloadConfig request started here is awaiting its response.
	bool isReloading() const { return m_reloadsInFlight > 0; }
	// Runs ReloadConfig on the backend and updates this cache + MCP status.
	QCoro::Task<client::Result<proto::ReloadConfigResult>> reload(std::optional<QString> cwdOverride = {});
signals:
	void changed();
	void mcpChanged();
	// A request failed.
	void failed(const QString& reason);

private:
	std::optional<QString> optionalCwd() const;
	QCoro::Task<> load();
	QCoro::Task<> loadMcp();

	Connection& m_connection;
	client::Backend& m_backend;
	QString m_cwd;
	proto::ServerConfig m_config;
	QVariantList m_agents;
	QVariantList m_models;
	QVariantList m_commands;
	QVariantList m_mcpServers;
	int m_configSerial = 0;
	int m_mcpSerial = 0;
	int m_reloadsInFlight = 0;
};

} // namespace refgui::workspace
