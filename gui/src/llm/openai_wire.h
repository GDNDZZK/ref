#pragma once

#include "llm/message.h"

#include <QByteArray>
#include <QByteArrayView>
#include <QList>
#include <QString>

#include <expected>
#include <optional>

// The Chat Completions wire format (OpenAI's openapi.yaml: CreateChatCompletionRequest,
// CreateChatCompletionStreamResponse), with no I/O, so it can be tested on recorded chunks.
namespace qutie::llm::openai
{

// The body of a streaming request for the reply to `history`.
// Assistant messages without text are left out: servers reject an assistant message
// with empty content, and a reply stopped while the model was still reasoning has none.
QByteArray requestBody(const QString& model, const QList<Message>& history);

// The message in an error response `body`: `error.message` if it is OpenAI's error
// object, else the body itself, shortened.
QString errorMessage(const QString& body);

// Decodes the `data` of each server-sent event of a reply and assembles the message.
//
//   data: {"choices":[{"delta":{"reasoning_content":"Hm"}}]}   -> ReasoningDelta "Hm"
//   data: {"choices":[{"delta":{"content":"Hi"}}]}             -> TextDelta "Hi"
//   data: {"choices":[{"delta":{},"finish_reason":"stop"}]}    -> outcome Stop
//   data: [DONE]
class StreamDecoder
{
public:
	// The deltas in one event. Once the outcome is known, later events are ignored:
	// after the finish reason only usage and [DONE] follow, and an error ends the reply.
	QList<StreamEvent> decode(QByteArrayView data);

	const AssistantMessage& message() const;
	// How the stream said the reply ended, once it has: a finish reason, or an error
	// that it reported or that made it undecodable.
	const std::optional<std::expected<StopReason, Error>>& outcome() const;

private:
	AssistantMessage m_message;
	std::optional<std::expected<StopReason, Error>> m_outcome;
};

} // namespace qutie::llm::openai
