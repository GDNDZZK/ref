#pragma once

#include <QString>
#include <QUrl>

namespace refgui::client
{

// The IPv4 loopback address, where a backend on this machine is reached.
inline const QString LoopbackHost = QStringLiteral("127.0.0.1");

// Where a backend listens and the secret it wants.
struct Endpoint
{
	QUrl url; // ws://host:port/
	QString token{};
};

} // namespace refgui::client
