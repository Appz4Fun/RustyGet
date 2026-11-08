/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2015-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2025 Denis <denis@nzbget.com>
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#ifndef SERVICE_H
#define SERVICE_H

#include "Thread.h"
#include "Log.h"
#include "Util.h"
#include "nzbget_rs.h"

class ServiceCoordinator;
extern ServiceCoordinator* g_ServiceCoordinator;

class Service
{
public:
	Service();
	virtual ~Service() = default;

	static const int Now = 0;
	static const int Sleep = -1;

protected:
	virtual int ServiceInterval() = 0;
	virtual void ServiceWork() = 0;
	void WakeUp();

private:
	time_t m_lastWork = 0;

	friend class ServiceCoordinator;
};

class ServiceCoordinator : public Thread
{
public:
	using ServiceList = std::vector<Service*>;

	ServiceCoordinator()
	{
		debug("Creating ServiceCoordinator");
	}

	~ServiceCoordinator() override
	{
		debug("Destroying ServiceCoordinator");
	}

	void Run() override
	{
		debug("Entering ServiceCoordinator-loop");

		while (!IsStopped())
		{
			// perform service work
			for (Service* service : m_services)
			{
				int serviceInterval = service->ServiceInterval();
				if (nzbget_rs_euh_service_should_work_now(
						serviceInterval, Service::Now,
						static_cast<int64_t>(Util::CurrentTime()),
						static_cast<int64_t>(service->m_lastWork)) != 0)
				{
					service->ServiceWork();
					service->m_lastWork = Util::CurrentTime();
				}
			}

			// calculate wait interval
			int waitInterval = 60 * 60 * 24; // something very large
			time_t curTime = Util::CurrentTime();
			for (Service* service : m_services)
			{
				int serviceInterval = service->ServiceInterval();
				int remaining = Service::Sleep;
				nzbget_rs_euh_service_calc_remaining_wait(
					serviceInterval, Service::Now, Service::Sleep,
					static_cast<int64_t>(curTime),
					static_cast<int64_t>(service->m_lastWork),
					waitInterval, &remaining, &waitInterval);
				debug("serviceInterval: %i, remaining: %i", serviceInterval, remaining);
			}

			debug("Waiting in ServiceCoordinator: %i", waitInterval);
			if (waitInterval > 0)
			{
				std::unique_lock<std::mutex> lk(m_waitMutex);
				m_waitCond.wait_for(lk, std::chrono::milliseconds(waitInterval * 1000), [&] { return m_workenUp || IsStopped(); });
				m_workenUp = false;
			}
		}

		debug("Exiting ServiceCoordinator-loop");
	}

	void Stop() override
	{
		Thread::Stop();

		// Resume Run() to exit it
		std::lock_guard<std::mutex> guard(m_waitMutex);
		m_waitCond.notify_all();
	}

private:
	ServiceList m_services;
	std::mutex m_waitMutex;
	std::condition_variable m_waitCond;
	bool m_workenUp = false;

	void RegisterService(Service* service)
	{
		m_services.push_back(service);
	}

	void WakeUp()
	{
		debug("Waking up ServiceCoordinator");
		// Resume Run()
		std::lock_guard<std::mutex> guard(m_waitMutex);
		m_workenUp = true;
		m_waitCond.notify_all();
	}

	friend class Service;
};

inline Service::Service()
{
	g_ServiceCoordinator->RegisterService(this);
}

inline void Service::WakeUp()
{
	g_ServiceCoordinator->WakeUp();
}

#endif
