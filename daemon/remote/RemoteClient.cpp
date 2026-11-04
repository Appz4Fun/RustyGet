/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2005 Bo Cordes Petersen <placebodk@sourceforge.net>
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2026 Denis <denis@nzbget.com>
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


#include "nzbget.h"
#include "RemoteClient.h"
#include "DownloadInfo.h"
#include "Options.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include "nzbget_rs.h"

// One entry of a list response, read by rust/src/binrpc.rs: its ints (host
// order) and strings; Next is false at an entry that doesn't fit the data,
// which ends the listing
template <size_t Ints, size_t Strings>
struct RsEntry
{
	uint32 ints[Ints];
	const char* strings[Strings];

	bool Next(const char* data, size_t len, size_t& at, const size_t (&fields)[Strings])
	{
		size_t starts[Strings];
		if (!nzbget_rs_binrpc_read_entry(data, len, &at, ints, Ints, fields, starts, Strings))
		{
			return false;
		}
		for (size_t k = 0; k < Strings; k++)
		{
			strings[k] = data + starts[k];
		}
		return true;
	}
};

// A listing from rust/src/binrpc.rs: the printed lines of a response's entries as one string, freed after
#define RS_PRINT_LISTING(buf) do { NzbgetRsBuf rsListing = (buf); printf("%s", rsListing.data); nzbget_rs_free(rsListing); } while (0)

void RemoteClient::printf(const char * msg,...)
{
	if (m_verbose)
	{
		va_list ap;
		va_start(ap, msg);
		::vprintf(msg, ap);
		va_end(ap);
	}
}

void RemoteClient::perror(const char * msg)
{
	if (m_verbose)
	{
		::perror(msg);
	}
}

bool RemoteClient::InitConnection()
{
	const char* controlIp = nzbget_rs_ccm_remote_client_is_wildcard_control_ip(g_Options->GetControlIp()) ?
		"127.0.0.1" : g_Options->GetControlIp();

	// Create a connection to the server (retry briefly if server is mid-reload)
	bool OK = false;
	for (int attempt = 0; attempt < 10; ++attempt)
	{
		m_connection = std::make_unique<Connection>(controlIp, g_Options->GetControlPort(), false);
		OK = m_connection->Connect();
		if (OK)
		{
			break;
		}
		Util::Sleep(50);
	}
	if (!OK)
	{
		printf("Unable to send request to nzbget-server at %s (port %i)\n", controlIp, g_Options->GetControlPort());
	}
	return OK;
}

void RemoteClient::InitMessageBase(SNzbRequestBase* messageBase, int request, int size)
{
	nzbget_rs_binrpc_init_message_base(
		(uint8_t*)messageBase,
		request,
		size,
		g_Options->GetControlUsername(),
		g_Options->GetControlPassword());
}

bool RemoteClient::ReceiveBoolResponse()
{
	printf("Request sent\n");

	// all bool-responses have the same format of structure, we use SNZBDownloadResponse here
	SNzbDownloadResponse BoolResponse;
	memset(&BoolResponse, 0, sizeof(BoolResponse));

	bool read = m_connection->Recv((char*)&BoolResponse, sizeof(BoolResponse));
	if (!nzbget_rs_tpr_binrpc_is_valid_response_header(
		read ? 1 : 0,
		(int)ntohl(BoolResponse.m_messageBase.m_signature),
		(int)NZBMESSAGE_SIGNATURE,
		(uint32)ntohl(BoolResponse.m_messageBase.m_structSize),
		(uint32)sizeof(BoolResponse)))
	{
		printf("No response or invalid response (timeout, not nzbget-server or wrong nzbget-server version)\n");
		return false;
	}

	int textLen = ntohl(BoolResponse.m_trailingDataLength);
	CString buf;
	buf.Reserve(textLen);
	read = m_connection->Recv(buf, textLen);
	if (!read)
	{
		printf("No response or invalid response (timeout, not nzbget-server or wrong nzbget-server version)\n");
		return false;
	}

	printf("server returned: %s\n", *buf);
	return ntohl(BoolResponse.m_success);
}

/*
 * Sends a message to the running nzbget process.
 */
