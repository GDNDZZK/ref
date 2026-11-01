#include "proto/todo_progress.h"

#include <algorithm>

using namespace Qt::StringLiterals;

namespace refgui::proto
{

TodoProgress todoProgress(const QList<Todo>& todos)
{
	TodoProgress progress;
	for(const Todo& todo : todos)
	{
		progress.total += todo.status == TodoStatus::Cancelled ? 0 : 1;
		progress.done += todo.status == TodoStatus::Completed ? 1 : 0;
	}
	const auto isStatus = [](TodoStatus status) { return [status](const Todo& todo) { return todo.status == status; }; };
	auto current = std::ranges::find_if(todos, isStatus(TodoStatus::InProgress));
	if(current == todos.end())
	{
		current = std::ranges::find_if(todos, isStatus(TodoStatus::Pending));
	}
	if(current != todos.end())
	{
		progress.current = *current;
	}
	return progress;
}

QString todoStatusName(TodoStatus status)
{
	switch(status)
	{
	case TodoStatus::Pending:
		return u"pending"_s;
	case TodoStatus::InProgress:
		return u"in_progress"_s;
	case TodoStatus::Completed:
		return u"completed"_s;
	case TodoStatus::Cancelled:
		return u"cancelled"_s;
	}
	Q_UNREACHABLE_RETURN(QString());
}

} // namespace refgui::proto
