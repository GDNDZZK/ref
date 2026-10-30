#include "client/stop_source.h"

#include <algorithm>
#include <utility>

namespace refgui::client
{

StopSource::StopSource(QObject* parent)
	: QObject(parent)
{
}

void StopSource::request()
{
	m_stopped = true;
	// Resuming runs the sleepers to their end; take them out first.
	const QList<Sleeper> sleepers = std::exchange(m_sleepers, {});
	for(const Sleeper& sleeper : sleepers)
	{
		delete sleeper.timer;
		sleeper.handle.resume();
	}
}

void StopSource::addSleeper(std::coroutine_handle<> handle, std::chrono::milliseconds duration)
{
	auto* timer = new QTimer(this);
	timer->setSingleShot(true);
	connect(timer, &QTimer::timeout, this, [this, timer] { wake(timer); });
	m_sleepers.append({handle, timer});
	timer->start(duration);
}

void StopSource::wake(QTimer* timer)
{
	const auto sleeper = std::find_if(m_sleepers.begin(), m_sleepers.end(), [timer](const Sleeper& s) { return s.timer == timer; });
	if(sleeper == m_sleepers.end())
	{
		return;
	}
	const std::coroutine_handle<> handle = sleeper->handle;
	m_sleepers.erase(sleeper);
	timer->deleteLater();
	handle.resume();
}

} // namespace refgui::client