bool RemoteClient::RequestServerDownload(const char* nzbFilename, const char* nzbContent,
	const char* category, bool autoCategory, bool addFirst, bool addPaused, int priority,
	const char* dupeKey, int dupeMode, int dupeScore)
{
	// Read the file into the buffer
	CharBuffer buffer;
	int length = 0;
	bool isUrl = nzbget_rs_ccm_remote_client_is_http_url(nzbContent) != 0;
	if (isUrl)
	{
		length = strlen(nzbContent) + 1;
	}
	else
	{
		if (!FileSystem::LoadFileIntoBuffer(nzbContent, buffer, false))
		{
			printf("Could not load file %s\n", nzbContent);
			return false;
		}
		length = buffer.Size();
	}

	bool OK = InitConnection();
	if (OK)
	{
		SNzbDownloadRequest DownloadRequest;
		nzbget_rs_binrpc_init_download_request(
			(uint8_t*)&DownloadRequest,
			g_Options->GetControlUsername(),
			g_Options->GetControlPassword(),
			nzbFilename,
			nzbContent,
			isUrl ? 1 : 0,
			category,
			addFirst ? 1 : 0,
			addPaused ? 1 : 0,
			priority,
			dupeScore,
			dupeMode,
			dupeKey,
			length,
			autoCategory ? 1 : 0);

		if (!m_connection->Send((char*)(&DownloadRequest), sizeof(DownloadRequest)))
		{
			perror("m_pConnection->Send");
			OK = false;
		}
		else
		{
			m_connection->Send(isUrl ? nzbContent : buffer, length);
			OK = ReceiveBoolResponse();
			m_connection->Disconnect();
		}
	}

	return OK;
}

void RemoteClient::BuildFileList(SNzbListResponse* listResponse, const char* trailingData, DownloadQueue* downloadQueue)
{
	size_t len = ntohl(listResponse->m_trailingDataLength);
	if (len == 0)
	{
		return;
	}
	size_t at = 0;

	// read nzb entries
	static const size_t nzbStrings[] = {12, 13, 14, 15, 16};
	for (uint32 i = 0; i < ntohl(listResponse->m_nrTrailingNzbEntries); i++)
	{
		RsEntry<17, 5> e;
		if (!e.Next(trailingData, len, at, nzbStrings))
		{
			return;
		}
		std::unique_ptr<MatchedNzbInfo> nzbInfo = std::make_unique<MatchedNzbInfo>();
		nzbInfo->SetId(e.ints[0]);
		nzbInfo->SetKind((NzbInfo::EKind)e.ints[1]);
		nzbInfo->SetSize(Util::JoinInt64(e.ints[3], e.ints[2]));
		nzbInfo->SetRemainingSize(Util::JoinInt64(e.ints[5], e.ints[4]));
		nzbInfo->SetPausedSize(Util::JoinInt64(e.ints[7], e.ints[6]));
		nzbInfo->SetPausedFileCount(e.ints[8]);
		nzbInfo->SetRemainingParCount(e.ints[9]);
		nzbInfo->SetFilename(e.strings[0]);
		nzbInfo->SetName(e.strings[1]);
		nzbInfo->SetDestDir(e.strings[2]);
		nzbInfo->SetCategory(e.strings[3]);
		nzbInfo->SetQueuedFilename(e.strings[4]);
		nzbInfo->SetPriority(e.ints[10]);
		nzbInfo->m_match = e.ints[11];

		downloadQueue->GetQueue()->Add(std::move(nzbInfo));
	}

	//read ppp entries
	static const size_t pppStrings[] = {1, 2};
	for (uint32 i = 0; i < ntohl(listResponse->m_nrTrailingPPPEntries); i++)
	{
		RsEntry<3, 2> e;
		if (!e.Next(trailingData, len, at, pppStrings))
		{
			return;
		}
		std::unique_ptr<NzbInfo>& nzbInfo = downloadQueue->GetQueue()->at(e.ints[0] - 1);
		nzbInfo->GetParameters()->SetParameter(e.strings[0], e.strings[1]);
	}

	//read file entries
	static const size_t fileStrings[] = {10, 11};
	for (uint32 i = 0; i < ntohl(listResponse->m_nrTrailingFileEntries); i++)
	{
		RsEntry<12, 2> e;
		if (!e.Next(trailingData, len, at, fileStrings))
		{
			return;
		}
		std::unique_ptr<MatchedFileInfo> fileInfo = std::make_unique<MatchedFileInfo>();
		fileInfo->SetId(e.ints[0]);
		fileInfo->SetSize(Util::JoinInt64(e.ints[3], e.ints[2]));
		fileInfo->SetRemainingSize(Util::JoinInt64(e.ints[5], e.ints[4]));
		fileInfo->SetPaused(e.ints[6]);
		fileInfo->SetSubject(e.strings[0]);
		fileInfo->SetFilename(e.strings[1]);
		fileInfo->SetFilenameConfirmed(e.ints[7]);
		fileInfo->SetActiveDownloads(e.ints[8]);
		fileInfo->m_match = e.ints[9];

		std::unique_ptr<NzbInfo>& nzbInfo = downloadQueue->GetQueue()->at(e.ints[1] - 1);
		// the group's count is its files' (the answer has none per group): it
		// stayed 0, and -L G never showed the threads of a downloading group
		nzbInfo->SetActiveDownloads(nzbInfo->GetActiveDownloads() + fileInfo->GetActiveDownloads());
		fileInfo->SetNzbInfo(nzbInfo.get());
		nzbInfo->GetFileList()->Add(std::move(fileInfo));
	}
}

