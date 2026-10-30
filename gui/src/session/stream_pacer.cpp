// Adapted from an early version of qutie by LightBlueCube
#include "session/stream_pacer.h"

#include <QTextBoundaryFinder>

#include <algorithm>
#include <utility>

using namespace std::chrono_literals;

namespace refgui::session
{
namespace
{

using Milliseconds = std::chrono::duration<double, std::milli>;

constexpr Milliseconds MinGap = 16ms;     // about a frame: anything faster shows as it arrives
constexpr Milliseconds MaxLag = 200ms;    // longest the shown text trails the stream
constexpr Milliseconds BurstWindow = 4ms; // events closer than this came in one network read
constexpr double GapSmoothing = 0.25;     // weight of the newest gap in the expected one

// Merges `chunk` into the last of `chunks` when it goes to the same target.
void appendMerged(QList<StreamPacer::Chunk>& chunks, const StreamPacer::Chunk& chunk)
{
	const bool continuesLast = !chunks.isEmpty() && chunks.last().sameTarget(chunk);
	if(!continuesLast)
	{
		chunks.append(chunk);
		return;
	}
	chunks.last().text += chunk.text;
}

// Length of the longest start of `text` that is at most `budget` code units long and
// ends between two graphemes.
qsizetype graphemePrefix(const QString& text, qsizetype budget)
{
	if(budget >= text.size())
	{
		return text.size();
	}
	QTextBoundaryFinder finder(QTextBoundaryFinder::Grapheme, text);
	finder.setPosition(budget);
	if(finder.isAtBoundary())
	{
		return budget;
	}
	return finder.toPreviousBoundary(); // the start of the text is a boundary
}

} // namespace

bool StreamPacer::Chunk::sameTarget(const Chunk& other) const
{
	return messageId == other.messageId && partIndex == other.partIndex && kind == other.kind;
}

void StreamPacer::push(const Chunk& chunk, Clock::time_point now)
{
	accrue(now);
	appendMerged(m_pending, chunk);
	m_backlog += chunk.text.size();

	// Only the gaps between network reads say when the next text is due. Events that came
	// in the same read would make it look like text arrives continuously.
	const bool sameRead = m_lastArrival && now - *m_lastArrival < BurstWindow;
	if(!sameRead)
	{
		if(m_lastArrival)
		{
			const Milliseconds gap = std::clamp(Milliseconds(now - *m_lastArrival), MinGap, MaxLag);
			m_gap = m_gap ? *m_gap + (gap - *m_gap) * GapSmoothing : gap;
		}
		m_lastArrival = now;
	}

	m_rate = static_cast<double>(m_backlog) / m_gap.value_or(MaxLag).count();
}

QList<StreamPacer::Chunk> StreamPacer::take(Clock::time_point now)
{
	accrue(now);

	QList<Chunk> due;
	while(!m_pending.isEmpty())
	{
		Chunk& next = m_pending.first();
		const qsizetype length = graphemePrefix(next.text, static_cast<qsizetype>(m_credit));
		if(length == 0)
		{
			break; // the next grapheme is not due yet
		}

		due.append({next.messageId, next.partIndex, next.kind, next.text.first(length)});
		next.text.remove(0, length);
		m_credit -= static_cast<double>(length);
		m_backlog -= length;
		if(!next.text.isEmpty())
		{
			break; // the rest of this chunk is not due yet
		}
		m_pending.removeFirst();
	}
	return due;
}

QList<StreamPacer::Chunk> StreamPacer::takeAll()
{
	m_backlog = 0;
	m_credit = 0;
	return std::exchange(m_pending, {});
}

bool StreamPacer::isEmpty() const
{
	return m_pending.isEmpty();
}

QString StreamPacer::pendingText(const QString& messageId, qsizetype partIndex) const
{
	QString text;
	for(const Chunk& chunk : m_pending)
	{
		if(chunk.messageId == messageId && chunk.partIndex == partIndex)
		{
			text += chunk.text;
		}
	}
	return text;
}

// Credit grows at the current rate. It never exceeds the backlog: credit banked while
// nothing waits would let the next arrival pop in at once.
void StreamPacer::accrue(Clock::time_point now)
{
	const Milliseconds elapsed = now - m_lastAccrual;
	m_lastAccrual = now;
	m_credit = std::min(m_credit + m_rate * elapsed.count(), static_cast<double>(m_backlog));
}

} // namespace refgui::session
