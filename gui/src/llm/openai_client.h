#pragma once

#include "llm/message.h"

#include <QCoroTask>
#include <QList>
#include <QNetworkAccessManager>
#include <QNetworkRequest>

#include <functional>
#include <stop_token>

namespace qutie::llm
{

// Streams replies from an OpenAI-compatible Chat Completions endpoint.
class OpenAiClient
{
public:
	explicit OpenAiClient(Endpoint endpoint);

	// The reply to `history`. Each delta goes to `onEvent` as it arrives; the task
	// completes with the message as far as it got and how the reply ended.
	// Requesting `stop` ends the reply as Cancelled before request_stop() returns.
	//
	// Preconditions: the client outlives the task; `onEvent` does not request `stop`.
	QCoro::Task<Completion> complete(QList<Message> history, std::function<void(const StreamEvent&)> onEvent, std::stop_token stop);

private:
	QNetworkRequest request() const;

	Endpoint m_endpoint;
	QNetworkAccessManager m_network;
};

} // namespace qutie::llm
