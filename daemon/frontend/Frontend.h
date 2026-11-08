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
#include "Log.h"
#include "DownloadInfo.h"
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2005 Bo Cordes Petersen <placebodk@users.sourceforge.net>
 *  Copyright (C) 2007-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */


#ifndef MESSAGEBASE_H
#define MESSAGEBASE_H

static const int32 NZBMESSAGE_SIGNATURE = 0x6E7A6231; // = "nzb-XX" (protocol version)
static const int NZBREQUESTFILENAMESIZE = 512;
static const int NZBREQUESTPASSWORDSIZE = 32;

/**
* NZBGet communication protocol uses only two basic data types: integer and char.
* Integer values are passed using network byte order (Big-Endian).
* Use function "htonl" and "ntohl" to convert integers to/from machine
* (host) byte order.
* All char-strings ends with nullptr-char.
*
* NOTE:
* NZBGet communication protocol is intended for usage only by NZBGet itself.
* The communication works only if server and client has the same version.
* The compatibility with previous program versions is not provided.
* Third-party programs should use JSON-RPC or XML-RPC to communicate with NZBGet.
*/

// Possible values for field "m_type" of struct "SNZBRequestBase":
enum ERemoteRequest
{
	rrDownload = 1,
	rrPauseUnpause,
	rrList,
	rrSetDownloadRate,
	rrDumpDebug,
	rrEditQueue,
	rrLog,
	rrShutdown,
	rrReload,
	rrVersion,
	rrPostQueue,
	rrWriteLog,
	rrScan,
	rrHistory
};

// Possible values for field "m_action" of struct "SNZBPauseUnpauseRequest":
enum ERemotePauseUnpauseAction
{
	rpDownload = 1, // pause/unpause download queue
	rpPostProcess, // pause/unpause post-processor queue
	rpScan // pause/unpause scan of incoming nzb-directory
};

// Possible values for field "m_matchMode" of struct "SNZBEditQueueRequest":
enum ERemoteMatchMode
{
	rmId = 1, // ID
	rmName, // Name
	rmRegEx // RegEx
};

// The basic SNZBRequestBase struct, used in all requests
struct SNzbRequestBase
{
	int32 m_signature; // Signature must be NZBMESSAGE_SIGNATURE in integer-value
	int32 m_structSize; // Size of the entire struct
	int32 m_type; // Message type, see enum in NZBMessageRequest-namespace
	char m_username[NZBREQUESTPASSWORDSIZE]; // User name
	char m_password[NZBREQUESTPASSWORDSIZE]; // Password
};

// The basic SNZBResposneBase struct, used in all responses
struct SNzbResponseBase
{
	int32 m_signature; // Signature must be NZBMESSAGE_SIGNATURE in integer-value
	int32 m_structSize; // Size of the entire struct
};

// A download request
struct SNzbDownloadRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
	char m_nzbFilename[NZBREQUESTFILENAMESIZE]; // Name of nzb-file. For URLs can be empty, then the filename is read from URL download response
	char m_category[NZBREQUESTFILENAMESIZE]; // Category, can be empty
	int32 m_addFirst; // 1 - add file to the top of download queue
	int32 m_addPaused; // 1 - pause added files
	int32 m_priority; // Priority for files (0 - default)
	int32 m_dupeScore; // Duplicate score
	int32 m_dupeMode; // Duplicate mode (EDupeMode)
	char m_dupeKey[NZBREQUESTFILENAMESIZE]; // Duplicate key
	int32 m_trailingDataLength; // Length of nzb-file in bytes
	int32 m_autoCategory;
	//char m_content[m_trailingDataLength]; // variable sized
};

// A download response
struct SNzbDownloadResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
	//char m_text[m_trailingDataLength]; // variable sized
};

// A list and status request
struct SNzbListRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
	int32 m_fileList; // 1 - return file list
	int32 m_serverState; // 1 - return server state
	int32 m_matchMode; // File/Group match mode, see enum eRemoteMatchMode (only values eRemoteMatchModeID (no filter) and eRemoteMatchModeRegEx are allowed)
	int32 m_matchGroup; // 0 - match files; 1 - match nzbs (when m_iMatchMode == eRemoteMatchModeRegEx)
	char m_pattern[NZBREQUESTFILENAMESIZE]; // RegEx Pattern (when m_iMatchMode == eRemoteMatchModeRegEx)
};

// A list response
struct SNzbListResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_entrySize; // Size of the SNZBListResponseEntry-struct
	int32 m_remainingSizeLo; // Remaining size in bytes, Low 32-bits of 64-bit value
	int32 m_remainingSizeHi; // Remaining size in bytes, High 32-bits of 64-bit value
	int32 m_downloadRate; // Current download speed, in Bytes pro Second
	int32 m_downloadLimit; // Current download limit, in Bytes pro Second
	int32 m_downloadPaused; // 1 - download queue is currently in paused-state
	int32 m_download2Paused; // 1 - download queue is currently in paused-state (second pause-register)
	int32 m_downloadStandBy; // 0 - there are currently downloads running, 1 - no downloads in progress (download queue paused or all download jobs completed)
	int32 m_postPaused; // 1 - post-processor queue is currently in paused-state
	int32 m_scanPaused; // 1 - scaning of incoming directory is currently in paused-state
	int32 m_threadCount; // Number of threads running
	int32 m_postJobCount; // Number of jobs in post-processor queue (including current job)
	int32 m_upTimeSec; // Server up time in seconds
	int32 m_downloadTimeSec; // Server download time in seconds (up_time - standby_time)
	int32 m_downloadedBytesLo; // Amount of data downloaded since server start, Low 32-bits of 64-bit value
	int32 m_downloadedBytesHi; // Amount of data downloaded since server start, High 32-bits of 64-bit value
	int32 m_regExValid; // 0 - error in RegEx-pattern, 1 - RegEx-pattern is valid (only when Request has eRemoteMatchModeRegEx)
	int32 m_nrTrailingNzbEntries; // Number of List-NZB-entries, following to this structure
	int32 m_nrTrailingPPPEntries; // Number of List-PPP-entries, following to this structure
	int32 m_nrTrailingFileEntries; // Number of List-File-entries, following to this structure
	int32 m_trailingDataLength; // Length of all List-entries, following to this structure
	// SNzbListResponseEntry m_nzbEntries[m_nrTrailingNZBEntries] // variable sized
	// SNzbListResponseEntry m_pppEntries[m_nrTrailingPPPEntries] // variable sized
	// SNzbListResponseEntry m_fileEntries[m_nrTrailingFileEntries] // variable sized
};

