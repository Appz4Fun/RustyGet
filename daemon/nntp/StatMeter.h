/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2014-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef STATMETER_H
#define STATMETER_H

#include <atomic>
#include <array>
#include "Log.h"
#include "Util.h"

class ServerVolume
{
public:
	struct Articles
	{
		uint32 failed;
		uint32 success;
	};
	struct Stats
	{
		uint32 bytes;
		Articles articles;
	};

	using VolumeArray = std::vector<int64>;
	using ArticlesArray = std::vector<Articles>;

	VolumeArray* BytesPerSeconds() { return &m_bytesPerSeconds; }
	VolumeArray* BytesPerMinutes() { return &m_bytesPerMinutes; }
	VolumeArray* BytesPerHours() { return &m_bytesPerHours; }
	VolumeArray* BytesPerDays() { return &m_bytesPerDays; }

	const ArticlesArray& GetArticlesPerDays() const { return m_articlesPerDays; }
	void SetArticlesPerDays(ArticlesArray articles) { m_articlesPerDays = std::move(articles); }
	void SetFirstDay(int firstDay) { m_firstDay = firstDay; }
	int GetFirstDay() { return m_firstDay; }
	void SetTotalBytes(int64 totalBytes) { m_totalBytes = totalBytes; }
	int64 GetTotalBytes() { return m_totalBytes; }
	void SetCustomBytes(int64 customBytes) { m_customBytes = customBytes; }
	int64 GetCustomBytes() { return m_customBytes; }
	int GetSecSlot() { return m_secSlot; }
	int GetMinSlot() { return m_minSlot; }
	int GetHourSlot() { return m_hourSlot; }
	int GetDaySlot() { return m_daySlot; }
	time_t GetDataTime() { return m_dataTime; }
	void SetDataTime(time_t dataTime) { m_dataTime = dataTime; }
	time_t GetCustomTime() { return m_customTime; }
	void SetCustomTime(time_t customTime) { m_customTime = customTime; }
	time_t GetCountersResetTime() { return m_countersResetTime; }
	void SetCountersResetTime(time_t time) { m_countersResetTime = time; }

	void AddStats(Stats stats);
	void CalcSlots(time_t locCurTime);
	void Reset();
	void ResetCustom();
	void LogDebugInfo();

private:
	void ResetVolume(VolumeArray& volume);
	void ResetArticles();

	VolumeArray m_bytesPerSeconds = VolumeArray(60);
	VolumeArray m_bytesPerMinutes = VolumeArray(60);
	VolumeArray m_bytesPerHours = VolumeArray(24);
	VolumeArray m_bytesPerDays;
	ArticlesArray m_articlesPerDays;

	int m_firstDay = 0;
	int64 m_totalBytes = 0;
	int64 m_customBytes = 0;

	/** Date/time when the data was last updated (time is in C/Unix format) */
	time_t m_dataTime = 0;

	/** Date/time of the last reset of custom counter (time is in C/Unix format) */
	time_t m_customTime = Util::CurrentTime();

	/** Date/time of the last reset of all counters (time is in C/Unix format) */
	time_t m_countersResetTime = Util::CurrentTime();

	int m_secSlot = 0;
	int m_minSlot = 0;
	int m_hourSlot = 0;
	int m_daySlot = 0;
};

using ServerVolumes = std::vector<ServerVolume>;
using GuardedServerVolumes = GuardedPtr<ServerVolumes>;

