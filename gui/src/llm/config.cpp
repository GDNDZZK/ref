#include "llm/config.h"

#include "infra/jsonc.h"

#include <QFile>
#include <QJsonObject>
#include <QStandardPaths>

using namespace Qt::StringLiterals;

namespace qutie::llm
{
namespace
{

constexpr auto Example = R"({
  "endpoint": {
    "baseUrl": "http://localhost:8080/v1",
    "model": "your-model",
    "apiKey": "sk-..." // optional
  }
})"_L1;

} // namespace

QString configPath()
{
	return QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation) + u"/qutie/config.jsonc"_s;
}

std::expected<Endpoint, QString> loadEndpoint(const QString& path)
{
	QFile file(path);
	if(!file.exists())
	{
		return std::unexpected(u"No configuration file at %1. Create it like this:\n\n%2"_s.arg(path, Example));
	}
	if(!file.open(QIODevice::ReadOnly))
	{
		return std::unexpected(u"Cannot read %1: %2"_s.arg(path, file.errorString()));
	}

	std::expected<Endpoint, QString> endpoint = parseEndpoint(file.readAll());
	if(!endpoint)
	{
		return std::unexpected(u"%1: %2"_s.arg(path, endpoint.error()));
	}
	return endpoint;
}

std::expected<Endpoint, QString> parseEndpoint(QByteArrayView source)
{
	const std::expected<QJsonDocument, jsonc::ParseError> document = jsonc::parse(source);
	if(!document)
	{
		const jsonc::ParseError& error = document.error();
		return std::unexpected(u"line %1, column %2: %3"_s.arg(error.line).arg(error.column).arg(error.message));
	}

	const QJsonObject endpoint = document->object().value("endpoint"_L1).toObject();
	const QString baseUrl = endpoint.value("baseUrl"_L1).toString();
	const QString model = endpoint.value("model"_L1).toString();
	if(baseUrl.isEmpty())
	{
		return std::unexpected(u"\"endpoint.baseUrl\" is missing."_s);
	}
	if(model.isEmpty())
	{
		return std::unexpected(u"\"endpoint.model\" is missing."_s);
	}

	const QUrl url(baseUrl, QUrl::StrictMode);
	const bool isHttp = url.isValid() && (url.scheme() == "http"_L1 || url.scheme() == "https"_L1);
	if(!isHttp)
	{
		return std::unexpected(u"\"endpoint.baseUrl\" is not an http or https URL: %1"_s.arg(baseUrl));
	}

	return Endpoint{.baseUrl = url, .model = model, .apiKey = endpoint.value("apiKey"_L1).toString()};
}

} // namespace qutie::llm