bool RemoteClient::RequestServerList(bool files, bool groups, const char* pattern)
{
	if (!InitConnection()) return false;

	SNzbListRequest ListRequest = {};
	InitMessageBase(&ListRequest.m_messageBase, rrList, sizeof(ListRequest));
	ListRequest.m_fileList = htonl(true);
	ListRequest.m_serverState = htonl(true);
	ListRequest.m_matchMode = htonl(pattern ? rmRegEx : rmId);
	ListRequest.m_matchGroup = htonl(groups);
	if (pattern)
	{
		strncpy(ListRequest.m_pattern, pattern, NZBREQUESTFILENAMESIZE - 1);
		ListRequest.m_pattern[NZBREQUESTFILENAMESIZE-1] = '\0';
	}

	if (!m_connection->Send((char*)(&ListRequest), sizeof(ListRequest)))
	{
		perror("m_pConnection->Send");
		return false;
	}

	printf("Request sent\n");

	// Now listen for the returned list
	SNzbListResponse ListResponse;
	bool read = m_connection->Recv((char*) &ListResponse, sizeof(ListResponse));
	if (!nzbget_rs_tpr_binrpc_is_valid_response_header(
		read ? 1 : 0,
		(int)ntohl(ListResponse.m_messageBase.m_signature),
		(int)NZBMESSAGE_SIGNATURE,
		(uint32)ntohl(ListResponse.m_messageBase.m_structSize),
		(uint32)sizeof(ListResponse)))
	{
		printf("No response or invalid response (timeout, not nzbget-server or wrong nzbget-server version)\n");
		return false;
	}

	CharBuffer buf;
	if (ntohl(ListResponse.m_trailingDataLength) > 0)
	{
		buf.Reserve(ntohl(ListResponse.m_trailingDataLength));
		if (!m_connection->Recv(buf, buf.Size()))
		{
			return false;
		}
	}

	m_connection->Disconnect();

	if (pattern && !ListResponse.m_regExValid)
	{
		printf("Error in regular expression\n");
		return false;
	}

	if (files)
	{
		if (ntohl(ListResponse.m_trailingDataLength) == 0)
		{
			printf("Server has no files queued for download\n");
		}
		else
		{
			NzbgetRsBuf formatted = nzbget_rs_binrpc_file_listing_from_wire(
				buf,
				buf.Size(),
				ntohl(ListResponse.m_nrTrailingNzbEntries),
				ntohl(ListResponse.m_nrTrailingPPPEntries),
				ntohl(ListResponse.m_nrTrailingFileEntries),
				pattern ? 1 : 0);
			if (formatted.data)
			{
				fwrite(formatted.data, 1, formatted.len, stdout);
			}
			nzbget_rs_free(formatted);
		}
	}

	if (groups)
	{
		if (ntohl(ListResponse.m_trailingDataLength) == 0)
		{
			printf("Server has no files queued for download\n");
		}
		else
		{
			NzbgetRsBuf formatted = nzbget_rs_binrpc_group_listing_from_wire(
				buf,
				buf.Size(),
				ntohl(ListResponse.m_nrTrailingNzbEntries),
				ntohl(ListResponse.m_nrTrailingPPPEntries),
				ntohl(ListResponse.m_nrTrailingFileEntries),
				pattern ? 1 : 0);
			if (formatted.data)
			{
				fwrite(formatted.data, 1, formatted.len, stdout);
			}
			nzbget_rs_free(formatted);
		}
	}

	NzbgetRsBinRpcStatusSummarySpec statusSpec{};
	statusSpec.include_remaining_size = (!files && !groups) ? 1 : 0;
	statusSpec.remaining_size = Util::JoinInt64(ntohl(ListResponse.m_remainingSizeHi), ntohl(ListResponse.m_remainingSizeLo));
	statusSpec.download_rate = ntohl(ListResponse.m_downloadRate);
	statusSpec.download_paused = ntohl(ListResponse.m_downloadPaused);
	statusSpec.download2_paused = ntohl(ListResponse.m_download2Paused);
	statusSpec.download_stand_by = ntohl(ListResponse.m_downloadStandBy);
	statusSpec.downloaded_bytes = Util::JoinInt64(ntohl(ListResponse.m_downloadedBytesHi), ntohl(ListResponse.m_downloadedBytesLo));
	statusSpec.download_time_sec = ntohl(ListResponse.m_downloadTimeSec);
	statusSpec.download_limit = ntohl(ListResponse.m_downloadLimit);
	statusSpec.up_time_sec = ntohl(ListResponse.m_upTimeSec);
	statusSpec.thread_count = ntohl(ListResponse.m_threadCount);
	statusSpec.post_job_count = ntohl(ListResponse.m_postJobCount);
	statusSpec.scan_paused = ntohl(ListResponse.m_scanPaused);
	statusSpec.post_paused = ntohl(ListResponse.m_postPaused);

	NzbgetRsBuf statusBuf = nzbget_rs_binrpc_status_summary(&statusSpec);
	if (statusBuf.data)
	{
		fwrite(statusBuf.data, 1, statusBuf.len, stdout);
	}
	nzbget_rs_free(statusBuf);

	return true;
}

