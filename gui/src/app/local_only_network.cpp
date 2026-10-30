#include "app/local_only_network.h"

#include <QNetworkRequest>
#include <QUrl>

using namespace Qt::StringLiterals;

bool LocalOnlyNetworkAccessManager::isAllowed(const QUrl& url)
{
	const QString scheme = url.scheme().toLower();
	return scheme == u"qrc"_s || scheme == u"data"_s;
}

QNetworkReply* LocalOnlyNetworkAccessManager::createRequest(Operation operation, const QNetworkRequest& request, QIODevice* body)
{
	if(isAllowed(request.url()))
	{
		return QNetworkAccessManager::createRequest(operation, request, body);
	}
	// A request without an address ends at once with an error, nothing is sent.
	QNetworkRequest refused(request);
	refused.setUrl(QUrl());
	return QNetworkAccessManager::createRequest(operation, refused, nullptr);
}

QNetworkAccessManager* LocalOnlyNetworkAccessManagerFactory::create(QObject* parent)
{
	return new LocalOnlyNetworkAccessManager(parent);
}