// A list response nzb entry
struct SNzbListResponseNzbEntry
{
	int32 m_id; // NZB-ID
	int32 m_kind; // Item Kind (see NZBInfo::Kind)
	int32 m_sizeLo; // Size of all files in bytes, Low 32-bits of 64-bit value
	int32 m_sizeHi; // Size of all files in bytes, High 32-bits of 64-bit value
	int32 m_remainingSizeLo; // Size of remaining (unpaused) files in bytes, Low 32-bits of 64-bit value
	int32 m_remainingSizeHi; // Size of remaining (unpaused) files in bytes, High 32-bits of 64-bit value
	int32 m_pausedSizeLo; // Size of npaused files in bytes, Low 32-bits of 64-bit value
	int32 m_pausedSizeHi; // Size of paused files in bytes, High 32-bits of 64-bit value
	int32 m_pausedCount; // Number of paused files
	int32 m_remainingParCount; // Number of remaining par-files
	int32 m_priority; // Download priority
	int32 m_match; // 1 - group matches the pattern (only when Request has eRemoteMatchModeRegEx)
	int32 m_filenameLen; // Length of Filename-string (m_szFilename), following to this record
	int32 m_nameLen; // Length of Name-string (m_szName), following to this record
	int32 m_destDirLen; // Length of DestDir-string (m_szDestDir), following to this record
	int32 m_categoryLen; // Length of Category-string (m_szCategory), following to this record
	int32 m_queuedFilenameLen; // Length of queued file name (m_szQueuedFilename), following to this record
	//char m_filename[m_filenameLen]; // variable sized
	//char m_name[m_nameLen]; // variable sized
	//char m_destDir[m_destDirLen]; // variable sized
	//char m_dategory[m_categoryLen]; // variable sized
	//char m_queuedFilename[m_queuedFilenameLen]; // variable sized
};

// A list response pp-parameter entry
struct SNzbListResponsePPPEntry
{
	int32 m_nzbIndex; // Index of NZB-Entry in m_NZBEntries-list
	int32 m_nameLen; // Length of Name-string (m_szName), following to this record
	int32 m_valueLen; // Length of Value-string (m_szValue), following to this record
	//char m_name[m_nameLen]; // variable sized
	//char m_value[m_valueLen]; // variable sized
};

// A list response file entry
struct SNzbListResponseFileEntry
{
	int32 m_id; // Entry-ID
	int32 m_nzbIndex; // Index of NZB-Entry in m_NZBEntries-list
	int32 m_fileSizeLo; // Filesize in bytes, Low 32-bits of 64-bit value
	int32 m_fileSizeHi; // Filesize in bytes, High 32-bits of 64-bit value
	int32 m_remainingSizeLo; // Remaining size in bytes, Low 32-bits of 64-bit value
	int32 m_remainingSizeHi; // Remaining size in bytes, High 32-bits of 64-bit value
	int32 m_paused; // 1 - file is paused
	int32 m_filenameConfirmed; // 1 - Filename confirmed (read from article body), 0 - Filename parsed from subject (can be changed after reading of article)
	int32 m_activeDownloads; // Number of active downloads for this file
	int32 m_match; // 1 - file matches the pattern (only when Request has eRemoteMatchModeRegEx)
	int32 m_subjectLen; // Length of Subject-string (m_szSubject), following to this record
	int32 m_filenameLen; // Length of Filename-string (m_szFilename), following to this record
	//char m_subject[m_subjectLen]; // variable sized
	//char m_filename[m_filenameLen]; // variable sized
};

// A log request
struct SNzbLogRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
	int32 m_idFrom; // Only one of these two parameters
	int32 m_lines; // can be set. The another one must be set to "0".
};

// A log response
struct SNzbLogResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_entrySize; // Size of the SNZBLogResponseEntry-struct
	int32 m_nrTrailingEntries; // Number of Log-entries, following to this structure
	int32 m_trailingDataLength; // Length of all Log-entries, following to this structure
	// SNZBLogResponseEntry m_entries[m_trailingEntries] // variable sized
};

// A log response entry
struct SNzbLogResponseEntry
{
	int32 m_id; // ID of Log-entry
	int32 m_kind; // see Message::Kind in "Log.h"
	int32 m_time; // time since the Epoch (00:00:00 UTC, January 1, 1970), measured in seconds.
	int32 m_textLen; // Length of Text-string (m_szText), following to this record
	//char m_text[m_textLen]; // variable sized
};

// A Pause/Unpause request
struct SNzbPauseUnpauseRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
	int32 m_pause; // 1 - server must be paused, 0 - server must be unpaused
	int32 m_action; // Action to be executed, see enum ERemotePauseUnpauseAction
};

// A Pause/Unpause response
struct SNzbPauseUnpauseResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
	//char m_text[m_trailingDataLength]; // variable sized
};

// Request setting the download rate
struct SNzbSetDownloadRateRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
	int32 m_downloadRate; // Speed limit, in Bytes pro Second
};

// A setting download rate response
struct SNzbSetDownloadRateResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
								//char m_text[m_trailingDataLength]; // variable sized
};

// edit queue request
struct SNzbEditQueueRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
	int32 m_action; // Action to be executed, see enum DownloadQueue::EEditAction
	int32 m_offset; // Offset to move (for m_iAction = 0)
	int32 m_matchMode; // File/Group match mode, see enum eRemoteMatchMode
	int32 m_nrTrailingIdEntries; // Number of ID-entries, following to this structure
	int32 m_nrTrailingNameEntries; // Number of Name-entries, following to this structure
	int32 m_trailingNameEntriesLen; // Length of all Name-entries, following to this structure
	int32 m_textLen; // Length of Text-string (m_szText), following to this record
	int32 m_trailingDataLength; // Length of Text-string and all ID-entries, following to this structure
	//char m_text[m_textLen]; // variable sized
	//int32 m_ids[m_nrTrailingIdEntries]; // variable sized array of IDs. For File-Actions - ID of file, for Group-Actions - ID of any file belonging to group
	//char* m_names[m_nrTrailingNameEntries]; // variable sized array of strings. For File-Actions - name of file incl. nzb-name as path, for Group-Actions - name of group
};

// An edit queue response
struct SNzbEditQueueResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
	//char m_text[m_trailingDataLength]; // variable sized
};

// Request dumping of debug info
struct SNzbDumpDebugRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
};

// Dumping of debug response
struct SNzbDumpDebugResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
	//char m_text[m_trailingDataLength]; // variable sized
};

// Shutdown server request
struct SNzbShutdownRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
};

// Shutdown server response
struct SNzbShutdownResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
	//char m_text[m_trailingDataLength]; // variable sized
};

// Reload server request
struct SNzbReloadRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
};

// Reload server response
struct SNzbReloadResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
	//char m_text[m_trailingDataLength]; // variable sized
};

// Server version request
struct SNzbVersionRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
};

// Server version response
struct SNzbVersionResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
	//char m_text[m_trailingDataLength]; // variable sized
};

// PostQueue request
struct SNzbPostQueueRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
};

// A PostQueue response
struct SNzbPostQueueResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_entrySize; // Size of the SNZBPostQueueResponseEntry-struct
	int32 m_nrTrailingEntries; // Number of PostQueue-entries, following to this structure
	int32 m_trailingDataLength; // Length of all PostQueue-entries, following to this structure
	// SNZBPostQueueResponseEntry m_entries[m_nrTrailingEntries] // variable sized
};

