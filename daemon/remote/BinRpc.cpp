/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2005 Bo Cordes Petersen <placebodk@sourceforge.net>
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#include "nzbget.h"
#include "BinRpc.h"
#include "Log.h"
#include "Options.h"
#include "WorkState.h"
#include "QueueEditor.h"
#include "Util.h"
#include "FileSystem.h"
#include "DownloadInfo.h"
#include "Scanner.h"
#include "StatMeter.h"
#include "UrlCoordinator.h"
#include "nzbget_rs.h"

extern void ExitProc();
extern void Reload();

const uint32 g_MessageRequestSizes[] =
	{ 0,
		sizeof(SNzbDownloadRequest),
		sizeof(SNzbPauseUnpauseRequest),
		sizeof(SNzbListRequest),
		sizeof(SNzbSetDownloadRateRequest),
		sizeof(SNzbDumpDebugRequest),
		sizeof(SNzbEditQueueRequest),
		sizeof(SNzbLogRequest),
		sizeof(SNzbShutdownRequest),
		sizeof(SNzbReloadRequest),
		sizeof(SNzbVersionRequest),
		sizeof(SNzbPostQueueRequest),
		sizeof(SNzbWriteLogRequest),
		sizeof(SNzbScanRequest),
		sizeof(SNzbHistoryRequest)
	};

static_assert(sizeof(g_MessageRequestSizes) / sizeof(g_MessageRequestSizes[0]) == rrHistory + 1);


class BinCommand
{
public:
	virtual ~BinCommand() {}
	virtual void Execute() = 0;
	void SetConnection(Connection* connection) { m_connection = connection; }
	void SetMessageBase(SNzbRequestBase* messageBase) { m_messageBase = messageBase; }

protected:
	Connection* m_connection;
	SNzbRequestBase* m_messageBase;

	bool ReceiveRequest(void* buffer, int size);
	bool ReceiveTrailing(CharBuffer& buffer, int64 len, int64 maxLen);
	void SendBoolResponse(bool success, const char* text);
};

// a fixed-size string field of a request ends within its field
template <size_t N> void TerminateField(char (&field)[N]) { field[N - 1] = '\0'; }

class DownloadBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class ListBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class LogBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class PauseUnpauseBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class EditQueueBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class SetDownloadRateBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class DumpDebugBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class ShutdownBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class ReloadBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class VersionBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class PostQueueBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class WriteLogBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class ScanBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

class HistoryBinCommand: public BinCommand
{
public:
	virtual void Execute();
};

//*****************************************************************
// BinProcessor

BinRpcProcessor::BinRpcProcessor()
{
	m_messageBase.m_signature = (int)NZBMESSAGE_SIGNATURE;
}

void BinRpcProcessor::Execute()
{
	// Read the first package which needs to be a request
	if (!m_connection->Recv(((char*)&m_messageBase) + sizeof(m_messageBase.m_signature), sizeof(m_messageBase) - sizeof(m_messageBase.m_signature)))
	{
		warn("Non-nzbget request received on port %i from %s", g_Options->GetControlPort(), m_connection->GetRemoteAddr());
		return;
	}

	// rust/src/binrpc.rs: the credentials and the request type
	uint32 type = 0;
	if (!nzbget_rs_binrpc_check(reinterpret_cast<const unsigned char*>(&m_messageBase), sizeof(m_messageBase),
		g_Options->GetControlUsername(), g_Options->GetControlPassword(), &type))
	{
		warn("nzbget request received on port %i from %s, but username or password invalid", g_Options->GetControlPort(), m_connection->GetRemoteAddr());
		return;
	}

	debug("%s request received from %s", nzbget_rs_binrpc_name(type), m_connection->GetRemoteAddr());

	Dispatch();
}


