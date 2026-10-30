#pragma once

#include "proto/types.h"

#include <QByteArray>
#include <QString>

#include <expected>
#include <optional>

namespace refgui::compose
{

constexpr qint64 MaxImageBytes = 20 * 1024 * 1024;

// Whether `text` (a pasted or dropped string) names an image file, and the image if so.
// Accepts a plain path, `~/…`, a `file://` URL, quotes around it, and `\ ` for spaces, as
// the TUI does. png, jpg/jpeg, gif and webp up to 20 MiB are images.
//   nullopt  not an image path: treat the text as text
//   error    it names an image file that cannot be used (unreadable, too large)
std::expected<std::optional<proto::NamedImage>, QString> imageFromPath(const QString& text);

// Image bytes from memory, e.g. a screenshot on the clipboard encoded as PNG.
proto::NamedImage imageFromBytes(const QByteArray& bytes, const QString& mime, const QString& name);

} // namespace refgui::compose