// A PostQueue response entry
struct SNzbPostQueueResponseEntry
{
	int32 m_id; // ID of Post-entry
	int32 m_stage; // See PrePostProcessor::EPostJobStage
	int32 m_stageProgress; // Progress of current stage, value in range 0..1000
	int32 m_fileProgress; // Progress of current file, value in range 0..1000
	int32 m_totalTimeSec; // Number of seconds this post-job is beeing processed (after it first changed the state from QUEUED).
	int32 m_stageTimeSec; // Number of seconds the current stage is beeing processed.
	int32 m_nzbFilenameLen; // Length of NZBFileName-string (m_szNZBFilename), following to this record
	int32 m_infoNameLen; // Length of Filename-string (m_szFilename), following to this record
	int32 m_destDirLen; // Length of DestDir-string (m_szDestDir), following to this record
	int32 m_progressLabelLen; // Length of ProgressLabel-string (m_szProgressLabel), following to this record
	//char m_nzbFilename[m_nzbFilenameLen]; // variable sized, may contain full path (local path on client) or only filename
	//char m_infoName[m_infoNameLen]; // variable sized
	//char m_destDir[m_destDirLen]; // variable sized
	//char m_progressLabel[m_progressLabelLen]; // variable sized
};

// Write log request
struct SNzbWriteLogRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
	int32 m_kind; // see Message::Kind in "Log.h"
	int32 m_trailingDataLength; // Length of nzb-file in bytes
	//char m_text[m_trailingDataLength]; // variable sized
};

// Write log response
struct SNzbWriteLogResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
	//char m_text[m_trailingDataLength]; // variable sized
};

// Scan nzb directory request
struct SNzbScanRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
	int32 m_syncMode; // 0 - asynchronous Scan (the command returns immediately), 1 - synchronous Scan (the command returns when the scan is completed)
};

// Scan nzb directory response
struct SNzbScanResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_success; // 0 - command failed, 1 - command executed successfully
	int32 m_trailingDataLength; // Length of Text-string (m_szText), following to this record
	//char m_text[m_trailingDataLength]; // variable sized
};

// A history request
struct SNzbHistoryRequest
{
	SNzbRequestBase m_messageBase; // Must be the first in the struct
	int32 m_hidden; // 0 - only return visible records, 1 - also return hidden records
};

// history response
struct SNzbHistoryResponse
{
	SNzbResponseBase m_messageBase; // Must be the first in the struct
	int32 m_entrySize; // Size of the SNZBHistoryResponseEntry-struct
	int32 m_nrTrailingEntries; // Number of History-entries, following to this structure
	int32 m_trailingDataLength; // Length of all History-entries, following to this structure
	// SNZBHistoryResponseEntry m_entries[m_nrTrailingEntries] // variable sized
};

// history entry
struct SNzbHistoryResponseEntry
{
	int32 m_id; // History-ID
	int32 m_kind; // Kind of Item: 1 - Collection (NZB), 2 - URL, 3 - DUP (hidden record)
	int32 m_time; // When the item was added to history. time since the Epoch (00:00:00 UTC, January 1, 1970), measured in seconds.
	int32 m_nicenameLen; // Length of Nicename-string (m_szNicename), following to this record
	// for Collection and Dup items (m_iKind = 1 or 2)
	int32 m_sizeLo; // Size of all files in bytes, Low 32-bits of 64-bit value
	int32 m_sizeHi; // Size of all files in bytes, High 32-bits of 64-bit value
	// for Collection items (m_iKind = 1)
	int32 m_fileCount; // Initial number of files included in NZB-file
	int32 m_parStatus; // See NZBInfo::EParStatus
	int32 m_scriptStatus; // See NZBInfo::EScriptStatus
	// for URL items (m_iKind = 2)
	int32 m_urlStatus; // See NZBInfo::EUrlStatus
	//char m_nicename[m_nicenameLen]; // variable sized
};

#endif

#include "QueueCoordinator.h"
#include "Options.h"
#include "Connection.h"
/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2005 Bo Cordes Petersen <placebodk@users.sourceforge.net>
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef REMOTECLIENT_H
#define REMOTECLIENT_H

#include "Options.h"
#include "Connection.h"
#include "DownloadInfo.h"

class RemoteClient
{
public:
	void SetVerbose(bool verbose) { m_verbose = verbose; };
	bool RequestServerDownload(const char* nzbFilename, const char* nzbContent,
		const char* category, bool autoCategory,
		bool addFirst, bool addPaused, int priority,
		const char* dupeKey, int dupeMode, int dupeScore);
	bool RequestServerList(bool files, bool groups, const char* pattern);
	bool RequestServerPauseUnpause(bool pause, ERemotePauseUnpauseAction action);
	bool RequestServerSetDownloadRate(int rate);
	bool RequestServerDumpDebug();
	bool RequestServerEditQueue(DownloadQueue::EEditAction action, int offset, const char* text,
		IdList* idList, NameList* nameList, ERemoteMatchMode matchMode);
	bool RequestServerLog(int lines);
	bool RequestServerShutdown();
	bool RequestServerReload();
	bool RequestServerVersion();
	bool RequestPostQueue();
	bool RequestWriteLog(int kind, const char* text);
	bool RequestScan(bool syncMode);
	bool RequestHistory(bool withHidden);
	void BuildFileList(SNzbListResponse* listResponse, const char* trailingData, DownloadQueue* downloadQueue);

private:
	class MatchedNzbInfo: public NzbInfo
	{
	public:
		bool m_match;
	};

	class MatchedFileInfo: public FileInfo
	{
	public:
		bool m_match;
	};

	std::unique_ptr<Connection> m_connection;
	bool m_verbose = true;

	bool InitConnection();
	void InitMessageBase(SNzbRequestBase* messageBase, int request, int size);
	bool ReceiveBoolResponse();
	void printf(const char* msg, ...);
	void perror(const char* msg);
};

#include "DownloadInfo.h"
#include "Options.h"
#include "Log.h"
#include "Util.h"
#include "Util.h"
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

inline void RemoteClient::printf(const char * msg,...)
{
	if (m_verbose)
	{
		va_list ap;
		va_start(ap, msg);
		::vprintf(msg, ap);
		va_end(ap);
	}
}

inline void RemoteClient::perror(const char * msg)
{
	if (m_verbose)
	{
		::perror(msg);
	}
}

inline bool RemoteClient::InitConnection()
{
	const char* controlIp = nzbget_rs_dqh_frontend_resolve_control_ip(g_Options->GetControlIp());

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
		printf("Unable to send request to RustyGet server at %s (port %i)\n", controlIp, g_Options->GetControlPort());
	}
	return OK;
}

inline void RemoteClient::InitMessageBase(SNzbRequestBase* messageBase, int request, int size)
{
	nzbget_rs_binrpc_init_message_base(
		(uint8_t*)messageBase,
		request,
		size,
		g_Options->GetControlUsername(),
		g_Options->GetControlPassword());
}

inline bool RemoteClient::ReceiveBoolResponse()
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
		printf("No response or invalid response (timeout, not RustyGet server or wrong RustyGet server version)\n");
		return false;
	}

	int textLen = ntohl(BoolResponse.m_trailingDataLength);
	CString buf;
	buf.Reserve(textLen);
	read = m_connection->Recv(buf, textLen);
	if (!read)
	{
		printf("No response or invalid response (timeout, not RustyGet server or wrong RustyGet server version)\n");
		return false;
	}

	printf("server returned: %s\n", *buf);
	return ntohl(BoolResponse.m_success);
}