void BinRpcProcessor::Dispatch()
{
	// rust/src/binrpc.rs
	uint32 expectedSize = 0, receivedSize = 0;
	if (!nzbget_rs_binrpc_size_ok(reinterpret_cast<const unsigned char*>(&m_messageBase), sizeof(m_messageBase),
		g_MessageRequestSizes, sizeof(g_MessageRequestSizes) / sizeof(g_MessageRequestSizes[0]), &expectedSize, &receivedSize))
	{
		error("Invalid size of request: expected %i Bytes, but received %i Bytes", expectedSize, receivedSize);
		return;
	}

	std::unique_ptr<BinCommand> command;

	switch (ntohl(m_messageBase.m_type))
	{
		case rrDownload:
			command = std::make_unique<DownloadBinCommand>();
			break;

		case rrList:
			command = std::make_unique<ListBinCommand>();
			break;

		case rrLog:
			command = std::make_unique<LogBinCommand>();
			break;

		case rrPauseUnpause:
			command = std::make_unique<PauseUnpauseBinCommand>();
			break;

		case rrEditQueue:
			command = std::make_unique<EditQueueBinCommand>();
			break;

		case rrSetDownloadRate:
			command = std::make_unique<SetDownloadRateBinCommand>();
			break;

		case rrDumpDebug:
			command = std::make_unique<DumpDebugBinCommand>();
			break;

		case rrShutdown:
			command = std::make_unique<ShutdownBinCommand>();
			break;

		case rrReload:
			command = std::make_unique<ReloadBinCommand>();
			break;

		case rrVersion:
			command = std::make_unique<VersionBinCommand>();
			break;

		case rrPostQueue:
			command = std::make_unique<PostQueueBinCommand>();
			break;

		case rrWriteLog:
			command = std::make_unique<WriteLogBinCommand>();
			break;

		case rrScan:
			command = std::make_unique<ScanBinCommand>();
			break;

		case rrHistory:
			command = std::make_unique<HistoryBinCommand>();
			break;

		default:
			error("Received unsupported request %i", ntohl(m_messageBase.m_type));
			break;
	}

	if (command)
	{
		command->SetConnection(m_connection);
		command->SetMessageBase(&m_messageBase);
		command->Execute();
	}
}

// The entries of one kind of a list response, collected under the locks as
// plain structs (the first Inline of them on the stack, the strings pointing
// into the queue or log): rust/src/binrpc.rs encodes them all in one call
template <class T, size_t Inline>
class RsBatch
{
public:
	RsBatch() : m_data(m_inline), m_capacity(Inline) {}
	RsBatch(const RsBatch&) = delete;

	// room for a count of entries known beforehand
	void Reserve(size_t count)
	{
		if (count > m_capacity)
		{
			Grow(count);
		}
	}

	void Add(const T& entry)
	{
		if (m_size == m_capacity)
		{
			Grow(m_capacity * 2);
		}
		m_data[m_size++] = entry;
	}

	const T* Data() const { return m_data; }
	size_t Size() const { return m_size; }

private:
	T m_inline[Inline]; // not initialized: the entries are plain structs
	std::unique_ptr<T[]> m_heap;
	T* m_data;
	size_t m_capacity;
	size_t m_size = 0;

	void Grow(size_t capacity)
	{
		std::unique_ptr<T[]> heap(new T[capacity]);
		memcpy(static_cast<void*>(heap.get()), m_data, m_size * sizeof(T));
		m_heap = std::move(heap);
		m_data = m_heap.get();
		m_capacity = capacity;
	}
};

// Match Rust's repr(C) layout, including pointer alignment on 32-bit targets.
#define RS_LAYOUT(type, first, ints, strings) \
    static_assert(offsetof(type, first) == ((4 * (ints) + alignof(NzbgetRsBinText) - 1) & \
        ~(alignof(NzbgetRsBinText) - 1)) && \
        sizeof(type) == offsetof(type, first) + (strings) * sizeof(NzbgetRsBinText), "BinRpc entry layout")
RS_LAYOUT(NzbgetRsBinNzb, filename, 12, 5);
RS_LAYOUT(NzbgetRsBinParam, name, 1, 2);
RS_LAYOUT(NzbgetRsBinFile, subject, 10, 2);
RS_LAYOUT(NzbgetRsBinLog, text, 3, 1);
RS_LAYOUT(NzbgetRsBinPost, filename, 6, 4);
RS_LAYOUT(NzbgetRsBinHistory, name, 9, 1);
#undef RS_LAYOUT

static NzbgetRsBinText RsText(const char* text)
{
	return {text, text ? strlen(text) : 0};
}

// The trailing data of a list response, encoded by rust/src/binrpc.rs
class RsBytes
{
public:
	RsBytes() : m_bytes{} {}
	char* Scratch() { return m_scratch; }
	size_t Capacity() const { return sizeof(m_scratch); }
	RsBytes(const RsBytes&) = delete;
	~RsBytes() { Free(); }

	RsBytes& operator=(NzbgetRsBinResponse bytes)
	{
		Free();
		m_bytes = bytes;
		return *this;
	}

	int Size() { return (int)m_bytes.len; }

	// the bytes, sent after the response
	void Send(Connection* connection)
	{
		if (m_bytes.len > 0)
		{
			connection->Send(m_bytes.data, (int)m_bytes.len);
		}
	}

private:
	NzbgetRsBinResponse m_bytes;
    // A complete small response and its final NUL, alive until Send returns.
    char m_scratch[16384];
    void Free()
    {
        if (m_bytes.ownedCapacity != 0)
        {
            nzbget_rs_free({m_bytes.data, m_bytes.len, m_bytes.ownedCapacity});
        }
    }
};

//*****************************************************************
// Commands