class StatMeter : public Debuggable
{
public:
	StatMeter();
	void Init();
	int64 CalcCurrentDownloadSpeed();
	int64 CalcMomentaryDownloadSpeed();
	void AddSpeedReading(int bytes);
	void AddServerStats(ServerVolume::Stats stats, int serverId);
	void CalcTotalStat(int* upTimeSec, int* dnTimeSec, int64* allBytes, bool* standBy);
	void CalcQuotaUsage(int64& monthBytes, int64& dayBytes);
	void IntervalCheck();
	void EnterLeaveStandBy(bool enter);
	GuardedServerVolumes GuardServerVolumes();
	// the next interval check saves the volumes (changed through the guard)
	void MarkChanged() { m_statChanged = true; }
	void Save();
	bool Load(bool* perfectServerMatch);

protected:
	virtual void LogDebugInfo();

private:
	// speed meter
	static const int SPEEDMETER_SLOTS = 30;
	static const int SPEEDMETER_SLOTSIZE = 1; //Split elapsed time into this number of secs.
	std::array<std::atomic<int>, SPEEDMETER_SLOTS> m_speedBytes;
	std::array<std::atomic<int>, SPEEDMETER_SLOTS> m_speedTime;
	std::atomic<time_t> m_speedCorrection{0};
	std::atomic<time_t> m_curSecTime{0};
	std::atomic<int64> m_speedTotalBytes{0};
	std::atomic<int> m_speedBytesIndex{0};
	std::atomic<int> m_curSecBytes{0};
	std::atomic<int> m_speedStartTime{0};
	std::mutex m_speedTotalBytesMtx;

	// time
	std::atomic<int64> m_allBytes{0};
	std::atomic<time_t> m_startServer{0};
	std::atomic<time_t> m_pausedFrom{0};
	std::atomic<time_t> m_startDownload{0};
	std::atomic<bool> m_standBy{true};
	time_t m_lastCheck = 0;
	time_t m_lastTimeOffset = 0;

	// data volume
	std::atomic<bool> m_statChanged{false};
	ServerVolumes m_serverVolumes;
	Mutex m_volumeMutex;

	void ResetSpeedStat();
	void AdjustTimeOffset();
	void CheckQuota();
	int CalcMonthSlots(const ServerVolume& volume);
};

extern StatMeter* g_StatMeter;

#include "nzbget_rs.h"
#include "Options.h"
#include "ServerPool.h"
#include "DiskState.h"

inline void ServerVolume::CalcSlots(time_t locCurTime)
{
	NzbgetRsVolumeSlots slots;
	nzbget_rs_volume_calc_slots(locCurTime, &m_firstDay, &slots);
	m_secSlot = slots.sec;
	m_minSlot = slots.min;
	m_hourSlot = slots.hour;
	m_daySlot = slots.day;
	if (slots.inRange)
	{
		size_t newBytesDaysLen = nzbget_rs_tpr_server_volume_required_day_len(
			m_daySlot, m_bytesPerDays.size());
		if (newBytesDaysLen > 0)
		{
			m_bytesPerDays.resize(newBytesDaysLen);
		}

		size_t newArtDaysLen = nzbget_rs_tpr_server_volume_required_day_len(
			m_daySlot, m_articlesPerDays.size());
		if (newArtDaysLen > 0)
		{
			m_articlesPerDays.resize(newArtDaysLen);
		}
	}
}

inline void ServerVolume::AddStats(Stats stats)
{
	time_t curTime = Util::CurrentTime();
	time_t locCurTime = curTime + g_WorkState->GetLocalTimeOffset();
	time_t locDataTime = m_dataTime + g_WorkState->GetLocalTimeOffset();

	int lastMinSlot = m_minSlot;
	int lastHourSlot = m_hourSlot;

	CalcSlots(locCurTime);

	int64 bytes = static_cast<int64>(stats.bytes);
	NzbgetRsVolumeSlots slots{m_secSlot, m_minSlot, m_hourSlot, m_daySlot, 0};
	nzbget_rs_volume_add(m_bytesPerSeconds.data(), m_bytesPerSeconds.size(), m_bytesPerMinutes.data(),
		m_bytesPerMinutes.size(), m_bytesPerHours.data(), m_bytesPerHours.size(), &slots, lastMinSlot,
		lastHourSlot, locCurTime, locDataTime, bytes);
	size_t daySlotIdx = m_daySlot >= 0 ? static_cast<size_t>(m_daySlot) : 0;
	int64 curDayBytes = m_daySlot >= 0 ? m_bytesPerDays[daySlotIdx] : 0;
	int curDayFailed = m_daySlot >= 0 ? m_articlesPerDays[daySlotIdx].failed : 0;
	int curDaySuccess = m_daySlot >= 0 ? m_articlesPerDays[daySlotIdx].success : 0;
	int updateDay = 0;
	int64 newDayBytes = 0, newTotalBytes = 0, newCustomBytes = 0;
	int newDayFailed = 0, newDaySuccess = 0;
	nzbget_rs_statmeter_accum_day_and_total_volume(
		m_daySlot, curDayBytes, curDayFailed, curDaySuccess,
		m_totalBytes, m_customBytes, bytes, stats.articles.failed, stats.articles.success,
		&updateDay, &newDayBytes, &newDayFailed, &newDaySuccess, &newTotalBytes, &newCustomBytes);
	if (updateDay != 0)
	{
		m_bytesPerDays[daySlotIdx] = newDayBytes;
		m_articlesPerDays[daySlotIdx].failed = newDayFailed;
		m_articlesPerDays[daySlotIdx].success = newDaySuccess;
	}
	m_totalBytes = newTotalBytes;
	m_customBytes = newCustomBytes;

	m_dataTime = curTime;
}

