/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef FRONTEND_H
#define FRONTEND_H

#include "Thread.h"
#include "Log.h"
#include "DownloadInfo.h"
#include "MessageBase.h"
#include "QueueEditor.h"
#include "Observer.h"
#include "Options.h"
#include "WorkState.h"
#include "Connection.h"
#include "RemoteClient.h"
#include "Util.h"
#include "StatMeter.h"
#include "nzbget_rs.h"

class Frontend : public Thread
{
public:
	Frontend()
	{
		debug("Creating Frontend");

		m_workStateObserver.m_owner = this;
		g_WorkState->Attach(&m_workStateObserver);

		m_updateInterval = g_Options->GetUpdateInterval();
	}

protected:
	bool m_summary = false;
	bool m_fileList = false;
	uint32 m_neededLogEntries = 0;
	uint32 m_neededLogFirstId = 0;
	int m_updateInterval;

	// summary
	int64 m_currentDownloadSpeed = 0;
	int64 m_remainingSize = 0;
	bool m_pauseDownload = false;
	int m_downloadLimit = 0;
	int m_threadCount = 0;
	int m_postJobCount = 0;
	int m_upTimeSec = 0;
	int m_dnTimeSec = 0;
	int64 m_allBytes = 0;
	bool m_standBy = false;
	std::mutex m_waitMutex;
	std::condition_variable m_waitCond;

	virtual void Stop()
	{
		Thread::Stop();

		m_waitCond.notify_all();
	}

	bool PrepareData()
	{
		if (IsRemoteMode())
		{
			if (IsStopped())
			{
				return false;
			}
			if (!RequestMessages() || (nzbget_rs_dqh_frontend_should_request_file_list(
				m_summary ? 1 : 0, m_fileList ? 1 : 0) != 0 && !RequestFileList()))
			{
				const char* controlIp = nzbget_rs_dqh_frontend_resolve_control_ip(g_Options->GetControlIp());
				printf("\nUnable to send request to RustyGet server at %s (port %i)    \n", controlIp, g_Options->GetControlPort());
				Stop();
				return false;
			}
		}
		else
		{
			if (m_summary)
			{
				m_currentDownloadSpeed = g_StatMeter->CalcCurrentDownloadSpeed();
				m_pauseDownload = g_WorkState->GetPauseDownload();
				m_downloadLimit = g_WorkState->GetSpeedLimit();
				m_threadCount = Thread::GetThreadCount();
				g_StatMeter->CalcTotalStat(&m_upTimeSec, &m_dnTimeSec, &m_allBytes, &m_standBy);

				GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
				m_postJobCount = 0;
				for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
				{
					m_postJobCount += nzbInfo->GetPostInfo() ? 1 : 0;
				}
				downloadQueue->CalcRemainingSize(&m_remainingSize, nullptr);
			}
		}
		return true;
	}

	void FreeData()
	{
		if (IsRemoteMode())
		{
			m_remoteMessages.clear();
			DownloadQueue::Guard()->GetQueue()->clear();
		}
	}

	GuardedMessageList GuardMessages()
	{
		if (IsRemoteMode())
		{
			return GuardedMessageList(&m_remoteMessages, nullptr);
		}
		else
		{
			return g_Log->GuardMessages();
		}
	}

	bool IsRemoteMode()
	{
		return g_Options->GetRemoteClientMode();
	}

	void InitMessageBase(SNzbRequestBase* messageBase, int request, int size)
	{
		nzbget_rs_binrpc_init_message_base(
			(uint8_t*)messageBase,
			request,
			size,
			g_Options->GetControlUsername(),
			g_Options->GetControlPassword());
	}

	void ServerPauseUnpause(bool pause)
	{
		if (IsRemoteMode())
		{
			RequestPauseUnpause(pause);
		}
		else
		{
			g_WorkState->SetResumeTime(0);
			g_WorkState->SetPauseDownload(pause);
		}
	}

	bool RequestPauseUnpause(bool pause)
	{
		RemoteClient client;
		client.SetVerbose(false);
		return client.RequestServerPauseUnpause(pause, rpDownload);
	}

	void ServerSetDownloadRate(int rate)
	{
		if (IsRemoteMode())
		{
			RequestSetDownloadRate(rate);
		}
		else
		{
			g_WorkState->SetSpeedLimit(rate);
		}
	}

	bool RequestSetDownloadRate(int rate)
	{
		RemoteClient client;
		client.SetVerbose(false);
		return client.RequestServerSetDownloadRate(rate);
	}

	bool ServerEditQueue(DownloadQueue::EEditAction action, int offset, int id)
	{
		if (IsRemoteMode())
		{
			return RequestEditQueue(action, offset, id);
		}
		else
		{
			NzbgetRsBuf offBuf = nzbget_rs_dqh_conn_format_port_str(offset);
			bool ok = DownloadQueue::Guard()->EditEntry(id, action, offBuf.data ? offBuf.data : "0");
			nzbget_rs_free(offBuf);
			return ok;
		}
	}

	bool RequestEditQueue(DownloadQueue::EEditAction action, int offset, int id)
	{
		RemoteClient client;
		client.SetVerbose(false);
		IdList ids = { id };
		return client.RequestServerEditQueue(action, offset, nullptr, &ids, nullptr, rmId);
	}

	void Wait(int milliseconds)
	{
		if (g_WorkState->GetPauseFrontend())
		{
			std::unique_lock<std::mutex> lk(m_waitMutex);
			m_waitCond.wait_for(lk, std::chrono::milliseconds(2000));
		}
		else
		{
			Util::Sleep(milliseconds);
		}
	}

private:
	class WorkStateObserver : public Observer
	{
	public:
		Frontend* m_owner;
		virtual void Update(Subject* caller, void* aspect) { m_owner->WorkStateUpdate(caller, aspect); }
	};

