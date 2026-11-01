#pragma once

#include "proto/types.h"

#include <QList>
#include <QString>

#include <optional>

namespace refgui::proto
{

// How far a session's to-do list is, as the sidebar badge, the tile header and the status line say it.
// The counting is the terminal UI's (ui/todo.rs): a cancelled item is in neither number.
struct TodoProgress
{
	int done = 0; // completed items
	int total = 0; // all items but the cancelled ones
	// What is worked on: the item in progress, else the first pending one. None when nothing is left to do.
	std::optional<Todo> current{};

	bool isFinished() const { return total > 0 && !current; }
};

TodoProgress todoProgress(const QList<Todo>& todos);

// The status as the todo tool names it: pending, in_progress, completed or cancelled.
QString todoStatusName(TodoStatus status);

} // namespace refgui::proto
