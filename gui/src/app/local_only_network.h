#pragma once

#include <QNetworkAccessManager>
#include <QQmlNetworkAccessManagerFactory>

// The network access of the QML engine, which loads URLs of text items and images. The program
// shows text from a model and from tools that cannot be trusted, and rendering such text must not
// reach the network (an image in Markdown, an <img> tag): only resources of the program itself
// and data URLs are served.
class LocalOnlyNetworkAccessManager : public QNetworkAccessManager
{
	Q_OBJECT

public:
	using QNetworkAccessManager::QNetworkAccessManager;

	// Whether the engine may load `url` through its network access.
	static bool isAllowed(const QUrl& url);

protected:
	QNetworkReply* createRequest(Operation operation, const QNetworkRequest& request, QIODevice* body) override;
};

class LocalOnlyNetworkAccessManagerFactory : public QQmlNetworkAccessManagerFactory
{
public:
	QNetworkAccessManager* create(QObject* parent) override;
};
