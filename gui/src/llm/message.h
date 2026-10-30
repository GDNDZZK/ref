#pragma once

#include <QString>
#include <QUrl>

#include <expected>
#include <variant>

namespace qutie::llm
{

struct UserMessage
{
	QString text;
};

// Default initializers keep Clang's -Wmissing-field-initializers quiet when
// designated initialization skips fields.
struct AssistantMessage
{
	QString text{};
	QString reasoning{};
	// The delta field the reasoning arrived in; set whenever `reasoning` is not empty.
	// No such field is in OpenAI's schema: servers that stream reasoning each pick a
	// name and expect it back in that field.
	QString reasoningField{};
};

using Message = std::variant<UserMessage, AssistantMessage>;

// Pieces of a reply, in the order they stream in.
struct TextDelta
{
	QString text;
};
struct ReasoningDelta
{
	QString text;
};
using StreamEvent = std::variant<TextDelta, ReasoningDelta>;

enum class StopReason
{
	Stop,          // the model finished its reply
	Length,        // cut off at the model's output limit
	ContentFilter, // stopped by the provider's filter
	Cancelled,     // stopped on request
};

struct Error
{
	QString message; // for the user
};

struct Completion
{
	AssistantMessage message; // as far as it got, also when the reply failed or was cancelled
	std::expected<StopReason, Error> end;
};

// An OpenAI-compatible server and the model to use there.
struct Endpoint
{
	QUrl baseUrl; // up to and including the API version, e.g. http://localhost:8080/v1
	QString model;
	QString apiKey{}; // sent as a bearer token unless empty
};

} // namespace qutie::llm