void BinCommand::SendBoolResponse(bool success, const char* text)
{
	// rust/src/binrpc.rs: the response, then the text and its NUL
	NzbgetRsBuf response = nzbget_rs_binrpc_bool_response(success, text);
	m_connection->Send(response.data, sizeof(SNzbDownloadResponse));
	m_connection->Send(response.data + sizeof(SNzbDownloadResponse), (int)(response.len - sizeof(SNzbDownloadResponse)));
	nzbget_rs_free(response);
}


bool BinCommand::ReceiveRequest(void* buffer, int size)
{
	memcpy(buffer, m_messageBase, sizeof(SNzbRequestBase));
	size -= sizeof(SNzbRequestBase);
	if (size > 0)
	{
		if (!m_connection->Recv(((char*)buffer) + sizeof(SNzbRequestBase), size))
		{
			error("invalid request");
			return false;
		}
	}
	return true;
}

// The data following a request: its length is the client's and is checked (no
// buffer was allocated for 0 or a negative one, and the data was used all the
// same); the buffer gets a terminator after it, as its strings are read with
// the C string functions
bool BinCommand::ReceiveTrailing(CharBuffer& buffer, int64 len, int64 maxLen)
{
	if (!nzbget_rs_binrpc_trailing_ok(len, maxLen))
	{
		error("invalid request: data length %" PRIi64, len);
		return false;
	}
	buffer.Reserve((int)len + 1);
	if (!buffer)
	{
		error("invalid request: no memory for %" PRIi64 " bytes", len);
		return false;
	}
	if (len > 0 && !m_connection->Recv(buffer, (int)len))
	{
		error("invalid request");
		return false;
	}
	buffer[(int)len] = '\0';
	return true;
}

void PauseUnpauseBinCommand::Execute()
{
	SNzbPauseUnpauseRequest PauseUnpauseRequest;
	if (!ReceiveRequest(&PauseUnpauseRequest, sizeof(PauseUnpauseRequest)))
	{
		return;
	}

	g_WorkState->SetResumeTime(0);

	switch (ntohl(PauseUnpauseRequest.m_action))
	{
		case rpDownload:
			g_WorkState->SetPauseDownload(ntohl(PauseUnpauseRequest.m_pause));
			break;

		case rpPostProcess:
			g_WorkState->SetPausePostProcess(ntohl(PauseUnpauseRequest.m_pause));
			break;

		case rpScan:
			g_WorkState->SetPauseScan(ntohl(PauseUnpauseRequest.m_pause));
			break;
	}

	SendBoolResponse(true, "Pause-/Unpause-Command completed successfully");
}

void SetDownloadRateBinCommand::Execute()
{
	SNzbSetDownloadRateRequest SetDownloadRequest;
	if (!ReceiveRequest(&SetDownloadRequest, sizeof(SetDownloadRequest)))
	{
		return;
	}

	g_WorkState->SetSpeedLimit(ntohl(SetDownloadRequest.m_downloadRate));
	SendBoolResponse(true, "Rate-Command completed successfully");
}

void DumpDebugBinCommand::Execute()
{
	SNzbDumpDebugRequest DumpDebugRequest;
	if (!ReceiveRequest(&DumpDebugRequest, sizeof(DumpDebugRequest)))
	{
		return;
	}

	g_Log->LogDebugInfo();
	SendBoolResponse(true, "Debug-Command completed successfully");
}

void ShutdownBinCommand::Execute()
{
	SNzbShutdownRequest ShutdownRequest;
	if (!ReceiveRequest(&ShutdownRequest, sizeof(ShutdownRequest)))
	{
		return;
	}

	SendBoolResponse(true, "Stopping server");
	ExitProc();
}

void ReloadBinCommand::Execute()
{
	SNzbReloadRequest ReloadRequest;
	if (!ReceiveRequest(&ReloadRequest, sizeof(ReloadRequest)))
	{
		return;
	}

	SendBoolResponse(true, "Reloading server");
	Reload();
}

void VersionBinCommand::Execute()
{
	SNzbVersionRequest VersionRequest;
	if (!ReceiveRequest(&VersionRequest, sizeof(VersionRequest)))
	{
		return;
	}

	SendBoolResponse(true, Util::VersionRevision());
}