/*
 * Sends a message to the running nzbget process.
 */
inline bool RemoteClient::RequestServerDownload(const char* nzbFilename, const char* nzbContent,
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

inline void RemoteClient::BuildFileList(SNzbListResponse* listResponse, const char* trailingData, DownloadQueue* downloadQueue)
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

inline bool RemoteClient::RequestServerList(bool files, bool groups, const char* pattern)
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
		printf("No response or invalid response (timeout, not RustyGet server or wrong RustyGet server version)\n");
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

inline bool RemoteClient::RequestServerLog(int lines)
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
		printf("No response or invalid response (timeout, not RustyGet server or wrong RustyGet server version)\n");
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

inline bool RemoteClient::RequestServerPauseUnpause(bool pause, ERemotePauseUnpauseAction action)
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

inline bool RemoteClient::RequestServerSetDownloadRate(int rate)
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

inline bool RemoteClient::RequestServerDumpDebug()
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

inline bool RemoteClient::RequestServerEditQueue(DownloadQueue::EEditAction action, int offset, const char* text,
	IdList* idList, NameList* nameList, ERemoteMatchMode matchMode)
{
	if (nzbget_rs_dqh_remote_client_is_empty_edit_target(
		idList ? idList->size() : 0, nameList ? nameList->size() : 0) != 0)
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

inline bool RemoteClient::RequestServerShutdown()
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

inline bool RemoteClient::RequestServerReload()
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

inline bool RemoteClient::RequestServerVersion()
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

inline bool RemoteClient::RequestPostQueue()
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
		printf("No response or invalid response (timeout, not RustyGet server or wrong RustyGet server version)\n");
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

inline bool RemoteClient::RequestWriteLog(int kind, const char* text)
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

inline bool RemoteClient::RequestScan(bool syncMode)
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

inline bool RemoteClient::RequestHistory(bool withHidden)
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
		printf("No response or invalid response (timeout, not RustyGet server or wrong RustyGet server version)\n");
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

#endif

#include "Util.h"
#include "ServerPool.h"
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
			nzbget_rs_binrpc_decode_log_response_entries(
				(const uint8_t*)(const char*)buf,
				(size_t)buf.Size(),
				ntohl(LogResponse.m_nrTrailingEntries),
				&m_remoteMessages,
				[](void* rawCtx, uint32_t id, uint32_t kind, uint32_t time, const char* text) {
					static_cast<MessageList*>(rawCtx)->emplace_back(
						id, (Message::EKind)kind, (time_t)time, text ? text : "");
				});
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

class LoggableFrontend : public Frontend
{
protected:
	virtual void Run()
	{
		debug("Entering LoggableFrontend-loop");

		while (!IsStopped())
		{
			Update();
			Wait(m_updateInterval);
		}
		Update();

		BeforeExit();

		debug("Exiting LoggableFrontend-loop");
	}

	virtual void Update()
	{
		if (!PrepareData())
		{
			FreeData();
			return;
		}

		BeforePrint();

		{
			GuardedMessageList messages = GuardMessages();
			if (!messages->empty())
			{
				Message& firstMessage = messages->front();
				int skip = 0;
				int start = nzbget_rs_feh_frontend_calc_log_start_index(
					m_neededLogFirstId, firstMessage.GetId(), &skip);
				if (skip)
				{
					PrintSkip();
				}
				for (uint32 i = (uint32)start; i < messages->size(); i++)
				{
					PrintMessage(messages->at(i));
					m_neededLogFirstId = messages->at(i).GetId();
				}
			}
		}

		PrintStatus();

		FreeData();

		fflush(stdout);
	}

	virtual void BeforePrint() {}
	virtual void BeforeExit() {}

	virtual void PrintMessage(Message& message)
	{
		NzbgetRsBuf line = nzbget_rs_frontend_format_log_message(
			0, (int)message.GetKind(), message.GetText());
		if (line.data)
		{
			fwrite(line.data, 1, line.len, stdout);
		}
		nzbget_rs_free(line);
	}

	virtual void PrintStatus() {}

	virtual void PrintSkip()
	{
		printf(".....\n");
	}
};

class ColoredFrontend : public LoggableFrontend
{
public:
	ColoredFrontend()
	{
		m_summary = true;
	}

protected:
	virtual void BeforePrint()
	{
		if (m_needGoBack)
		{
			printf("\r\033[1A");
			m_needGoBack = false;
		}
	}

	virtual void PrintMessage(Message& message)
	{
		NzbgetRsBuf line = nzbget_rs_frontend_format_log_message(
			1, (int)message.GetKind(), message.GetText());
		if (line.data)
		{
			fwrite(line.data, 1, line.len, stdout);
		}
		nzbget_rs_free(line);
	}

	virtual void PrintStatus()
	{
		printf("\033[s");
		NzbgetRsBuf status = nzbget_rs_colored_frontend_format_status_bar(
			m_standBy ? 1 : 0,
			m_pauseDownload ? 1 : 0,
			m_currentDownloadSpeed,
			m_remainingSize,
			m_downloadLimit,
			m_postJobCount,
			m_threadCount,
			"\033[K");
		if (status.data)
		{
			fwrite(status.data, 1, status.len, stdout);
		}
		nzbget_rs_free(status);
		m_needGoBack = true;
	}

	virtual void PrintSkip()
	{
		printf(".....\033[K\n");
	}

	virtual void BeforeExit()
	{
		if (IsRemoteMode())
		{
			printf("\n");
		}
	}

private:
	bool m_needGoBack = false;
};


/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef NCURSESFRONTEND_H
#define NCURSESFRONTEND_H

#ifndef DISABLE_CURSES

#include <string>
#include "Log.h"
#include "DownloadInfo.h"

class NCursesFrontend : public Frontend
{
public:
	NCursesFrontend();
	virtual ~NCursesFrontend();

protected:
	virtual void Run();

private:
	enum EInputMode
	{
		normal,
		editQueue,
		downloadRate
	};

	bool m_useColor = true;
	int m_dataUpdatePos = 0;
	bool m_updateNextTime = false;
	int m_screenHeight = 0;
	int m_screenWidth = 0;
	int m_queueWinTop = 0;
	int m_queueWinHeight = 0;
	int m_queueWinClientHeight = 0;
	int m_messagesWinTop = 0;
	int m_messagesWinHeight = 0;
	int m_messagesWinClientHeight = 0;
	int m_selectedQueueEntry = 0;
	int m_lastEditEntry = -1;
	bool m_lastPausePars = false;
	int m_queueScrollOffset = 0;
	std::string m_hint;
	time_t m_startHint;
	int m_colWidthFiles;
	int m_colWidthTotal;
	int m_colWidthLeft;

	// Inputting numbers
	int m_inputNumberIndex = 0;
	int m_inputValue;

	void* m_window; // WINDOW*

	EInputMode m_inputMode = normal;
	bool m_showNzbname;
	bool m_showTimestamp;
	bool m_groupFiles;
	int m_queueWindowPercentage = 50;

	void PlotText(const char*  string, int row, int pos, int colorPair, bool blink);
	void PlotLine(const char*  string, int row, int pos, int colorPair);
	void PrintMessages();
	void PrintQueue();
	void PrintFileQueue();
	void PrintFilename(FileInfo* fileInfo, int row, bool selected);
	void PrintGroupQueue();
	void ResetColWidths();
	void PrintGroupname(NzbInfo* nzbInfo, int row, bool selected, bool calcColWidth);
	void PrintTopHeader(char* header, int lineNr, bool upTime);
	int PrintMessage(Message& msg, int row, int maxLines);
	void PrintKeyInputBar();
	void PrintStatus();
	void UpdateInput(int initialKey);
	void Update(int key);
	void SetCurrentQueueEntry(int entry);
	void CalcWindowSizes();
	void RefreshScreen();
	int ReadConsoleKey();
	int CalcQueueSize();
	void NeedUpdateData();
	bool EditQueue(DownloadQueue::EEditAction action, int offset);
	void SetHint(const char* hint);
};

#endif

#ifndef DISABLE_CURSES

#include "Options.h"
#include "Util.h"
#include "nzbget_rs.h"

inline void curses_clear()
{
	nzbget_rs_term_clear();
}

extern void ExitProc();

static const int NCURSES_COLORPAIR_TEXT		= 1;
static const int NCURSES_COLORPAIR_INFO		= 2;
static const int NCURSES_COLORPAIR_WARNING		= 3;
static const int NCURSES_COLORPAIR_ERROR		= 4;
static const int NCURSES_COLORPAIR_DEBUG		= 5;
static const int NCURSES_COLORPAIR_DETAIL		= 6;
static const int NCURSES_COLORPAIR_STATUS		= 7;
static const int NCURSES_COLORPAIR_KEYBAR		= 8;
static const int NCURSES_COLORPAIR_INFOLINE	= 9;
static const int NCURSES_COLORPAIR_TEXTHIGHL 	= 10;
static const int NCURSES_COLORPAIR_CURSOR		= 11;
static const int NCURSES_COLORPAIR_HINT		= 12;

static const int MAX_SCREEN_WIDTH				= 512;

static const int READKEY_EMPTY = -1;
static const int KEY_DOWN      = 258;
static const int KEY_UP        = 259;
static const int KEY_HOME      = 262;
static const int KEY_BACKSPACE = 263;
static const int KEY_NPAGE     = 338;
static const int KEY_PPAGE     = 339;
static const int KEY_END       = 360;

inline NCursesFrontend::NCursesFrontend()
	: m_startHint(0)
	, m_colWidthFiles(0)
	, m_colWidthTotal(0)
	, m_colWidthLeft(0)
	, m_inputValue(0)
{
	m_summary = true;
	m_fileList = true;

	m_showNzbname = g_Options->GetCursesNzbName();
	m_showTimestamp = g_Options->GetCursesTime();
	m_groupFiles = g_Options->GetCursesGroup();

	int colorSupport = nzbget_rs_term_init();
	m_window = reinterpret_cast<void*>(1);
	m_useColor = colorSupport > 0;
}

inline NCursesFrontend::~NCursesFrontend()
{
	nzbget_rs_term_shutdown();
	printf("\n");
	SetHint(nullptr);
}

inline void NCursesFrontend::Run()
{
	debug("Entering NCursesFrontend-loop");

	m_dataUpdatePos = 0;

	while (!IsStopped())
	{
		// The data (queue and log) is updated each m_iUpdateInterval msec,
		// but the window is updated more often for better reaction on user's input

		bool updateNow = false;
		int key = ReadConsoleKey();

		int updateNext = m_updateNextTime ? 1 : 0;
		updateNow = nzbget_rs_sfw_ncurses_decide_loop_update(
			key != READKEY_EMPTY ? 1 : 0, &updateNext, m_dataUpdatePos) != 0;
		m_updateNextTime = updateNext != 0;

		if (updateNow)
		{
			Update(key);
		}

		if (m_dataUpdatePos <= 0)
		{
			m_dataUpdatePos = m_updateInterval;
		}

		// update more often (sleep shorter) if need faster reaction on user input
		int sleepInterval = nzbget_rs_sfw_ncurses_select_sleep_interval(m_inputMode == normal ? 1 : 0);
		Wait(sleepInterval);
		m_dataUpdatePos -= sleepInterval;
	}

	FreeData();

	debug("Exiting NCursesFrontend-loop");
}

inline void NCursesFrontend::NeedUpdateData()
{
	m_dataUpdatePos = 10;
	m_updateNextTime = true;
}

inline void NCursesFrontend::Update(int key)
{
	// Figure out how big the screen is
	CalcWindowSizes();

	if (m_dataUpdatePos <= 0)
	{
		FreeData();
		m_neededLogEntries = m_messagesWinClientHeight;
		if (!PrepareData())
		{
			return;
		}

		// recalculate frame sizes
		CalcWindowSizes();
	}

	if (m_inputMode == editQueue)
	{
		int queueSize = CalcQueueSize();
		if (nzbget_rs_dqh_ncurses_should_reset_empty_edit_queue(1, queueSize) != 0)
		{
			m_selectedQueueEntry = 0;
			m_inputMode = normal;
		}
	}

	//------------------------------------------
	// Print Current NZBInfoList
	//------------------------------------------
	if (m_queueWinHeight > 0)
	{
		PrintQueue();
	}

	//------------------------------------------
	// Print Messages
	//------------------------------------------
	if (m_messagesWinHeight > 0)
	{
		PrintMessages();
	}

	PrintStatus();

	PrintKeyInputBar();

	UpdateInput(key);

	RefreshScreen();
}

inline void NCursesFrontend::CalcWindowSizes()
{
	int nrRows = 24, nrColumns = 80;
	nzbget_rs_term_get_size(&nrRows, &nrColumns);
	// the line buffers hold MAX_SCREEN_WIDTH columns (a wider terminal wrote past
	// them), and messages wrap at the width less 8 (a divisor of zero at 8)
	int queueSize = CalcQueueSize();
	int qh = 0, qch = 0, mtop = 0, mh = 0, mch = 0;
	nzbget_rs_feh_ncurses_calc_window_sizes(
		nrRows, nrColumns, MAX_SCREEN_WIDTH, m_queueWindowPercentage, queueSize,
		&nrColumns, &qh, &qch, &mtop, &mh, &mch);
	if (nrRows != m_screenHeight || nrColumns != m_screenWidth)
	{
		curses_clear();
		m_screenHeight = nrRows;
		m_screenWidth = nrColumns;
	}
	m_queueWinTop = 0;
	m_queueWinHeight = qh;
	m_queueWinClientHeight = qch;
	m_messagesWinTop = mtop;
	m_messagesWinHeight = mh;
	m_messagesWinClientHeight = mch;
}

inline int NCursesFrontend::CalcQueueSize()
{
	int queueSize = 0;
	if (m_groupFiles)
	{
		queueSize = DownloadQueue::Guard()->GetQueue()->size();
	}
	else
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			queueSize += nzbInfo->GetFileList()->size();
		}
	}
	return queueSize;
}

