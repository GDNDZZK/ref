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

constexpr qsizetype MaxPathLength = 4096;

const QString FileScheme = u"file:"_s;

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

	if(path.startsWith(FileScheme, Qt::CaseInsensitive))
	{
		// Qt knows the forms of a file URL on each platform (file:///C:/x, file://host/share, file://localhost/x).
		path = QUrl(path).toLocalFile();
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

std::expected<std::optional<proto::NamedImage>, QString> imageFromPath(const QString& text)
{
	return imageFromFile(pathOf(text));
}

std::expected<std::optional<proto::NamedImage>, QString> imageFromUrl(const QUrl& url)
{
	if(!url.isLocalFile())
	{
		return std::nullopt;
	}
	return imageFromFile(url.toLocalFile());
}

std::expected<std::optional<proto::NamedImage>, QString> imageFromFile(const QString& path)
{
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

proto::NamedImage imageFromBytes(const QByteArray& bytes, const QString& mime, const QString& name)
{
	return {.mime = mime, .data = QString::fromLatin1(bytes.toBase64()), .name = name};
}

} // namespace refgui::compose
