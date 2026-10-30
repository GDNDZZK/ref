#include "notify/notification_text.h"

#include <QCoreApplication>

using namespace Qt::StringLiterals;

namespace refgui::notify
{
namespace
{

constexpr qsizetype MaxDetailLength = 200;

QString tr(const char* source)
{
	return QCoreApplication::translate("refgui::notify", source);
}

// Errors can be long and span lines; a notification has room for the first line.
QString firstLine(const QString& text)
{
	QString line = text.trimmed().section(u'\n', 0, 0).trimmed();
	if(line.size() > MaxDetailLength)
	{
		line = line.left(MaxDetailLength).trimmed() + u"\u2026"_s;
	}
	return line;
}

QString bodyOf(const proto::NotifyFrame& frame)
{
	const QString detail = frame.detail ? firstLine(*frame.detail) : QString();
	switch(frame.kind)
	{
	case proto::NotifyFrame::Kind::Permission:
		return detail.isEmpty() ? tr("Waiting for permission") : tr("Waiting for permission to use %1").arg(detail);
	case proto::NotifyFrame::Kind::TurnDone:
		return tr("Finished");
	case proto::NotifyFrame::Kind::TurnError:
		return detail.isEmpty() ? tr("Stopped with an error") : tr("Stopped with an error: %1").arg(detail);
	}
	Q_UNREACHABLE_RETURN(QString());
}

} // namespace

Notification makeNotification(const proto::NotifyFrame& frame)
{
	const QString title = frame.sessionTitle.trimmed().isEmpty() ? u"ref"_s : frame.sessionTitle.trimmed();
	return {.sessionId = frame.sessionId, .kind = frame.kind, .title = title, .body = bodyOf(frame)};
}

} // namespace refgui::notify
