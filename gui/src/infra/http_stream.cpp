#include "infra/http_stream.h"

#include <QNetworkAccessManager>
#include <QNetworkReply>

#include <coroutine>
#include <memory>

namespace refgui::http
{
namespace
{

// RFC 9110, 15.3: the 2xx class.
constexpr int FirstSuccessStatus = 200;
constexpr int FirstRedirectionStatus = 300;

bool isSuccess(int status)
{
	return status >= FirstSuccessStatus && status < FirstRedirectionStatus;
}

int statusOf(const QNetworkReply& reply)
{
	return reply.attribute(QNetworkRequest::HttpStatusCodeAttribute).toInt(); // 0 until the headers arrive
}

// The coroutine resumes inside the reply's own finished() emission (see Finished),
// so the reply must outlive that emission.
struct DeleteLater
{
	void operator()(QObject* object) const
	{
		object->deleteLater();
	}
};

// Resumes the awaiting coroutine directly from the reply's finished() emission.
//
// QCoro's awaiter for QNetworkReply resumes through a queued connection
// (qcoronetworkreply.cpp:69-77): an aborted transfer would complete only on a later
// event loop pass, possibly after the owner of the coroutine is gone. abort() emits
// finished() synchronously (qnetworkreplyhttpimpl.cpp:278-298), so resuming directly
// completes the task within abort().
class Finished
{
public:
	explicit Finished(QNetworkReply& reply)
		: m_reply(reply)
	{
	}

	bool await_ready() const
	{
		return m_reply.isFinished(); // e.g. aborted at once by a stop requested before the call
	}

	void await_suspend(std::coroutine_handle<> coroutine)
	{
		m_connection = QObject::connect(&m_reply, &QNetworkReply::finished, &m_reply, [this, coroutine] {
			QObject::disconnect(m_connection);
			coroutine.resume();
		});
	}

	void await_resume() const
	{
	}

private:
	QNetworkReply& m_reply;
	QMetaObject::Connection m_connection;
};

} // namespace

QCoro::Task<std::expected<void, Error>> postStreaming(QNetworkAccessManager& network, QNetworkRequest request, QByteArray body,
	std::function<void(QByteArrayView chunk)> onChunk, std::stop_token stop)
{
	const std::unique_ptr<QNetworkReply, DeleteLater> reply(network.post(request, body));

	// A 2xx body streams to the caller; any other is kept as the error message.
	QByteArray errorBody;
	const auto take = [&] {
		const QByteArray data = reply->readAll();
		if(isSuccess(statusOf(*reply)))
		{
			onChunk(data);
		}
		else
		{
			errorBody += data;
		}
	};
	QMetaObject::Connection reading = QObject::connect(reply.get(), &QIODevice::readyRead, reply.get(), take);

	// A stop runs the rest of this coroutine from within reply->abort(): abort() emits
	// finished() before it returns, and Finished resumes from that emission.

	{
		const std::stop_callback abortOnStop(stop, [&reply] { reply->abort(); });
		co_await Finished(*reply);
	}

	// The reply outlives this frame until deleteLater; nothing may call into the frame then.
	QObject::disconnect(reading);
	const bool hasUnread = reply->isOpen() && reply->bytesAvailable() > 0; // closed once aborted
	if(hasUnread)
	{
		take();
	}

	const int status = statusOf(*reply);
	const bool isStatusError = status != 0 && !isSuccess(status);
	if(isStatusError)
	{
		const QString message = errorBody.isEmpty() ? reply->errorString() : QString::fromUtf8(errorBody);
		co_return std::unexpected(Error{.status = status, .message = message});
	}
	if(reply->error() != QNetworkReply::NoError)
	{
		co_return std::unexpected(Error{.status = 0, .message = reply->errorString()});
	}
	co_return std::expected<void, Error>();
}

} // namespace refgui::http
