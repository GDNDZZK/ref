#pragma once

#include <QList>
#include <QObject>
#include <QTimer>

#include <chrono>
#include <coroutine>

namespace refgui::client
{

// Lets coroutines that wait be told to stop. A coroutine that nobody resumes is never freed, so a
// wait that can outlive its owner (a pause, a poll) is a `sleep()` here, and the owner calls
// `request()` before it goes: every sleeper wakes up at once and finds `stopped()` set, leaves
// its loop and returns, so its frame is freed.
class StopSource : public QObject
{
public:
	explicit StopSource(QObject* parent = nullptr);

	bool stopped() const { return m_stopped; }

	// Wakes every sleeper, now and in the future (a sleep after this returns at once).
	void request();

	struct SleepAwaiter
	{
		StopSource& source;
		std::chrono::milliseconds duration;

		bool await_ready() const noexcept { return source.m_stopped; }
		void await_suspend(std::coroutine_handle<> waiter) { source.addSleeper(waiter, duration); }
		void await_resume() const noexcept {}
	};

	// `co_await stop.sleep(100ms)` waits that long, or until request().
	SleepAwaiter sleep(std::chrono::milliseconds duration) { return {*this, duration}; }

private:
	struct Sleeper
	{
		std::coroutine_handle<> handle;
		QTimer* timer;
	};

	void addSleeper(std::coroutine_handle<> handle, std::chrono::milliseconds duration);
	void wake(QTimer* timer);

	QList<Sleeper> m_sleepers;
	bool m_stopped = false;
};

} // namespace refgui::client
