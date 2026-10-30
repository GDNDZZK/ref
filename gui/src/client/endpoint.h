#pragma once

#include <QString>
#include <QUrl>

namespace refgui::client
{

// Where a backend listens and the secret it wants.
struct Endpoint
{
	QUrl url; // ws://host:port/
	QString token{};
};

} // namespace refgui::client
