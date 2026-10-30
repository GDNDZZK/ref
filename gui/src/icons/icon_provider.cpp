#include "icons/icon_provider.h"

#include <QFile>
#include <QSvgRenderer>
#include <QPainter>
#include <QUrlQuery>

using namespace Qt::StringLiterals;

namespace refgui::icons
{

namespace
{

const QByteArray CurrentColor = "currentColor"_ba;
const QString ColorKey = u"color"_s;
constexpr QChar QueryStart = u'?';
constexpr QChar GridSeparator = u'-';

QString iconPath(const QString& name)
{
	return u":/icons/%1.svg"_s.arg(name);
}

} // namespace

bool parseIconRequest(const QString& id, IconRequest& request)
{
	const qsizetype queryStart = id.indexOf(QueryStart);
	request.name = id.left(queryStart);
	request.color = Qt::black;

	const qsizetype gridStart = request.name.lastIndexOf(GridSeparator);
	bool gridIsNumber = false;
	request.grid = request.name.mid(gridStart + 1).toInt(&gridIsNumber);
	if(gridStart < 0 || !gridIsNumber)
	{
		return false;
	}

	if(queryStart < 0)
	{
		return true;
	}
	QString hex = QUrlQuery(id.mid(queryStart + 1)).queryItemValue(ColorKey, QUrl::FullyDecoded);
	if(!hex.startsWith(u'#'))
	{
		hex.prepend(u'#');
	}
	const QColor parsed = QColor::fromString(hex);
	if(parsed.isValid())
	{
		request.color = parsed;
	}
	return true;
}

QByteArray colouredSvg(const QByteArray& svg, const QColor& color)
{
	QByteArray result = svg;
	result.replace(CurrentColor, color.name(QColor::HexRgb).toLatin1());
	return result;
}

IconProvider::IconProvider() : QQuickImageProvider(QQuickImageProvider::Image) {}

QImage IconProvider::requestImage(const QString& id, QSize* size, const QSize& requestedSize)
{
	IconRequest request;
	if(!parseIconRequest(id, request))
	{
		return {};
	}
	QFile file(iconPath(request.name));
	if(!file.open(QIODevice::ReadOnly))
	{
		return {};
	}
	QSvgRenderer renderer(colouredSvg(file.readAll(), request.color));
	if(!renderer.isValid())
	{
		return {};
	}

	const QSize deviceSize = requestedSize.isValid() ? requestedSize : QSize(request.grid, request.grid);
	QImage image(deviceSize, QImage::Format_ARGB32_Premultiplied);
	image.fill(Qt::transparent);
	QPainter painter(&image);
	// SVG has no #aarrggbb colours, so the alpha is applied when painting.
	painter.setOpacity(request.color.alphaF());
	renderer.render(&painter);
	painter.end();
	*size = deviceSize;
	return image;
}

} // namespace refgui::icons
