#include "proto/todo_progress.h"

#include <QtTest>

using namespace Qt::StringLiterals;
using namespace refgui::proto;

namespace
{

Todo todo(const QString& content, TodoStatus status)
{
	return {.content = content, .status = status};
}

} // namespace

class TestTodoProgress : public QObject
{
	Q_OBJECT

private slots:
	void nothingToDoIsNotFinished()
	{
		const TodoProgress progress = todoProgress({});

		QCOMPARE(progress.done, 0);
		QCOMPARE(progress.total, 0);
		QVERIFY(!progress.current);
		QVERIFY(!progress.isFinished());
	}

	void theItemInProgressIsTheCurrentOne()
	{
		const TodoProgress progress = todoProgress({todo(u"a"_s, TodoStatus::Completed),
		                                            todo(u"b"_s, TodoStatus::Pending),
		                                            todo(u"c"_s, TodoStatus::InProgress),
		                                            todo(u"d"_s, TodoStatus::Pending)});

		QCOMPARE(progress.done, 1);
		QCOMPARE(progress.total, 4);
		QVERIFY(progress.current);
		QCOMPARE(progress.current->content, u"c"_s);
		QVERIFY(!progress.isFinished());
	}

	void withoutOneInProgressTheNextPendingIsCurrent()
	{
		const TodoProgress progress = todoProgress({todo(u"a"_s, TodoStatus::Completed), todo(u"b"_s, TodoStatus::Pending), todo(u"c"_s, TodoStatus::Pending)});

		QVERIFY(progress.current);
		QCOMPARE(progress.current->content, u"b"_s);
		QCOMPARE(progress.current->status, TodoStatus::Pending);
	}

	void cancelledItemsAreInNeitherNumber()
	{
		const TodoProgress progress = todoProgress({todo(u"a"_s, TodoStatus::Completed),
		                                            todo(u"b"_s, TodoStatus::Cancelled),
		                                            todo(u"c"_s, TodoStatus::Cancelled),
		                                            todo(u"d"_s, TodoStatus::Pending)});

		QCOMPARE(progress.done, 1);
		QCOMPARE(progress.total, 2);
		QCOMPARE(progress.current->content, u"d"_s);
	}

	void aCancelledItemIsNeverCurrent()
	{
		const TodoProgress progress = todoProgress({todo(u"a"_s, TodoStatus::Cancelled), todo(u"b"_s, TodoStatus::Completed)});

		QVERIFY(!progress.current);
		QVERIFY(progress.isFinished());
	}

	void allDoneIsFinished()
	{
		const TodoProgress progress = todoProgress({todo(u"a"_s, TodoStatus::Completed), todo(u"b"_s, TodoStatus::Completed)});

		QCOMPARE(progress.done, 2);
		QCOMPARE(progress.total, 2);
		QVERIFY(!progress.current);
		QVERIFY(progress.isFinished());
	}

	void onlyCancelledItemsAreNoProgress()
	{
		const TodoProgress progress = todoProgress({todo(u"a"_s, TodoStatus::Cancelled)});

		QCOMPARE(progress.total, 0);
		QVERIFY(!progress.isFinished());
	}

	void statusNamesAreTheTodoToolsOwn()
	{
		QCOMPARE(todoStatusName(TodoStatus::Pending), u"pending"_s);
		QCOMPARE(todoStatusName(TodoStatus::InProgress), u"in_progress"_s);
		QCOMPARE(todoStatusName(TodoStatus::Completed), u"completed"_s);
		QCOMPARE(todoStatusName(TodoStatus::Cancelled), u"cancelled"_s);
	}
};

QTEST_GUILESS_MAIN(TestTodoProgress)
#include "tst_todo_progress.moc"
