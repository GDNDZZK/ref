#pragma once

#include <QString>

namespace refgui::session
{

// Markdown from the model or from a command, made safe to hand to a Qt text item that renders it.
// Rendering must not fetch anything or interpret raw HTML: an image `![alt](src)` becomes the link
// `[alt](src)`, and a `<` that opens a tag or comment is escaped so that it shows as text. Code
// is left as it is: spans, fenced blocks (``` and ~~~, also one that is not closed yet) and
// indented blocks.
QString markdownForDisplay(const QString& markdown);

} // namespace refgui::session
