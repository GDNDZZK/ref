#pragma once

#include <QByteArray>
#include <QCache>
#include <QQuickAsyncImageProvider>
#include <QString>
#include <QStringList>

#include <functional>

class ChatSession;

// Serves the pictures of a conversation: "image://chatimage/<session id>/<image id>". The bytes
// come from the backend (GetImage) when the Image asks, and stay cached while they are in use.
class ChatImageProvider : public QQuickAsyncImageProvider
{
public:
	// Finds the session an id belongs to. Called on the GUI thread.
	using SessionLookup = std::function<ChatSession*(const QString& sessionId)>;

	explicit ChatImageProvider(SessionLookup lookup);
	~ChatImageProvider() override;

	QQuickImageResponse* requestImageResponse(const QString& id, const QSize& requestedSize) override;
	QObject* cacheHost() const;
	void evictMessages(const QString& sessionId, const QStringList& messageIds);
	void clearAll();

private:
	class Host;

	// Fetches on the GUI thread, where the sessions live; the loader threads only ask.
	Host* m_host;
};
