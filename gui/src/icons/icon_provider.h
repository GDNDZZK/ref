#pragma once

#include <QQuickImageProvider>
#include <QString>

namespace refgui::icons
{

// What an image URL asks for: "image://icons/<name>-<grid>?color=<hex>".
struct IconRequest
{
	QString name; // File name without ".svg", e.g. "check-16".
	int grid = 0; // Logical size the icon is drawn for (16 or 20).
	QColor color;
};

// Parses the part of the URL after "image://icons/". The colour is hex with or without '#'
// (QML's Qt.color().toString() gives "#rrggbb" or "#aarrggbb"); it defaults to black.
// Returns false when the name does not end in a grid size.
bool parseIconRequest(const QString& id, IconRequest& request);

// The SVG text with every currentColor replaced by the colour (its alpha is ignored).
QByteArray colouredSvg(const QByteArray& svg, const QColor& color);

// Serves the line icons in the theme's colours. The icon is rasterised at the size the Image
// asks for (sourceSize = logical size * devicePixelRatio), so strokes land on device pixels.
class IconProvider : public QQuickImageProvider
{
public:
	IconProvider();

	QImage requestImage(const QString& id, QSize* size, const QSize& requestedSize) override;
};

} // namespace refgui::icons