inline void NCursesFrontend::PlotLine(const char* str, int row, int pos, int colorPair)
{
	NzbgetRsBuf lineBuf = nzbget_rs_dqh_ncurses_format_plot_line(
		str, m_screenWidth, pos, MAX_SCREEN_WIDTH);
	PlotText(lineBuf.data ? lineBuf.data : "", row, pos, colorPair, false);
	nzbget_rs_free(lineBuf);
}

inline void NCursesFrontend::PlotText(const char * string, int row, int pos, int colorPair, bool blink)
{
	nzbget_rs_term_plot_text(string, row, pos, m_useColor ? 1 : 0, colorPair, blink ? 1 : 0);
}

inline void NCursesFrontend::RefreshScreen()
{
	nzbget_rs_term_refresh(m_screenHeight, m_screenWidth);
}

inline void NCursesFrontend::PrintMessages()
{
	int lineNr = m_messagesWinTop;

	PlotLine(
		nzbget_rs_tpr_ncurses_format_messages_header(m_useColor ? 1 : 0),
		lineNr++, 0, NCURSES_COLORPAIR_INFOLINE);

	int line = lineNr + m_messagesWinClientHeight - 1;
	int linesToPrint = m_messagesWinClientHeight;

	GuardedMessageList messages = GuardMessages();

	// print messages from bottom
	for (int i = (int)messages->size() - 1; i >= 0 && linesToPrint > 0; i--)
	{
		int printedLines = PrintMessage(messages->at(i), line, linesToPrint);
		line -= printedLines;
		linesToPrint -= printedLines;
	}

	if (linesToPrint > 0)
	{
		// too few messages, print them again from top
		line = lineNr + m_messagesWinClientHeight - 1;
		while (linesToPrint-- > 0)
		{
			PlotLine("", line--, 0, NCURSES_COLORPAIR_TEXT);
		}
		int linesToPrint2 = m_messagesWinClientHeight;
		for (int i = (int)messages->size() - 1; i >= 0 && linesToPrint2 > 0; i--)
		{
			int printedLines = PrintMessage(messages->at(i), line, linesToPrint2);
			line -= printedLines;
			linesToPrint2 -= printedLines;
		}
	}
}

