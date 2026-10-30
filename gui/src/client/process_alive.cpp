#include "client/process_alive.h"

#include <QtSystemDetection>

#ifdef Q_OS_WIN
#include <windows.h>
#else
#include <cerrno>
#include <csignal>
#include <limits>
#endif

namespace refgui::client
{

#ifdef Q_OS_WIN

bool processIsAlive(quint32 pid)
{
	if(pid == 0)
	{
		return false;
	}
	HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
	if(process == nullptr)
	{
		return GetLastError() == ERROR_ACCESS_DENIED; // exists, but belongs to someone we may not query
	}
	DWORD exitCode = 0;
	const bool queried = GetExitCodeProcess(process, &exitCode) != 0;
	CloseHandle(process);
	return !queried || exitCode == STILL_ACTIVE;
}

#else

bool processIsAlive(quint32 pid)
{
	const bool representable = pid != 0 && pid <= static_cast<quint32>(std::numeric_limits<pid_t>::max());
	if(!representable)
	{
		return false;
	}
	// Signal 0 only checks that the process exists and may be signaled.
	return kill(static_cast<pid_t>(pid), 0) == 0 || errno == EPERM;
}

#endif

} // namespace refgui::client