inline void ServerVolume::Reset()
{
	m_totalBytes = 0;
	m_dataTime = 0;
	m_firstDay = 0;
	m_countersResetTime = Util::CurrentTime();

	ResetVolume(m_bytesPerSeconds);
	ResetVolume(m_bytesPerMinutes);
	ResetVolume(m_bytesPerHours);
	ResetVolume(m_bytesPerDays);

	ResetArticles();
	ResetCustom();
}

inline void ServerVolume::ResetCustom()
{
	m_customBytes = 0;
	m_customTime = Util::CurrentTime();
}

inline void ServerVolume::ResetVolume(VolumeArray& volume)
{
	for (int64& size : volume)
	{
		size = 0;
	}
}

inline void ServerVolume::ResetArticles()
{
	for (Articles& articles : m_articlesPerDays)
	{
		articles.failed = 0;
		articles.success = 0;
	}
}

inline void ServerVolume::LogDebugInfo()
{
	info("   ---------- ServerVolume");

	NzbgetRsBuf secsBuf = nzbget_rs_dqh_statmeter_format_indexed_bytes_line(
		0, m_bytesPerSeconds.data(), m_bytesPerSeconds.size());
	info("Secs: %s", secsBuf.data ? secsBuf.data : "");
	nzbget_rs_free(secsBuf);

	NzbgetRsBuf minsBuf = nzbget_rs_dqh_statmeter_format_indexed_bytes_line(
		0, m_bytesPerMinutes.data(), m_bytesPerMinutes.size());
	info("Mins: %s", minsBuf.data ? minsBuf.data : "");
	nzbget_rs_free(minsBuf);

	NzbgetRsBuf hoursBuf = nzbget_rs_dqh_statmeter_format_indexed_bytes_line(
		0, m_bytesPerHours.data(), m_bytesPerHours.size());
	info("Hours: %s", hoursBuf.data ? hoursBuf.data : "");
	nzbget_rs_free(hoursBuf);

	NzbgetRsBuf daysBuf = nzbget_rs_dqh_statmeter_format_indexed_bytes_line(
		m_firstDay, m_bytesPerDays.data(), m_bytesPerDays.size());
	info("Days: %s", daysBuf.data ? daysBuf.data : "");
	nzbget_rs_free(daysBuf);
}

inline StatMeter::StatMeter()
{
	debug("Creating StatMeter");

	ResetSpeedStat();
}

inline void StatMeter::Init()
{
	m_startServer = Util::CurrentTime();
	m_lastCheck = m_startServer.load();
	AdjustTimeOffset();

	m_serverVolumes.resize(1 + g_ServerPool->GetServers()->size());
}

