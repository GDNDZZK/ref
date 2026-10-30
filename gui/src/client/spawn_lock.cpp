#include "client/spawn_lock.h"

#include <QDir>
#include <QtSystemDetection>

#ifdef Q_OS_WIN
#include <windows.h>
#else
#include <cerrno>
#include <fcntl.h>
#include <sys/file.h>
#include <unistd.h>
#endif

namespace refgui::client
{
namespace
{

const QString LockFileName = QStringLiteral(".spawn.lock");

} // namespace

#ifdef Q_OS_WIN

struct SpawnLock::Handle
{
	HANDLE file = INVALID_HANDLE_VALUE;
};

SpawnLock::~SpawnLock()
{
	CloseHandle(m_handle->file); // also releases the lock
}

// Same call as Rust's File::try_lock: exclusive, no waiting, the first 4 GiB.
std::expected<std::unique_ptr<SpawnLock>, SpawnLock::Failure> SpawnLock::tryAcquire(const QString& directory)
{
	QDir().mkpath(directory);
	const QString path = QDir::toNativeSeparators(QDir(directory).filePath(LockFileName));
	HANDLE file = CreateFileW(reinterpret_cast<LPCWSTR>(path.utf16()),
	                          GENERIC_WRITE,
	                          FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
	                          nullptr,
	                          OPEN_ALWAYS,
	                          FILE_ATTRIBUTE_NORMAL,
	                          nullptr);
	if(file == INVALID_HANDLE_VALUE)
	{
		return std::unexpected(Failure::Unavailable);
	}
	OVERLAPPED range{};
	if(!LockFileEx(file, LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY, 0, MAXDWORD, MAXDWORD, &range))
	{
		const bool held = GetLastError() == ERROR_LOCK_VIOLATION;
		CloseHandle(file);
		return std::unexpected(held ? Failure::HeldElsewhere : Failure::Unavailable);
	}
	return std::unique_ptr<SpawnLock>(new SpawnLock(std::make_unique<Handle>(Handle{file})));
}

#else

struct SpawnLock::Handle
{
	int fd = -1;
};

SpawnLock::~SpawnLock()
{
	close(m_handle->fd); // also releases the lock
}

std::expected<std::unique_ptr<SpawnLock>, SpawnLock::Failure> SpawnLock::tryAcquire(const QString& directory)
{
	QDir().mkpath(directory);
	const QByteArray path = QFile::encodeName(QDir(directory).filePath(LockFileName));
	constexpr mode_t fileMode = 0644;
	const int fd = open(path.constData(), O_WRONLY | O_CREAT | O_CLOEXEC, fileMode);
	if(fd < 0)
	{
		return std::unexpected(Failure::Unavailable);
	}
	if(flock(fd, LOCK_EX | LOCK_NB) != 0)
	{
		const bool held = errno == EWOULDBLOCK;
		close(fd);
		return std::unexpected(held ? Failure::HeldElsewhere : Failure::Unavailable);
	}
	return std::unique_ptr<SpawnLock>(new SpawnLock(std::make_unique<Handle>(Handle{fd})));
}

#endif

SpawnLock::SpawnLock(std::unique_ptr<Handle> handle)
	: m_handle(std::move(handle))
{
}

} // namespace refgui::client
