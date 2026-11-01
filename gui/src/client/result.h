#pragma once

#include <QString>

#include <expected>

// The outcome of a request, as plain data. Every layer above client may use this header;
// it does no I/O (see docs/ARCHITECTURE.md, section 1).
namespace refgui::client
{

// The result of a request: the answer, or the reason it failed (the backend's error text,
// or one of the two below).
template<typename T>
using Result = std::expected<T, QString>;

// The errors of a request that got no answer because there was no connection, or it went.
inline const QString NotConnected = QStringLiteral("not connected");
inline const QString ConnectionClosed = QStringLiteral("connection closed");

// Whether `error` says the connection is gone, as opposed to the backend refusing.
inline bool isConnectionLost(const QString& error)
{
	return error == NotConnected || error == ConnectionClosed;
}

} // namespace refgui::client