inline void StatMeter::AdjustTimeOffset()
{
	time_t utcTime = Util::CurrentTime();
	tm tmSplittedTime;
	gmtime_r(&utcTime, &tmSplittedTime);
	tmSplittedTime.tm_isdst = -1;
	time_t locTime = mktime(&tmSplittedTime);
	int localDelta = 0;
	int totalOffset = nzbget_rs_statmeter_calc_local_time_offset(
		(int64_t)utcTime, (int64_t)locTime, g_Options->GetTimeCorrection(), &localDelta);
	g_WorkState->SetLocalTimeOffset(totalOffset);
	m_lastTimeOffset = utcTime;

	debug("UTC delta: %i (%i+%i)", g_WorkState->GetLocalTimeOffset(), localDelta, g_Options->GetTimeCorrection());
}

inline void StatMeter::IntervalCheck()
{
	time_t m_curTime = Util::CurrentTime();
	int64 clockDelta = 0;
	int adjustDownload = 0;
	int adjustTz = 0;
	if (nzbget_rs_sus_eval_interval_check(
		static_cast<int64>(m_curTime),
		static_cast<int64>(m_lastCheck),
		static_cast<int64>(m_lastTimeOffset),
		static_cast<int64>(m_startDownload.load()),
		m_standBy ? 1 : 0,
		&clockDelta,
		&adjustDownload,
		&adjustTz))
	{
		m_startServer += static_cast<time_t>(clockDelta);
		if (adjustDownload)
		{
			m_startDownload += static_cast<time_t>(clockDelta);
		}
	}
	if (adjustTz)
	{
		AdjustTimeOffset();
	}

	m_lastCheck = m_curTime;

	CheckQuota();

	if (m_statChanged)
	{
		Save();
	}
}

inline void StatMeter::EnterLeaveStandBy(bool enter)
{
	m_standBy = enter;
	if (enter)
	{
		m_pausedFrom = Util::CurrentTime();
	}
	else
	{
		time_t startDownload = 0;
		if (!m_startDownload.compare_exchange_strong(startDownload, Util::CurrentTime()))
		{
			m_startDownload = (time_t)nzbget_rs_statmeter_calc_resumed_download_start(
				(int64_t)m_startDownload.load(),
				(int64_t)Util::CurrentTime(),
				(int64_t)m_pausedFrom.load());
		}

		m_pausedFrom = 0;
		ResetSpeedStat();
	}
}

inline void StatMeter::CalcTotalStat(int* upTimeSec, int* dnTimeSec, int64* allBytes, bool* standBy)
{
	time_t startServer = m_startServer.load();
	*standBy = m_standBy;
	nzbget_rs_sus_calc_total_stat(
		static_cast<int64>(Util::CurrentTime()),
		static_cast<int64>(startServer),
		static_cast<int64>(m_startDownload.load()),
		static_cast<int64>(m_pausedFrom.load()),
		*standBy ? 1 : 0,
		upTimeSec,
		dnTimeSec);
	*allBytes = m_allBytes;
}

inline int64 StatMeter::CalcCurrentDownloadSpeed()
{
	if (m_standBy)
	{
		return 0;
	}
	return nzbget_rs_sus_calc_current_download_speed(
		0,
		static_cast<int64>(Util::CurrentTime()),
		m_speedStartTime.load(),
		m_speedTotalBytes.load());
}

inline int64 StatMeter::CalcMomentaryDownloadSpeed()
{
	time_t curTime = Util::CurrentTime();
	return nzbget_rs_sus_calc_momentary_download_speed(
		static_cast<int64>(curTime),
		static_cast<int64>(m_curSecTime.load()),
		m_curSecBytes.load());
}

