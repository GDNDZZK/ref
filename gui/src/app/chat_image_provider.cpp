#include "app/chat_image_provider.h"

#include "app/chat_session.h"

#include <QCoreApplication>
#include <QImage>
#include <QQuickTextureFactory>
#include <QCoroTask>

#include <utility>

using namespace Qt::StringLiterals;

namespace
{

constexpr qsizetype CacheLimitBytes = 64 * 1024 * 1024;

} // namespace

// Lives on the GUI thread. A request comes in queued from a loader thread; the answer goes
// back through `fetched`, which the response's own thread receives queued.
class ChatImageProvider::Host : public QObject
{
	Q_OBJECT

public:
	explicit Host(SessionLookup lookup)
		: m_lookup(std::move(lookup))
		, m_cache(CacheLimitBytes)
	{
	}

	// Answers with `fetched`, to every response that waits for the key.
	Q_INVOKABLE void fetch(const QString& sessionId, const QString& imageId)
	{
		fetchTo(sessionId, imageId);
	}

signals:
	void fetched(const QString& key, const QByteArray& bytes, const QString& error);

private:
	QCoro::Task<> fetchTo(QString sessionId, QString imageId)
	{
		const QString key = sessionId + u'/' + imageId;
		if(const QByteArray* cached = m_cache.object(key))
		{
			emit fetched(key, *cached, {});
			co_return;
		}
		ChatSession* session = m_lookup(sessionId);
		if(session == nullptr)
		{
			emit fetched(key, {}, u"no such session"_s);
			co_return;
		}
		const auto result = co_await session->fetchImage(imageId);
		if(!result)
		{
			emit fetched(key, {}, result.error());
			co_return;
		}
		m_cache.insert(key, new QByteArray(*result), static_cast<int>(result->size()));
		emit fetched(key, *result, {});
	}

	SessionLookup m_lookup;
	QCache<QString, QByteArray> m_cache;
};

namespace
{

class Response : public QQuickImageResponse
{
	Q_OBJECT

public:
	Response(QString key, QSize requestedSize)
		: m_key(std::move(key))
		, m_requestedSize(requestedSize)
	{
	}

	QQuickTextureFactory* textureFactory() const override { return QQuickTextureFactory::textureFactoryForImage(m_image); }
	QString errorString() const override { return m_error; }

public slots:
	void onFetched(const QString& key, const QByteArray& bytes, const QString& error)
	{
		if(key != m_key || m_done)
		{
			return; // an answer to another response, or a repeat
		}
		m_done = true;
		m_error = error;
		if(error.isEmpty())
		{
			m_image = QImage::fromData(bytes);
			if(m_image.isNull())
			{
				m_error = u"not a picture"_s;
			}
			else if(m_requestedSize.isValid() && (m_image.width() > m_requestedSize.width() || m_image.height() > m_requestedSize.height()))
			{
				m_image = m_image.scaled(m_requestedSize, Qt::KeepAspectRatio, Qt::SmoothTransformation);
			}
		}
		emit finished();
	}

private:
	QString m_key;
	QSize m_requestedSize;
	QImage m_image;
	QString m_error;
	bool m_done = false;
};

} // namespace

ChatImageProvider::ChatImageProvider(SessionLookup lookup)
	: m_host(new Host(std::move(lookup)))
{
	m_host->moveToThread(QCoreApplication::instance()->thread());
}

ChatImageProvider::~ChatImageProvider()
{
	m_host->deleteLater();
}

bool ChatImageProvider::parseId(const QString& id, QString& sessionId, QString& imageId)
{
	const qsizetype slash = id.indexOf(u'/');
	if(slash <= 0 || slash == id.size() - 1)
	{
		return false;
	}
	sessionId = id.left(slash);
	imageId = id.mid(slash + 1);
	return true;
}

QQuickImageResponse* ChatImageProvider::requestImageResponse(const QString& id, const QSize& requestedSize)
{
	auto* response = new Response(id, requestedSize);
	QString sessionId;
	QString imageId;
	if(!parseId(id, sessionId, imageId))
	{
		QMetaObject::invokeMethod(response, "onFetched", Qt::QueuedConnection, Q_ARG(QString, id), Q_ARG(QByteArray, QByteArray()), Q_ARG(QString, u"bad image id"_s));
		return response;
	}
	QObject::connect(m_host, &Host::fetched, response, &Response::onFetched);
	QMetaObject::invokeMethod(m_host, "fetch", Qt::QueuedConnection, Q_ARG(QString, sessionId), Q_ARG(QString, imageId));
	return response;
}

#include "chat_image_provider.moc"
