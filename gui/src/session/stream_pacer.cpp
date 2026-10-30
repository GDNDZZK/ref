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

qsizetype lengthOf(const llm::StreamEvent& event)
{
	return std::visit([](const auto& delta) { return delta.text.size(); }, event);
}

// Merges `event` into the last of `events` when it is of the same kind.
void appendMerged(QList<llm::StreamEvent>& events, const llm::StreamEvent& event)
{
	const bool continuesLast = !events.isEmpty() && events.last().index() == event.index();
	if(!continuesLast)
	{
		events.append(event);
		return;
	}
	std::visit([&event](auto& last) { last.text += std::get<std::decay_t<decltype(last)>>(event).text; }, events.last());
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

void StreamPacer::push(const llm::StreamEvent& event, Clock::time_point now)
{
	accrue(now);
	appendMerged(m_pending, event);
	m_backlog += lengthOf(event);

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

QList<llm::StreamEvent> StreamPacer::take(Clock::time_point now)
{
	accrue(now);

	QList<llm::StreamEvent> due;
	while(!m_pending.isEmpty())
	{
		const qsizetype length = std::visit(
			[this, &due](auto& delta) {
				const qsizetype prefix = graphemePrefix(delta.text, static_cast<qsizetype>(m_credit));
				if(prefix > 0)
				{
					due.append(std::decay_t<decltype(delta)>{delta.text.first(prefix)});
					delta.text.remove(0, prefix);
				}
				return prefix;
			},
			m_pending.first());
		if(length == 0)
		{
			break; // the next grapheme is not due yet
		}

		m_credit -= static_cast<double>(length);
		m_backlog -= length;
		if(lengthOf(m_pending.first()) > 0)
		{
			break; // the rest of this delta is not due yet
		}
		m_pending.removeFirst();
	}
	return due;
}

QList<llm::StreamEvent> StreamPacer::takeAll()
{
	m_backlog = 0;
	m_credit = 0;
	return std::exchange(m_pending, {});
}

bool StreamPacer::isEmpty() const
{
	return m_pending.isEmpty();
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
