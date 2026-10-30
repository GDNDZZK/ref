#include "compose/image_file.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QUrl>

using namespace Qt::StringLiterals;

namespace refgui::compose
{
namespace
{

constexpr qint64 MaxImageBytes = 20 * 1024 * 1024;
constexpr qsizetype MaxPathLength = 4096;

const QString FileScheme = u"file://"_s;

// The mime type of an image file by its extension, or an empty string.
QString mimeOf(const QString& path)
{
	static const QList<std::pair<QString, QString>> byExtension = {
		{u".png"_s, u"image/png"_s},
		{u".jpg"_s, u"image/jpeg"_s},
		{u".jpeg"_s, u"image/jpeg"_s},
		{u".gif"_s, u"image/gif"_s},
		{u".webp"_s, u"image/webp"_s},
	};
	for(const auto& [extension, mime] : byExtension)
	{
		if(path.endsWith(extension, Qt::CaseInsensitive))
		{
			return mime;
		}
	}
	return {};
}

// The path a pasted string stands for, or an empty string when it cannot be one.
QString pathOf(const QString& text)
{
	QString path = text.trimmed();
	const auto isQuote = [](QChar c) { return c == u'\'' || c == u'"'; };
	while(!path.isEmpty() && isQuote(path.front()))
	{
		path.removeFirst();
	}
	while(!path.isEmpty() && isQuote(path.back()))
	{
		path.removeLast();
	}

	if(path.startsWith(FileScheme))
	{
		path = QUrl::fromPercentEncoding(path.mid(FileScheme.size()).toUtf8());
	}
	path.replace(u"\\ "_s, u" "_s);
	if(path.isEmpty() || path.contains(u'\n') || path.size() > MaxPathLength)
	{
		return {};
	}
	if(path.startsWith(u"~/"_s))
	{
		path = QDir::homePath() + path.mid(1);
	}
	return path;
}

} // namespace

std::expected<std::optional<NamedImage>, QString> imageFromPath(const QString& text)
{
	const QString path = pathOf(text);
	const QString mime = mimeOf(path);
	const QFileInfo info(path);
	if(mime.isEmpty() || !info.isFile())
	{
		return std::nullopt;
	}
	if(info.size() > MaxImageBytes)
	{
		return std::unexpected(QCoreApplication::translate("refgui::compose", "%1 is larger than 20 MiB.").arg(info.fileName()));
	}

	QFile file(path);
	if(!file.open(QIODevice::ReadOnly))
	{
		return std::unexpected(QCoreApplication::translate("refgui::compose", "Cannot read %1: %2").arg(info.fileName(), file.errorString()));
	}
	return imageFromBytes(file.readAll(), mime, info.fileName());
}

NamedImage imageFromBytes(const QByteArray& bytes, const QString& mime, const QString& name)
{
	proto::ImageData image;
	image.mime = mime;
	image.data = QString::fromLatin1(bytes.toBase64());
	image.size = static_cast<quint64>(bytes.size());
	return {.image = std::move(image), .name = name};
}

} // namespace refgui::compose