inline int NCursesFrontend::PrintMessage(Message& msg, int row, int maxLines)
{
	CString text;

	if (m_showTimestamp)
	{
		time_t rawtime = msg.GetTime() + g_Options->GetTimeCorrection();
		NzbgetRsBuf tsBuf = nzbget_rs_tpr_ncurses_format_timestamp_message(
			*Util::FormatTime(rawtime), msg.GetText());
		text = tsBuf.data ? tsBuf.data : "";
		nzbget_rs_free(tsBuf);
	}
	else
	{
		text = msg.GetText();
	}

	// replace some special characters with spaces
	int len = (int)strlen(text);
	nzbget_rs_ccm_ncurses_sanitize_message_chars((char*)text, (size_t)len);
	int winWidth = 0, msgLines = 0;
	nzbget_rs_ccm_ncurses_calc_msg_lines(len, m_screenWidth, &winWidth, &msgLines);

	int lines = 0;
	for (int i = msgLines - 1; i >= 0 && lines < maxLines; i--)
	{
		int r = row - msgLines + i + 1;
		PlotLine(text + winWidth * i, r, 8, NCURSES_COLORPAIR_TEXT);
		PlotText(
			nzbget_rs_dqh_ncurses_message_type_label((int)msg.GetKind(), i == 0 ? 1 : 0),
			r, 0,
			nzbget_rs_dqh_ncurses_message_color_pair((int)msg.GetKind()),
			false);
		lines++;
	}

	return lines;
}

inline void NCursesFrontend::PrintStatus()
{
	int statusRow = m_screenHeight - 2;

	NzbgetRsNcursesStatusSpec spec{};
	spec.stand_by = m_standBy ? 1 : 0;
	spec.pause_download = m_pauseDownload ? 1 : 0;
	spec.current_download_speed = m_currentDownloadSpeed;
	spec.remaining_size = m_remainingSize;
	spec.download_limit = m_downloadLimit;
	spec.post_job_count = m_postJobCount;
	spec.dn_time_sec = m_dnTimeSec;
	spec.all_bytes = m_allBytes;
	spec.thread_count = m_threadCount;

	NzbgetRsBuf buf = nzbget_rs_ncurses_format_status_bar(&spec);
	if (buf.data)
	{
		PlotLine(buf.data, statusRow, 0, NCURSES_COLORPAIR_STATUS);
	}
	nzbget_rs_free(buf);
}

inline void NCursesFrontend::PrintKeyInputBar()
{
	int queueSize = CalcQueueSize();
	int inputBarRow = m_screenHeight - 1;

	if (!m_hint.empty())
	{
		time_t time = Util::CurrentTime();
		if (nzbget_rs_sfw_ncurses_is_hint_active((int64_t)time, (int64_t)m_startHint) != 0)
		{
			PlotLine(m_hint.c_str(), inputBarRow, 0, NCURSES_COLORPAIR_HINT);
			return;
		}
		else
		{
			SetHint(nullptr);
		}
	}

	switch (m_inputMode)
	{
	case normal:
		PlotLine(
			nzbget_rs_sfw_ncurses_select_normal_keybar(m_groupFiles ? 1 : 0),
			inputBarRow, 0, NCURSES_COLORPAIR_KEYBAR);
		break;
	case editQueue:
	{
		const char* status = nzbget_rs_ccm_ncurses_select_edit_queue_keybar(
			m_selectedQueueEntry, queueSize);

		PlotLine(status, inputBarRow, 0, NCURSES_COLORPAIR_KEYBAR);
		break;
	}
	case downloadRate:
	{
		NzbgetRsBuf hintBuf = nzbget_rs_tpr_ncurses_format_download_rate_hint(m_inputValue);
		PlotLine(hintBuf.data ? hintBuf.data : "", inputBarRow, 0, NCURSES_COLORPAIR_KEYBAR);
		nzbget_rs_free(hintBuf);

		// Print the cursor
		PlotText(" ", inputBarRow, 15 + m_inputNumberIndex, NCURSES_COLORPAIR_CURSOR, true);
		break;
	}
	}
}

inline void NCursesFrontend::SetHint(const char* hint)
{
	if (hint) m_hint = hint;
	if (!m_hint.empty()) m_startHint = Util::CurrentTime();
}

inline void NCursesFrontend::PrintQueue()
{
	if (m_groupFiles)
	{
		PrintGroupQueue();
	}
	else
	{
		PrintFileQueue();
	}
}

inline void NCursesFrontend::PrintFileQueue()
{
	int lineNr = m_queueWinTop + 1;
	int64 remaining = 0;
	int64 paused = 0;
	int pausedFiles = 0;
	int fileNum = 0;

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			if (nzbget_rs_dqh_ncurses_is_row_in_scroll_window(
				fileNum, m_queueScrollOffset, m_queueWinHeight) != 0)
			{
				PrintFilename(fileInfo, lineNr++, fileNum == m_selectedQueueEntry);
			}
			fileNum++;

			nzbget_rs_dqh_ncurses_accum_file_queue_stats(
				fileInfo->GetPaused() ? 1 : 0,
				fileInfo->GetRemainingSize(),
				&remaining,
				&paused,
				&pausedFiles);
		}
	}

	NzbgetRsBuf headerBuf = nzbget_rs_ncurses_format_file_queue_header(
		m_useColor ? 1 : 0, fileNum, pausedFiles, remaining, paused);
	if (fileNum > 0)
	{
		if (headerBuf.data)
		{
			PrintTopHeader(headerBuf.data, m_queueWinTop, true);
		}
		nzbget_rs_free(headerBuf);
	}
	else
	{
		lineNr--;
		if (headerBuf.data)
		{
			PrintTopHeader(headerBuf.data, lineNr++, true);
		}
		nzbget_rs_free(headerBuf);
		PlotLine("Ready to receive nzb-job", lineNr++, 0, NCURSES_COLORPAIR_TEXT);
	}
}

