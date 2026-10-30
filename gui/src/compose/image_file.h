#pragma once

#include "compose/prompt_draft.h"

#include <QByteArray>
#include <QString>

#include <expected>
#include <optional>

namespace refgui::compose
{

// Whether `text` (a pasted or dropped string) names an image file, and the image if so.
// Accepts a plain path, `~/…`, a `file://` URL, quotes around it, and `\ ` for spaces, as
// the TUI does. png, jpg/jpeg, gif and webp up to 20 MiB are images.
//   nullopt  not an image path: treat the text as text
//   error    it names an image file that cannot be used (unreadable, too large)
std::expected<std::optional<NamedImage>, QString> imageFromPath(const QString& text);

// Image bytes from memory, e.g. a screenshot on the clipboard encoded as PNG.
NamedImage imageFromBytes(const QByteArray& bytes, const QString& mime, const QString& name);

} // namespace refgui::compose
