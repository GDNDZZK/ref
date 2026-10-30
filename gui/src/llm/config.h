#pragma once

#include "llm/message.h"

#include <QByteArrayView>
#include <QString>

#include <expected>

// qutie's configuration file, JSON with comments and trailing commas:
//
//   {
//     "endpoint": {
//       "baseUrl": "http://localhost:8080/v1",
//       "model": "qwen3-coder",
//       "apiKey": "sk-..."   // optional
//     }
//   }
//
// Errors are messages for the user.
namespace refgui::llm
{

// $XDG_CONFIG_HOME/qutie/config.jsonc, by default ~/.config/qutie/config.jsonc.
QString configPath();

std::expected<Endpoint, QString> loadEndpoint(const QString& path);
std::expected<Endpoint, QString> parseEndpoint(QByteArrayView source);

} // namespace refgui::llm