bool RemoteClient::RequestServerLog(int lines)
{
	if (!InitConnection()) return false;

	SNzbLogRequest LogRequest = {};
	InitMessageBase(&LogRequest.m_messageBase, rrLog, sizeof(LogRequest));
	LogRequest.m_lines = htonl(lines);
	LogRequest.m_idFrom = 0;

	if (!m_connection->Send((char*)(&LogRequest), sizeof(LogRequest)))
	{
		perror("m_pConnection->Send");
		return false;
	}

	printf("Request sent\n");

	// Now listen for the returned log
	SNzbLogResponse LogResponse;
	bool read = m_connection->Recv((char*) &LogResponse, sizeof(LogResponse));
	if (!nzbget_rs_tpr_binrpc_is_valid_response_header(
		read ? 1 : 0,
		(int)ntohl(LogResponse.m_messageBase.m_signature),
		(int)NZBMESSAGE_SIGNATURE,
		(uint32)ntohl(LogResponse.m_messageBase.m_structSize),
		(uint32)sizeof(LogResponse)))
	{
		printf("No response or invalid response (timeout, not nzbget-server or wrong nzbget-server version)\n");
		return false;
	}

	CharBuffer buf;
	if (ntohl(LogResponse.m_trailingDataLength) > 0)
	{
		buf.Reserve(ntohl(LogResponse.m_trailingDataLength));
		if (!m_connection->Recv(buf, buf.Size()))
		{
			return false;
		}
	}

	m_connection->Disconnect();

	if (LogResponse.m_trailingDataLength == 0)
	{
		printf("Log is empty\n");
	}
	else
	{
		printf("Log (last %i entries)\n", ntohl(LogResponse.m_nrTrailingEntries));
		printf("-----------------------------------\n");

		// rust/src/binrpc.rs: the lines of all the entries that fit the data
		RS_PRINT_LISTING(nzbget_rs_binrpc_log_listing(buf, buf.Size(), ntohl(LogResponse.m_nrTrailingEntries)));

		printf("-----------------------------------\n");
	}

	return true;
}

