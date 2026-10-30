#pragma once

#include <QString>

#include <expected>
#include <memory>

namespace refgui::client
{

// Exclusive advisory lock on `<dir>/.spawn.lock`, held until destruction. It serializes
// backend launches across frontends (the TUI takes the same lock the same way), so that
// frontends losing their backend at the same moment do not each start one.
class SpawnLock
{
public:
	enum class Failure
	{
		HeldElsewhere, // try again later
		Unavailable,   // the lock file cannot be opened or locked at all
	};

	// Takes the lock if nobody holds it.
	static std::expected<std::unique_ptr<SpawnLock>, Failure> tryAcquire(const QString& directory);

	~SpawnLock();
	SpawnLock(const SpawnLock&) = delete;
	SpawnLock& operator=(const SpawnLock&) = delete;

private:
	struct Handle;
	explicit SpawnLock(std::unique_ptr<Handle> handle);

	std::unique_ptr<Handle> m_handle;
};

} // namespace refgui::client
