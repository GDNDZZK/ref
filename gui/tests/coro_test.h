#pragma once

#include <QCoroTask>
#include <QEventLoop>
#include <QTimer>
#include <QtTest>

#include <chrono>
#include <optional>

namespace refgui::test
{

constexpr std::chrono::milliseconds AwaitTimeout{5000};

// Runs the event loop until `task` is done and returns its result. Fails the test
// when it takes longer than AwaitTimeout.
template<typename T>
std::optional<T> await(QCoro::Task<T> task)
{
	std::optional<T> result;
	QEventLoop loop;
	QTimer timeout;
	timeout.setSingleShot(true);
	QObject::connect(&timeout, &QTimer::timeout, &loop, &QEventLoop::quit);

	[](QCoro::Task<T> awaited, std::optional<T>& out, QEventLoop& waiting) -> QCoro::Task<> {
		out = co_await awaited;
		waiting.quit();
	}(std::move(task), result, loop);

	if(!result)
	{
		timeout.start(AwaitTimeout);
		loop.exec();
	}
	if(!result)
	{
		QTest::qFail("coroutine did not finish in time", __FILE__, __LINE__);
	}
	return result;
}

} // namespace refgui::test
