#include "icons/app_icon.h"

#include <QPainter>
#include <QPixmap>
#include <QString>
#include <QSvgRenderer>

using namespace Qt::StringLiterals;

namespace refgui::icons
{

namespace
{

// Compiled in from assets/app by src/icons/CMakeLists.txt.
const QString SmallMarkPath = u":/app/ref-small.svg"_s;
const QString LargeMarkPath = u":/app/ref.svg"_s;

} // namespace

QImage appIconImage(int pixels)
{
	if(!AppIconSizes.contains(pixels))
	{
		return {};
	}
	QSvgRenderer renderer(pixels <= AppIconSmallMarkMaxSize ? SmallMarkPath : LargeMarkPath);
	if(!renderer.isValid())
	{
		return {};
	}
	QImage image(pixels, pixels, QImage::Format_ARGB32_Premultiplied);
	image.fill(Qt::transparent);
	QPainter painter(&image);
	renderer.render(&painter);
	painter.end();
	return image;
}

QIcon appIcon()
{
	QIcon icon;
	for(const int pixels : AppIconSizes)
	{
		icon.addPixmap(QPixmap::fromImage(appIconImage(pixels)));
	}
	return icon;
}

} // namespace refgui::icons