	MessageList m_remoteMessages;
	WorkStateObserver m_workStateObserver;

	bool RequestMessages()
	{
		const char* controlIp = nzbget_rs_dqh_frontend_resolve_control_ip(g_Options->GetControlIp());
		Connection connection(controlIp, g_Options->GetControlPort(), false);

		bool OK = connection.Connect();
		if (!OK)
		{
			return false;
		}

		SNzbLogRequest LogRequest = {};
		InitMessageBase(&LogRequest.m_messageBase, rrLog, sizeof(LogRequest));
		LogRequest.m_lines = htonl(m_neededLogEntries);
		LogRequest.m_idFrom = htonl(nzbget_rs_sfw_frontend_resolve_log_id_from(
			m_neededLogEntries, m_neededLogFirstId));

		if (!connection.Send((char*)(&LogRequest), sizeof(LogRequest)))
		{
			return false;
		}

		// Now listen for the returned log
		SNzbLogResponse LogResponse;
		bool read = connection.Recv((char*) &LogResponse, sizeof(LogResponse));
		if (!nzbget_rs_sfw_rpc_is_response_header_valid(
			read ? 1 : 0,
			ntohl(LogResponse.m_messageBase.m_signature),
			(uint32_t)NZBMESSAGE_SIGNATURE,
			ntohl(LogResponse.m_messageBase.m_structSize),
			(uint32_t)sizeof(LogResponse)))
		{
			return false;
		}

		CharBuffer buf;
		if (ntohl(LogResponse.m_trailingDataLength) > 0)
		{
			buf.Reserve(ntohl(LogResponse.m_trailingDataLength));
			if (!connection.Recv(buf, buf.Size()))
			{
				return false;
			}
		}

		connection.Disconnect();

		if (ntohl(LogResponse.m_trailingDataLength) > 0)
		{
			char* bufPtr = (char*)buf;
			for (uint32 i = 0; i < ntohl(LogResponse.m_nrTrailingEntries); i++)
			{
				SNzbLogResponseEntry* logAnswer = (SNzbLogResponseEntry*) bufPtr;

				char* text = bufPtr + sizeof(SNzbLogResponseEntry);

				m_remoteMessages.emplace_back(ntohl(logAnswer->m_id), (Message::EKind)ntohl(logAnswer->m_kind), ntohl(logAnswer->m_time), text);

				bufPtr += sizeof(SNzbLogResponseEntry) + ntohl(logAnswer->m_textLen);
			}
		}

		return true;
	}

	bool RequestFileList()
	{
		const char* controlIp = nzbget_rs_dqh_frontend_resolve_control_ip(g_Options->GetControlIp());
		Connection connection(controlIp, g_Options->GetControlPort(), false);

		bool OK = connection.Connect();
		if (!OK)
		{
			return false;
		}

		SNzbListRequest ListRequest = {};
		InitMessageBase(&ListRequest.m_messageBase, rrList, sizeof(ListRequest));
		ListRequest.m_fileList = htonl(m_fileList);
		ListRequest.m_serverState = htonl(m_summary);

		if (!connection.Send((char*)(&ListRequest), sizeof(ListRequest)))
		{
			return false;
		}

		// Now listen for the returned list
		SNzbListResponse ListResponse;
		bool read = connection.Recv((char*) &ListResponse, sizeof(ListResponse));
		if (!nzbget_rs_sfw_rpc_is_response_header_valid(
			read ? 1 : 0,
			ntohl(ListResponse.m_messageBase.m_signature),
			(uint32_t)NZBMESSAGE_SIGNATURE,
			ntohl(ListResponse.m_messageBase.m_structSize),
			(uint32_t)sizeof(ListResponse)))
		{
			return false;
		}

		CharBuffer buf;
		if (ntohl(ListResponse.m_trailingDataLength) > 0)
		{
			buf.Reserve(ntohl(ListResponse.m_trailingDataLength));
			if (!connection.Recv(buf, buf.Size()))
			{
				return false;
			}
		}

		connection.Disconnect();

		if (m_summary)
		{
			m_pauseDownload = ntohl(ListResponse.m_downloadPaused);
			m_remainingSize = Util::JoinInt64(ntohl(ListResponse.m_remainingSizeHi), ntohl(ListResponse.m_remainingSizeLo));
			m_currentDownloadSpeed = ntohl(ListResponse.m_downloadRate);
			m_downloadLimit = ntohl(ListResponse.m_downloadLimit);
			m_threadCount = ntohl(ListResponse.m_threadCount);
			m_postJobCount = ntohl(ListResponse.m_postJobCount);
			m_upTimeSec = ntohl(ListResponse.m_upTimeSec);
			m_dnTimeSec = ntohl(ListResponse.m_downloadTimeSec);
			m_standBy = ntohl(ListResponse.m_downloadStandBy);
			m_allBytes = Util::JoinInt64(ntohl(ListResponse.m_downloadedBytesHi), ntohl(ListResponse.m_downloadedBytesLo));
		}

		if (nzbget_rs_dqh_frontend_should_build_remote_file_list(
			m_fileList ? 1 : 0, ntohl(ListResponse.m_trailingDataLength)) != 0)
		{
			RemoteClient client;
			client.SetVerbose(false);

			client.BuildFileList(&ListResponse, buf, DownloadQueue::Guard());
		}

		return true;
	}

	void WorkStateUpdate(Subject*, void*)
	{
		m_waitCond.notify_all();
	}
};

#endif
