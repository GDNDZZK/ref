#include "llm/openai_wire.h"

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonValue>

#include <array>

using namespace Qt::StringLiterals;

namespace refgui::llm::openai
{
namespace
{

// Fields servers stream reasoning in; none of them is in OpenAI's schema. Some servers
// send the same text in more than one, so only the first non-empty one counts.
constexpr std::array ReasoningFields{"reasoning_content"_L1, "reasoning"_L1, "reasoning_text"_L1};

constexpr qsizetype MaxQuotedLength = 500; // characters of server text quoted in an error

QString shortened(const QString& text)
{
	if(text.size() <= MaxQuotedLength)
	{
		return text;
	}
	return text.first(MaxQuotedLength) + u'…';
}

// OpenAI sends {"error": {"message": ..., "type": ..., "code": ...}}; some servers
// send {"error": "..."} instead.
QString errorText(const QJsonValue& error)
{
	if(error.isString())
	{
		return error.toString();
	}
	const QJsonObject object = error.toObject();
	const QString message = object.value("message"_L1).toString();
	if(!message.isEmpty())
	{
		return message;
	}
	return shortened(QString::fromUtf8(QJsonDocument(object).toJson(QJsonDocument::Compact)));
}

std::expected<StopReason, Error> stopReason(const QString& finishReason)
{
	if(finishReason == "stop"_L1 || finishReason == "end"_L1) // "end": some servers
	{
		return StopReason::Stop;
	}
	if(finishReason == "length"_L1)
	{
		return StopReason::Length;
	}
	if(finishReason == "content_filter"_L1)
	{
		return StopReason::ContentFilter;
	}
	// Including "tool_calls": no tools are offered yet.
	return std::unexpected(Error{u"The reply ended for a reason qutie does not support yet: \"%1\"."_s.arg(finishReason)});
}

struct Reasoning
{
	QLatin1StringView field;
	QString text;
};

std::optional<Reasoning> reasoningIn(const QJsonObject& delta)
{
	for(const QLatin1StringView field : ReasoningFields)
	{
		const QString text = delta.value(field).toString();
		if(!text.isEmpty())
		{
			return Reasoning{.field = field, .text = text};
		}
	}
	return std::nullopt;
}

QJsonObject toJson(const UserMessage& message)
{
	return {{u"role"_s, u"user"_s}, {u"content"_s, message.text}};
}

// Reasoning goes back in the field it came in. Servers that do not want it ignore it.
QJsonObject toJson(const AssistantMessage& message)
{
	QJsonObject json{{u"role"_s, u"assistant"_s}, {u"content"_s, message.text}};
	if(!message.reasoning.isEmpty())
	{
		json.insert(message.reasoningField, message.reasoning);
	}
	return json;
}

} // namespace

QByteArray requestBody(const QString& model, const QList<Message>& history)
{
	QJsonArray messages;
	for(const Message& message : history)
	{
		if(const auto* user = std::get_if<UserMessage>(&message))
		{
			messages.append(toJson(*user));
			continue;
		}
		const auto& assistant = std::get<AssistantMessage>(message);
		if(!assistant.text.isEmpty())
		{
			messages.append(toJson(assistant));
		}
	}

	const QJsonObject body{{u"model"_s, model}, {u"messages"_s, messages}, {u"stream"_s, true}};
	return QJsonDocument(body).toJson(QJsonDocument::Compact);
}

QString errorMessage(const QString& body)
{
	const QJsonObject json = QJsonDocument::fromJson(body.toUtf8()).object(); // empty unless an object
	if(json.contains("error"_L1))
	{
		return errorText(json.value("error"_L1));
	}
	return shortened(body.trimmed());
}

QList<StreamEvent> StreamDecoder::decode(QByteArrayView data)
{
	if(m_outcome || data == QByteArrayView("[DONE]"))
	{
		return {};
	}

	const QJsonDocument document = QJsonDocument::fromJson(data.toByteArray());
	if(!document.isObject())
	{
		m_outcome = std::unexpected(Error{u"The server sent data that is not a JSON object: %1"_s.arg(shortened(QString::fromUtf8(data)))});
		return {};
	}
	const QJsonObject chunk = document.object();
	if(chunk.contains("error"_L1))
	{
		m_outcome = std::unexpected(Error{u"The server reported an error: %1"_s.arg(errorText(chunk.value("error"_L1)))});
		return {};
	}

	// One choice is requested. The usage chunk at the end has none.
	const QJsonArray choices = chunk.value("choices"_L1).toArray();
	if(choices.isEmpty())
	{
		return {};
	}
	const QJsonObject choice = choices.first().toObject();
	const QJsonObject delta = choice.value("delta"_L1).toObject();

	QList<StreamEvent> events;
	if(const std::optional<Reasoning> reasoning = reasoningIn(delta))
	{
		if(m_message.reasoningField.isEmpty())
		{
			m_message.reasoningField = reasoning->field;
		}
		m_message.reasoning += reasoning->text;
		events.append(ReasoningDelta{reasoning->text});
	}

	const QString text = delta.value("content"_L1).toString(); // null while the model reasons
	if(!text.isEmpty())
	{
		m_message.text += text;
		events.append(TextDelta{text});
	}

	const QJsonValue finishReason = choice.value("finish_reason"_L1); // null until the last chunk
	if(finishReason.isString())
	{
		m_outcome = stopReason(finishReason.toString());
	}
	return events;
}

const AssistantMessage& StreamDecoder::message() const
{
	return m_message;
}

const std::optional<std::expected<StopReason, Error>>& StreamDecoder::outcome() const
{
	return m_outcome;
}

} // namespace refgui::llm::openai
