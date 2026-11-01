#include "proto/notify_kind.h"

#include <array>

using namespace Qt::StringLiterals;

namespace refgui::proto
{
namespace
{

using Kind = NotifyFrame::Kind;

constexpr std::array AllKinds{Kind::Permission, Kind::TurnDone, Kind::TurnError, Kind::Question};

} // namespace

QString notifyKindName(Kind kind)
{
	switch(kind)
	{
	case Kind::Permission:
		return u"permission"_s;
	case Kind::TurnDone:
		return u"turn_done"_s;
	case Kind::TurnError:
		return u"turn_error"_s;
	case Kind::Question:
		return u"question"_s;
	}
	Q_UNREACHABLE_RETURN(QString());
}

std::optional<Kind> notifyKindFromName(QStringView name)
{
	for(const Kind kind : AllKinds)
	{
		if(name == notifyKindName(kind))
		{
			return kind;
		}
	}
	return std::nullopt;
}

} // namespace refgui::proto
