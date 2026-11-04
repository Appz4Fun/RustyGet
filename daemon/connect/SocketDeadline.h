#ifndef NZBGET_SOCKET_DEADLINE_H
#define NZBGET_SOCKET_DEADLINE_H

#include <chrono>
#include <functional>
#include <algorithm>
#include <cstdint>
#include <cerrno>
#include <poll.h>

// Optional probe-only deadline. Ordinary download sockets keep their existing I/O.
struct SocketDeadline
{
	std::chrono::steady_clock::time_point end;
	std::function<bool()> cancelled;
	bool Stopped() const { return std::chrono::steady_clock::now() >= end || (cancelled && cancelled()); }
	bool Wait(SOCKET socket, bool read, bool write) const
	{
		while (!Stopped())
		{
			auto remaining = std::chrono::duration_cast<std::chrono::milliseconds>(end - std::chrono::steady_clock::now()).count();
			int milliseconds = static_cast<int>(std::max<int64_t>(1, std::min<int64_t>(50, remaining)));
			pollfd descriptor{socket, static_cast<short>((read ? POLLIN : 0) | (write ? POLLOUT : 0)), 0};
			int result = poll(&descriptor, 1, milliseconds);
			if (result < 0 && errno != EINTR) return false;
			if (result > 0) return !Stopped();
		}
		return false;
	}
};
#endif