inline void NCursesFrontend::PrintFilename(FileInfo * fileInfo, int row, bool selected)
{
	NzbgetRsNcursesFileRowSpec spec{};
	spec.edit_queue_mode = m_inputMode == editQueue ? 1 : 0;
	spec.selected = selected ? 1 : 0;
	spec.use_color = m_useColor ? 1 : 0;
	spec.color_highl = NCURSES_COLORPAIR_TEXTHIGHL;
	spec.color_text = NCURSES_COLORPAIR_TEXT;
	spec.id = fileInfo->GetId();
	spec.priority = fileInfo->GetNzbInfo()->GetPriority();
	spec.active_downloads = fileInfo->GetActiveDownloads();
	spec.paused = fileInfo->GetPaused() ? 1 : 0;
	spec.show_nzbname = m_showNzbname ? 1 : 0;
	spec.path_sep = (uint8_t)PATH_SEPARATOR;
	spec.nzb_name = fileInfo->GetNzbInfo()->GetName();
	spec.filename = fileInfo->GetFilename();
	spec.size = fileInfo->GetSize();
	spec.remaining_size = fileInfo->GetRemainingSize();

	int color = NCURSES_COLORPAIR_TEXT;
	NzbgetRsBuf buf = nzbget_rs_ncurses_format_file_row(&spec, &color);
	if (buf.data)
	{
		PlotLine(buf.data, row, 0, color);
	}
	nzbget_rs_free(buf);
}

inline void NCursesFrontend::PrintTopHeader(char* header, int lineNr, bool upTime)
{
	int time = upTime ? m_upTimeSec : m_dnTimeSec;
	NzbgetRsBuf buf = nzbget_rs_ncurses_format_top_header(
		header, m_screenWidth, upTime ? 1 : 0, time);
	if (buf.data)
	{
		PlotLine(buf.data, lineNr, 0, NCURSES_COLORPAIR_INFOLINE);
	}
	nzbget_rs_free(buf);
}

inline void NCursesFrontend::PrintGroupQueue()
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	int lineNr = m_queueWinTop;

	if (downloadQueue->GetQueue()->empty())
	{
		NzbgetRsBuf headerBuf = nzbget_rs_ncurses_format_group_queue_header(
			m_useColor ? 1 : 0, 0, 0, 0);
		if (headerBuf.data)
		{
			PrintTopHeader(headerBuf.data, lineNr++, false);
		}
		nzbget_rs_free(headerBuf);
		PlotLine("Ready to receive nzb-job", lineNr++, 0, NCURSES_COLORPAIR_TEXT);
	}
	else
	{
		lineNr++;

		ResetColWidths();
		int calcLineNr = lineNr;
		int i = 0;
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			if (nzbget_rs_dqh_ncurses_is_row_in_scroll_window(
				i, m_queueScrollOffset, m_queueWinHeight) != 0)
			{
				PrintGroupname(nzbInfo, calcLineNr++, false, true);
			}
			i++;
		}

		int64 remaining = 0;
		int64 paused = 0;
		i = 0;
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			if (nzbget_rs_dqh_ncurses_is_row_in_scroll_window(
				i, m_queueScrollOffset, m_queueWinHeight) != 0)
			{
				PrintGroupname(nzbInfo, lineNr++, i == m_selectedQueueEntry, false);
			}
			i++;
			nzbget_rs_dqh_ncurses_accum_group_queue_stats(
				nzbInfo->GetRemainingSize(),
				nzbInfo->GetPausedSize(),
				&remaining,
				&paused);
		}

		NzbgetRsBuf headerBuf = nzbget_rs_ncurses_format_group_queue_header(
			m_useColor ? 1 : 0, (int)downloadQueue->GetQueue()->size(), remaining, paused);
		if (headerBuf.data)
		{
			PrintTopHeader(headerBuf.data, m_queueWinTop, false);
		}
		nzbget_rs_free(headerBuf);
	}
}

inline void NCursesFrontend::ResetColWidths()
{
	m_colWidthFiles = 0;
	m_colWidthTotal = 0;
	m_colWidthLeft = 0;
}

inline void NCursesFrontend::PrintGroupname(NzbInfo* nzbInfo, int row, bool selected, bool calcColWidth)
{
	NzbgetRsNcursesGroupRowSpec spec{};
	spec.edit_queue_mode = m_inputMode == editQueue ? 1 : 0;
	spec.selected = selected ? 1 : 0;
	spec.use_color = m_useColor ? 1 : 0;
	spec.color_highl = NCURSES_COLORPAIR_TEXTHIGHL;
	spec.color_text = NCURSES_COLORPAIR_TEXT;
	spec.screen_width = m_screenWidth;
	spec.calc_col_width = calcColWidth ? 1 : 0;
	spec.id = nzbInfo->GetId();
	spec.priority = nzbInfo->GetPriority();
	spec.active_downloads = nzbInfo->GetActiveDownloads();
	spec.name = nzbInfo->GetName();
	spec.file_count = (int)nzbInfo->GetFileList()->size();
	spec.paused_file_count = nzbInfo->GetPausedFileCount();
	spec.size = nzbInfo->GetSize();
	spec.remaining_size = nzbInfo->GetRemainingSize();
	spec.paused_size = nzbInfo->GetPausedSize();
	spec.stand_by = m_standBy ? 1 : 0;
	spec.pause_download = m_pauseDownload ? 1 : 0;
	spec.current_download_speed = m_currentDownloadSpeed;

	int color = NCURSES_COLORPAIR_TEXT;
	NzbgetRsBuf buf = nzbget_rs_ncurses_format_group_row(
		&spec, &m_colWidthFiles, &m_colWidthTotal, &m_colWidthLeft, &color);
	if (!calcColWidth && buf.data)
	{
		PlotLine(buf.data, row, 0, color);
	}
	nzbget_rs_free(buf);
}

