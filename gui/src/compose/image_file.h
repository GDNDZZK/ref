#pragma once

#include "proto/types.h"

#include <QByteArray>
#include <QString>
#include <QUrl>

#include <expected>
#include <optional>

namespace refgui::compose
{

constexpr qint64 MaxImageMiB = 20;
constexpr qint64 MaxImageBytes = MaxImageMiB * 1024 * 1024;

// An image, nothing (not an image file) or why the image file cannot be used.
using ImageResult = std::expected<std::optional<proto::NamedImage>, QString>;

// Whether `text` (a pasted or dropped string) names an image file, and the image if so.
// Accepts a plain path, `~/…`, a `file://` URL, quotes around it, and `\ ` for spaces, as
// the TUI does. png, jpg/jpeg, gif and webp up to MaxImageMiB are images.
//   nullopt  not an image path: treat the text as text
//   error    it names an image file that cannot be used (unreadable, too large)
ImageResult imageFromPath(const QString& text);

// The same for a URL, e.g. a dropped file: only local files can be images.
ImageResult imageFromUrl(const QUrl& url);

// Image bytes from memory, e.g. a screenshot on the clipboard encoded as PNG.
proto::NamedImage imageFromBytes(const QByteArray& bytes, const QString& mime, const QString& name);

} // namespace refgui::compose
