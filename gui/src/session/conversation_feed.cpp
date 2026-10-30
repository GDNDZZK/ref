#include "session/conversation_feed.h"

#include <chrono>

using namespace std::chrono_literals;

namespace refgui::session
{
namespace
{

constexpr std::chrono::milliseconds TickInterval = 16ms; // about a frame
// After this long without text, the pace of the last stream says nothing about the next.
constexpr std::chrono::milliseconds PaceMemory = 1s;

// The text of a text or reasoning part, null for the other kinds.
const QString* textOf(const proto::Part& part)
{
	if(const auto* text = std::get_if<proto::TextPart>(&part))
	{
		return &text->text;
	}
	if(const auto* reasoning = std::get_if<proto::ReasoningPart>(&part))
	{
		return &reasoning->text;
	}
	return nullptr;
}

QString* textOf(proto::Part& part)
{
	return const_cast<QString*>(textOf(std::as_const(part)));
}

proto::Delta::Kind deltaKindOf(const proto::Part& part)
{
	return std::holds_alternative<proto::ReasoningPart>(part) ? proto::Delta::Kind::Reasoning : proto::Delta::Kind::Text;
}

} // namespace

ConversationFeed::ConversationFeed(ConversationModel& model, QObject* parent)
	: QObject(parent)
	, m_model(model)
{
	m_ticker.setTimerType(Qt::PreciseTimer);
	m_ticker.setInterval(TickInterval);
	connect(&m_ticker, &QTimer::timeout, this, &ConversationFeed::tick);
}

void ConversationFeed::reset(const QList<proto::Message>& messages)
{
	const bool wasPending = hasPending();
	m_pacer = {};
	m_ticker.stop();
	m_model.reset(messages);
	updatePending(wasPending);
}

void ConversationFeed::apply(const proto::SessionEvent& event)
{
	if(const auto* message = std::get_if<proto::MessageEvent>(&event))
	{
		applyMessage(message->message);
	}
	else if(const auto* part = std::get_if<proto::PartEvent>(&event))
	{
		applyPart(*part);
	}
	else if(const auto* delta = std::get_if<proto::DeltaEvent>(&event))
	{
		applyDelta(*delta);
	}
	else if(const auto* removed = std::get_if<proto::RemovedEvent>(&event))
	{
		flush();
		m_model.removeMessage(removed->messageId);
	}
	else if(const auto* reset = std::get_if<proto::ResetEvent>(&event))
	{
		this->reset(reset->messages);
	}
}

void ConversationFeed::flush()
{
	const bool wasPending = hasPending();
	show(m_pacer.takeAll());
	m_ticker.stop();
	updatePending(wasPending);
}

bool ConversationFeed::hasPending() const
{
	return !m_pacer.isEmpty();
}

void ConversationFeed::applyMessage(proto::Message message)
{
	const qsizetype known = m_model.partCount(message.id);
	bool needsFlush = hasPending() && message.parts.size() != known;
	for(qsizetype i = 0; !needsFlush && hasPending() && i < message.parts.size() && i < known; ++i)
	{
		needsFlush = fitOf(message.id, i, message.parts.at(i)) == Fit::Incompatible;
	}
	if(needsFlush)
	{
		flush();
	}

	QList<StreamPacer::Chunk> deferred;
	for(qsizetype i = 0; i < message.parts.size() && i < known; ++i)
	{
		if(const auto chunk = deferText(message.id, i, message.parts[i]))
		{
			deferred.append(*chunk);
		}
	}
	m_model.setMessage(message);
	for(const StreamPacer::Chunk& chunk : deferred)
	{
		push(chunk);
	}
}

void ConversationFeed::applyPart(const proto::PartEvent& event)
{
	const qsizetype known = m_model.partCount(event.messageId);
	if(known < 0)
	{
		return;
	}

	const bool replacesPart = event.index < known;
	if(hasPending() && (!replacesPart || fitOf(event.messageId, event.index, event.part) == Fit::Incompatible))
	{
		flush();
	}

	proto::Part part = event.part;
	std::optional<StreamPacer::Chunk> deferred;
	if(replacesPart)
	{
		deferred = deferText(event.messageId, event.index, part);
	}
	m_model.setPart(event.messageId, event.index, part);
	if(deferred)
	{
		push(*deferred);
	}
}

void ConversationFeed::applyDelta(const proto::DeltaEvent& event)
{
	const qsizetype known = m_model.partCount(event.messageId);
	if(known < 0)
	{
		return;
	}

	const bool createsPart = event.index >= known;
	if(createsPart)
	{
		flush(); // the new part must not appear before text that is still waiting
	}
	if(event.delta.kind == proto::Delta::Kind::ToolInput)
	{
		m_model.appendDelta(event.messageId, event.index, event.delta);
		return;
	}

	if(createsPart)
	{
		m_model.appendDelta(event.messageId, event.index, {event.delta.kind, QString()});
	}
	push({event.messageId, event.index, event.delta.kind, event.delta.text});
}

ConversationFeed::Fit ConversationFeed::fitOf(const QString& messageId, qsizetype index, const proto::Part& part) const
{
	const std::optional<QString> shown = m_model.partText(messageId, index);
	const QString* incoming = textOf(part);
	if(!shown || !incoming)
	{
		return m_pacer.pendingText(messageId, index).isEmpty() ? Fit::Equal : Fit::Incompatible;
	}

	const QString known = *shown + m_pacer.pendingText(messageId, index);
	if(*incoming == known)
	{
		return Fit::Equal;
	}
	return incoming->startsWith(known) ? Fit::Extends : Fit::Incompatible;
}

std::optional<StreamPacer::Chunk> ConversationFeed::deferText(const QString& messageId, qsizetype index, proto::Part& part) const
{
	QString* incoming = textOf(part);
	const std::optional<QString> shown = m_model.partText(messageId, index);
	if(!incoming || !shown || fitOf(messageId, index, part) == Fit::Incompatible)
	{
		return std::nullopt;
	}

	const qsizetype knownSize = shown->size() + m_pacer.pendingText(messageId, index).size();
	StreamPacer::Chunk chunk{messageId, index, deltaKindOf(part), incoming->mid(knownSize)};
	*incoming = *shown;
	if(chunk.text.isEmpty())
	{
		return std::nullopt;
	}
	return chunk;
}

void ConversationFeed::push(const StreamPacer::Chunk& chunk)
{
	const bool wasPending = hasPending();
	const auto now = StreamPacer::Clock::now();
	if(!wasPending && now - m_lastPush > PaceMemory)
	{
		m_pacer = {};
	}
	m_lastPush = now;
	m_pacer.push(chunk, now);
	m_ticker.start();
	updatePending(wasPending);
}

void ConversationFeed::tick()
{
	const bool wasPending = hasPending();
	show(m_pacer.take(StreamPacer::Clock::now()));
	if(!hasPending())
	{
		m_ticker.stop();
	}
	updatePending(wasPending);
}

void ConversationFeed::show(const QList<StreamPacer::Chunk>& chunks)
{
	for(const StreamPacer::Chunk& chunk : chunks)
	{
		m_model.appendDelta(chunk.messageId, chunk.partIndex, {chunk.kind, chunk.text});
	}
}

void ConversationFeed::updatePending(bool before)
{
	if(hasPending() != before)
	{
		emit pendingChanged();
	}
}

} // namespace refgui::session