void DownloadBinCommand::Execute()
{
	SNzbDownloadRequest DownloadRequest;
	if (!ReceiveRequest(&DownloadRequest, sizeof(DownloadRequest)))
	{
		return;
	}

	TerminateField(DownloadRequest.m_nzbFilename);
	TerminateField(DownloadRequest.m_category);
	TerminateField(DownloadRequest.m_dupeKey);

	int bufLen = (int)ntohl(DownloadRequest.m_trailingDataLength);
	CharBuffer nzbContent;
	if (bufLen <= 0 || !ReceiveTrailing(nzbContent, bufLen, 768 * 1024 * 1024))
	{
		error("invalid request");
		return;
	}

	int priority = ntohl(DownloadRequest.m_priority);
	bool addPaused = ntohl(DownloadRequest.m_addPaused);
	bool addTop = ntohl(DownloadRequest.m_addFirst);
	bool autoCategory = ntohl(DownloadRequest.m_autoCategory);
	int dupeMode = ntohl(DownloadRequest.m_dupeMode);
	int dupeScore = ntohl(DownloadRequest.m_dupeScore);

	bool ok = false;

	if (nzbget_rs_binrpc_is_url(nzbContent))
	{
		// add url
		std::unique_ptr<NzbInfo> nzbInfo = std::make_unique<NzbInfo>();
		nzbInfo->SetKind(NzbInfo::nkUrl);
		nzbInfo->SetUrl(nzbContent);
		nzbInfo->SetFilename(DownloadRequest.m_nzbFilename);
		nzbInfo->SetCategory(DownloadRequest.m_category);
		nzbInfo->SetAutoCategory(autoCategory);
		nzbInfo->SetPriority(priority);
		nzbInfo->SetAddUrlPaused(addPaused);
		nzbInfo->SetDupeKey(DownloadRequest.m_dupeKey);
		nzbInfo->SetDupeScore(dupeScore);
		nzbInfo->SetDupeMode((EDupeMode)dupeMode);

		g_UrlCoordinator->AddUrlToQueue(std::move(nzbInfo), addTop);

		ok = true;
	}
	else
	{
		ok = g_Scanner->AddExternalFile(
			DownloadRequest.m_nzbFilename,
			DownloadRequest.m_category,
			autoCategory,
			priority,
			DownloadRequest.m_dupeKey,
			dupeScore,
			static_cast<EDupeMode>(dupeMode),
			nullptr,
			addTop,
			addPaused,
			nullptr,
			nullptr,
			nzbContent,
			bufLen,
			nullptr
		) != Scanner::asFailed;
	}

	{
		NzbgetRsBuf msgBuf = nzbget_rs_tpr_binrpc_format_download_response_msg(
			ok ? 1 : 0, FileSystem::BaseFileName(DownloadRequest.m_nzbFilename));
		SendBoolResponse(ok, msgBuf.data ? msgBuf.data : "");
		nzbget_rs_free(msgBuf);
	}
}

