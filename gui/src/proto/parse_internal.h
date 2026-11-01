#pragma once

#include "proto/json_reader.h"
#include "proto/types.h"

#include <QJsonObject>
#include <QLatin1StringView>
#include <QString>

#include <array>
#include <cstddef>
#include <utility>

// Readers shared by the parse functions; they throw json::Failure.
namespace refgui::proto::json
{

ToolState readToolState(const QString& path, const QJsonValue& value);
ModelRef readModelRef(const Fields& fields);
SessionInfo readSessionInfo(const Fields& fields);
Part readPart(const Fields& fields);
Message readMessage(const Fields& fields);
QList<Message> readMessages(const Fields& fields, QLatin1StringView key);
ImageData readImageData(const Fields& fields);
PermissionRequest readPermissionRequest(const Fields& fields);
QuestionRequest readQuestionRequest(const Fields& fields);
NamedImage readNamedImage(const Fields& fields);

// The `result` object of a response.
inline Fields resultFields(const QJsonObject& result)
{
	return Fields(result, QStringLiteral("result"));
}

// Maps the wire name of an enum value to the value; unknown names fail as "unknown".
template<typename Enum, std::size_t N>
Enum readEnum(const QString& path, const QString& name, const std::array<std::pair<QLatin1StringView, Enum>, N>& names)
{
	for(const auto& [wireName, value] : names)
	{
		if(name == wireName)
		{
			return value;
		}
	}
	failUnknown(path, QStringLiteral("unknown value \"%1\"").arg(name));
}

} // namespace refgui::proto::json
