#include "llm/openai_client.h"

#include "infra/http_stream.h"
#include "infra/sse.h"
#include "llm/openai_wire.h"

#include <chrono>
#include <optional>
#include <utility>

using namespace Qt::StringLiterals;
using namespace std::chrono_literals;

namespace refgui::llm
{
namespace
{

// Aborts a reply when nothing arrives for this long. Generous: a local model can work
// through a long prompt for minutes before it sends the first token.
constexpr std::chrono::milliseconds IdleTimeout = 5min;

QString describe(const http::Error& error)
{
	if(error.status != 0)
	{
		return u"The server answered with HTTP %1: %2"_s.arg(error.status).arg(openai::errorMessage(error.message));
	}
	return u"The request failed: %1"_s.arg(error.message);
}

// What the stream said wins: a reply whose finish reason arrived is complete even if
// the connection then broke.
std::expected<StopReason, Error> ending(const std::optional<std::expected<StopReason, Error>>& outcome,
	const std::expected<void, http::Error>& transfer, const std::stop_token& stop)
{
	if(outcome)
	{
		return *outcome;
	}
	if(stop.stop_requested())
	{
		return StopReason::Cancelled;
	}
	if(!transfer)
	{
		return std::unexpected(Error{describe(transfer.error())});
	}
	return std::unexpected(Error{u"The connection closed before the reply was complete."_s});
}

} // namespace

OpenAiClient::OpenAiClient(Endpoint endpoint)
	: m_endpoint(std::move(endpoint))
{
}

QCoro::Task<Completion> OpenAiClient::complete(QList<Message> history, std::function<void(const StreamEvent&)> onEvent, std::stop_token stop)
{
	sse::Parser parser;
	openai::StreamDecoder decoder;
	const auto onChunk = [&](QByteArrayView chunk) {
		for(const sse::Event& event : parser.feed(chunk))
		{
			for(const StreamEvent& delta : decoder.decode(event.data))
			{
				onEvent(delta);
			}
		}
	};

	const std::expected<void, http::Error> transfer
		= co_await http::postStreaming(m_network, request(), openai::requestBody(m_endpoint.model, history), onChunk, stop);
	co_return Completion{.message = decoder.message(), .end = ending(decoder.outcome(), transfer, stop)};
}

QNetworkRequest OpenAiClient::request() const
{
	QUrl url = m_endpoint.baseUrl;
	const QString basePath = url.path().endsWith(u'/') ? url.path() : url.path() + u'/';
	url.setPath(basePath + u"chat/completions"_s);

	QNetworkRequest chatRequest(url);
	chatRequest.setRawHeader("Content-Type"_ba, "application/json"_ba);
	chatRequest.setRawHeader("Accept"_ba, "text/event-stream"_ba);
	if(!m_endpoint.apiKey.isEmpty())
	{
		chatRequest.setRawHeader("Authorization"_ba, "Bearer "_ba + m_endpoint.apiKey.toUtf8());
	}
	chatRequest.setTransferTimeout(IdleTimeout);
	return chatRequest;
}

} // namespace refgui::llm
