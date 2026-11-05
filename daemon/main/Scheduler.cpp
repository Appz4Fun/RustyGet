/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2008-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024-2025 Denis <denis@nzbget.com>
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


#include "nzbget.h"
#include "Scheduler.h"
#include "Options.h"
#include "WorkState.h"
#include "Log.h"
#include "NewsServer.h"
#include "ServerPool.h"
#include "FeedInfo.h"
#include "FeedCoordinator.h"
#include "SchedulerScript.h"

#include "nzbget_rs.h"

void Scheduler::AddTask(std::unique_ptr<Task> task)
{
	Guard guard(m_taskListMutex);
	m_taskList.push_back(std::move(task));
}

void Scheduler::FirstCheck()
{
	{
		Guard guard(m_taskListMutex);

		std::sort(m_taskList.begin(), m_taskList.end(),
			[](const std::unique_ptr<Task>& task1, const std::unique_ptr<Task>& task2)
		{
			return nzbget_rs_sfw_scheduler_task_less(
				task1->m_hours, task1->m_minutes, task2->m_hours, task2->m_minutes) != 0;
		});
	}

	// check all tasks for the last week
	CheckTasks();
}

void Scheduler::ScheduleNextWork()
{
	// Ideally we should calculate wait time until next scheduler task or until resume time.
	// The first isn't trivial and the second requires watching/reaction on changed scheduled resume time.
	// We do it simpler instead: check once per minute, when seconds are changing from 59 to 00.

	time_t curTime = Util::CurrentTime();
	tm sched;
	gmtime_r(&curTime, &sched);
	sched.tm_min++;
	sched.tm_sec = 0;
	time_t nextMinute = Util::Timegm(&sched);

	m_serviceInterval = static_cast<int>(nextMinute - curTime);
}

void Scheduler::ServiceWork()
{
	debug("Scheduler service work");

	if (!DownloadQueue::IsLoaded())
	{
		return;
	}

	debug("Scheduler service work: doing work");

	if (!m_firstChecked)
	{
		FirstCheck();
		m_firstChecked = true;
		return;
	}

	m_executeProcess = true;
	CheckTasks();
	CheckScheduledResume();
	ScheduleNextWork();
}

void Scheduler::CheckTasks()
{
	PrepareLog();

	{
		Guard guard(m_taskListMutex);

		time_t current = Util::CurrentTime();

		std::vector<NzbgetRsSchedTask> tasks;
		tasks.reserve(m_taskList.size());
		for (Task* task : &m_taskList)
		{
			tasks.push_back({task->m_hours, task->m_minutes, task->m_weekDaysBits,
				static_cast<long long>(task->m_lastExecuted)});
		}
		std::vector<size_t> due(m_taskList.size() * 9);
		long long lastCheck = m_lastCheck;
		int reset = 0;
		// StatMeter can change the atomic offset between these two readings.
		// Preserve the original readings and reuse them if the buffer grows.
		int currentOffset = tasks.empty() ? 0 : g_WorkState->GetLocalTimeOffset();
		int lastCheckOffset = tasks.empty() ? 0 : g_WorkState->GetLocalTimeOffset();
		auto calendar = [](long long value, NzbgetRsSchedTm* result)
		{
			time_t time = static_cast<time_t>(value);
			tm fields{};
			gmtime_r(&time, &fields);
			*result = {static_cast<long long>(fields.tm_year) + 1900, fields.tm_mon,
				fields.tm_mday, fields.tm_hour, fields.tm_min, fields.tm_sec, fields.tm_wday};
		};
		size_t dueCount;
		while (true)
		{
			dueCount = nzbget_rs_scheduler_check(tasks.data(), tasks.size(), &lastCheck, current,
				currentOffset, lastCheckOffset, due.data(), due.size(), &reset, calendar);
			if (dueCount <= due.size())
			{
				break;
			}
			// Leap corrections in libc's calendar can exceed the usual count * 9.
			// An undersized output leaves all input state unchanged for this retry.
			due.resize(dueCount);
		}

		if (reset)
		{
			debug("Reset scheduled tasks (detected clock change greater than 90 minutes or negative)");
			m_executeProcess = false;
		}
		for (size_t i = 0; i < tasks.size(); i++)
		{
			m_taskList[i]->m_lastExecuted = static_cast<time_t>(tasks[i].lastExecuted);
		}
		for (size_t i = 0; i < dueCount; i++)
		{
			ExecuteTask(m_taskList[due[i]].get());
		}
		m_lastCheck = static_cast<time_t>(lastCheck);
	}

	PrintLog();
}