inline void StatMeter::AddSpeedReading(int bytes)
{
	std::lock_guard<std::mutex> guard{m_speedTotalBytesMtx};

	time_t curTime = Util::CurrentTime();
	int64 curSecTime = static_cast<int64>(m_curSecTime.load());
	int64 curSecBytes = m_curSecBytes.load();
	int32_t speedBytes[SPEEDMETER_SLOTS];
	int32_t speedTime[SPEEDMETER_SLOTS];
	for (int i = 0; i < SPEEDMETER_SLOTS; i++)
	{
		speedBytes[i] = m_speedBytes[i].load();
		speedTime[i] = m_speedTime[i].load();
	}
	int speedBytesIndex = m_speedBytesIndex.load();
	int64 speedTotalBytes = m_speedTotalBytes.load();
	int speedStartTime = m_speedStartTime.load();
	int64 speedCorrection = static_cast<int64>(m_speedCorrection);
	int64 allBytes = m_allBytes.load();

	nzbget_rs_sus_add_speed_reading(
		bytes,
		static_cast<int64>(curTime),
		&curSecTime,
		&curSecBytes,
		speedBytes,
		speedTime,
		&speedBytesIndex,
		&speedTotalBytes,
		&speedStartTime,
		&speedCorrection,
		&allBytes);

	m_curSecTime = static_cast<time_t>(curSecTime);
	m_curSecBytes = curSecBytes;
	for (int i = 0; i < SPEEDMETER_SLOTS; i++)
	{
		m_speedBytes[i] = speedBytes[i];
		m_speedTime[i] = speedTime[i];
	}
	m_speedBytesIndex = speedBytesIndex;
	m_speedTotalBytes = speedTotalBytes;
	m_speedStartTime = speedStartTime;
	m_speedCorrection = static_cast<time_t>(speedCorrection);
	m_allBytes = allBytes;
}

inline void StatMeter::ResetSpeedStat()
{
	time_t curTime = Util::CurrentTime();
	m_speedStartTime = (int)curTime / SPEEDMETER_SLOTSIZE - 1;
	for (int i = 0; i < SPEEDMETER_SLOTS; i++)
	{
		m_speedBytes[i] = 0;
		m_speedTime[i] = m_speedStartTime.load();
	}
	m_speedBytesIndex = 0;
	m_speedTotalBytes = 0;
	m_speedCorrection = curTime;
	m_curSecTime =	0;
	m_curSecBytes = 0;
}

inline void StatMeter::LogDebugInfo()
{
	info("   ---------- SpeedMeter");
	int32_t speed = 0, timeDiff = 0;
	nzbget_rs_dqh_statmeter_calc_debug_speed_and_timediff(
		CalcCurrentDownloadSpeed(), (int)Util::CurrentTime(),
		m_speedStartTime.load(), SPEEDMETER_SLOTSIZE, &speed, &timeDiff);
	info("      Speed: %i", speed);
	info("      SpeedStartTime: %i", m_speedStartTime.load());
	info("      SpeedTotalBytes: %" PRIi64, m_speedTotalBytes.load());
	info("      SpeedBytesIndex: %i", m_speedBytesIndex.load());
	info("      AllBytes: %" PRIi64, m_allBytes.load());
	info("      Time: %i", (int)Util::CurrentTime());
	info("      TimeDiff: %i", timeDiff);
	for (int i=0; i < SPEEDMETER_SLOTS; i++)
	{
		info("      Bytes[%i]: %i, Time[%i]: %i", i, m_speedBytes[i].load(), i, m_speedTime[i].load());
	}

	Guard guard(m_volumeMutex);
	int index = 0;
	for (ServerVolume& serverVolume : m_serverVolumes)
	{
		info("      ServerVolume %i", index);
		serverVolume.LogDebugInfo();
		index++;
	}
}

inline void StatMeter::AddServerStats(ServerVolume::Stats stats, int serverId)
{
	Guard guard(m_volumeMutex);
	m_serverVolumes[0].AddStats(stats);
	m_serverVolumes[serverId].AddStats(stats);
	m_statChanged = true;
}

inline GuardedServerVolumes StatMeter::GuardServerVolumes()
{
	GuardedServerVolumes serverVolumes(&m_serverVolumes, &m_volumeMutex);

	for (ServerVolume& serverVolume : m_serverVolumes)
	{
		ServerVolume::Stats stats;
		stats.bytes = 0;
		stats.articles.failed = 0;
		stats.articles.success = 0;
		serverVolume.AddStats(stats);
	}

	return serverVolumes;
}

