#pragma once

#include "client/endpoint.h"

#include <QCoroTask>
#include <QString>

#include <chrono>
#include <expected>
#include <functional>
#include <variant>

// Finding, starting and connecting to ref-server. The same procedure as the TUI's
// (crates/client), so all frontends share one backend:
//
//   Auto      instance files ──> live backend ──> connect
//                  └ none ──> lock ──> start the ref-server next to us ──> wait for its instance file ──> connect
//   Explicit  url (+ token) ──> connect, never starts anything
namespace refgui::client::launcher
{

struct Auto
{
};
struct Explicit
{
	Endpoint endpoint;
};
using Target = std::variant<Auto, Explicit>;

// Tries to connect to `endpoint` within the timeout and, if that works, stays connected.
using TryConnect = std::function<QCoro::Task<bool>(const Endpoint&, std::chrono::milliseconds)>;

// Connects to `target` with `tryConnect` and returns the endpoint that worked, or why none did.
// `onStartingBackend` is called every few hundred milliseconds while a started backend boots.
QCoro::Task<std::expected<Endpoint, QString>> connectTarget(Target target,
                                                            TryConnect tryConnect,
                                                            std::function<void()> onStartingBackend,
                                                            QString instancesDirectory);

// The ref-server next to the running executable, else the one on PATH.
QString serverBinary();

// A free port in 10000-65535, found by binding it on loopback. Zero when none was found.
quint16 pickFreePort();

} // namespace refgui::client::launcher