void ListBinCommand::Execute()
{
	SNzbListRequest ListRequest;
	if (!ReceiveRequest(&ListRequest, sizeof(ListRequest)))
	{
		return;
	}

	SNzbListResponse ListResponse;
	memset(&ListResponse, 0, sizeof(ListResponse));
	ListResponse.m_messageBase.m_signature = htonl(NZBMESSAGE_SIGNATURE);
	ListResponse.m_messageBase.m_structSize = htonl(sizeof(ListResponse));
	ListResponse.m_entrySize = htonl(sizeof(SNzbListResponseFileEntry));
	ListResponse.m_regExValid = 0;

	RsBytes entries;

	if (ntohl(ListRequest.m_fileList))
	{
		ERemoteMatchMode matchMode = (ERemoteMatchMode)ntohl(ListRequest.m_matchMode);
		bool matchGroup = ntohl(ListRequest.m_matchGroup);
		TerminateField(ListRequest.m_pattern);
		const char* pattern = ListRequest.m_pattern;

		std::unique_ptr<RegEx> regEx;
		if (matchMode == rmRegEx)
		{
			regEx = std::make_unique<RegEx>(pattern);
			ListResponse.m_regExValid = regEx->IsValid();
		}

		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		int nrNzbEntries = downloadQueue->GetQueue()->size();
		int nrPPPEntries = 0;
		int nrFileEntries = 0;

		// the entries, collected under the lock for rust/src/binrpc.rs
		RsBatch<NzbgetRsBinNzb, 16> nzbEntries;
		RsBatch<NzbgetRsBinParam, 32> pppEntries;
		RsBatch<NzbgetRsBinFile, 32> fileEntries;
		size_t pppCount = 0, fileCount = 0;
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			pppCount += nzbInfo->GetParameters()->size();
			fileCount += nzbInfo->GetFileList()->size();
		}
		nzbEntries.Reserve(nrNzbEntries);
		pppEntries.Reserve(pppCount);
		fileEntries.Reserve(fileCount);

		// nzb entries
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			uint32 sizeHi, sizeLo, remainingSizeHi, remainingSizeLo, pausedSizeHi, pausedSizeLo;
			Util::SplitInt64(nzbInfo->GetSize(), &sizeHi, &sizeLo);
			Util::SplitInt64(nzbInfo->GetRemainingSize(), &remainingSizeHi, &remainingSizeLo);
			Util::SplitInt64(nzbInfo->GetPausedSize(), &pausedSizeHi, &pausedSizeLo);
			nzbEntries.Add({(uint32)nzbInfo->GetId(), (uint32)nzbInfo->GetKind(), sizeLo, sizeHi, remainingSizeLo,
				remainingSizeHi, pausedSizeLo, pausedSizeHi, (uint32)nzbInfo->GetPausedFileCount(),
				(uint32)nzbInfo->GetRemainingParCount(), (uint32)nzbInfo->GetPriority(),
				nzbget_rs_tpr_binrpc_eval_nzb_group_match(
					matchGroup ? 1 : 0, regEx ? 1 : 0,
					(matchGroup && regEx && regEx->Match(nzbInfo->GetName())) ? 1 : 0),
				RsText(nzbInfo->GetFilename()), RsText(nzbInfo->GetName()), RsText(nzbInfo->GetDestDir()),
				RsText(nzbInfo->GetCategory()), RsText(nzbInfo->GetQueuedFilename())});
		}

		// ppp entries
		int nzbIndex = 0;
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			nzbIndex++;
			for (NzbParameter& nzbParameter : nzbInfo->GetParameters())
			{
				pppEntries.Add({(uint32)nzbIndex, RsText(nzbParameter.GetName()), RsText(nzbParameter.GetValue())});
				nrPPPEntries++;
			}
		}

		// file entries: the usual owner is already known from this walk.
		int ownerIndex = 0;
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			ownerIndex++;
			for (FileInfo* fileInfo : nzbInfo->GetFileList())
			{
				nrFileEntries++;
				int nzbIndex = ownerIndex;
				if (fileInfo->GetNzbInfo() != nzbInfo)
				{
					// Retain the first-match behavior for an unusual parent.
					nzbIndex = 0;
					for (uint32 i = 0; i < downloadQueue->GetQueue()->size(); i++)
					{
						nzbIndex++;
						if (downloadQueue->GetQueue()->at(i).get() == fileInfo->GetNzbInfo())
						{
							break;
						}
					}
				}

				bool match = false;
				if (regEx && !matchGroup)
				{
					NzbgetRsBuf matchNameBuf = nzbget_rs_tpr_binrpc_format_file_match_name(
						fileInfo->GetNzbInfo()->GetName(), FileSystem::BaseFileName(fileInfo->GetFilename()));
					match = regEx->Match(matchNameBuf.data ? matchNameBuf.data : "");
					nzbget_rs_free(matchNameBuf);
				}

				uint32 fileSizeHi, fileSizeLo, remainingSizeHi, remainingSizeLo;
				Util::SplitInt64(fileInfo->GetSize(), &fileSizeHi, &fileSizeLo);
				Util::SplitInt64(fileInfo->GetRemainingSize(), &remainingSizeHi, &remainingSizeLo);
				fileEntries.Add({(uint32)fileInfo->GetId(), (uint32)nzbIndex, fileSizeLo, fileSizeHi, remainingSizeLo,
					remainingSizeHi, (uint32)fileInfo->GetPaused(), (uint32)fileInfo->GetFilenameConfirmed(),
					(uint32)fileInfo->GetActiveDownloads(), (uint32)match,
					RsText(fileInfo->GetSubject()), RsText(fileInfo->GetFilename())});
			}
		}

		entries = nzbget_rs_binrpc_encode_list_into(nzbEntries.Data(), nzbEntries.Size(), pppEntries.Data(), pppEntries.Size(),
			fileEntries.Data(), fileEntries.Size(), entries.Scratch(), entries.Capacity());

		ListResponse.m_nrTrailingNzbEntries = htonl(nrNzbEntries);
		ListResponse.m_nrTrailingPPPEntries = htonl(nrPPPEntries);
		ListResponse.m_nrTrailingFileEntries = htonl(nrFileEntries);
		ListResponse.m_trailingDataLength = htonl(entries.Size());
	}

	if (htonl(ListRequest.m_serverState))
	{
		int postJobCount = 0;
		int64 remainingSize;
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
			{
				postJobCount += nzbInfo->GetPostInfo() ? 1 : 0;
			}
			downloadQueue->CalcRemainingSize(&remainingSize, nullptr);
		}

		uint32 sizeHi, sizeLo;
		uint32 downloadRate = Util::SafeIntCast<int64, uint32>(g_StatMeter->CalcCurrentDownloadSpeed());
		ListResponse.m_downloadRate = htonl(downloadRate);
		Util::SplitInt64(remainingSize, &sizeHi, &sizeLo);
		ListResponse.m_remainingSizeHi = htonl(sizeHi);
		ListResponse.m_remainingSizeLo = htonl(sizeLo);
		ListResponse.m_downloadLimit = htonl(g_WorkState->GetSpeedLimit());
		ListResponse.m_downloadPaused = htonl(g_WorkState->GetPauseDownload());
		ListResponse.m_postPaused = htonl(g_WorkState->GetPausePostProcess());
		ListResponse.m_scanPaused = htonl(g_WorkState->GetPauseScan());
		ListResponse.m_threadCount = htonl(Thread::GetThreadCount() - 1); // not counting itself
		ListResponse.m_postJobCount = htonl(postJobCount);

		int upTimeSec, dnTimeSec;
		int64 allBytes;
		bool standBy;
		g_StatMeter->CalcTotalStat(&upTimeSec, &dnTimeSec, &allBytes, &standBy);
		ListResponse.m_upTimeSec = htonl(upTimeSec);
		ListResponse.m_downloadTimeSec = htonl(dnTimeSec);
		ListResponse.m_downloadStandBy = htonl(standBy);
		Util::SplitInt64(allBytes, &sizeHi, &sizeLo);
		ListResponse.m_downloadedBytesHi = htonl(sizeHi);
		ListResponse.m_downloadedBytesLo = htonl(sizeLo);
	}

	// Send the request answer
	m_connection->Send((char*) &ListResponse, sizeof(ListResponse));

	// Send the data
	entries.Send(m_connection);
}