inline void StatMeter::Save()
{
	if (!g_Options->GetServerMode())
	{
		return;
	}

	Guard guard(m_volumeMutex);
	g_DiskState->SaveStats(g_ServerPool->GetServers(), &m_serverVolumes);
	m_statChanged = false;
}

inline bool StatMeter::Load(bool* perfectServerMatch)
{
	Guard guard(m_volumeMutex);

	bool ok = g_DiskState->LoadStats(g_ServerPool->GetServers(), &m_serverVolumes, perfectServerMatch);

	for (ServerVolume& serverVolume : m_serverVolumes)
	{
		serverVolume.CalcSlots(serverVolume.GetDataTime() + g_WorkState->GetLocalTimeOffset());
	}

	return ok;
}

inline void StatMeter::CheckQuota()
{
	if (!nzbget_rs_dqh_statmeter_has_any_quota_configured(
		g_Options->GetDailyQuota(), g_Options->GetMonthlyQuota()))
	{
		return;
	}

	int64 monthBytes, dayBytes;
	CalcQuotaUsage(monthBytes, dayBytes);

	int action = 0;
	bool quotaReached = nzbget_rs_sus_eval_quota_status(
		g_Options->GetMonthlyQuota(),
		g_Options->GetDailyQuota(),
		monthBytes,
		dayBytes,
		g_WorkState->GetQuotaReached() ? 1 : 0,
		&action) != 0;
	if (action == 1)
	{
		warn("Monthly quota reached at %s", *Util::FormatSize(monthBytes));
	}
	else if (action == 2)
	{
		warn("Daily quota reached at %s", *Util::FormatSize(dayBytes));
	}
	else if (action == 3)
	{
		info("Quota lifted");
	}
	g_WorkState->SetQuotaReached(quotaReached);
}

inline void StatMeter::CalcQuotaUsage(int64& monthBytes, int64& dayBytes)
{
	Guard guard(m_volumeMutex);

	ServerVolume totalVolume = m_serverVolumes[0];

	time_t locTime = Util::CurrentTime() + g_WorkState->GetLocalTimeOffset();
	int elapsedSlots = CalcMonthSlots(totalVolume);
	nzbget_rs_sus_calc_quota_usage(
		static_cast<int64>(locTime),
		totalVolume.GetFirstDay(),
		totalVolume.BytesPerDays()->data(),
		totalVolume.BytesPerDays()->size(),
		elapsedSlots,
		&monthBytes,
		&dayBytes);
	debug("month volume: %i MB", nzbget_rs_dqh_statmeter_bytes_to_mb_i32(monthBytes));
}

inline int StatMeter::CalcMonthSlots(const ServerVolume& volume)
{
	int elapsedDays;

	time_t locCurTime = Util::CurrentTime() + g_WorkState->GetLocalTimeOffset();
	tm dayparts;
	gmtime_r(&locCurTime, &dayparts);

	if (nzbget_rs_dqh_statmeter_is_before_quota_start_day(
		g_Options->GetQuotaStartDay(), dayparts.tm_mday) != 0)
	{
		dayparts.tm_mon--;
		dayparts.tm_mday = g_Options->GetQuotaStartDay();
		time_t prevMonth = Util::Timegm(&dayparts);
		tm prevparts;
		gmtime_r(&prevMonth, &prevparts);
		if (nzbget_rs_dqh_statmeter_did_prev_month_day_overflow(
			prevparts.tm_mday, g_Options->GetQuotaStartDay()) != 0)
		{
			dayparts.tm_mday = 1;
			dayparts.tm_mon++;
			prevMonth = Util::Timegm(&dayparts);
		}
		elapsedDays = nzbget_rs_statmeter_calc_month_slots_prev_month(
			(int64_t)locCurTime, (int64_t)prevMonth);
	}
	else
	{
		elapsedDays = nzbget_rs_statmeter_calc_month_slots_same_month(
			dayparts.tm_mday, g_Options->GetQuotaStartDay());
	}

	return elapsedDays;
}

#endif