bool RemoteClient::RequestServerPauseUnpause(bool pause, ERemotePauseUnpauseAction action)
{
	if (!InitConnection()) return false;

	SNzbPauseUnpauseRequest PauseUnpauseRequest = {};
	InitMessageBase(&PauseUnpauseRequest.m_messageBase, rrPauseUnpause, sizeof(PauseUnpauseRequest));
	PauseUnpauseRequest.m_pause = htonl(pause);
	PauseUnpauseRequest.m_action = htonl(action);

	if (!m_connection->Send((char*)(&PauseUnpauseRequest), sizeof(PauseUnpauseRequest)))
	{
		perror("m_pConnection->Send");
		m_connection->Disconnect();
		return false;
	}

	bool OK = ReceiveBoolResponse();
	m_connection->Disconnect();

	return OK;
}

bool RemoteClient::RequestServerSetDownloadRate(int rate)
{
	if (!InitConnection()) return false;

	SNzbSetDownloadRateRequest SetDownloadRateRequest = {};
	InitMessageBase(&SetDownloadRateRequest.m_messageBase, rrSetDownloadRate, sizeof(SetDownloadRateRequest));
	SetDownloadRateRequest.m_downloadRate = htonl(rate);

	if (!m_connection->Send((char*)(&SetDownloadRateRequest), sizeof(SetDownloadRateRequest)))
	{
		perror("m_pConnection->Send");
		m_connection->Disconnect();
		return false;
	}

	bool OK = ReceiveBoolResponse();
	m_connection->Disconnect();

	return OK;
}

bool RemoteClient::RequestServerDumpDebug()
{
	if (!InitConnection()) return false;

	SNzbDumpDebugRequest DumpDebugInfo = {};
	InitMessageBase(&DumpDebugInfo.m_messageBase, rrDumpDebug, sizeof(DumpDebugInfo));

	if (!m_connection->Send((char*)(&DumpDebugInfo), sizeof(DumpDebugInfo)))
	{
		perror("m_pConnection->Send");
		m_connection->Disconnect();
		return false;
	}

	bool OK = ReceiveBoolResponse();
	m_connection->Disconnect();

	return OK;
}

bool RemoteClient::RequestServerEditQueue(DownloadQueue::EEditAction action, int offset, const char* text,
	IdList* idList, NameList* nameList, ERemoteMatchMode matchMode)
{
	if ((idList == nullptr || idList->size() == 0) && (nameList == nullptr || nameList->size() == 0))
	{
		printf("File(s) not specified\n");
		return false;
	}

	if (!InitConnection()) return false;

	std::vector<const char*> namePtrs;
	if (nameList && !nameList->empty())
	{
		namePtrs.reserve(nameList->size());
		for (CString& name : nameList)
		{
			namePtrs.push_back(*name);
		}
	}

	int textLen = 0;
	int nameLength = 0;
	NzbgetRsBuf trailingBuf = nzbget_rs_binrpc_encode_edit_queue_trailing(
		text,
		idList && !idList->empty() ? idList->data() : nullptr,
		idList ? idList->size() : 0,
		namePtrs.empty() ? nullptr : namePtrs.data(),
		namePtrs.size(),
		&textLen,
		&nameLength);
	int length = (int)trailingBuf.len;

	SNzbEditQueueRequest EditQueueRequest = {};
	InitMessageBase(&EditQueueRequest.m_messageBase, rrEditQueue, sizeof(EditQueueRequest));
	EditQueueRequest.m_action = htonl(action);
	EditQueueRequest.m_matchMode = htonl(matchMode);
	EditQueueRequest.m_offset = htonl((int)offset);
	EditQueueRequest.m_textLen = htonl(textLen);
	EditQueueRequest.m_nrTrailingIdEntries = htonl(idList ? idList->size() : 0);
	EditQueueRequest.m_nrTrailingNameEntries = htonl((int)namePtrs.size());
	EditQueueRequest.m_trailingNameEntriesLen = htonl(nameLength);
	EditQueueRequest.m_trailingDataLength = htonl(length);

	bool OK = false;
	if (!m_connection->Send((char*)(&EditQueueRequest), sizeof(EditQueueRequest)))
	{
		perror("m_pConnection->Send");
	}
	else
	{
		if (length > 0 && trailingBuf.data)
		{
			m_connection->Send(trailingBuf.data, length);
		}
		OK = ReceiveBoolResponse();
		m_connection->Disconnect();
	}

	nzbget_rs_free(trailingBuf);
	m_connection->Disconnect();
	return OK;
}

