#pragma once

#include <QByteArray>
#include <QByteArrayList>
#include <QHash>
#include <QTcpServer>
#include <QTcpSocket>
#include <QUrl>

using namespace Qt::StringLiterals;

// An HTTP/1.1 server on localhost that answers every request with `response`, its
// body in chunked transfer encoding, one HTTP chunk per entry of `chunks`.
class FakeServer
{
public:
	enum class Ending
	{
		Complete, // the body ends properly
		Drop,     // the connection closes in the middle of the body
		Hold,     // the body never ends
	};

	struct Response
	{
		int status = 200;
		QByteArrayList chunks{};
		Ending ending = Ending::Complete;
	};

	Response response;

	FakeServer()
	{
		QObject::connect(&m_server, &QTcpServer::newConnection, &m_server, [this] { accept(); });
		if(!m_server.listen(QHostAddress::LocalHost))
		{
			qFatal("FakeServer cannot listen: %s", qPrintable(m_server.errorString()));
		}
	}

	QUrl baseUrl() const
	{
		return QUrl(u"http://127.0.0.1:%1/v1"_s.arg(m_server.serverPort()));
	}

	int requestCount() const
	{
		return m_requestCount;
	}
	// Request line and headers of the latest request.
	const QByteArray& lastHead() const
	{
		return m_lastHead;
	}
	const QByteArray& lastBody() const
	{
		return m_lastBody;
	}

	// A chunk carrying one server-sent event.
	static QByteArray event(const QByteArray& data)
	{
		return "data: "_ba + data + "\n\n"_ba;
	}

private:
	void accept()
	{
		while(QTcpSocket* socket = m_server.nextPendingConnection())
		{
			QObject::connect(socket, &QTcpSocket::readyRead, socket, [this, socket] { read(*socket); });
			QObject::connect(socket, &QTcpSocket::disconnected, socket, [this, socket] {
				m_buffers.remove(socket);
				socket->deleteLater();
			});
		}
	}

	void read(QTcpSocket& socket)
	{
		QByteArray& buffer = m_buffers[&socket];
		buffer += socket.readAll();

		const qsizetype headEnd = buffer.indexOf("\r\n\r\n");
		if(headEnd < 0)
		{
			return;
		}
		const QByteArray head = buffer.first(headEnd);
		const qsizetype bodyStart = headEnd + 4;
		const qsizetype bodyLength = contentLength(head);
		if(buffer.size() - bodyStart < bodyLength)
		{
			return;
		}

		m_lastHead = head;
		m_lastBody = buffer.sliced(bodyStart, bodyLength);
		buffer.remove(0, bodyStart + bodyLength);
		++m_requestCount;
		answer(socket);
	}

	static qsizetype contentLength(const QByteArray& head)
	{
		for(const QByteArray& line : head.split('\n'))
		{
			if(line.toLower().startsWith("content-length:"))
			{
				return static_cast<qsizetype>(line.sliced(line.indexOf(':') + 1).trimmed().toLongLong());
			}
		}
		return 0;
	}

	void answer(QTcpSocket& socket) const
	{
		const bool isSuccess = response.status == 200;
		QByteArray out = "HTTP/1.1 "_ba + QByteArray::number(response.status) + " Fake\r\n"_ba;
		out += isSuccess ? "Content-Type: text/event-stream\r\n"_ba : "Content-Type: application/json\r\n"_ba;
		out += "Transfer-Encoding: chunked\r\n\r\n"_ba;
		for(const QByteArray& chunk : response.chunks)
		{
			out += QByteArray::number(chunk.size(), 16) + "\r\n"_ba + chunk + "\r\n"_ba;
		}

		switch(response.ending)
		{
		case Ending::Complete:
			socket.write(out + "0\r\n\r\n"_ba);
			break;
		case Ending::Drop:
			socket.write(out);
			socket.disconnectFromHost(); // after the pending bytes are written
			break;
		case Ending::Hold:
			socket.write(out);
			break;
		}
	}

	// Before the server: closing its sockets on destruction calls back into these.
	QHash<QTcpSocket*, QByteArray> m_buffers;
	int m_requestCount = 0;
	QByteArray m_lastHead;
	QByteArray m_lastBody;
	QTcpServer m_server;
};