void LogBinCommand::Execute()
{
	SNzbLogRequest LogRequest;
	if (!ReceiveRequest(&LogRequest, sizeof(LogRequest)))
	{
		return;
	}

	RsBytes entries;
	int nrEntries;
	int bufsize;

	{
		GuardedMessageList messages = g_Log->GuardMessages();

		int start = 0;
		nzbget_rs_sap_binrpc_log_slice_bounds(
			ntohl(LogRequest.m_lines),
			ntohl(LogRequest.m_idFrom),
			messages->size(),
			messages->empty() ? 0 : (uint32)messages->front().GetId(),
			&start, &nrEntries);

		// the messages from start on, collected under the lock for rust/src/binrpc.rs
		RsBatch<NzbgetRsBinLog, 32> log;
		if ((size_t)start < messages->size())
		{
			log.Reserve(messages->size() - (size_t)start);
		}
		for (uint32 i = (uint32)start; i < messages->size(); i++)
		{
			Message& message = messages->at(i);
			log.Add({(uint32)message.GetId(), (uint32)message.GetKind(), (uint32)(int)message.GetTime(),
				RsText(message.GetText())});
		}
		entries = nzbget_rs_binrpc_encode_log_into(log.Data(), log.Size(), entries.Scratch(), entries.Capacity());
		bufsize = entries.Size();
	}

	SNzbLogResponse LogResponse;
	LogResponse.m_messageBase.m_signature = htonl(NZBMESSAGE_SIGNATURE);
	LogResponse.m_messageBase.m_structSize = htonl(sizeof(LogResponse));
	LogResponse.m_entrySize = htonl(sizeof(SNzbLogResponseEntry));
	LogResponse.m_nrTrailingEntries = htonl(nrEntries);
	LogResponse.m_trailingDataLength = htonl(bufsize);

	// Send the request answer
	m_connection->Send((char*) &LogResponse, sizeof(LogResponse));

	// Send the data
	if (bufsize > 0)
	{
		entries.Send(m_connection);
	}
}