inline bool NCursesFrontend::EditQueue(DownloadQueue::EEditAction action, int offset)
{
	int ID = 0;

	if (m_groupFiles)
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		if (m_selectedQueueEntry >= 0 && m_selectedQueueEntry < (int)downloadQueue->GetQueue()->size())
		{
			std::unique_ptr<NzbInfo>& nzbInfo = downloadQueue->GetQueue()->at(m_selectedQueueEntry);
			ID = nzbInfo->GetId();
			int newLastPausePars = m_lastPausePars ? 1 : 0;
			action = (DownloadQueue::EEditAction)nzbget_rs_feh_ncurses_resolve_group_pause_action(
				(int)action, nzbInfo->GetRemainingSize(), nzbInfo->GetPausedSize(),
				nzbInfo->GetRemainingParCount(), m_lastPausePars ? 1 : 0,
				m_lastEditEntry, m_selectedQueueEntry, &newLastPausePars);
			m_lastPausePars = newLastPausePars != 0;
		}

		// map file-edit-actions to group-edit-actions
		action = (DownloadQueue::EEditAction)nzbget_rs_tpr_ncurses_map_file_to_group_action((int)action);
	}
	else
	{
		int fileNum = 0;
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			for (FileInfo* fileInfo : nzbInfo->GetFileList())
			{
				if (m_selectedQueueEntry == fileNum)
				{
					ID = fileInfo->GetId();
					action = (DownloadQueue::EEditAction)nzbget_rs_tpr_ncurses_resolve_file_pause_action(
						(int)action, (int)DownloadQueue::eaFilePause,
						(int)DownloadQueue::eaFileResume, fileInfo->GetPaused() ? 1 : 0);
				}
				fileNum++;
			}
		}
	}

	m_lastEditEntry = m_selectedQueueEntry;

	NeedUpdateData();

	if (ID != 0)
	{
		return ServerEditQueue(action, offset, ID);
	}
	else
	{
		return false;
	}
}

inline void NCursesFrontend::SetCurrentQueueEntry(int entry)
{
	int queueSize = CalcQueueSize();

	nzbget_rs_feh_ncurses_calc_scroll_and_selection(
		entry, queueSize, m_queueScrollOffset, m_queueWinClientHeight,
		&m_selectedQueueEntry, &m_queueScrollOffset);
}


/*
 * Process keystrokes starting with the initialKey, which must not be
 * READKEY_EMPTY but has alread been set via ReadConsoleKey.
 */
inline void NCursesFrontend::UpdateInput(int initialKey)
{
	int key = initialKey;
	while (key != READKEY_EMPTY)
	{
		int queueSize = CalcQueueSize();

		// Normal or edit queue mode
		if (m_inputMode == normal || m_inputMode == editQueue)
		{
			switch (key)
			{
			case 'q':
				// Key 'q' for quit
				ExitProc();
				break;
			case 'z':
				// show/hide NZBFilename
				m_showNzbname = !m_showNzbname;
				break;
			case 'w':
				// swicth window sizes
				m_queueWindowPercentage = nzbget_rs_ccm_ncurses_next_queue_win_pct(
					m_queueWindowPercentage, m_inputMode == editQueue ? 1 : 0);
				CalcWindowSizes();
				SetCurrentQueueEntry(m_selectedQueueEntry);
				break;
			case 'g':
				// group/ungroup files
				m_groupFiles = !m_groupFiles;
				SetCurrentQueueEntry(m_selectedQueueEntry);
				NeedUpdateData();
				break;
			}
		}

		// Normal mode
		if (m_inputMode == normal)
		{
			switch (key)
			{
			case 'p':
				// Key 'p' for pause
				if (!IsRemoteMode())
				{
					info("%s", nzbget_rs_tpr_ncurses_pause_log_msg(m_pauseDownload ? 1 : 0));
				}
				ServerPauseUnpause(!m_pauseDownload);
				break;
			case 'e':
			case 10: // return
			case 13: // enter
			{
				int nextWinPct = m_queueWindowPercentage;
				if (nzbget_rs_dqh_ncurses_enter_edit_queue_win_pct(
					queueSize, m_queueWindowPercentage, &nextWinPct) != 0)
				{
					m_inputMode = editQueue;
					m_queueWindowPercentage = nextWinPct;
					return;
				}
				break;
			}
			case 'r':
				// Download rate
				m_inputMode = downloadRate;
				m_inputNumberIndex = 0;
				m_inputValue = 0;
				return;
			case 't':
				// show/hide Timestamps
				m_showTimestamp = !m_showTimestamp;
				break;
			}
		}

		// Edit Queue mode
		if (m_inputMode == editQueue)
		{
			switch (key)
			{
			case 'e':
			case 10: // return
			case 13: // enter
				m_inputMode = normal;
				return;
			case KEY_DOWN:
				if (m_selectedQueueEntry < queueSize - 1)
				{
					SetCurrentQueueEntry(m_selectedQueueEntry + 1);
				}
				break;
			case KEY_UP:
				if (m_selectedQueueEntry > 0)
				{
					SetCurrentQueueEntry(m_selectedQueueEntry - 1);
				}
				break;
			case KEY_PPAGE:
			case KEY_NPAGE:
			{
				int newScroll = m_queueScrollOffset;
				int newEntry = m_selectedQueueEntry;
				if (nzbget_rs_tpr_ncurses_calc_page_nav(
					key == KEY_NPAGE ? 1 : 0, m_selectedQueueEntry, queueSize,
					m_queueScrollOffset, m_queueWinClientHeight, &newScroll, &newEntry))
				{
					m_queueScrollOffset = newScroll;
					SetCurrentQueueEntry(newEntry);
				}
				break;
			}
			case KEY_HOME:
				SetCurrentQueueEntry(0);
				break;
			case KEY_END:
				SetCurrentQueueEntry(nzbget_rs_dqh_ncurses_last_queue_index(queueSize));
				break;
			case 'p':
				// Key 'p' for pause
				EditQueue(DownloadQueue::eaFilePause, 0);
				break;
			case 'd':
				SetHint(" Use Uppercase \"D\" for delete");
				break;
			case 'D':
				// Delete entry
				if (EditQueue(DownloadQueue::eaFileDelete, 0))
				{
					SetCurrentQueueEntry(m_selectedQueueEntry);
				}
				break;
			case 'u':
				if (EditQueue(DownloadQueue::eaFileMoveOffset, -1))
				{
					SetCurrentQueueEntry(m_selectedQueueEntry - 1);
				}
				break;
			case 'n':
				if (EditQueue(DownloadQueue::eaFileMoveOffset, +1))
				{
					SetCurrentQueueEntry(m_selectedQueueEntry + 1);
				}
				break;
			case 't':
				if (EditQueue(DownloadQueue::eaFileMoveTop, 0))
				{
					SetCurrentQueueEntry(0);
				}
				break;
			case 'b':
				if (EditQueue(DownloadQueue::eaFileMoveBottom, 0))
				{
					SetCurrentQueueEntry(nzbget_rs_dqh_ncurses_last_queue_index(queueSize));
				}
				break;
			}
		}

		// Edit download rate input mode
		if (m_inputMode == downloadRate)
		{
			int rateStep = nzbget_rs_feh_ncurses_step_rate_input(
				key, KEY_BACKSPACE, &m_inputNumberIndex, &m_inputValue);
			if (rateStep == 1)
			{
				ServerSetDownloadRate(nzbget_rs_tpr_webdl_kb_to_bytes(m_inputValue));
				m_inputMode = normal;
				return;
			}
			else if (rateStep == 2)
			{
				m_inputMode = normal;
				return;
			}
		}

		key = ReadConsoleKey();
	}
}

inline int NCursesFrontend::ReadConsoleKey()
{
	return nzbget_rs_term_read_key();
}

#endif

#endif

#endif
