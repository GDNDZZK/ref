#pragma once

#include "client/backend.h"

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
	Q_PROPERTY(QVariantList mcpServers READ mcpServers NOTIFY mcpChanged)

public:
	explicit BackendConfig(client::Backend& backend, QObject* parent = nullptr);

	// Agents as maps: name, description, model (the id of the agent's own, or empty), color
	// (empty for none), creatable, canSpawn.
	QVariantList agents() const { return m_agents; }
	// Models as maps: provider, providerName, id, name, context, images, reasoning, variants.
	QVariantList models() const { return m_models; }
	// User-defined commands as maps: name, description, source.
	QVariantList commands() const { return m_commands; }
	QString defaultAgent() const { return m_config.defaultAgent; }
	const proto::ServerConfig& config() const { return m_config; }
	// MCP servers as maps: name, status, error (empty for none), tools.
	QVariantList mcpServers() const { return m_mcpServers; }

	// The variants of a model, empty when it has none or is unknown.
	Q_INVOKABLE QStringList variantsOf(const QString& provider, const QString& modelId) const;
	// Asks the backend for the status of the MCP servers.
	Q_INVOKABLE void refreshMcp();

signals:
	void changed();
	void mcpChanged();
	// A request failed.
	void failed(const QString& reason);

private:
	QCoro::Task<> load();
	QCoro::Task<> loadMcp();

	client::Backend& m_backend;
	proto::ServerConfig m_config;
	QVariantList m_agents;
	QVariantList m_models;
	QVariantList m_commands;
	QVariantList m_mcpServers;
};

} // namespace refgui::workspace