void EditQueueBinCommand::Execute()
{
	SNzbEditQueueRequest EditQueueRequest;
	if (!ReceiveRequest(&EditQueueRequest, sizeof(EditQueueRequest)))
	{
		return;
	}

	int nrIdEntries = ntohl(EditQueueRequest.m_nrTrailingIdEntries);
	int nrNameEntries = ntohl(EditQueueRequest.m_nrTrailingNameEntries);
	int nameEntriesLen = ntohl(EditQueueRequest.m_trailingNameEntriesLen);
	int action = ntohl(EditQueueRequest.m_action);
	int matchMode = ntohl(EditQueueRequest.m_matchMode);
	int offset = ntohl(EditQueueRequest.m_offset);
	int textLen = ntohl(EditQueueRequest.m_textLen);
	uint32 bufLength = ntohl(EditQueueRequest.m_trailingDataLength);

	// the parts' sizes from the client, checked against each other in 64 bits
	// (the unsigned sum wrapped for negative counts) and against the data
	if (!nzbget_rs_binrpc_edit_sizes(nrIdEntries, nrNameEntries, nameEntriesLen, textLen, bufLength))
	{
		error("Invalid struct size");
		return;
	}

	CharBuffer buf;
	if (!ReceiveTrailing(buf, bufLength, 64 * 1024 * 1024))
	{
		return;
	}
	if (textLen > 0)
	{
		buf[textLen - 1] = '\0';
	}

	if (nrIdEntries <= 0 && nrNameEntries <= 0)
	{
		SendBoolResponse(false, "Edit-Command failed: no IDs/Names specified");
		return;
	}

	// no text is an empty one: the edits read it as a string (a priority, a name)
	const char* text = textLen > 0 ? *buf : "";

	IdList cIdList;
	NameList cNameList;

	static_assert(sizeof(IdList::value_type) == sizeof(uint32_t), "ids are 32 bits");
	cIdList.resize(nrIdEntries);
	if (!nzbget_rs_binrpc_edit_split(buf, bufLength, textLen, nrIdEntries, nrNameEntries, (uint32_t*)cIdList.data()))
	{
		error("Invalid struct size");
		return;
	}
	// each name ends within the data: checked above
	cNameList.reserve(nrNameEntries);
	const char* names = buf + textLen + nrIdEntries * sizeof(int32);
	for (int i = 0; i < nrNameEntries; i++)
	{
		cNameList.push_back(names);
		names += strlen(names) + 1;
	}

	bool useOffsetStr = nzbget_rs_sap_binrpc_edit_queue_use_offset_str(
		action, (int)DownloadQueue::eaFileMoveOffset, (int)DownloadQueue::eaGroupMoveOffset) != 0;
	NzbgetRsBuf offBuf = useOffsetStr ? nzbget_rs_dqh_conn_format_port_str(offset) : NzbgetRsBuf{};
	bool ok = DownloadQueue::Guard()->EditList(
		nrIdEntries > 0 ? &cIdList : nullptr,
		nrNameEntries > 0 ? &cNameList : nullptr,
		(DownloadQueue::EMatchMode)matchMode, (DownloadQueue::EEditAction)action,
		useOffsetStr ? (offBuf.data ? offBuf.data : "0") : text);
	if (useOffsetStr)
	{
		nzbget_rs_free(offBuf);
	}

	if (ok)
	{
		SendBoolResponse(true, "Edit-Command completed successfully");
	}
	else
	{
#ifndef HAVE_REGEX_H
		if ((DownloadQueue::EMatchMode)matchMode == DownloadQueue::mmRegEx)
		{
			SendBoolResponse(false, "Edit-Command failed: the program was compiled without RegEx-support");
			return;
		}
#endif
		SendBoolResponse(false, "Edit-Command failed");
	}
}

void PostQueueBinCommand::Execute()
{
	SNzbPostQueueRequest PostQueueRequest;
	if (!ReceiveRequest(&PostQueueRequest, sizeof(PostQueueRequest)))
	{
		return;
	}

	SNzbPostQueueResponse PostQueueResponse;
	memset(&PostQueueResponse, 0, sizeof(PostQueueResponse));
	PostQueueResponse.m_messageBase.m_signature = htonl(NZBMESSAGE_SIGNATURE);
	PostQueueResponse.m_messageBase.m_structSize = htonl(sizeof(PostQueueResponse));
	PostQueueResponse.m_entrySize = htonl(sizeof(SNzbPostQueueResponseEntry));

	RsBytes entries;

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		// Make a data structure and copy all the elements of the list into it
		NzbList* nzbList = downloadQueue->GetQueue();

		int NrEntries = 0;
		time_t curTime = Util::CurrentTime();
		// collected under the lock for rust/src/binrpc.rs
		RsBatch<NzbgetRsBinPost, 16> jobs;
		jobs.Reserve(nzbList->size());

		for (NzbInfo* nzbInfo : nzbList)
		{
			PostInfo* postInfo = nzbInfo->GetPostInfo();
			if (!postInfo)
			{
				continue;
			}

			NrEntries++;
			jobs.Add({(uint32)nzbInfo->GetId(), (uint32)postInfo->GetStage(), (uint32)postInfo->GetStageProgress(),
				(uint32)postInfo->GetFileProgress(),
				nzbget_rs_tpr_binrpc_calc_elapsed_sec((int64_t)curTime, (int64_t)postInfo->GetStartTime()),
				nzbget_rs_tpr_binrpc_calc_elapsed_sec((int64_t)curTime, (int64_t)postInfo->GetStageTime()),
				RsText(postInfo->GetNzbInfo()->GetFilename()), RsText(postInfo->GetNzbInfo()->GetName()),
				RsText(postInfo->GetNzbInfo()->GetDestDir()), RsText(postInfo->GetProgressLabel())});
		}
		entries = nzbget_rs_binrpc_encode_post_queue_into(jobs.Data(), jobs.Size(), entries.Scratch(), entries.Capacity());

		PostQueueResponse.m_nrTrailingEntries = htonl(NrEntries);
		PostQueueResponse.m_trailingDataLength = htonl(entries.Size());
	}

	// Send the request answer
	m_connection->Send((char*) &PostQueueResponse, sizeof(PostQueueResponse));

	// Send the data
	entries.Send(m_connection);
}


