#include "proto/parse.h"
#include "proto/parse_internal.h"

using namespace Qt::StringLiterals;

namespace refgui::proto
{
namespace json
{
namespace
{

QStringList readStrings(const Fields& fields, QLatin1StringView key)
{
	QStringList strings;
	const QJsonArray array = fields.arrayOrEmpty(key);
	for(qsizetype i = 0; i < array.size(); ++i)
	{
		if(!array.at(i).isString())
		{
			fail(fields.childPath(key) + u"[%1]"_s.arg(i), u"expected a string"_s);
		}
		strings.append(array.at(i).toString());
	}
	return strings;
}

AgentInfo readAgent(const Fields& fields)
{
	AgentInfo agent{.name = fields.string("name"_L1), .description = fields.string("description"_L1)};
	if(const std::optional<QJsonObject> model = fields.optionalObject("model"_L1))
	{
		agent.model = readModelRef(Fields(*model, fields.childPath("model"_L1)));
	}
	agent.creatable = fields.booleanOr("creatable"_L1, false);
	agent.canSpawn = fields.booleanOr("can_spawn"_L1, false);
	agent.color = fields.optionalString("color"_L1);
	return agent;
}

ModelInfo readModelInfo(const Fields& fields)
{
	return {.provider = fields.string("provider"_L1),
	        .providerName = fields.string("provider_name"_L1),
	        .id = fields.string("id"_L1),
	        .name = fields.string("name"_L1),
	        .context = static_cast<quint64>(fields.integerOr("context"_L1, 0)),
	        .output = static_cast<quint64>(fields.integerOr("output"_L1, 0)),
	        .images = fields.booleanOr("images"_L1, false),
	        .reasoning = fields.booleanOr("reasoning"_L1, false),
	        .variants = readStrings(fields, "variants"_L1)};
}

CommandInfo readCommand(const Fields& fields)
{
	return {.name = fields.string("name"_L1), .description = fields.string("description"_L1), .source = fields.string("source"_L1)};
}

McpServerStatus readMcpServer(const Fields& fields)
{
	return {.name = fields.string("name"_L1),
	        .status = fields.string("status"_L1),
	        .error = fields.optionalString("error"_L1),
	        .tools = readStrings(fields, "tools"_L1)};
}

// The status lines are [name, text] pairs.
QList<QPair<QString, QString>> readMcpLines(const Fields& fields)
{
	QList<QPair<QString, QString>> lines;
	const QJsonArray array = fields.arrayOrEmpty("mcp"_L1);
	for(qsizetype i = 0; i < array.size(); ++i)
	{
		const QJsonArray pair = array.at(i).toArray();
		const QString path = fields.childPath("mcp"_L1) + u"[%1]"_s.arg(i);
		if(pair.size() != 2 || !pair.at(0).isString() || !pair.at(1).isString())
		{
			fail(path, u"expected [name, text]"_s);
		}
		lines.append({pair.at(0).toString(), pair.at(1).toString()});
	}
	return lines;
}

ServerConfig readServerConfig(const Fields& fields)
{
	return {.agents = fields.list<AgentInfo>("agents"_L1, readAgent),
	        .defaultAgent = fields.string("default_agent"_L1),
	        .models = fields.list<ModelInfo>("models"_L1, readModelInfo),
	        .commands = fields.listOrEmpty<CommandInfo>("commands"_L1, readCommand),
	        .mcp = readMcpLines(fields)};
}

} // namespace
} // namespace json

Parsed<ServerConfig> parseConfigResult(const QJsonObject& result)
{
	return json::guarded([&] {
		const json::Fields fields(result, u"result"_s);
		return json::readServerConfig(json::Fields(fields.object("config"_L1), fields.childPath("config"_L1)));
	});
}

Parsed<QList<McpServerStatus>> parseMcpResult(const QJsonObject& result)
{
	return json::guarded([&] { return json::Fields(result, u"result"_s).list<McpServerStatus>("servers"_L1, json::readMcpServer); });
}

Parsed<DirListing> parseDirsResult(const QJsonObject& result)
{
	return json::guarded([&] {
		const json::Fields fields(result, u"result"_s);
		return DirListing{.path = fields.string("path"_L1), .dirs = json::readStrings(fields, "dirs"_L1)};
	});
}

Parsed<QString> parseTextResult(const QJsonObject& result)
{
	return json::guarded([&] { return json::Fields(result, u"result"_s).string("text"_L1); });
}

Parsed<QString> parsePathResult(const QJsonObject& result)
{
	return json::guarded([&] { return json::Fields(result, u"result"_s).string("path"_L1); });
}

} // namespace refgui::proto
