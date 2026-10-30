#pragma once

#include <QByteArray>
#include <QByteArrayView>
#include <QCoroTask>
#include <QNetworkRequest>
#include <QString>

#include <expected>
#include <functional>
#include <stop_token>

class QNetworkAccessManager;

namespace qutie::http
{

struct Error
{
	int status;      // HTTP status of a non-2xx response; 0 when the transfer itself failed
	QString message; // the response body for a status error, else Qt's description
};

// POSTs `body` and hands the response body to `onChunk` piece by piece as it arrives,
// if the status is 2xx. Completes when the transfer does.
//
// Requesting `stop` aborts the transfer and completes the task before request_stop()
// returns, so an owner may stop in its destructor without the coroutine resuming after
// it is gone. A stopped transfer completes with a status-0 error; callers tell
// cancellation apart by their own token.
//
// Preconditions: `network` outlives the task; `onChunk` does not request `stop`.
QCoro::Task<std::expected<void, Error>> postStreaming(QNetworkAccessManager& network, QNetworkRequest request, QByteArray body,
	std::function<void(QByteArrayView chunk)> onChunk, std::stop_token stop);

} // namespace qutie::http