void WriteLogBinCommand::Execute()
{
	SNzbWriteLogRequest WriteLogRequest;
	if (!ReceiveRequest(&WriteLogRequest, sizeof(WriteLogRequest)))
	{
		return;
	}

	CharBuffer recvBuffer;
	if (!ReceiveTrailing(recvBuffer, (int)ntohl(WriteLogRequest.m_trailingDataLength), 1024 * 1024))
	{
		return;
	}

	bool OK = true;
	switch ((Message::EKind)ntohl(WriteLogRequest.m_kind))
	{
		case Message::mkDetail:
			detail("%s", *recvBuffer);
			break;
		case Message::mkInfo:
			info("%s", *recvBuffer);
			break;
		case Message::mkWarning:
			warn("%s", *recvBuffer);
			break;
		case Message::mkError:
			error("%s", *recvBuffer);
			break;
		case Message::mkDebug:
			debug("%s", *recvBuffer);
			break;
		default:
			OK = false;
	}
	SendBoolResponse(OK, nzbget_rs_dqh_binrpc_write_log_response_msg(OK ? 1 : 0));
}

void ScanBinCommand::Execute()
{
	SNzbScanRequest ScanRequest;
	if (!ReceiveRequest(&ScanRequest, sizeof(ScanRequest)))
	{
		return;
	}

	bool syncMode = ntohl(ScanRequest.m_syncMode);

	g_Scanner->ScanNzbDir(syncMode);
	SendBoolResponse(true, nzbget_rs_dqh_binrpc_scan_response_msg(syncMode ? 1 : 0));
}

void HistoryBinCommand::Execute()
{
	SNzbHistoryRequest HistoryRequest;
	if (!ReceiveRequest(&HistoryRequest, sizeof(HistoryRequest)))
	{
		return;
	}

	bool showHidden = ntohl(HistoryRequest.m_hidden);

	SNzbHistoryResponse HistoryResponse;
	memset(&HistoryResponse, 0, sizeof(HistoryResponse));
	HistoryResponse.m_messageBase.m_signature = htonl(NZBMESSAGE_SIGNATURE);
	HistoryResponse.m_messageBase.m_structSize = htonl(sizeof(HistoryResponse));
	HistoryResponse.m_entrySize = htonl(sizeof(SNzbHistoryResponseEntry));

	RsBytes entries;

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		int nrEntries = 0;
		// collected under the lock for rust/src/binrpc.rs
		RsBatch<NzbgetRsBinHistory, 32> history;
		history.Reserve(downloadQueue->GetHistory()->size());
		for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
		{
			if (nzbget_rs_tpr_binrpc_should_include_history_entry(
				(int)historyInfo->GetKind(), (int)HistoryInfo::hkDup, showHidden ? 1 : 0))
			{
				nrEntries++;
				// fields a kind doesn't set are 0
				uint32 sizeHi = 0, sizeLo = 0, fileCount = 0, parStatus = 0, scriptStatus = 0, urlStatus = 0;
				int histFieldKind = nzbget_rs_dqh_binrpc_classify_history_entry_fields(
					(int)historyInfo->GetKind(),
					(int)HistoryInfo::hkNzb,
					(int)HistoryInfo::hkDup,
					(int)HistoryInfo::hkUrl,
					showHidden ? 1 : 0);
				if (histFieldKind == 1)
				{
					NzbInfo* nzbInfo = historyInfo->GetNzbInfo();
					Util::SplitInt64(nzbInfo->GetSize(), &sizeHi, &sizeLo);
					fileCount = nzbInfo->GetFileCount();
					parStatus = nzbInfo->GetParStatus();
					scriptStatus = nzbInfo->GetScriptStatuses()->CalcTotalStatus();
				}
				else if (histFieldKind == 2)
				{
					DupInfo* dupInfo = historyInfo->GetDupInfo();
					Util::SplitInt64(dupInfo->GetSize(), &sizeHi, &sizeLo);
				}
				else if (histFieldKind == 3)
				{
					NzbInfo* nzbInfo = historyInfo->GetNzbInfo();
					urlStatus = nzbInfo->GetUrlStatus();
				}
				history.Add({(uint32)historyInfo->GetId(), (uint32)historyInfo->GetKind(), (uint32)(int)historyInfo->GetTime(),
					sizeLo, sizeHi, fileCount, parStatus, scriptStatus, urlStatus, RsText(historyInfo->GetName())});
			}
		}
		entries = nzbget_rs_binrpc_encode_history_into(history.Data(), history.Size(), entries.Scratch(), entries.Capacity());

		HistoryResponse.m_nrTrailingEntries = htonl(nrEntries);
		HistoryResponse.m_trailingDataLength = htonl(entries.Size());
	}

	// Send the request answer
	m_connection->Send((char*) &HistoryResponse, sizeof(HistoryResponse));

	// Send the data
	entries.Send(m_connection);
}
