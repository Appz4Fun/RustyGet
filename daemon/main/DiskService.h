/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2015-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */


#ifndef DISKSERVICE_H
#define DISKSERVICE_H

#include "Service.h"
#include "Observer.h"
#include "Options.h"
#include "WorkState.h"
#include "StatMeter.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include "nzbget_rs.h"

class DiskService : public Service, public Observer
{
public:
	DiskService()
	{
		g_WorkState->Attach(this);
	}

protected:
	virtual int ServiceInterval()
	{
		return nzbget_rs_euh_disk_service_interval(
			m_waitingRequiredDir ? 1 : 0,
			g_Options->GetDiskSpace(),
			g_WorkState->GetDownloading() ? 1 : 0,
			Service::Sleep);
	}

	virtual void ServiceWork()
	{
		debug("Disk service work");

		if (nzbget_rs_dqh_disk_service_should_check_disk_space(
			g_Options->GetDiskSpace(), g_WorkState->GetDownloading() ? 1 : 0) != 0)
		{
			// check free disk space every 1 second
			CheckDiskSpace();
		}

		if (m_waitingRequiredDir)
		{
			CheckRequiredDir();
		}
	}

	virtual void Update(Subject* caller, void* aspect)
	{
		WakeUp();
	}

private:
	bool m_waitingRequiredDir = true;
	bool m_waitingReported = false;

	void CheckDiskSpace()
	{
		debug("Disk service work: check disk space");

		auto res = FileSystem::GetDiskState(g_Options->GetDestDir());
		if (res.has_value() && g_Options->GetDiskSpace() >= 0)
		{
			const auto& value = res.value();
			if (nzbget_rs_euh_disk_service_is_low_space(value.available, g_Options->GetDiskSpace()) != 0)
			{
				warn("Low disk space on %s. Pausing download", g_Options->GetDestDir());
				g_WorkState->SetPauseDownload(true);
			}
		}

		if (!Util::EmptyStr(g_Options->GetInterDir()))
		{
			res = FileSystem::GetDiskState(g_Options->GetInterDir());
			if (res.has_value() && g_Options->GetDiskSpace() >= 0)
			{
				const auto& value = res.value();
				if (nzbget_rs_euh_disk_service_is_low_space(value.available, g_Options->GetDiskSpace()) != 0)
				{
					warn("Low disk space on %s. Pausing download", g_Options->GetInterDir());
					g_WorkState->SetPauseDownload(true);
				}
			}
		}
	}

	void CheckRequiredDir()
	{
		debug("Disk service work: check required dir");

		if (!Util::EmptyStr(g_Options->GetRequiredDir()))
		{
			bool allExist = true;
			bool wasWaitingReported = m_waitingReported;
			// split RequiredDir into tokens
			Tokenizer tok(g_Options->GetRequiredDir(), ",;");
			while (const char* dir = tok.Next())
			{
				int shouldLogWaiting = 0;
				int nextWaitingReported = m_waitingReported ? 1 : 0;
				if (nzbget_rs_dqh_disk_service_eval_missing_required_dir(
					FileSystem::FileExists(dir) ? 1 : 0,
					FileSystem::DirectoryExists(dir) ? 1 : 0,
					wasWaitingReported ? 1 : 0,
					m_waitingReported ? 1 : 0,
					&shouldLogWaiting,
					&nextWaitingReported) != 0)
				{
					if (shouldLogWaiting != 0)
					{
						info("Waiting for required directory %s", dir);
					}
					m_waitingReported = nextWaitingReported != 0;
					allExist = false;
				}
			}
			if (!allExist)
			{
				return;
			}
		}

		if (m_waitingReported)
		{
			info("All required directories available");
		}

		g_WorkState->SetTempPauseDownload(false);
		g_WorkState->SetTempPausePostprocess(false);
		m_waitingRequiredDir = false;
	}
};

#endif