bool RemoteClient::RequestServerShutdown()
{
	if (!InitConnection()) return false;

	SNzbShutdownRequest ShutdownRequest = {};
	InitMessageBase(&ShutdownRequest.m_messageBase, rrShutdown, sizeof(ShutdownRequest));

	bool OK = m_connection->Send((char*)(&ShutdownRequest), sizeof(ShutdownRequest));
	if (OK)
	{
		OK = ReceiveBoolResponse();
	}
	else
	{
		perror("m_pConnection->Send");
	}

	m_connection->Disconnect();
	return OK;
}

bool RemoteClient::RequestServerReload()
{
	if (!InitConnection()) return false;

	SNzbReloadRequest ReloadRequest = {};
	InitMessageBase(&ReloadRequest.m_messageBase, rrReload, sizeof(ReloadRequest));

	bool OK = m_connection->Send((char*)(&ReloadRequest), sizeof(ReloadRequest));
	if (OK)
	{
		OK = ReceiveBoolResponse();
	}
	else
	{
		perror("m_pConnection->Send");
	}

	m_connection->Disconnect();
	return OK;
}

bool RemoteClient::RequestServerVersion()
{
	if (!InitConnection()) return false;

	SNzbVersionRequest VersionRequest = {};
	InitMessageBase(&VersionRequest.m_messageBase, rrVersion, sizeof(VersionRequest));

	bool OK = m_connection->Send((char*)(&VersionRequest), sizeof(VersionRequest));
	if (OK)
	{
		OK = ReceiveBoolResponse();
	}
	else
	{
		perror("m_pConnection->Send");
	}

	m_connection->Disconnect();
	return OK;
}

bool RemoteClient::RequestPostQueue()
{
	if (!InitConnection()) return false;

	SNzbPostQueueRequest PostQueueRequest = {};
	InitMessageBase(&PostQueueRequest.m_messageBase, rrPostQueue, sizeof(PostQueueRequest));

	if (!m_connection->Send((char*)(&PostQueueRequest), sizeof(PostQueueRequest)))
	{
		perror("m_pConnection->Send");
		return false;
	}

	printf("Request sent\n");

	// Now listen for the returned list
	SNzbPostQueueResponse PostQueueResponse;
	bool read = m_connection->Recv((char*) &PostQueueResponse, sizeof(PostQueueResponse));
	if (!nzbget_rs_tpr_binrpc_is_valid_response_header(
		read ? 1 : 0,
		(int)ntohl(PostQueueResponse.m_messageBase.m_signature),
		(int)NZBMESSAGE_SIGNATURE,
		(uint32)ntohl(PostQueueResponse.m_messageBase.m_structSize),
		(uint32)sizeof(PostQueueResponse)))
	{
		printf("No response or invalid response (timeout, not nzbget-server or wrong nzbget-server version)\n");
		return false;
	}

	CharBuffer buf;
	if (ntohl(PostQueueResponse.m_trailingDataLength) > 0)
	{
		buf.Reserve(ntohl(PostQueueResponse.m_trailingDataLength));
		if (!m_connection->Recv(buf, buf.Size()))
		{
			return false;
		}
	}

	m_connection->Disconnect();

	if (ntohl(PostQueueResponse.m_trailingDataLength) == 0)
	{
		printf("Server has no jobs queued for post-processing\n");
	}
	else
	{
		printf("Post-Processing List\n");
		printf("-----------------------------------\n");

		// rust/src/binrpc.rs: the lines of all the entries that fit the data
		RS_PRINT_LISTING(nzbget_rs_binrpc_post_queue_listing(buf, buf.Size(), ntohl(PostQueueResponse.m_nrTrailingEntries)));

		printf("-----------------------------------\n");
	}

	return true;
}