void Scheduler::ExecuteTask(Task* task)
{
#ifdef DEBUG
	const char* commandName[] = { "Pause", "Unpause", "Pause Post-processing", "Unpause Post-processing",
		"Set download rate", "Execute process", "Execute script",
		"Pause Scan", "Unpause Scan", "Enable Server", "Disable Server", "Fetch Feed" };
	debug("Executing scheduled command: %s", commandName[task->m_command]);
#endif

	bool executeProcess = nzbget_rs_sfw_scheduler_should_execute_process(
		m_executeProcess ? 1 : 0, task->m_hours, Task::STARTUP_TASK) != 0;

	switch (task->m_command)
	{
		case scDownloadRate:
			if (!task->m_param.empty())
			{
				g_WorkState->SetSpeedLimit(
					nzbget_rs_sfw_scheduler_calc_speed_limit_bytes(atoi(task->m_param.c_str())));
				m_downloadRateChanged = true;
			}
			break;

		case scPauseDownload:
		case scUnpauseDownload:
			g_WorkState->SetPauseDownload(task->m_command == scPauseDownload);
			m_pauseDownloadChanged = true;
			break;

		case scPausePostProcess:
		case scUnpausePostProcess:
			g_WorkState->SetPausePostProcess(task->m_command == scPausePostProcess);
			m_pausePostProcessChanged = true;
			break;

		case scPauseScan:
		case scUnpauseScan:
			g_WorkState->SetPauseScan(task->m_command == scPauseScan);
			m_pauseScanChanged = true;
			break;

		case scExtensions:
		case scProcess:
			if (executeProcess)
			{
				SchedulerScriptController::StartScript(task->m_param.c_str(), task->m_command == scProcess, task->m_id);
			}
			break;

		case scActivateServer:
		case scDeactivateServer:
			EditServer(task->m_command == scActivateServer, task->m_param.c_str());
			break;

		case scFetchFeed:
			if (executeProcess)
			{
				FetchFeed(task->m_param.c_str());
				break;
			}
	}
}

void Scheduler::PrepareLog()
{
	m_downloadRateChanged = false;
	m_pauseDownloadChanged = false;
	m_pausePostProcessChanged = false;
	m_pauseScanChanged = false;
	m_serverChanged = false;
}

void Scheduler::PrintLog()
{
	if (m_downloadRateChanged)
	{
		info("Scheduler: setting download rate to %i KB/s",
			nzbget_rs_dqh_scheduler_bytes_to_kb(g_WorkState->GetSpeedLimit()));
	}
	if (m_pauseDownloadChanged)
	{
		info("Scheduler: %s download",
			nzbget_rs_dqh_scheduler_pause_action_str(g_WorkState->GetPauseDownload() ? 1 : 0));
	}
	if (m_pausePostProcessChanged)
	{
		info("Scheduler: %s post-processing",
			nzbget_rs_dqh_scheduler_pause_action_str(g_WorkState->GetPausePostProcess() ? 1 : 0));
	}
	if (m_pauseScanChanged)
	{
		info("Scheduler: %s scan",
			nzbget_rs_dqh_scheduler_pause_action_str(g_WorkState->GetPauseScan() ? 1 : 0));
	}
	if (m_serverChanged)
	{
		int index = 0;
		for (NewsServer* server : g_ServerPool->GetServers())
		{
			if (server->GetActive() != m_serverStatusList[index])
			{
				info("Scheduler: %s %s",
					nzbget_rs_dqh_scheduler_server_action_str(server->GetActive() ? 1 : 0),
					server->GetName());
			}
			index++;
		}
		g_ServerPool->Changed();
	}
}

void Scheduler::EditServer(bool active, const char* serverList)
{
	Tokenizer tok(serverList, ",;");
	while (const char* serverRef = tok.Next())
	{
		int id = atoi(serverRef);
		for (NewsServer* server : g_ServerPool->GetServers())
		{
			if (nzbget_rs_sfw_scheduler_matches_server_ref(
				id, server->GetId(), server->GetName(), serverRef) != 0)
			{
				if (!m_serverChanged)
				{
					// store old server status for logging
					m_serverStatusList.clear();
					m_serverStatusList.reserve(g_ServerPool->GetServers()->size());
					for (NewsServer* server2 : g_ServerPool->GetServers())
					{
						m_serverStatusList.push_back(server2->GetActive());
					}
				}
				m_serverChanged = true;
				server->SetActive(active);
				break;
			}
		}
	}
}

void Scheduler::FetchFeed(const char* feedList)
{
	Tokenizer tok(feedList, ",;");
	while (const char* feedRef = tok.Next())
	{
		int id = atoi(feedRef);
		for (FeedInfo* feed : g_FeedCoordinator->GetFeeds())
		{
			int targetFeedId = 0;
			if (nzbget_rs_sfw_scheduler_matches_feed_ref(
				id, feed->GetId(), feed->GetName(), feedRef, &targetFeedId) != 0)
			{
				g_FeedCoordinator->FetchFeed(targetFeedId);
				break;
			}
		}
	}
}

void Scheduler::CheckScheduledResume()
{
	time_t resumeTime = g_WorkState->GetResumeTime();
	time_t currentTime = Util::CurrentTime();
	if (nzbget_rs_sfw_scheduler_should_autoresume((int64_t)resumeTime, (int64_t)currentTime) != 0)
	{
		info("Autoresume");
		g_WorkState->SetResumeTime(0);
		g_WorkState->SetPauseDownload(false);
		g_WorkState->SetPausePostProcess(false);
		g_WorkState->SetPauseScan(false);
	}
}
