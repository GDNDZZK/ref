#pragma once

#include <QString>
#include <QStringList>
#include <QVariantList>
#include <QVariantMap>

namespace refgui::session::tool_content
{

// What a tool card shows, read out of the call. The formats are the ones the tools write; text that
// does not parse yields empty results, and the card falls back to the plain input and output.

// The icon (a name in the icon set) of a tool. MCP tools are named after their server.
QString iconName(const QString& toolName);

// The arguments of a call as an object; empty for text that is not a JSON object (yet).
QVariantMap parseInput(const QString& inputJson);

// The items of a `todo` call as [{content, status}]. Entries without content are skipped; a status
// other than in_progress, completed and cancelled reads as pending.
QVariantList todos(const QVariantMap& input);

// One line from the input that tells calls of the same tool apart: command, path, pattern, url,
// recipient, progress. Empty when the tool has none.
QString detail(const QString& toolName, const QVariantMap& input);

// Output of read and grep: `[path#TAG]` headers, then rows `N:text`, `*N:text` (a match) and
// ` N:text` (context), `...` between non-adjacent ones. Returns [{path, tag, rows, notes}] where
// rows are [{number, text, kind}] (kind "line", "match", "context" or "gap") and notes are the
// other lines.
QVariantList listing(const QString& output);

// A unified diff as rows [{text, sign, kind}]; kind is "added", "removed", "hunk", "file" or
// "context", and sign is "+", "-" or empty.
QVariantList diffRows(const QString& diff);

// "src/" and "parser.rs" of "src/parser.rs": {directory, name, isDirectory}. The name of a directory
// keeps its trailing slash.
QVariantMap splitPath(const QString& path);

// "512 B", "1.5 KiB" or "2.0 MiB".
QString sizeText(qint64 bytes);

QString firstLine(const QString& text);

// The first of `keys` that holds non-empty text in `input`, else empty.
QString firstText(const QVariantMap& input, const QStringList& keys);

} // namespace refgui::session::tool_content