bool RemoteClient::RequestWriteLog(int kind, const char* text)
{
	if (!InitConnection()) return false;

	SNzbWriteLogRequest WriteLogRequest = {};
	InitMessageBase(&WriteLogRequest.m_messageBase, rrWriteLog, sizeof(WriteLogRequest));
	WriteLogRequest.m_kind = htonl(kind);
	int length = strlen(text) + 1;
	WriteLogRequest.m_trailingDataLength = htonl(length);

	if (!m_connection->Send((char*)(&WriteLogRequest), sizeof(WriteLogRequest)))
	{
		perror("m_pConnection->Send");
		return false;
	}

	m_connection->Send(text, length);
	bool OK = ReceiveBoolResponse();
	m_connection->Disconnect();
	return OK;
}

bool RemoteClient::RequestScan(bool syncMode)
{
	if (!InitConnection()) return false;

	SNzbScanRequest ScanRequest = {};
	InitMessageBase(&ScanRequest.m_messageBase, rrScan, sizeof(ScanRequest));

	ScanRequest.m_syncMode = htonl(syncMode);

	bool OK = m_connection->Send((char*)(&ScanRequest), sizeof(ScanRequest));
	if (OK)
	{
		OK = ReceiveBoolResponse();
	}
	else
	{
		perror("m_pConnection->Send");
	}

	m_connection->Disconnect();
	return OK;
}

bool RemoteClient::RequestHistory(bool withHidden)
{
	if (!InitConnection()) return false;

	SNzbHistoryRequest HistoryRequest = {};
	InitMessageBase(&HistoryRequest.m_messageBase, rrHistory, sizeof(HistoryRequest));
	HistoryRequest.m_hidden = htonl(withHidden);

	if (!m_connection->Send((char*)(&HistoryRequest), sizeof(HistoryRequest)))
	{
		perror("m_pConnection->Send");
		return false;
	}

	printf("Request sent\n");

	// Now listen for the returned list
	SNzbHistoryResponse HistoryResponse;
	bool read = m_connection->Recv((char*) &HistoryResponse, sizeof(HistoryResponse));
	if (!nzbget_rs_tpr_binrpc_is_valid_response_header(
		read ? 1 : 0,
		(int)ntohl(HistoryResponse.m_messageBase.m_signature),
		(int)NZBMESSAGE_SIGNATURE,
		(uint32)ntohl(HistoryResponse.m_messageBase.m_structSize),
		(uint32)sizeof(HistoryResponse)))
	{
		printf("No response or invalid response (timeout, not nzbget-server or wrong nzbget-server version)\n");
		return false;
	}

	CharBuffer buf;
	if (ntohl(HistoryResponse.m_trailingDataLength) > 0)
	{
		buf.Reserve(ntohl(HistoryResponse.m_trailingDataLength));
		if (!m_connection->Recv(buf, buf.Size()))
		{
			return false;
		}
	}

	m_connection->Disconnect();

	if (ntohl(HistoryResponse.m_trailingDataLength) == 0)
	{
		printf("Server has no files in history\n");
	}
	else
	{
		printf("History (most recent first)\n");
		printf("-----------------------------------\n");

		// rust/src/binrpc.rs: the lines of all the entries that fit the data
		RS_PRINT_LISTING(nzbget_rs_binrpc_history_listing(buf, buf.Size(), ntohl(HistoryResponse.m_nrTrailingEntries)));

		printf("-----------------------------------\n");
		printf("Items: %i\n", ntohl(HistoryResponse.m_nrTrailingEntries));
	}

	return true;
}
