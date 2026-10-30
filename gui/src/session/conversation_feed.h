#pragma once

#include "proto/types.h"
#include "session/conversation_model.h"
#include "session/stream_pacer.h"

#include <QObject>
#include <QTimer>

namespace refgui::session
{

// Applies session events to a ConversationModel and paces the streamed text.
//
// Deltas of text and reasoning go through a StreamPacer and reach the model a frame at
// a time. What the server has said ("truth") is the shown text plus what the pacer
// still holds. When a whole message or part arrives, only the text beyond that is
// queued; everything else in it is shown at once. If the new text does not continue
// the old, or the event changes the structure (a new part, a removal), the pacer is
// flushed first so that nothing appears out of order.
class ConversationFeed : public QObject
{
	Q_OBJECT

public:
	explicit ConversationFeed(ConversationModel& model, QObject* parent = nullptr);

	// Loads the history of another session and drops whatever was waiting to be shown.
	void reset(const QList<proto::Message>& messages);
	// Handles message, part, delta, removal and reset events; ignores the others.
	void apply(const proto::SessionEvent& event);
	// Shows all waiting text now.
	void flush();
	// True while streamed text is waiting to be shown.
	bool hasPending() const;

signals:
	void pendingChanged();

private:
	void applyMessage(proto::Message message);
	void applyPart(const proto::PartEvent& event);
	void applyDelta(const proto::DeltaEvent& event);
	// Drops what was waiting to be shown and installs the history with `install` of the model.
	void replace(const QList<proto::Message>& messages, void (ConversationModel::*install)(const QList<proto::Message>&));

	enum class Fit
	{
		Equal,        // same text as what is known
		Extends,      // starts with what is known
		Incompatible, // anything else
	};
	Fit fitOf(const QString& messageId, qsizetype index, const proto::Part& part) const;
	// Cuts the text of `part` down to what is shown and returns the rest for the pacer.
	std::optional<StreamPacer::Chunk> deferText(const QString& messageId, qsizetype index, proto::Part& part) const;

	void push(const StreamPacer::Chunk& chunk);
	void tick();
	void show(const QList<StreamPacer::Chunk>& chunks);
	void updatePending(bool before);

	ConversationModel& m_model;
	StreamPacer m_pacer;
	QTimer m_ticker;
	StreamPacer::Clock::time_point m_lastPush;
};

} // namespace refgui::session
