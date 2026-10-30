// Adapted from an early version of qutie by LightBlueCube
#pragma once

#include "proto/types.h"

#include <QList>

#include <chrono>
#include <optional>

namespace refgui::session
{

// Evens out the pace of a streamed reply. Servers send text in bursts: a token at a
// time, or a few hundred characters every couple of hundred milliseconds. Shown as it
// arrives, the second kind pops in a paragraph at a time.
//
// The pacer holds what arrived and lets it out at a steady rate: each arrival sets the
// rate so the backlog runs out just as the next arrival is due, judged by the recent
// gaps between arrivals. Shown text trails the stream by about one gap, at most MaxLag.
//
//   arrives   ####          ####          ####
//   shown     \____________ \____________ \____________   steady, one gap behind
//
// Releases end on grapheme boundaries, so an emoji or a combined character never shows
// half-made.
class StreamPacer
{
public:
	using Clock = std::chrono::steady_clock;

	// Text streamed into one part of a message. Chunks of the same part and kind merge.
	struct Chunk
	{
		QString messageId;
		qsizetype partIndex = 0;
		proto::Delta::Kind kind = proto::Delta::Kind::Text;
		QString text;

		bool sameTarget(const Chunk& other) const;
	};

	void push(const Chunk& chunk, Clock::time_point now);
	// What is due by `now`, in arrival order, adjacent chunks of a target merged.
	QList<Chunk> take(Clock::time_point now);
	QList<Chunk> takeAll();
	bool isEmpty() const;
	// Text of the part that was pushed but not taken yet.
	QString pendingText(const QString& messageId, qsizetype partIndex) const;

private:
	void accrue(Clock::time_point now);

	QList<Chunk> m_pending; // adjacent chunks of a target merged
	qsizetype m_backlog = 0;           // UTF-16 code units in m_pending
	std::optional<Clock::time_point> m_lastArrival;
	std::optional<std::chrono::duration<double, std::milli>> m_gap; // expected time between arrivals, once one was seen
	Clock::time_point m_lastAccrual;
	double m_rate = 0;   // code units per millisecond
	double m_credit = 0; // code units due but not taken yet
};

} // namespace refgui::session
