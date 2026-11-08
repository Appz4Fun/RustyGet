/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef DISKSTATE_H

#include <functional>
#include <vector>
#include "DownloadInfo.h"
#include "Options.h"
#include "Util.h"
#include "Log.h"

#ifndef DISKSTATE_H
#define DISKSTATE_H

class ServerVolume;
using ServerVolumes = std::vector<ServerVolume>;
class StateDiskFile;

class DiskState
{
public:
	bool DownloadQueueExists();
	bool SaveDownloadQueue(DownloadQueue* downloadQueue, bool saveHistory);
	bool LoadDownloadQueue(DownloadQueue* downloadQueue, Servers* servers);
	/* the last load read a queue, history or progress file an earlier build of the
	 * branch wrote (formats 65/66): saving it again writes the upstream format */
	bool LegacyQueueRead() { return m_legacyQueueRead; }
	bool SaveDownloadProgress(DownloadQueue* downloadQueue);
	bool SaveFile(FileInfo* fileInfo, bool replace = false);
	bool LoadFile(FileInfo* fileInfo, bool fileSummary, bool articles);
	bool SaveAllFileInfos(DownloadQueue* downloadQueue);
	void DiscardQuickFileInfos();
	bool SaveFileState(FileInfo* fileInfo, bool completed, bool replace = false);
	bool LoadFileState(FileInfo* fileInfo, Servers* servers, bool completed);
	bool LoadArticles(FileInfo* fileInfo);
	void DiscardDownloadQueue();
	void DiscardFile(int fileId, bool deleteData, bool deletePartialState, bool deleteCompletedState);
	void DiscardFiles(NzbInfo* nzbInfo, bool deleteLog = true);
	bool SaveFeeds(Feeds* feeds, FeedHistory* feedHistory);
	bool LoadFeeds(Feeds* feeds, FeedHistory* feedHistory);
	bool SaveStats(Servers* servers, ServerVolumes* serverVolumes);
	bool LoadStats(Servers* servers, ServerVolumes* serverVolumes, bool* perfectMatch);
	void CleanupTempDir(DownloadQueue* downloadQueue);
	void WriteCacheFlag();
	void DeleteCacheFlag();
	void AppendNzbMessage(int nzbId, Message::EKind kind, const char* text);
	void LoadNzbMessages(int nzbId, MessageList* messages);

private:
	// the format version of the file state LoadFileState read last
	int m_lastFileStateVersion = 0;
	bool SaveFileInfo(FileInfo* fileInfo, StateDiskFile& outfile, bool articles);
	bool LoadFileInfo(FileInfo* fileInfo, StateDiskFile& outfile, int formatVersion, bool fileSummary, bool articles);
	bool SaveFileState(FileInfo* fileInfo, StateDiskFile& outfile, bool completed);
	bool LoadFileState(FileInfo* fileInfo, Servers* servers, StateDiskFile& infile, int formatVersion, bool completed);
	void SaveQueue(NzbList* queue, StateDiskFile& outfile);
	bool LoadQueue(NzbList* queue, Servers* servers, StateDiskFile& infile, int formatVersion);
	void SaveProgress(NzbList* queue, StateDiskFile& outfile, int changedCount);
	bool LoadProgress(NzbList* queue, Servers* servers, StateDiskFile& infile, int formatVersion);
	/* a queue, history or progress file that couldn't be read is renamed
	 * "<name>.unreadable-<time>": the next save would overwrite it */
	void SetAsideQueueFiles();
	void RaiseIdsPastQueueDir();
	void SaveIdFloor();
	void ConvertLegacyFileStates(Servers* servers);
	bool m_legacyQueueRead = false;
	void SaveNzbInfo(NzbInfo* nzbInfo, StateDiskFile& outfile);
	// this build's own data (duplicate repair) after upstream's in a queue file
	void SaveDupeExtras(NzbInfo* nzbInfo, StateDiskFile& outfile);
	bool LoadDupeExtras(NzbInfo* nzbInfo, StateDiskFile& infile, int formatVersion);
	void SaveDupeTrailer(const std::vector<NzbInfo*>& nzbs, StateDiskFile& outfile);
	bool LoadDupeTrailer(const std::function<NzbInfo*(int)>& find, StateDiskFile& infile);
	bool LoadNzbInfo(NzbInfo* nzbInfo, Servers* servers, StateDiskFile& infile, int formatVersion);
	void SaveDupInfo(DupInfo* dupInfo, StateDiskFile& outfile);
	bool LoadDupInfo(DupInfo* dupInfo, StateDiskFile& infile, int formatVersion);
	void SaveHistory(HistoryList* history, StateDiskFile& outfile);
	bool LoadHistory(HistoryList* history, Servers* servers, StateDiskFile& infile, int formatVersion);
	bool SaveFeedStatus(Feeds* feeds, StateDiskFile& outfile);
	bool LoadFeedStatus(Feeds* feeds, StateDiskFile& infile, int formatVersion);
	bool SaveFeedHistory(FeedHistory* feedHistory, StateDiskFile& outfile);
	bool LoadFeedHistory(FeedHistory* feedHistory, StateDiskFile& infile, int formatVersion);
	bool SaveServerInfo(Servers* servers, StateDiskFile& outfile);
	bool LoadServerInfo(Servers* servers, StateDiskFile& infile, int formatVersion, bool* perfectMatch);
	bool SaveVolumeStat(ServerVolumes* serverVolumes, StateDiskFile& outfile);
	bool LoadVolumeStat(Servers* servers, ServerVolumes* serverVolumes, StateDiskFile& infile, int formatVersion);
	void CalcFileStats(DownloadQueue* downloadQueue, int formatVersion);
	bool LoadAllFileInfos(DownloadQueue* downloadQueue);
	bool LoadAllFileStates(DownloadQueue* downloadQueue, Servers* servers);
	bool LoadServerStats(ServerStatList* serverStatList, Servers* servers, StateDiskFile& infile);
	void CleanupQueueDir(DownloadQueue* downloadQueue);
};

extern DiskState* g_DiskState;

#include "ServerPool.h"

#include "Options.h"
#include "Log.h"
#include "Util.h"
#include "Util.h"
#include "nzbget_rs.h"

static const char* FORMATVERSION_SIGNATURE = "nzbget diskstate file version ";
// The queue, history and file-state files are written in upstream nzbget's formats
// (64 and 7), so that an nzbget without duplicate repair still reads them: it ignores
// the trailer that carries this build's own data (stream-repair jobs, recovery
// counters, decoded file sizes). Earlier builds of this branch wrote that data inline
// as formats 65/66 and 8/9: those are still read, once, and rewritten in the new form.
inline const int DISKSTATE_QUEUE_VERSION = 64;
inline const int DISKSTATE_QUEUE_LEGACY_VERSION = 66;
inline const int DISKSTATE_FILE_VERSION = 7;
inline const int DISKSTATE_FILE_LEGACY_VERSION = 9;
// the line that starts this build's data after upstream's in a queue, history or progress file
inline const char* DUPESTATE_TRAILER = "#dupestate 1";
inline const char* UNPACKSTATE_TRAILER = "#unpackstate 1";
// the line that carries a file state's decoded file size after upstream's data
inline const char* DECODED_TRAILER = "#decoded ";
inline const int DISKSTATE_STATS_VERSION = 4;
inline const int DISKSTATE_FEEDS_VERSION = 3;

class StateDiskFile : public DiskFile
{
public:
	int64 PrintLine(const char* format, ...) PRINTF_SYNTAX(2);
	char* ReadLine(char* buffer, int64 size);
	int ScanLine(const char* format, ...) SCANF_SYNTAX(2);
};

static inline void WriteRsBuf(StateDiskFile& outfile, NzbgetRsBuf buf)
{
	if (buf.data && buf.len > 0)
	{
		outfile.Write(buf.data, buf.len);
	}
	nzbget_rs_free(buf);
}


inline int64 StateDiskFile::PrintLine(const char* format, ...)
{
	// a line of any length: one of 1,024 characters or more (a long url sent with
	// appendurl) was written past a fixed buffer, onto the stack, and the state file
	// got garbage (unreadable at the next start) or nzbget crashed (SIGSEGV). The
	// reader cuts a longer line to its buffer.
	va_list ap;
	va_start(ap, format);
	va_list ap2;
	va_copy(ap2, ap);
	int len = vsnprintf(nullptr, 0, format, ap);
	va_end(ap);
	if (len < 0)
	{
		va_end(ap2);
		return 0;
	}
	std::vector<char> str(len + 1);
	vsnprintf(str.data(), str.size(), format, ap2);
	va_end(ap2);

	// a line break inside a value (a name, key or url sent through the API) split
	// the record over two lines, and the whole file was set aside as unreadable at
	// the next start: it is kept as a space
	nzbget_rs_dsp_sanitize_line_breaks(str.data(), (size_t)len);

	// replacing terminating <NULL> with <LF>
	str[len++] = '\n';

	Write(str.data(), len);

	return len;
}

inline char* StateDiskFile::ReadLine(char* buffer, int64 size)
{
	if (!DiskFile::ReadLine(buffer, size))
	{
		return nullptr;
	}

	// remove traling '\n'
	int needsScroll = 0;
	size_t newLen = 0;
	if (nzbget_rs_dsp_inspect_read_line(buffer, &needsScroll, &newLen) != 0)
	{
		if (needsScroll != 0)
		{
			// the line is longer than "size", scroll file position to the end of the line
			for (char skipbuf[1024]; DiskFile::ReadLine(skipbuf, 1024); )
			{
				int skipScroll = 0;
				if (nzbget_rs_dsp_inspect_read_line(skipbuf, &skipScroll, nullptr) == 0 || skipScroll == 0)
				{
					break;
				}
			}
		}

		buffer[newLen] = 0;
	}

	return buffer;
}

/*
* Standard "fscanf" scans beoynd current line if the next line is empty.
* This wrapper fixes that.
*/
inline int StateDiskFile::ScanLine(const char* format, ...)
{
	char line[1024];
	if (!ReadLine(line, sizeof(line)))
	{
		return 0;
	}

	va_list ap;
	va_start(ap, format);
	int res = vsscanf(line, format, ap);
	va_end(ap);

	return res;
}


class StateFile
{
public:
	/* readVersion: the newest version read (default: the one written) */
	StateFile(const char* filename, int formatVersion, bool transactional, int readVersion = 0);
	void Discard();
	void SetFlush(bool flush) { m_flush = flush; }
	bool FileExists();
	StateDiskFile* BeginWrite();
	bool FinishWrite();
	StateDiskFile* BeginRead();
	int GetFileVersion() { return m_fileVersion; }
	const char* GetDestFilename() { return m_destFilename; }

private:
	BString<1024> m_destFilename;
	BString<1024> m_tempFilename;
	int m_formatVersion;
	int m_readVersion;
	bool m_transactional;
	bool m_flush = true;
	int m_fileVersion = 0;
	StateDiskFile m_file;

	int ParseFormatVersion(const char* formatSignature);
};


inline StateFile::StateFile(const char* filename, int formatVersion, bool transactional, int readVersion) :
	m_formatVersion(formatVersion),
	m_readVersion(nzbget_rs_dqh_diskstate_resolve_read_version(readVersion, formatVersion)),
	m_transactional(transactional)
{
	NzbgetRsBuf destBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		g_Options->GetQueueDir(), PATH_SEPARATOR, filename ? filename : "");
	m_destFilename = destBuf.data ? destBuf.data : "";
	nzbget_rs_free(destBuf);
	if (m_transactional)
	{
		NzbgetRsBuf tempBuf = nzbget_rs_dqh_diskstate_format_new_path(*m_destFilename);
		m_tempFilename = tempBuf.data ? tempBuf.data : "";
		nzbget_rs_free(tempBuf);
	}
	else
	{
		m_tempFilename = *m_destFilename;
	}
}

inline void StateFile::Discard()
{
	FileSystem::DeleteFile(m_destFilename);
	// a temp file left from an interrupted write would be "restored" at the next
	// start as if the write had finished (a partial file, all state set aside)
	if (m_transactional)
	{
		FileSystem::DeleteFile(m_tempFilename);
	}
}

/* Parse signature and return format version number
*/
inline int StateFile::ParseFormatVersion(const char* formatSignature)
{
	return nzbget_rs_dsp_parse_format_version(formatSignature);
}

inline bool StateFile::FileExists()
{
	return nzbget_rs_dqh_diskstate_state_file_exists(
		FileSystem::FileExists(m_destFilename) ? 1 : 0,
		m_transactional ? 1 : 0,
		m_transactional && FileSystem::FileExists(m_tempFilename) ? 1 : 0) != 0;
}

inline StateDiskFile* StateFile::BeginWrite()
{
	if (!m_file.Open(m_tempFilename, StateDiskFile::omWrite))
	{
		error("Error saving diskstate: Could not create file %s: %s", *m_tempFilename,
			*FileSystem::GetLastErrorMessage());
		return nullptr;
	}

	m_file.PrintLine("%s%i", FORMATVERSION_SIGNATURE, m_formatVersion);

	return &m_file;
}

inline bool StateFile::FinishWrite()
{
	if (!m_transactional)
	{
		m_file.Close();
		return true;
	}

	// flush file content before renaming
	if (nzbget_rs_dqh_diskstate_should_flush_queue_file(
		m_flush ? 1 : 0, g_Options->GetFlushQueue() ? 1 : 0) != 0)
	{
		debug("Flushing data for file %s", FileSystem::BaseFileName(m_tempFilename));
		m_file.Flush();
		CString errmsg;
		if (!m_file.Sync(errmsg))
		{
			warn("Could not flush file %s into disk: %s", *m_tempFilename, *errmsg);
		}
	}

	m_file.Close();

	// now rename to dest file name
	FileSystem::DeleteFile(m_destFilename);
	if (!FileSystem::MoveFile(m_tempFilename, m_destFilename))
	{
		error("Error saving diskstate: Could not rename file %s to %s: %s",
			*m_tempFilename, *m_destFilename, *FileSystem::GetLastErrorMessage());
		return false;
	}

	// flush directory buffer after renaming
	if (nzbget_rs_dqh_diskstate_should_flush_queue_file(
		m_flush ? 1 : 0, g_Options->GetFlushQueue() ? 1 : 0) != 0)
	{
		debug("Flushing directory for file %s", FileSystem::BaseFileName(m_destFilename));
		CString errmsg;
		if (!FileSystem::FlushDirBuffers(m_destFilename, errmsg))
		{
			warn("Could not flush directory buffers for file %s into disk: %s", *m_destFilename, *errmsg);
		}
	}

	return true;
}

inline StateDiskFile* StateFile::BeginRead()
{
	// with the dest-file there, a temp-file is what an interrupted write left (the
	// dest-file is deleted only right before the temp-file takes its place)
	int recoveryMode = nzbget_rs_dqh_diskstate_classify_begin_read_recovery(
		m_transactional ? 1 : 0,
		FileSystem::FileExists(m_destFilename) ? 1 : 0,
		FileSystem::FileExists(m_tempFilename) ? 1 : 0);
	if (recoveryMode == 1)
	{
		FileSystem::DeleteFile(m_tempFilename);
	}
	else if (recoveryMode == 2)
	{
		// disaster recovery: temp-file exists but the dest-file doesn't
		warn("Restoring diskstate file %s from %s", FileSystem::BaseFileName(m_destFilename), FileSystem::BaseFileName(m_tempFilename));
		if (!FileSystem::MoveFile(m_tempFilename, m_destFilename))
		{
			error("Error restoring diskstate: Could not rename file %s to %s: %s",
				*m_tempFilename, *m_destFilename, *FileSystem::GetLastErrorMessage());
			return nullptr;
		}
	}

	if (!m_file.Open(m_destFilename, StateDiskFile::omRead))
	{
		error("Error reading diskstate: could not open file %s: %s", *m_destFilename,
			*FileSystem::GetLastErrorMessage());
		return nullptr;
	}

	char FileSignatur[128];
	m_file.ReadLine(FileSignatur, sizeof(FileSignatur));
	m_fileVersion = ParseFormatVersion(FileSignatur);
	if (nzbget_rs_dqh_diskstate_is_file_version_unsupported(m_fileVersion, m_readVersion) != 0)
	{
		error("Could not load diskstate file %s due to file version mismatch", *m_destFilename);
		m_file.Close();
		return nullptr;
	}

	return &m_file;
}


/* Save Download Queue to Disk.
 * The Disk State consists of file "queue", which contains the order of files,
 * and of one diskstate-file for each file in download queue.
 * This function saves file "queue" and files with NZB-info. It does not
 * save file-infos.
 */
inline bool DiskState::SaveDownloadQueue(DownloadQueue* downloadQueue, bool saveHistory)
{
	debug("Saving queue and history to disk");

	bool ok = true;

	{
		StateFile stateFile("queue", DISKSTATE_QUEUE_VERSION, true);
		if (!downloadQueue->GetQueue()->empty())
		{
			StateDiskFile* outfile = stateFile.BeginWrite();
			if (!outfile)
			{
				return false;
			}

			// save nzb-infos
			SaveQueue(downloadQueue->GetQueue(), *outfile);

			// now rename to dest file name
			ok = stateFile.FinishWrite();
		}
		else
		{
			stateFile.Discard();
		}
	}

	if (saveHistory)
	{
		StateFile stateFile("history", DISKSTATE_QUEUE_VERSION, true);
		if (!downloadQueue->GetHistory()->empty())
		{
			StateDiskFile* outfile = stateFile.BeginWrite();
			if (!outfile)
			{
				return false;
			}

			// save history
			SaveHistory(downloadQueue->GetHistory(), *outfile);

			// now rename to dest file name
			ok &= stateFile.FinishWrite();
		}
		else
		{
			stateFile.Discard();
		}
	}

	// progress-file isn't needed after saving of full queue data
	StateFile progressStateFile("progress", DISKSTATE_QUEUE_VERSION, true);
	progressStateFile.Discard();

	SaveIdFloor();



	return ok;
}

inline bool DiskState::LoadDownloadQueue(DownloadQueue* downloadQueue, Servers* servers)
{
	debug("Loading queue from disk");

	bool ok = false;
	bool queueFilesFailed = false;
	bool fileInfosLoaded = false;
	int formatVersion = 0;

	// formats 65/66 are read as an earlier build of this branch wrote them (it may
	// run again after this one); a file that doesn't read so, or of a newer format,
	// is set aside below rather than lost to the next save
	int queueReadVersion = DISKSTATE_QUEUE_LEGACY_VERSION;

	{
		StateFile stateFile("queue", DISKSTATE_QUEUE_VERSION, true, queueReadVersion);
		if (stateFile.FileExists())
		{
			StateDiskFile* infile = stateFile.BeginRead();
			if (!infile)
			{
				{ queueFilesFailed = true; goto error; }
			}

			formatVersion = stateFile.GetFileVersion();
			int isLegacy = 0;
			int loadInlineHistory = 0;
			int verStatus = nzbget_rs_dsp_check_queue_format_version(
				formatVersion, DISKSTATE_QUEUE_VERSION, &isLegacy, &loadInlineHistory);
			m_legacyQueueRead |= isLegacy != 0;
			if (verStatus == 1)
			{
				error("Failed to read queue: diskstate file is corrupted");
				{ queueFilesFailed = true; goto error; }
			}
			else if (verStatus == 2)
			{
				error("Failed to read queue and history data. Only queue and history from NZBGet v13 or newer can be converted by this RustyGet version. "
					"Old queue and history data still can be converted using NZBGet v16 as an intermediate version.");
				{ queueFilesFailed = true; goto error; }
			}

			if (!LoadQueue(downloadQueue->GetQueue(), servers, *infile, formatVersion)) { queueFilesFailed = true; goto error; }

			if (loadInlineHistory != 0)
			{
				if (!LoadHistory(downloadQueue->GetHistory(), servers, *infile, formatVersion)) { queueFilesFailed = true; goto error; }
			}
		}
	}

	{
		StateFile stateFile("progress", DISKSTATE_QUEUE_VERSION, true, queueReadVersion);
		if (stateFile.FileExists())
		{
			StateDiskFile* infile = stateFile.BeginRead();
			if (!infile)
			{
				{ queueFilesFailed = true; goto error; }
			}

			if (stateFile.GetFileVersion() <= 0)
			{
				error("Failed to read queue: diskstate file is corrupted");
				{ queueFilesFailed = true; goto error; }
			}

			m_legacyQueueRead |= stateFile.GetFileVersion() != DISKSTATE_QUEUE_VERSION;
			if (!LoadProgress(downloadQueue->GetQueue(), servers, *infile, stateFile.GetFileVersion())) { queueFilesFailed = true; goto error; }
		}
	}

	if (nzbget_rs_dqh_diskstate_should_load_separate_history_file(formatVersion) != 0)
	{
		StateFile stateFile("history", DISKSTATE_QUEUE_VERSION, true, queueReadVersion);
		if (stateFile.FileExists())
		{
			StateDiskFile* infile = stateFile.BeginRead();
			if (!infile)
			{
				{ queueFilesFailed = true; goto error; }
			}

			if (stateFile.GetFileVersion() <= 0)
			{
				error("Failed to read queue: diskstate file is corrupted");
				{ queueFilesFailed = true; goto error; }
			}

			m_legacyQueueRead |= stateFile.GetFileVersion() != DISKSTATE_QUEUE_VERSION;
			if (!LoadHistory(downloadQueue->GetHistory(), servers, *infile, stateFile.GetFileVersion())) { queueFilesFailed = true; goto error; }
		}
	}

	LoadAllFileInfos(downloadQueue);
	fileInfosLoaded = true;

	CleanupQueueDir(downloadQueue);

	if (!LoadAllFileStates(downloadQueue, servers)) goto error;

	// file states an earlier build of this branch wrote (formats 8/9) are rewritten
	// in the upstream format, history items' too, which are otherwise only rewritten
	// when touched: after a downgrade upstream could not retry such an item
	ConvertLegacyFileStates(servers);

	ok = true;

error:

	if (!ok)
	{
		error("Error reading diskstate for download queue and history");
	}
	if (nzbget_rs_dqh_diskstate_should_reload_fallback_file_infos(
		fileInfosLoaded ? 1 : 0, downloadQueue->GetQueue()->empty() ? 1 : 0) != 0)
	{
		// the downloads that did load keep running: with only the stubs the queue
		// file holds (no name, size or groups) they showed blank and the next save
		// wrote the stubs over their file infos
		LoadAllFileInfos(downloadQueue);
	}
	if (queueFilesFailed)
	{
		// the next save would overwrite them: kept under another name instead
		SetAsideQueueFiles();
	}
	RaiseIdsPastQueueDir();

	NzbInfo::ResetGenId(true);
	FileInfo::ResetGenId(true);

	CalcFileStats(downloadQueue, formatVersion);

	return ok;
}

/*
 * The ids the queue dir holds files for: new ids are counted on past them. With the
 * queue and history set aside as unreadable, the ids restarted at the few items that
 * loaded: new items got the ids of old ones and took over their logs, file states
 * and dupe-search state (P0-c).
 */
inline void DiskState::RaiseIdsPastQueueDir()
{
	// the last ids handed out: a download deleted for good leaves no file behind,
	// and without them its id went to the next download after a restart (a client
	// that still knew the old id then edited or deleted the new download)
	CharBuffer ids;
	NzbgetRsBuf idsPathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		g_Options->GetQueueDir(), PATH_SEPARATOR, "ids");
	BString<1024> idsFilename = idsPathBuf.data ? idsPathBuf.data : "";
	nzbget_rs_free(idsPathBuf);
	if (FileSystem::FileExists(idsFilename) && FileSystem::LoadFileIntoBuffer(idsFilename, ids, true))
	{
		int nzbId = 0;
		int fileId = 0;
		if (nzbget_rs_dsp_parse_ids_file(ids, &nzbId, &fileId))
		{
			NzbInfo::RaiseIdFloor(nzbId);
			FileInfo::RaiseIdFloor(fileId);
		}
	}

	DirBrowser dir(g_Options->GetQueueDir());
	while (const char* filename = dir.Next())
	{
		int isNzbLog = 0;
		int id = nzbget_rs_dsp_classify_queue_dir_id(filename, &isNzbLog);
		if (id > 0)
		{
			if (isNzbLog)
			{
				NzbInfo::RaiseIdFloor(id);
			}
			else
			{
				FileInfo::RaiseIdFloor(id);
			}
		}
	}
}

/*
 * Keeps the last ids handed out in the queue dir (file "ids", which older
 * versions ignore), for RaiseIdsPastQueueDir.
 */
inline void DiskState::SaveIdFloor()
{
	nzbget_rs_diskstate_save_id_floor(
		g_Options->GetQueueDir(), PATH_SEPARATOR, NzbInfo::LastGenId(), FileInfo::LastGenId());
}

inline bool DiskState::SaveDownloadProgress(DownloadQueue* downloadQueue)
{
	int count = 0;
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		count += nzbInfo->GetChanged() ? 1 : 0;
	}

	debug("Saving queue progress to disk");

	bool ok = true;

	{
		StateFile stateFile("progress", DISKSTATE_QUEUE_VERSION, true);
		if (count > 0)
		{
			StateDiskFile* outfile = stateFile.BeginWrite();
			if (!outfile)
			{
				return false;
			}

			SaveProgress(downloadQueue->GetQueue(), *outfile, count);

			// now rename to dest file name
			ok = stateFile.FinishWrite();
		}
		else
		{
			stateFile.Discard();
		}
	}

	return ok;
}

inline void DiskState::ConvertLegacyFileStates(Servers* servers)
{
	// "<id>" (file info), "<id>s" (partial state), "<id>c" (state of a completed file)
	std::vector<std::pair<int, std::string>> legacy;
	DirBrowser dir(g_Options->GetQueueDir());
	while (const char* filename = dir.Next())
	{
		if (nzbget_rs_dqh_diskstate_is_candidate_queue_file_name(filename) == 0)
		{
			continue;
		}
		NzbgetRsBuf pathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			g_Options->GetQueueDir(), PATH_SEPARATOR, filename);
		BString<1024> path = pathBuf.data ? pathBuf.data : "";
		nzbget_rs_free(pathBuf);
		DiskFile file;
		char header[100] = {0};
		if (!file.Open(path, DiskFile::omRead) || !file.ReadLine(header, sizeof(header)))
		{
			continue;
		}
		file.Close();
		int legacyId = 0;
		int suffixKind = 0;
		if (nzbget_rs_dsp_classify_legacy_file_state(
			filename, header, DISKSTATE_FILE_VERSION, DISKSTATE_FILE_LEGACY_VERSION, &legacyId, &suffixKind))
		{
			legacy.emplace_back(legacyId, nzbget_rs_dqh_diskstate_legacy_suffix_str(suffixKind));
		}
	}
	if (legacy.empty())
	{
		return;
	}

	// each file is written anew and renamed over the old one: a file that another user
	// owns (an earlier nzbget run under another account) can't be opened for writing,
	// but in a writable QueueDir it can be replaced. Not flushed one by one: thousands
	// of syncs would hold up the start for minutes.
	int converted = 0;
	for (const auto& entry : legacy)
	{
		FileInfo fileInfo;
		fileInfo.SetId(entry.first);
		bool ok;
		if (entry.second.empty())
		{
			ok = LoadFile(&fileInfo, true, true) && SaveFile(&fileInfo, true);
		}
		else
		{
			bool completed = entry.second == "c";
			ok = LoadFile(&fileInfo, false, true) && LoadFileState(&fileInfo, servers, completed) &&
				SaveFileState(&fileInfo, completed, true);
		}
		converted += ok ? 1 : 0;
	}
	info("Converted %i of %i file state(s) of an earlier build to the upstream format", converted, (int)legacy.size());
}

inline void DiskState::SetAsideQueueFiles()
{
	time_t now = Util::CurrentTime();
	for (const char* name : { "queue", "history", "progress" })
	{
		NzbgetRsBuf fnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			g_Options->GetQueueDir(), PATH_SEPARATOR, name);
		BString<1024> filename = fnBuf.data ? fnBuf.data : "";
		nzbget_rs_free(fnBuf);
		if (FileSystem::FileExists(filename))
		{
			NzbgetRsBuf keptBuf = nzbget_rs_dqh_diskstate_format_unreadable_path(*filename, (int64)now);
			BString<1024> keptName = keptBuf.data ? keptBuf.data : "";
			nzbget_rs_free(keptBuf);
			if (FileSystem::MoveFile(filename, keptName))
			{
				error("Diskstate file %s could not be read; it is kept as %s", name,
					FileSystem::BaseFileName(keptName));
			}
		}
	}
}

inline void DiskState::SaveQueue(NzbList* queue, StateDiskFile& outfile)
{
	debug("Saving nzb list to disk");

	outfile.PrintLine("%i", (int)queue->size());
	for (NzbInfo* nzbInfo : queue)
	{
		SaveNzbInfo(nzbInfo, outfile);
	}

	std::vector<NzbInfo*> nzbs;
	for (NzbInfo* nzbInfo : queue)
	{
		nzbs.push_back(nzbInfo);
	}
	SaveDupeTrailer(nzbs, outfile);
}

inline bool DiskState::LoadQueue(NzbList* queue, Servers* servers, StateDiskFile& infile, int formatVersion)
{
	debug("Loading nzb list from disk");

	// load nzb-infos
	int size;
	if (infile.ScanLine("%i", &size) != 1) goto error;
	for (int i = 0; i < size; i++)
	{
		std::unique_ptr<NzbInfo> nzbInfo = std::make_unique<NzbInfo>();
		if (!LoadNzbInfo(nzbInfo.get(), servers, infile, formatVersion)) goto error;
		queue->push_back(std::move(nzbInfo));
	}

	// (before format 57 the history follows in the same file: no trailer there)
	if (formatVersion >= 57 && formatVersion <= DISKSTATE_QUEUE_VERSION &&
		!LoadDupeTrailer([queue](int id) { return queue->Find(id); }, infile)) goto error;

	return true;

error:
	error("Error reading nzb list from disk");
	return false;
}

inline void DiskState::SaveProgress(NzbList* queue, StateDiskFile& outfile, int changedCount)
{
	debug("Saving nzb progress to disk");

	outfile.PrintLine("%i", changedCount);
	for (NzbInfo* nzbInfo : queue)
	{
		if (nzbInfo->GetChanged())
		{
			outfile.PrintLine("%i", nzbInfo->GetId());
			SaveNzbInfo(nzbInfo, outfile);
		}
	}

	std::vector<NzbInfo*> changed;
	for (NzbInfo* nzbInfo : queue)
	{
		if (nzbInfo->GetChanged())
		{
			changed.push_back(nzbInfo);
		}
	}
	SaveDupeTrailer(changed, outfile);
}

inline bool DiskState::LoadProgress(NzbList* queue, Servers* servers, StateDiskFile& infile, int formatVersion)
{
	debug("Loading nzb progress from disk");

	// load nzb-infos
	int size;
	if (infile.ScanLine("%i", &size) != 1) goto error;
	for (int i = 0; i < size; i++)
	{
		int id;
		if (infile.ScanLine("%i", &id) != 1) goto error;

		NzbInfo* nzbInfo = queue->Find(id);
		if (!nzbInfo)
		{
			// a progress record of a collection no longer queued (the queue was saved,
			// then nzbget stopped before the progress file was discarded): read past it.
			// It aborted the whole load, and queue and history were set aside
			warn("Skipping saved progress of NZB with id %i, which is no longer queued", id);
			NzbInfo stale;
			if (!LoadNzbInfo(&stale, servers, infile, formatVersion)) goto error;
			continue;
		}

		if (!LoadNzbInfo(nzbInfo, servers, infile, formatVersion)) goto error;
	}

	if (formatVersion <= DISKSTATE_QUEUE_VERSION &&
		!LoadDupeTrailer([queue](int id) { return queue->Find(id); }, infile)) goto error;

	return true;

error:
	error("Error reading nzb progress from disk");
	return false;
}

inline void DiskState::SaveDupeExtras(NzbInfo* nzbInfo, StateDiskFile& outfile)
{
	const StreamRepairJobList* repairJobs = nzbInfo->GetStreamRepairJobs();
	std::vector<NzbgetRsDiskStreamHole> allHoles;
	std::vector<NzbgetRsDiskRepairJob> jobs;
	jobs.reserve(repairJobs->size());
	size_t totalHoles = 0;
	for (const StreamRepairJob& job : *repairJobs)
	{
		totalHoles += job.GetHoles()->size();
	}
	allHoles.reserve(totalHoles);

	for (const StreamRepairJob& job : *repairJobs)
	{
		const StreamRangeList* holes = job.GetHoles();
		size_t startIdx = allHoles.size();
		for (const StreamRange& hole : *holes)
		{
			allHoles.push_back(NzbgetRsDiskStreamHole{ hole.Offset, hole.Size });
		}
		jobs.push_back(NzbgetRsDiskRepairJob{
			job.GetFileId(),
			job.GetDecodedFileSize(),
			job.GetFailedSize(),
			job.GetMissedSize(),
			job.GetParFile() ? 1 : 0,
			job.GetFailedArticles(),
			job.GetStepsHash(),
			job.GetFilename(),
			holes->empty() ? nullptr : &allHoles[startIdx],
			holes->size()
		});
	}

	WriteRsBuf(outfile, nzbget_rs_diskstate_format_dupe_extras(
		nzbInfo->GetDupeRecoveredArticles(),
		nzbInfo->GetDupeRecoveredBytes(),
		nzbInfo->GetDupeRecoveredHoles(),
		jobs.data(),
		jobs.size()));
}

inline bool DiskState::LoadDupeExtras(NzbInfo* nzbInfo, StateDiskFile& infile, int formatVersion)
{
	char buf[1024];
	uint32 recoveredHigh = 0, recoveredLow = 0;
	int recoveredArticles = 0, recoveredHoles = 0;
	int headerScanCnt;
	headerScanCnt = infile.ScanLine("%i,%u,%u,%i", &recoveredArticles, &recoveredHigh, &recoveredLow,
		&recoveredHoles);
	if (headerScanCnt != 4 || !nzbget_rs_dsp_validate_dupe_extras_header(
		headerScanCnt, recoveredArticles, recoveredHoles)) goto error;
	nzbInfo->SetDupeRecoveredArticles(recoveredArticles);
	nzbInfo->SetDupeRecoveredBytes(Util::JoinInt64(recoveredHigh, recoveredLow));
	nzbInfo->SetDupeRecoveredHoles(recoveredHoles);

	int repairCount;
	{
		repairCount = 0;
		int repairScanCnt = infile.ScanLine("%i", &repairCount);
		if (repairScanCnt != 1 || repairCount < 0 || repairCount > 100000) goto error;
	}
	for (int i = 0; i < repairCount; i++)
	{
		int fileId = 0, parFile = 0, failedArticles = 0;
		uint32 decodedHigh = 0, decodedLow = 0, failedHigh = 0, failedLow = 0, missedHigh = 0, missedLow = 0;
		uint32 stepsHigh = 0, stepsLow = 0;
		int expectedJobFields = nzbget_rs_dqh_diskstate_expected_repair_job_fields(formatVersion);
		int jobScanCnt = expectedJobFields == 11 ?
			infile.ScanLine("%i,%u,%u,%u,%u,%u,%u,%i,%i,%u,%u", &fileId, &decodedHigh,
				&decodedLow, &failedHigh, &failedLow, &missedHigh, &missedLow, &parFile,
				&failedArticles, &stepsHigh, &stepsLow) :
			infile.ScanLine("%i,%u,%u,%u,%u,%u,%u,%i,%i", &fileId, &decodedHigh,
				&decodedLow, &failedHigh, &failedLow, &missedHigh, &missedLow, &parFile,
				&failedArticles);
		if (jobScanCnt != expectedJobFields || !nzbget_rs_dsp_validate_repair_job_header(
			jobScanCnt, expectedJobFields, fileId, parFile, failedArticles)) goto error;
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		int holeCount;
		{
			holeCount = 0;
			int holeScanCnt = infile.ScanLine("%i", &holeCount);
			if (holeScanCnt != 1 || holeCount < 0 || holeCount > 1000000) goto error;
		}
		int64 decodedSize = Util::JoinInt64(decodedHigh, decodedLow);
		StreamRangeList holes;
		holes.reserve(holeCount);
		for (int j = 0; j < holeCount; j++)
		{
			uint32 offsetHigh, offsetLow, sizeHigh, sizeLow;
			if (infile.ScanLine("%u,%u,%u,%u", &offsetHigh, &offsetLow, &sizeHigh, &sizeLow) != 4) goto error;
			int64 offset = 0;
			int64 size = 0;
			if (!nzbget_rs_dsp_validate_repair_hole(offsetHigh, offsetLow, sizeHigh, sizeLow, decodedSize, &offset, &size)) goto error;
			holes.push_back({offset, size});
		}
		nzbInfo->GetStreamRepairJobs()->emplace_back(fileId, buf, decodedSize,
			Util::JoinInt64(failedHigh, failedLow), Util::JoinInt64(missedHigh, missedLow),
			failedArticles, parFile != 0, std::move(holes),
			(uint64)Util::JoinInt64(stepsHigh, stepsLow));
	}


	return true;

error:
	return false;
}

// this build's data after upstream's: the extras of the nzbs that have any
inline void DiskState::SaveDupeTrailer(const std::vector<NzbInfo*>& nzbs, StateDiskFile& outfile)
{
	std::vector<NzbInfo*> withExtras;
	for (NzbInfo* nzbInfo : nzbs)
	{
		if (nzbget_rs_dsp_has_dupe_extras(
			nzbInfo->GetDupeRecoveredArticles(),
			nzbInfo->GetDupeRecoveredBytes(),
			nzbInfo->GetDupeRecoveredHoles(),
			nzbInfo->GetStreamRepairJobs()->size()))
		{
			withExtras.push_back(nzbInfo);
		}
	}
	outfile.PrintLine("%s", DUPESTATE_TRAILER);
	outfile.PrintLine("%i", (int)withExtras.size());
	for (NzbInfo* nzbInfo : withExtras)
	{
		outfile.PrintLine("%i", nzbInfo->GetId());
		SaveDupeExtras(nzbInfo, outfile);
	}

	outfile.PrintLine("%s", UNPACKSTATE_TRAILER);
	outfile.PrintLine("%i", (int)nzbs.size());
	for (NzbInfo* nzbInfo : nzbs)
	{
		std::vector<const char*> archivePtrs;
		std::vector<CString>* archives = nzbInfo->GetExtractedArchives();
		if (archives)
		{
			archivePtrs.reserve(archives->size());
			for (const CString& arc : *archives)
			{
				archivePtrs.push_back(*arc ? *arc : "");
			}
		}
		WriteRsBuf(outfile, nzbget_rs_diskstate_format_unpack_trailer_item(
			nzbInfo->GetId(),
			static_cast<int>(nzbInfo->GetDirectUnpackStatus()),
			nzbInfo->GetUnpackTried() ? 1 : 0,
			nzbInfo->GetParRepaired() ? 1 : 0,
			nzbInfo->GetLastUnpackStatus(),
			archivePtrs.data(),
			archivePtrs.size()));
	}
}

inline bool DiskState::LoadDupeTrailer(const std::function<NzbInfo*(int)>& find, StateDiskFile& infile)
{
	char buf[1024];
	if (!infile.ReadLine(buf, sizeof(buf)) || !nzbget_rs_dqh_bytes_equal(buf, DUPESTATE_TRAILER))
	{
		return true;	// a file upstream nzbget wrote, or one without any
	}
	int count = 0;
	int countScanCnt = infile.ScanLine("%i", &count);
	if (nzbget_rs_dqh_diskstate_is_nonneg_count_scan_invalid(countScanCnt, count) != 0) goto error;
	for (int i = 0; i < count; i++)
	{
		int id;
		if (infile.ScanLine("%i", &id) != 1) goto error;
		NzbInfo* nzbInfo = find(id);
		NzbInfo unknown;	// an nzb no longer listed: its extras are read and dropped
		if (!LoadDupeExtras(nzbInfo ? nzbInfo : &unknown, infile, DISKSTATE_QUEUE_LEGACY_VERSION)) goto error;
	}
	if (infile.ReadLine(buf, sizeof(buf)) && nzbget_rs_dqh_bytes_equal(buf, UNPACKSTATE_TRAILER))
	{
		int unpackCount = 0;
		int unpackScanCnt = infile.ScanLine("%i", &unpackCount);
		if (nzbget_rs_dqh_diskstate_is_nonneg_count_scan_invalid(unpackScanCnt, unpackCount) != 0) goto error;
		for (int i = 0; i < unpackCount; i++)
		{
			int id = 0;
			if (infile.ScanLine("%i", &id) != 1) goto error;
			NzbInfo* nzbInfo = find(id);
			NzbInfo unknown;
			NzbInfo* target = nzbInfo ? nzbInfo : &unknown;

			if (!infile.ReadLine(buf, sizeof(buf))) goto error;
			char line2[1024];
			char line3[1024];
			if (!infile.ReadLine(line2, sizeof(line2))) goto error;
			if (!infile.ReadLine(line3, sizeof(line3))) goto error;
			int directUnpackStatus = 0, unpackTriedBool = 0, parRepairedBool = 0, lastUnpackStatus = 0;
			if (!nzbget_rs_dsp_parse_unpack_state_lines(
				buf, line2, line3,
				static_cast<int>(NzbInfo::nsRunning), static_cast<int>(NzbInfo::nsNone),
				&directUnpackStatus, &unpackTriedBool, &parRepairedBool, &lastUnpackStatus)) goto error;
			target->SetDirectUnpackStatus(static_cast<NzbInfo::EDirectUnpackStatus>(directUnpackStatus));
			target->SetUnpackTried(unpackTriedBool != 0);
			target->SetParRepaired(parRepairedBool != 0);
			target->SetLastUnpackStatus(lastUnpackStatus);

			std::vector<char> largeBuf(65536);
			if (!infile.ReadLine(largeBuf.data(), largeBuf.size())) goto error;
			char* archivesStr = largeBuf.data();
			if (!strncmp(archivesStr, "ExtractedArchives=", 18))
			{
				archivesStr += 18;
			}
			std::vector<CString>* archives = target->GetExtractedArchives();
			if (archives)
			{
				archives->clear();
				size_t cursor = 0, tokStart = 0, tokLen = 0;
				while (nzbget_rs_dqh_diskstate_next_comma_token(
					archivesStr, cursor, &tokStart, &tokLen, &cursor))
				{
					archives->emplace_back(archivesStr + tokStart, (int)tokLen);
				}
			}
		}
	}
	return true;

error:
	error("Error reading duplicate repair state from disk");
	return false;
}

inline void DiskState::SaveNzbInfo(NzbInfo* nzbInfo, StateDiskFile& outfile)
{
	NzbgetRsDiskNzbInfoInput fields{};
	fields.id = nzbInfo->GetId();
	fields.kind = static_cast<int>(nzbInfo->GetKind());
	fields.url = nzbInfo->GetUrl();
	fields.filename = nzbInfo->GetFilename();
	fields.dest_dir = nzbInfo->GetDestDir();
	fields.final_dir = nzbInfo->GetFinalDir();
	fields.hard_link_path = nzbInfo->GetHardLinkPath().c_str();
	fields.meta_name = nzbInfo->GetMetaName().c_str();
	fields.queued_filename = nzbInfo->GetQueuedFilename();
	fields.name = nzbInfo->GetName();
	fields.category = nzbInfo->GetCategory();
	fields.priority = (int)nzbInfo->GetPriority();
	fields.has_post_info = nzbInfo->GetPostInfo() ? 1 : 0;
	fields.post_stage = nzbInfo->GetPostInfo() ? (int)nzbInfo->GetPostInfo()->GetStage() : 0;
	fields.delete_paused = (int)nzbInfo->GetDeletePaused();
	fields.many_dupe_files = (int)nzbInfo->GetManyDupeFiles();
	fields.feed_id = nzbInfo->GetFeedId();
	fields.par_status = (int)nzbInfo->GetParStatus();
	fields.unpack_status = (int)nzbInfo->GetUnpackStatus();
	fields.move_status = (int)nzbInfo->GetMoveStatus();
	fields.par_rename_status = (int)nzbInfo->GetParRenameStatus();
	fields.rar_rename_status = (int)nzbInfo->GetRarRenameStatus();
	fields.direct_rename_status = (int)nzbInfo->GetDirectRenameStatus();
	fields.delete_status = (int)nzbInfo->GetDeleteStatus();
	fields.mark_status = (int)nzbInfo->GetMarkStatus();
	fields.url_status = (int)nzbInfo->GetUrlStatus();
	fields.unpack_cleaned_up_disk = (int)nzbInfo->GetUnpackCleanedUpDisk();
	fields.health_paused = (int)nzbInfo->GetHealthPaused();
	fields.add_url_paused = (int)nzbInfo->GetAddUrlPaused();
	fields.file_count = nzbInfo->GetFileCount();
	fields.parked_file_count = nzbInfo->GetParkedFileCount();
	fields.message_count = nzbInfo->GetMessageCount();
	fields.min_time = (int)nzbInfo->GetMinTime();
	fields.max_time = (int)nzbInfo->GetMaxTime();
	fields.par_full = (int)nzbInfo->GetParFull();
	fields.force_par_full = nzbInfo->GetPostInfo() ? (int)nzbInfo->GetPostInfo()->GetForceParFull() : 0;
	fields.force_repair = nzbInfo->GetPostInfo() ? (int)nzbInfo->GetPostInfo()->GetForceRepair() : 0;
	fields.extra_par_blocks = nzbInfo->GetExtraParBlocks();
	fields.full_content_hash = nzbInfo->GetFullContentHash();
	fields.filtered_content_hash = nzbInfo->GetFilteredContentHash();
	fields.size = nzbInfo->GetSize();
	fields.success_size = nzbInfo->GetSuccessSize();
	fields.failed_size = nzbInfo->GetFailedSize();
	fields.par_size = nzbInfo->GetParSize();
	fields.par_success_size = nzbInfo->GetParSuccessSize();
	fields.par_failed_size = nzbInfo->GetParFailedSize();
	fields.total_articles = nzbInfo->GetTotalArticles();
	fields.success_articles = nzbInfo->GetSuccessArticles();
	fields.failed_articles = nzbInfo->GetFailedArticles();
	fields.dupe_key = nzbInfo->GetDupeKey();
	fields.dupe_mode = (int)nzbInfo->GetDupeMode();
	fields.dupe_score = nzbInfo->GetDupeScore();
	fields.dupe_hint = (int)nzbInfo->GetDupeHint();
	fields.downloaded_size = nzbInfo->GetDownloadedSize();
	fields.download_sec = nzbInfo->GetDownloadSec();
	fields.post_total_sec = nzbInfo->GetPostTotalSec();
	fields.par_sec = nzbInfo->GetParSec();
	fields.repair_sec = nzbInfo->GetRepairSec();
	fields.unpack_sec = nzbInfo->GetUnpackSec();
	fields.skip_script_processing = static_cast<int>(nzbInfo->GetSkipScriptProcessing());
	fields.skip_disk_write = static_cast<int>(nzbInfo->GetSkipDiskWrite());
	fields.desired_server_id = nzbInfo->GetDesiredServerId();

	std::vector<NzbgetRsDiskCompletedFile> completedFiles;
	completedFiles.reserve(nzbInfo->GetCompletedFiles()->size());
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		completedFiles.push_back(NzbgetRsDiskCompletedFile{
			completedFile.GetId(),
			(int)completedFile.GetStatus(),
			completedFile.GetCrc(),
			(int)completedFile.GetParFile(),
			completedFile.GetHash16k(),
			completedFile.GetParSetId(),
			completedFile.GetFilename(),
			completedFile.GetOrigname()
		});
	}

	std::vector<NzbgetRsDiskParam> params;
	params.reserve(nzbInfo->GetParameters()->size());
	for (NzbParameter& parameter : nzbInfo->GetParameters())
	{
		params.push_back(NzbgetRsDiskParam{ parameter.GetName(), parameter.GetValue() });
	}

	std::vector<NzbgetRsDiskScriptStatus> scripts;
	scripts.reserve(nzbInfo->GetScriptStatuses()->size());
	for (ScriptStatus& scriptStatus : nzbInfo->GetScriptStatuses())
	{
		scripts.push_back(NzbgetRsDiskScriptStatus{ (int)scriptStatus.GetStatus(), scriptStatus.GetName() });
	}

	std::vector<NzbgetRsDiskServerStat> stats;
	stats.reserve(nzbInfo->GetServerStats()->size());
	for (ServerStat& serverStat : nzbInfo->GetServerStats())
	{
		stats.push_back(NzbgetRsDiskServerStat{
			serverStat.GetServerId(),
			serverStat.GetSuccessArticles(),
			serverStat.GetFailedArticles()
		});
	}

	std::vector<NzbgetRsDiskFileStub> fileEntries;
	fileEntries.reserve(nzbInfo->GetFileList()->size());
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		if (!fileInfo->GetDeleted())
		{
			fileEntries.push_back(NzbgetRsDiskFileStub{
				fileInfo->GetId(),
				(int)fileInfo->GetPaused(),
				(int)fileInfo->GetExtraPriority()
			});
		}
	}

	WriteRsBuf(outfile, nzbget_rs_diskstate_format_nzb_info(
		&fields,
		completedFiles.data(), completedFiles.size(),
		params.data(), params.size(),
		scripts.data(), scripts.size(),
		stats.data(), stats.size(),
		fileEntries.data(), fileEntries.size()));
}

inline bool DiskState::LoadNzbInfo(NzbInfo* nzbInfo, Servers* servers, StateDiskFile& infile, int formatVersion)
{
	char buf[10240];
	int parameterCount = 0;
	int skipScriptProcessing = 0;
	int skipDiskWrite = 0;
	int desiredServerId = 0;
	int numItems = 0;

	int id;
	if (infile.ScanLine("%i", &id) != 1) goto error;
	nzbInfo->SetId(id);

	int kind;
	if (infile.ScanLine("%i", &kind) != 1) goto error;
	nzbInfo->SetKind((NzbInfo::EKind)kind);

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	nzbInfo->SetUrl(buf);

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	nzbInfo->SetFilename(buf);

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	nzbInfo->SetDestDir(buf);

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	nzbInfo->SetFinalDir(buf);

	if (formatVersion >= 63) 
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		nzbInfo->SetHardLinkPath(buf);
	}

	// format 65 was written only by pre-release DupeArticleFallback builds,
	// which had the stream-repair state below but not the meta name
	if (nzbget_rs_dqh_diskstate_has_meta_name_field(formatVersion) != 0)
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		nzbInfo->SetMetaName(buf);
	}

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	nzbInfo->SetQueuedFilename(buf);

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	if (!Util::EmptyStr(buf))
	{
		nzbInfo->SetName(buf);
	}

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	nzbInfo->SetCategory(buf);

	int priority, postStage, deletePaused, manyDupeFiles, feedId;
	if (formatVersion >= 54)
	{
		if (infile.ScanLine("%i,%i,%i,%i,%i", &priority, &postStage, &deletePaused, &manyDupeFiles, &feedId) != 5) goto error;
	}
	else
	{
		if (infile.ScanLine("%i,%i,%i,%i", &priority, &postStage, &deletePaused, &manyDupeFiles) != 4) goto error;
		feedId = 0;
	}
	nzbInfo->SetPriority(priority);
	nzbInfo->SetDeletePaused((bool)deletePaused);
	nzbInfo->SetManyDupeFiles((bool)manyDupeFiles);
	if (postStage > 0)
	{
		nzbInfo->EnterPostProcess();
		postStage = nzbget_rs_dsp_normalize_post_stage(postStage, formatVersion, (int)PostInfo::ptFinished);
		nzbInfo->GetPostInfo()->SetStage((PostInfo::EStage)postStage);
	}
	nzbInfo->SetFeedId(feedId);

	int parStatus, unpackStatus, moveStatus, parRenameStatus, rarRenameStatus,
		directRenameStatus, deleteStatus, markStatus, urlStatus;
	if (formatVersion >= 60)
	{
		if (infile.ScanLine("%i,%i,%i,%i,%i,%i,%i,%i,%i", &parStatus,
			&unpackStatus, &moveStatus, &parRenameStatus, &rarRenameStatus, &directRenameStatus,
			&deleteStatus, &markStatus, &urlStatus) != 9) goto error;
	}
	else if (formatVersion >= 58)
	{
		directRenameStatus = 0;
		if (infile.ScanLine("%i,%i,%i,%i,%i,%i,%i,%i", &parStatus,
			&unpackStatus, &moveStatus, &parRenameStatus, &rarRenameStatus, &deleteStatus,
			&markStatus, &urlStatus) != 8) goto error;
	}
	else
	{
		rarRenameStatus = directRenameStatus = 0;
		if (infile.ScanLine("%i,%i,%i,%i,%i,%i,%i", &parStatus, &unpackStatus, &moveStatus,
			&parRenameStatus, &deleteStatus, &markStatus, &urlStatus) != 7) goto error;
	}
	nzbInfo->SetParStatus((NzbInfo::EParStatus)parStatus);
	nzbInfo->SetUnpackStatus((NzbInfo::EPostUnpackStatus)unpackStatus);
	nzbInfo->SetMoveStatus((NzbInfo::EMoveStatus)moveStatus);
	nzbInfo->SetParRenameStatus((NzbInfo::EPostRenameStatus)parRenameStatus);
	nzbInfo->SetRarRenameStatus((NzbInfo::EPostRenameStatus)rarRenameStatus);
	nzbInfo->SetDirectRenameStatus((NzbInfo::EDirectRenameStatus)directRenameStatus);
	nzbInfo->SetDeleteStatus((NzbInfo::EDeleteStatus)deleteStatus);
	nzbInfo->SetMarkStatus((NzbInfo::EMarkStatus)markStatus);
	if (nzbInfo->GetKind() == NzbInfo::nkNzb || (NzbInfo::EUrlStatus)urlStatus >= NzbInfo::lsFailed)
	{
		nzbInfo->SetUrlStatus((NzbInfo::EUrlStatus)urlStatus);
	}

	int unpackCleanedUpDisk, healthPaused, addUrlPaused;
	if (infile.ScanLine("%i,%i,%i", &unpackCleanedUpDisk, &healthPaused, &addUrlPaused) != 3) goto error;
	nzbInfo->SetUnpackCleanedUpDisk((bool)unpackCleanedUpDisk);
	nzbInfo->SetHealthPaused((bool)healthPaused);
	nzbInfo->SetAddUrlPaused((bool)addUrlPaused);

	int fileCount, parkedFileCount, messageCount;
	if (formatVersion >= 52)
	{
		if (infile.ScanLine("%i,%i,%i", &fileCount, &parkedFileCount, &messageCount) != 3) goto error;
	}
	else
	{
		if (infile.ScanLine("%i,%i", &fileCount, &parkedFileCount) != 2) goto error;
		messageCount = 0;
	}
	nzbInfo->SetFileCount(fileCount);
	nzbInfo->SetParkedFileCount(parkedFileCount);
	nzbInfo->SetMessageCount(messageCount);

	int minTime, maxTime;
	if (infile.ScanLine("%i,%i", &minTime, &maxTime) != 2) goto error;
	nzbInfo->SetMinTime((time_t)minTime);
	nzbInfo->SetMaxTime((time_t)maxTime);

	if (formatVersion >= 51)
	{
		int parFull, forceParFull, forceRepair, extraParBlocks = 0;
		if (formatVersion >= 55)
		{
			if (infile.ScanLine("%i,%i,%i,%i", &parFull, &forceParFull, &forceRepair, &extraParBlocks) != 4) goto error;
		}
		else
		{
			if (infile.ScanLine("%i,%i,%i", &parFull, &forceParFull, &forceRepair) != 3) goto error;
		}
		nzbInfo->SetParFull((bool)parFull);
		nzbInfo->SetExtraParBlocks(extraParBlocks);
		if (nzbInfo->GetPostInfo())
		{
			nzbInfo->GetPostInfo()->SetForceParFull((bool)forceParFull);
			nzbInfo->GetPostInfo()->SetForceRepair((bool)forceRepair);
		}
	}

	uint32 fullContentHash, filteredContentHash;
	if (infile.ScanLine("%u,%u", &fullContentHash, &filteredContentHash) != 2) goto error;
	nzbInfo->SetFullContentHash(fullContentHash);
	nzbInfo->SetFilteredContentHash(filteredContentHash);

	uint32 High1, Low1, High2, Low2, High3, Low3;
	if (infile.ScanLine("%u,%u,%u,%u,%u,%u", &High1, &Low1, &High2, &Low2, &High3, &Low3) != 6) goto error;
	nzbInfo->SetSize(Util::JoinInt64(High1, Low1));
	nzbInfo->SetSuccessSize(Util::JoinInt64(High2, Low2));
	nzbInfo->SetFailedSize(Util::JoinInt64(High3, Low3));
	nzbInfo->SetCurrentSuccessSize(nzbInfo->GetSuccessSize());
	nzbInfo->SetCurrentFailedSize(nzbInfo->GetFailedSize());

	if (infile.ScanLine("%u,%u,%u,%u,%u,%u", &High1, &Low1, &High2, &Low2, &High3, &Low3) != 6) goto error;
	nzbInfo->SetParSize(Util::JoinInt64(High1, Low1));
	nzbInfo->SetParSuccessSize(Util::JoinInt64(High2, Low2));
	nzbInfo->SetParFailedSize(Util::JoinInt64(High3, Low3));
	nzbInfo->SetParCurrentSuccessSize(nzbInfo->GetParSuccessSize());
	nzbInfo->SetParCurrentFailedSize(nzbInfo->GetParFailedSize());

	int totalArticles, successArticles, failedArticles;
	if (infile.ScanLine("%i,%i,%i", &totalArticles, &successArticles, &failedArticles) != 3) goto error;
	nzbInfo->SetTotalArticles(totalArticles);
	nzbInfo->SetSuccessArticles(successArticles);
	nzbInfo->SetFailedArticles(failedArticles);
	nzbInfo->SetCurrentSuccessArticles(successArticles);
	nzbInfo->SetCurrentFailedArticles(failedArticles);

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	nzbInfo->SetDupeKey(buf);

	int dupeMode, dupeScore, dupeHint;
	dupeHint = 0; //clang requires initialization in a separate line (due to goto statements)
	if (formatVersion >= 61)
	{
		if (infile.ScanLine("%i,%i,%i", &dupeMode, &dupeScore, &dupeHint) != 3) goto error;
	}
	else
	{
		if (infile.ScanLine("%i,%i", &dupeMode, &dupeScore) != 2) goto error;
	}
	nzbInfo->SetDupeMode((EDupeMode)dupeMode);
	nzbInfo->SetDupeScore(dupeScore);
	// (the hint went into the mode: every item came back from a restart with its
	// mode replaced by its hint - FORCE and ALL read SCORE)
	nzbInfo->SetDupeHint((NzbInfo::EDupeHint)dupeHint);

	if (formatVersion >= 48)
	{
		uint32 High4, Low4, downloadSec, postTotalSec, parSec, repairSec, unpackSec;
		if (infile.ScanLine("%u,%u,%i,%i,%i,%i,%i", &High4, &Low4, &downloadSec, &postTotalSec, &parSec, &repairSec, &unpackSec) != 7) goto error;
		nzbInfo->SetDownloadedSize(Util::JoinInt64(High4, Low4));
		nzbInfo->SetDownloadSec(downloadSec);
		nzbInfo->SetPostTotalSec(postTotalSec);
		nzbInfo->SetParSec(parSec);
		nzbInfo->SetRepairSec(repairSec);
		nzbInfo->SetUnpackSec(unpackSec);
	}

	nzbInfo->GetCompletedFiles()->clear();
	if (infile.ScanLine("%i", &fileCount) != 1) goto error;
	for (int i = 0; i < fileCount; i++)
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;

		int id = 0;
		const char* fileName = buf;
		int status = 0;
		uint32 crc = 0;
		int parFile = 0;
		const char* hash16k = nullptr;
		const char* parSetId = nullptr;
		char filenameBuf[1024];
		char origName[1024] = {};

		if (!nzbget_rs_diskstate_parse_completed_file_line(
			buf, formatVersion, &id, &status, &crc, &parFile, &hash16k, &parSetId, &fileName))
		{
			goto error;
		}
		if (formatVersion >= 62)
		{
			if (!infile.ReadLine(filenameBuf, sizeof(filenameBuf))) goto error;
			fileName = filenameBuf;
			if (!infile.ReadLine(origName, sizeof(origName))) goto error;
		}

		nzbInfo->GetCompletedFiles()->emplace_back(
			id, 
			Util::EmptyStr(fileName) ? "" : fileName,
			Util::EmptyStr(origName) ? "" : origName,
			static_cast<CompletedFile::EStatus>(status), 
			crc, 
			static_cast<bool>(parFile),
			Util::EmptyStr(hash16k) ? "" : hash16k,
			Util::EmptyStr(parSetId) ? "" : parSetId);
	}

	nzbInfo->GetParameters()->clear();

	numItems = infile.ScanLine("%i,%i,%i,%i", 
		&parameterCount, 
		&skipScriptProcessing, 
		&skipDiskWrite,
		&desiredServerId);

	{
		if (numItems != 1 && numItems != 4) goto error;
		if (numItems == 4)
		{
			nzbInfo->SetSkipScriptProcessing(static_cast<bool>(skipScriptProcessing));
			nzbInfo->SetSkipDiskWrite(static_cast<bool>(skipDiskWrite));
			nzbInfo->SetDesiredServerId(desiredServerId);
		}
	}

	for (int i = 0; i < parameterCount; i++)
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;

		size_t eqPos = 0;
		if (nzbget_rs_dqh_diskstate_find_char_pos(buf, '=', &eqPos) != 0)
		{
			buf[eqPos] = '\0';
			nzbInfo->GetParameters()->SetParameter(buf, buf + eqPos + 1);
		}
	}

	nzbInfo->GetScriptStatuses()->clear();
	int scriptCount;
	if (infile.ScanLine("%i", &scriptCount) != 1) goto error;
	for (int i = 0; i < scriptCount; i++)
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;

		size_t commaPos = 0;
		if (nzbget_rs_dqh_diskstate_find_char_pos(buf, ',', &commaPos) != 0)
		{
			const char* scriptName = buf + commaPos + 1;
			int status = atoi(buf);
			if (status > 1 && formatVersion < 25) status--;
			nzbInfo->GetScriptStatuses()->emplace_back(scriptName, (ScriptStatus::EStatus)status);
		}
	}

	if (!LoadServerStats(nzbInfo->GetServerStats(), servers, infile)) goto error;
	nzbInfo->GetCurrentServerStats()->ListOp(nzbInfo->GetServerStats(), ServerStatList::soSet);

	if (formatVersion < 52)
	{
		int logCount;
		if (infile.ScanLine("%i", &logCount) != 1) goto error;
		for (int i = 0; i < logCount; i++)
		{
			if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		}
	}

	nzbInfo->GetFileList()->clear();
	if (infile.ScanLine("%i", &fileCount) != 1) goto error;
	for (int i = 0; i < fileCount; i++)
	{
		uint32 id, paused, time;
		int extraPriority;

		if (formatVersion >= 56)
		{
			if (infile.ScanLine("%i,%i,%i", &id, &paused, &extraPriority) != 3) goto error;
		}
		else
		{
			if (infile.ScanLine("%i,%i,%i,%i", &id, &paused, &time, &extraPriority) != 4) goto error;
		}

		std::unique_ptr<FileInfo> fileInfo = std::make_unique<FileInfo>();
		fileInfo->SetId(id);
		fileInfo->SetPaused(paused);
		if (formatVersion < 56)
		{
			fileInfo->SetTime(time);
		}
		fileInfo->SetExtraPriority((bool)extraPriority);
		fileInfo->SetNzbInfo(nzbInfo);
		nzbInfo->GetFileList()->Add(std::move(fileInfo));
	}

	nzbInfo->GetStreamRepairJobs()->clear();
	// earlier builds of this branch wrote it inline (formats 65 and 66); the trailer
	// of the queue file carries it since
	if ((formatVersion == 65 || formatVersion == 66) &&
		!LoadDupeExtras(nzbInfo, infile, formatVersion)) goto error;

	// restore unpack state (R3)
	if (formatVersion >= 67)
	{
		char line2[1024];
		char line3[1024];
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		if (!infile.ReadLine(line2, sizeof(line2))) goto error;
		if (!infile.ReadLine(line3, sizeof(line3))) goto error;
		int directUnpackStatus = 0, unpackTriedBool = 0, parRepairedBool = 0, lastUnpackStatus = 0;
		if (!nzbget_rs_dsp_parse_unpack_state_lines(
			buf, line2, line3,
			static_cast<int>(NzbInfo::nsRunning), static_cast<int>(NzbInfo::nsNone),
			&directUnpackStatus, &unpackTriedBool, &parRepairedBool, &lastUnpackStatus)) goto error;
		nzbInfo->SetDirectUnpackStatus(static_cast<NzbInfo::EDirectUnpackStatus>(directUnpackStatus));
		nzbInfo->SetUnpackTried(unpackTriedBool != 0);
		nzbInfo->SetParRepaired(parRepairedBool != 0);
		nzbInfo->SetLastUnpackStatus(lastUnpackStatus);

		std::vector<char> largeBuf(65536);
		if (!infile.ReadLine(largeBuf.data(), largeBuf.size())) goto error;
		char* archivesStr = largeBuf.data();
		if (!strncmp(archivesStr, "ExtractedArchives=", 18))
		{
			archivesStr += 18;
		}
		std::vector<CString>* archives = nzbInfo->GetExtractedArchives();
		if (archives)
		{
			archives->clear();
			size_t cursor = 0, tokStart = 0, tokLen = 0;
			while (nzbget_rs_dqh_diskstate_next_comma_token(
				archivesStr, cursor, &tokStart, &tokLen, &cursor))
			{
				archives->emplace_back(archivesStr + tokStart, (int)tokLen);
			}
		}
	}

	return true;

error:
	error("Error reading nzb info from disk");
	return false;
}

inline bool DiskState::LoadServerStats(ServerStatList* serverStatList, Servers* servers, StateDiskFile& infile)
{
	int statCount;
	if (infile.ScanLine("%i", &statCount) != 1) goto error;
	for (int i = 0; i < statCount; i++)
	{
		int serverId, successArticles, failedArticles;
		if (infile.ScanLine("%i,%i,%i", &serverId, &successArticles, &failedArticles) != 3) goto error;

		if (servers)
		{
			// find server (id could change if config file was edited)
			for (NewsServer* newsServer : servers)
			{
				if (nzbget_rs_dqh_diskstate_matches_server_state_id(
					newsServer->GetStateId(), serverId) != 0)
				{
					serverStatList->StatOp(newsServer->GetId(), successArticles, failedArticles, ServerStatList::soSet);
				}
			}
		}
	}

	return true;

error:
	error("Error reading server stats from disk");
	return false;
}

inline bool DiskState::SaveFile(FileInfo* fileInfo, bool replace)
{
	debug("Saving FileInfo %i to disk", fileInfo->GetId());

	NzbgetRsBuf fnBuf = nzbget_rs_dqh_conn_format_port_str(fileInfo->GetId());
	BString<100> filename = fnBuf.data ? fnBuf.data : "";
	nzbget_rs_free(fnBuf);
	StateFile stateFile(filename, DISKSTATE_FILE_VERSION, replace);
	stateFile.SetFlush(!replace);

	StateDiskFile* outfile = stateFile.BeginWrite();
	if (!outfile)
	{
		return false;
	}

	return SaveFileInfo(fileInfo, *outfile, true) && stateFile.FinishWrite();
}

inline bool DiskState::SaveFileInfo(FileInfo* fileInfo, StateDiskFile& outfile, bool articles)
{
	NzbgetRsDiskFileInfoInput fields{};
	fields.subject = fileInfo->GetSubject();
	fields.filename = fileInfo->GetFilename();
	fields.origname = fileInfo->GetOrigname() ? fileInfo->GetOrigname() : "";
	fields.output_filename = fileInfo->GetOutputFilename().c_str();
	fields.filename_confirmed = (int)fileInfo->GetFilenameConfirmed();
	fields.time = (int)fileInfo->GetTime();
	fields.output_initialized = (int)fileInfo->GetOutputInitialized();
	fields.size = fileInfo->GetSize();
	fields.missed_size = fileInfo->GetMissedSize();
	fields.par_file = (int)fileInfo->GetParFile();
	fields.total_articles = fileInfo->GetTotalArticles();
	fields.missed_articles = fileInfo->GetMissedArticles();

	std::vector<const char*> groups;
	groups.reserve(fileInfo->GetGroups()->size());
	for (CString& group : fileInfo->GetGroups())
	{
		groups.push_back(*group);
	}

	std::vector<NzbgetRsDiskArticleInfo> articleEntries;
	if (articles)
	{
		articleEntries.reserve(fileInfo->GetArticles()->size());
		for (ArticleInfo* articleInfo : fileInfo->GetArticles())
		{
			articleEntries.push_back(NzbgetRsDiskArticleInfo{
				articleInfo->GetPartNumber(),
				articleInfo->GetSize(),
				nzbget_rs_dqh_diskstate_select_original_or_current_msg_id(
					articleInfo->GetDupeOriginalMessageId(),
					articleInfo->GetMessageId())
			});
		}
	}

	WriteRsBuf(outfile, nzbget_rs_diskstate_format_file_info(
		&fields,
		groups.data(), groups.size(),
		articles ? 1 : 0,
		articleEntries.data(), articleEntries.size()));

	return true;
}

inline bool DiskState::LoadArticles(FileInfo* fileInfo)
{
	return LoadFile(fileInfo, false, true);
}

inline bool DiskState::LoadFile(FileInfo* fileInfo, bool fileSummary, bool articles)
{
	debug("Loading FileInfo %i from disk", fileInfo->GetId());

	NzbgetRsBuf fnBuf = nzbget_rs_dqh_conn_format_port_str(fileInfo->GetId());
	BString<100> filename = fnBuf.data ? fnBuf.data : "";
	nzbget_rs_free(fnBuf);
	StateFile stateFile(filename, DISKSTATE_FILE_VERSION, false, DISKSTATE_FILE_LEGACY_VERSION);

	StateDiskFile* infile = stateFile.BeginRead();
	if (!infile)
	{
		return false;
	}

	return LoadFileInfo(fileInfo, *infile, stateFile.GetFileVersion(), fileSummary, articles);
}

inline bool DiskState::LoadFileInfo(FileInfo* fileInfo, StateDiskFile& infile, int formatVersion, bool fileSummary, bool articles)
{
	char buf[1024];

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	if (fileSummary && !Util::EmptyStr(buf)) fileInfo->SetSubject(buf);

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	if (fileSummary && !Util::EmptyStr(buf)) fileInfo->SetFilename(buf);

	if (formatVersion >= 6)
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		if (fileSummary && !Util::EmptyStr(buf)) fileInfo->SetOrigname(buf);
	}

	if (formatVersion >= 7)
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		if (!Util::EmptyStr(buf))
		{
			fileInfo->SetOutputFilename(buf);
		}
	}

	if (formatVersion >= 5)
	{
		int time = 0, filenameConfirmed = 0, outputInitialized = 0;
		int cnt = infile.ScanLine("%i,%i,%i", &filenameConfirmed, &time, &outputInitialized);
		if (cnt < 2) goto error;
		if (fileSummary) fileInfo->SetFilenameConfirmed(static_cast<bool>(filenameConfirmed));
		if (fileSummary) fileInfo->SetTime(static_cast<time_t>(time));
		if (fileSummary && cnt >= 3)
		{
			bool directWriteOpt = g_Options && g_Options->GetDirectWrite();
			bool forceDirectWrite = fileInfo->GetForceDirectWrite();
			bool hasOwnOutFile = false;
			bool hasOutTmp = false;
			bool hasForceDest = false;
			if (nzbget_rs_dqh_diskstate_should_probe_direct_write_files(
				outputInitialized, directWriteOpt ? 1 : 0, forceDirectWrite ? 1 : 0) != 0)
			{
				const char* destDir = fileInfo->GetNzbInfo() ? fileInfo->GetNzbInfo()->GetDestDir() : nullptr;
				hasOwnOutFile = !fileInfo->GetOutputFilename().empty() &&
					FileSystem::FileExists(fileInfo->GetOutputFilename().c_str());
				if (destDir && !Util::EmptyStr(destDir))
				{
					NzbgetRsBuf outTmpBuf = nzbget_rs_dqh_diskstate_format_out_tmp_path(
						destDir, PATH_SEPARATOR, fileInfo->GetId());
					hasOutTmp = FileSystem::FileExists(outTmpBuf.data ? outTmpBuf.data : "");
					nzbget_rs_free(outTmpBuf);
				}
				if (forceDirectWrite && destDir && !Util::EmptyStr(destDir) && !Util::EmptyStr(fileInfo->GetFilename()))
				{
					NzbgetRsBuf forceDestBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
						destDir, PATH_SEPARATOR, fileInfo->GetFilename());
					hasForceDest = FileSystem::FileExists(forceDestBuf.data ? forceDestBuf.data : "");
					nzbget_rs_free(forceDestBuf);
				}
			}
			bool restoredOutputInit = nzbget_rs_dsp_eval_restored_output_init(
				outputInitialized,
				directWriteOpt ? 1 : 0,
				forceDirectWrite ? 1 : 0,
				hasOwnOutFile ? 1 : 0,
				hasOutTmp ? 1 : 0,
				hasForceDest ? 1 : 0) != 0;
			fileInfo->SetOutputInitialized(restoredOutputInit);
		}
	}
	else if (formatVersion >= 4)
	{
		int time;
		if (infile.ScanLine("%i", &time) != 1) goto error;
		if (fileSummary) fileInfo->SetTime(static_cast<time_t>(time));
	}

	uint32 High, Low;
	if (infile.ScanLine("%u,%u", &High, &Low) != 2) goto error;
	if (fileSummary) fileInfo->SetSize(Util::JoinInt64(High, Low));
	if (fileSummary) fileInfo->SetRemainingSize(fileInfo->GetSize());

	if (infile.ScanLine("%u,%u", &High, &Low) != 2) goto error;
	if (fileSummary) fileInfo->SetMissedSize(Util::JoinInt64(High, Low));
	if (fileSummary) fileInfo->SetRemainingSize(fileInfo->GetSize() - fileInfo->GetMissedSize());

	int parFile;
	if (infile.ScanLine("%i", &parFile) != 1) goto error;
	if (fileSummary) fileInfo->SetParFile(static_cast<bool>(parFile));

	int totalArticles, missedArticles;
	if (infile.ScanLine("%i,%i", &totalArticles, &missedArticles) != 2) goto error;
	if (fileSummary) fileInfo->SetTotalArticles(totalArticles);
	if (fileSummary) fileInfo->SetMissedArticles(missedArticles);

	int size;
	if (infile.ScanLine("%i", &size) != 1) goto error;
	for (int i = 0; i < size; i++)
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		if (fileSummary && !Util::EmptyStr(buf)) fileInfo->GetGroups()->push_back(buf);
	}

	if (articles)
	{
		if (infile.ScanLine("%i", &size) != 1) goto error;
		for (int i = 0; i < size; i++)
		{
			int PartNumber, PartSize;
			if (infile.ScanLine("%i,%i", &PartNumber, &PartSize) != 2) goto error;

			if (!infile.ReadLine(buf, sizeof(buf))) goto error;

			std::unique_ptr<ArticleInfo> articleInfo = std::make_unique<ArticleInfo>();
			articleInfo->SetPartNumber(PartNumber);
			articleInfo->SetSize(PartSize);
			articleInfo->SetMessageId(buf);
			fileInfo->GetArticles()->push_back(std::move(articleInfo));
		}
	}

	return true;

error:
	error("Error reading diskstate for file %i", fileInfo->GetId());
	return false;
}

inline bool DiskState::SaveFileState(FileInfo* fileInfo, bool completed, bool replace)
{
	debug("Saving FileState %i to disk", fileInfo->GetId());

	NzbgetRsBuf fnBuf = nzbget_rs_dqh_diskstate_format_file_state_name(
		fileInfo->GetId(), completed ? 1 : 0);
	BString<100> filename = fnBuf.data ? fnBuf.data : "";
	nzbget_rs_free(fnBuf);
	StateFile stateFile(filename, DISKSTATE_FILE_VERSION, replace);
	stateFile.SetFlush(!replace);

	StateDiskFile* outfile = stateFile.BeginWrite();
	if (!outfile)
	{
		return false;
	}

	return SaveFileState(fileInfo, *outfile, completed) && stateFile.FinishWrite();
}

inline bool DiskState::SaveFileState(FileInfo* fileInfo, StateDiskFile& outfile, bool completed)
{
	NzbgetRsDiskFileStateInput fields{};
	fields.success_articles = fileInfo->GetSuccessArticles();
	fields.failed_articles = fileInfo->GetFailedArticles();
	fields.remaining_size = fileInfo->GetRemainingSize();
	fields.success_size = fileInfo->GetSuccessSize();
	fields.failed_size = fileInfo->GetFailedSize();
	fields.filename = fileInfo->GetFilename();
	fields.hash16k = fileInfo->GetHash16k() ? fileInfo->GetHash16k() : "";
	fields.par_set_id = fileInfo->GetParSetId() ? fileInfo->GetParSetId() : "";
	fields.par_file = (int)fileInfo->GetParFile();
	fields.decoded_file_size = fileInfo->GetDecodedFileSize();
	fields.output_initialized = (int)fileInfo->GetOutputInitialized();

	std::vector<NzbgetRsDiskServerStat> stats;
	stats.reserve(fileInfo->GetServerStats()->size());
	for (ServerStat& serverStat : fileInfo->GetServerStats())
	{
		stats.push_back(NzbgetRsDiskServerStat{
			serverStat.GetServerId(),
			serverStat.GetSuccessArticles(),
			serverStat.GetFailedArticles()
		});
	}

	std::vector<NzbgetRsDiskArticleState> articles;
	articles.reserve(fileInfo->GetArticles()->size());
	for (ArticleInfo* articleInfo : fileInfo->GetArticles())
	{
		int savedStatus = 0;
		int stagedFallback = nzbget_rs_dsp_calc_staged_fallback(
			completed ? 1 : 0,
			(int)articleInfo->GetStatus(),
			(int)ArticleInfo::aiFinished,
			articleInfo->GetDupeFallbackRound(),
			articleInfo->GetResultFilename() ? 1 : 0,
			(int)ArticleInfo::aiUndefined,
			&savedStatus);
		articles.push_back(NzbgetRsDiskArticleState{
			savedStatus,
			articleInfo->GetSegmentOffset(),
			articleInfo->GetSegmentSize(),
			articleInfo->GetCrc(),
			stagedFallback
		});
	}

	WriteRsBuf(outfile, nzbget_rs_diskstate_format_file_state(
		&fields,
		stats.data(), stats.size(),
		articles.data(), articles.size()));

	outfile.Close();
	return true;
}

inline bool DiskState::LoadFileState(FileInfo* fileInfo, Servers* servers, bool completed)
{
	debug("Loading FileInfo %i from disk", fileInfo->GetId());

	NzbgetRsBuf fnBuf = nzbget_rs_dqh_diskstate_format_file_state_name(
		fileInfo->GetId(), completed ? 1 : 0);
	BString<100> filename = fnBuf.data ? fnBuf.data : "";
	nzbget_rs_free(fnBuf);
	StateFile stateFile(filename, DISKSTATE_FILE_VERSION, false, DISKSTATE_FILE_LEGACY_VERSION);

	StateDiskFile* infile = stateFile.BeginRead();
	if (!infile)
	{
		return false;
	}

	m_lastFileStateVersion = stateFile.GetFileVersion();
	return LoadFileState(fileInfo, servers, *infile, stateFile.GetFileVersion(), completed);
}

inline bool DiskState::LoadFileState(FileInfo* fileInfo, Servers* servers, StateDiskFile& infile, int formatVersion, bool completed)
{
	bool hasArticles = !fileInfo->GetArticles()->empty();

	int successArticles, failedArticles;
	if (infile.ScanLine("%i,%i", &successArticles, &failedArticles) != 2) goto error;
	fileInfo->SetSuccessArticles(successArticles);
	fileInfo->SetFailedArticles(failedArticles);

	uint32 High1, Low1, High2, Low2, High3, Low3;
	if (infile.ScanLine("%u,%u,%u,%u,%u,%u", &High1, &Low1, &High2, &Low2, &High3, &Low3) != 6) goto error;
	fileInfo->SetRemainingSize(Util::JoinInt64(High1, Low1));
	fileInfo->SetSuccessSize(Util::JoinInt64(High2, Low2));
	fileInfo->SetFailedSize(Util::JoinInt64(High3, Low3));
	if (formatVersion >= 8)
	{
		uint32 decodedHigh, decodedLow;
		if (infile.ScanLine("%u,%u", &decodedHigh, &decodedLow) != 2) goto error;
		fileInfo->SetDecodedFileSize(Util::JoinInt64(decodedHigh, decodedLow));
	}

	char buf[1024];

	if (formatVersion >= 4)
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		if (!Util::EmptyStr(buf)) fileInfo->SetFilename(buf);
	}

	if (formatVersion >= 5)
	{
		if (!infile.ReadLine(buf, sizeof(buf))) goto error;
		fileInfo->SetHash16k(*buf ? buf : nullptr);
		if (formatVersion >= 6)
		{
			if (!infile.ReadLine(buf, sizeof(buf))) goto error;
			fileInfo->SetParSetId(*buf ? buf : nullptr);
		}
		int parFile = 0;
		if (infile.ScanLine("%i", &parFile) != 1) goto error;
		fileInfo->SetParFile((bool)parFile);
	}

	if (!LoadServerStats(fileInfo->GetServerStats(), servers, infile)) goto error;

	int completedArticles;
	completedArticles = 0; //clang requires initialization in a separate line (due to goto statements)

	int size;
	if (infile.ScanLine("%i", &size) != 1) goto error;
	// the state of a file with its articles loaded can't list more (a stale or
	// damaged state file did: at() threw out of the queue coordinator). Fewer is
	// fine, as upstream has it: older queues saved some states with none
	if (!nzbget_rs_dsp_validate_article_count(
		size, hasArticles ? 1 : 0, (int)fileInfo->GetArticles()->size()))
	{
		goto error;
	}
	for (int i = 0; i < size; i++)
	{
		if (!hasArticles)
		{
			fileInfo->GetArticles()->push_back(std::make_unique<ArticleInfo>());
		}
		std::unique_ptr<ArticleInfo>& pa = fileInfo->GetArticles()->at(i);

		int statusInt = 0;

		int64 segmentOffset = 0;
		uint32 crc = 0;
		int segmentSize = 0;
		int stagedFallback = 0;
		int fields = 0;
		if (formatVersion >= 2)
		{
			fields = infile.ScanLine("%i,%" PRIi64 ",%i,%u,%i", &statusInt, &segmentOffset,
				&segmentSize, &crc, &stagedFallback);
		}
		else
		{
			fields = infile.ScanLine("%i", &statusInt);
		}
		bool needStagedCheck = nzbget_rs_dsp_need_staged_file_check(
			completed ? 1 : 0,
			formatVersion,
			statusInt,
			(int)ArticleInfo::aiFinished,
			segmentOffset,
			segmentSize,
			stagedFallback,
			pa->GetPartNumber(),
			g_Options->GetDirectWrite() ? 1 : 0,
			fileInfo->GetForceDirectWrite() ? 1 : 0) != 0;
		bool stagedFileExists = false;
		if (needStagedCheck)
		{
			NzbgetRsBuf stagedBuf = nzbget_rs_dqh_diskstate_format_staged_part_path(
				g_Options->GetTempDir(), PATH_SEPARATOR, fileInfo->GetId(), pa->GetPartNumber());
			stagedFileExists = FileSystem::FileExists(stagedBuf.data ? stagedBuf.data : "");
			nzbget_rs_free(stagedBuf);
		}
		NzbgetRsArticleLoadEval eval{};
		if (!nzbget_rs_dsp_eval_article_load_state(
			formatVersion,
			completed ? 1 : 0,
			fields,
			statusInt,
			segmentOffset,
			segmentSize,
			stagedFallback,
			pa->GetPartNumber(),
			stagedFileExists ? 1 : 0,
			g_Options->GetDirectWrite() ? 1 : 0,
			fileInfo->GetForceDirectWrite() ? 1 : 0,
			pa->GetResultFilename() ? 1 : 0,
			completedArticles,
			size,
			&eval))
		{
			goto error;
		}
		if (formatVersion >= 2)
		{
			pa->SetSegmentOffset(eval.segment_offset);
			pa->SetSegmentSize(eval.segment_size);
			pa->SetCrc(crc);
		}
		pa->SetDupeFallbackRound(eval.dupe_fallback_round);
		if (eval.need_result_filename)
		{
			NzbgetRsBuf resBuf = nzbget_rs_dqh_diskstate_format_staged_part_path(
				g_Options->GetTempDir(), PATH_SEPARATOR, fileInfo->GetId(), pa->GetPartNumber());
			pa->SetResultFilename(resBuf.data ? resBuf.data : "");
			nzbget_rs_free(resBuf);
		}
		completedArticles = eval.completed_articles;
		pa->SetStatus((ArticleInfo::EStatus)eval.status);
	}

	fileInfo->SetCompletedArticles(completedArticles);

	if (formatVersion < 8)
	{
		// data after upstream's data, where this build wrote it
		char trailer[1024];
		while (infile.ReadLine(trailer, sizeof(trailer)))
		{
			int64 decodedSize = 0;
			int outputInit = 0;
			int trailerKind = nzbget_rs_dsp_parse_legacy_file_state_trailer(
				trailer, &decodedSize, &outputInit);
			if (trailerKind == 1)
			{
				fileInfo->SetDecodedFileSize(decodedSize);
			}
			else if (trailerKind == 2)
			{
				fileInfo->SetOutputInitialized(outputInit != 0);
			}
		}
	}

	// Validate or recover DirectWrite OutputInitialized against the actual on-disk direct-write output file
	if (g_Options && (g_Options->GetDirectWrite() || fileInfo->GetForceDirectWrite()))
	{
		const char* destDir = fileInfo->GetNzbInfo() ? fileInfo->GetNzbInfo()->GetDestDir() : nullptr;
		bool hasOwnOutFile = !fileInfo->GetOutputFilename().empty() &&
			FileSystem::FileExists(fileInfo->GetOutputFilename().c_str());
		bool hasOutTmp = false;
		if (destDir && !Util::EmptyStr(destDir))
		{
			NzbgetRsBuf outTmpBuf = nzbget_rs_dqh_diskstate_format_out_tmp_path(
				destDir, PATH_SEPARATOR, fileInfo->GetId());
			hasOutTmp = FileSystem::FileExists(outTmpBuf.data ? outTmpBuf.data : "");
			nzbget_rs_free(outTmpBuf);
		}
		bool hasCompletedDest = false;
		if (nzbget_rs_dqh_diskstate_can_check_completed_dest_file(
			completed ? 1 : 0,
			fileInfo->GetForceDirectWrite() ? 1 : 0,
			Util::EmptyStr(destDir) ? 1 : 0,
			Util::EmptyStr(fileInfo->GetFilename()) ? 1 : 0) != 0)
		{
			NzbgetRsBuf compDestBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				destDir, PATH_SEPARATOR, fileInfo->GetFilename());
			hasCompletedDest = FileSystem::FileExists(compDestBuf.data ? compDestBuf.data : "");
			nzbget_rs_free(compDestBuf);
		}
		bool hasDirectWriteOutput = hasOwnOutFile || hasOutTmp || hasCompletedDest;

		int newOutputInitialized = 0;
		int pickPathKind = 0;
		nzbget_rs_dsp_reconcile_direct_write_output(
			fileInfo->GetOutputInitialized() ? 1 : 0,
			hasOwnOutFile ? 1 : 0,
			hasOutTmp ? 1 : 0,
			hasCompletedDest ? 1 : 0,
			fileInfo->GetSuccessArticles(),
			completed ? 1 : 0,
			fileInfo->GetOutputFilename().empty() ? 1 : 0,
			&newOutputInitialized,
			&pickPathKind);
		fileInfo->SetOutputInitialized(newOutputInitialized != 0);
		if (pickPathKind != 0)
		{
			NzbgetRsBuf pickedBuf = pickPathKind == 1 ?
				nzbget_rs_dqh_diskstate_format_out_tmp_path(destDir, PATH_SEPARATOR, fileInfo->GetId()) :
				nzbget_rs_tpr_article_writer_format_dest_file_path(destDir, PATH_SEPARATOR, fileInfo->GetFilename());
			fileInfo->SetOutputFilename(pickedBuf.data ? pickedBuf.data : "");
			nzbget_rs_free(pickedBuf);
		}
	}

	infile.Close();
	return true;

error:
	infile.Close();
	error("Error reading diskstate for file %i", fileInfo->GetId());
	return false;
}

inline void DiskState::DiscardFiles(NzbInfo* nzbInfo, bool deleteLog)
{
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		DiscardFile(fileInfo->GetId(), true, true, true);
	}

	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		if (completedFile.GetStatus() != CompletedFile::cfSuccess)
		{
			DiscardFile(completedFile.GetId(), true, true, true);
		}
	}

	if (deleteLog)
	{
		NzbgetRsBuf logBuf = nzbget_rs_dqh_diskstate_format_nzb_log_path(
			g_Options->GetQueueDir(), PATH_SEPARATOR, nzbInfo->GetId());
		FileSystem::DeleteFile(logBuf.data ? logBuf.data : "");
		nzbget_rs_free(logBuf);
	}
}

inline void DiskState::SaveDupInfo(DupInfo* dupInfo, StateDiskFile& outfile)
{
	WriteRsBuf(outfile, nzbget_rs_diskstate_format_dup_info(
		(int)dupInfo->GetStatus(),
		dupInfo->GetSize(),
		dupInfo->GetFullContentHash(),
		dupInfo->GetFilteredContentHash(),
		dupInfo->GetDupeScore(),
		(int)dupInfo->GetDupeMode(),
		dupInfo->GetName(),
		dupInfo->GetDupeKey()));
}

inline bool DiskState::LoadDupInfo(DupInfo* dupInfo, StateDiskFile& infile, int formatVersion)
{
	char buf[1024];

	int status;
	uint32 High, Low;
	uint32 fullContentHash, filteredContentHash = 0;
	int dupeScore, dupeMode;
	if (infile.ScanLine("%i,%u,%u,%u,%u,%i,%i", &status, &High, &Low, &fullContentHash, &filteredContentHash, &dupeScore, &dupeMode) != 7) goto error;

	dupInfo->SetStatus((DupInfo::EStatus)status);
	dupInfo->SetFullContentHash(fullContentHash);
	dupInfo->SetFilteredContentHash(filteredContentHash);
	dupInfo->SetSize(Util::JoinInt64(High, Low));
	dupInfo->SetDupeScore(dupeScore);
	dupInfo->SetDupeMode((EDupeMode)dupeMode);

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	dupInfo->SetName(buf);

	if (!infile.ReadLine(buf, sizeof(buf))) goto error;
	dupInfo->SetDupeKey(buf);

	return true;

error:
	return false;
}

inline void DiskState::SaveHistory(HistoryList* history, StateDiskFile& outfile)
{
	debug("Saving history to disk");

	outfile.PrintLine("%i", (int)history->size());
	for (HistoryInfo* historyInfo : history)
	{
		outfile.PrintLine("%i,%i,%i", historyInfo->GetId(), (int)historyInfo->GetKind(), (int)historyInfo->GetTime());

		if (historyInfo->GetKind() == HistoryInfo::hkNzb || historyInfo->GetKind() == HistoryInfo::hkUrl)
		{
			SaveNzbInfo(historyInfo->GetNzbInfo(), outfile);
		}
		else if (historyInfo->GetKind() == HistoryInfo::hkDup)
		{
			SaveDupInfo(historyInfo->GetDupInfo(), outfile);
		}
	}

	std::vector<NzbInfo*> nzbs;
	for (HistoryInfo* historyInfo : history)
	{
		if (historyInfo->GetKind() == HistoryInfo::hkNzb || historyInfo->GetKind() == HistoryInfo::hkUrl)
		{
			nzbs.push_back(historyInfo->GetNzbInfo());
		}
	}
	SaveDupeTrailer(nzbs, outfile);
}

inline bool DiskState::LoadHistory(HistoryList* history, Servers* servers, StateDiskFile& infile, int formatVersion)
{
	debug("Loading history from disk");

	int size;
	if (infile.ScanLine("%i", &size) != 1) goto error;
	for (int i = 0; i < size; i++)
	{
		std::unique_ptr<HistoryInfo> historyInfo;
		HistoryInfo::EKind kind = HistoryInfo::hkNzb;
		int id = 0;
		int time;

		int kindval = 0;
		if (infile.ScanLine("%i,%i,%i", &id, &kindval, &time) != 3) goto error;
		kind = (HistoryInfo::EKind)kindval;

		if (kind == HistoryInfo::hkNzb)
		{
			std::unique_ptr<NzbInfo> nzbInfo = std::make_unique<NzbInfo>();
			if (!LoadNzbInfo(nzbInfo.get(), servers, infile, formatVersion)) goto error;
			nzbInfo->LeavePostProcess();
			historyInfo = std::make_unique<HistoryInfo>(std::move(nzbInfo));
		}
		else if (kind == HistoryInfo::hkUrl)
		{
			std::unique_ptr<NzbInfo> nzbInfo = std::make_unique<NzbInfo>();
			if (!LoadNzbInfo(nzbInfo.get(), servers, infile, formatVersion)) goto error;
			historyInfo = std::make_unique<HistoryInfo>(std::move(nzbInfo));
		}
		else if (kind == HistoryInfo::hkDup)
		{
			std::unique_ptr<DupInfo> dupInfo = std::make_unique<DupInfo>();
			if (!LoadDupInfo(dupInfo.get(), infile, formatVersion)) goto error;
			dupInfo->SetId(id);
			historyInfo = std::make_unique<HistoryInfo>(std::move(dupInfo));
		}
		else
		{
			// a kind no version writes (a damaged record): the file can't be read on
			error("Unknown history record kind %i", kindval);
			goto error;
		}

		historyInfo->SetTime((time_t)time);

		history->push_back(std::move(historyInfo));
	}

	if (formatVersion >= 57 && formatVersion <= DISKSTATE_QUEUE_VERSION &&
		!LoadDupeTrailer([history](int id) -> NzbInfo*
			{
				for (HistoryInfo* historyInfo : history)
				{
					if (nzbget_rs_dqh_diskstate_matches_history_nzb_id(
						(int)historyInfo->GetKind(),
						(int)HistoryInfo::hkNzb,
						(int)HistoryInfo::hkUrl,
						(historyInfo->GetKind() == HistoryInfo::hkNzb || historyInfo->GetKind() == HistoryInfo::hkUrl) ?
							historyInfo->GetNzbInfo()->GetId() : 0,
						id) != 0)
					{
						return historyInfo->GetNzbInfo();
					}
				}
				return nullptr;
			}, infile)) goto error;

	return true;

error:
	error("Error reading diskstate for history");
	return false;
}

/*
 * Deletes whole download queue including history.
 */
inline void DiskState::DiscardDownloadQueue()
{
	debug("Discarding queue");

	for (const char* name : { "queue", "history" })
	{
		NzbgetRsBuf qBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			g_Options->GetQueueDir(), PATH_SEPARATOR, name);
		FileSystem::DeleteFile(qBuf.data ? qBuf.data : "");
		nzbget_rs_free(qBuf);
	}

	DirBrowser dir(g_Options->GetQueueDir());
	while (const char* filename = dir.Next())
	{
		// delete all files whose names have only characters '0'..'9'
		bool onlyNums = true;
		for (const char* p = filename; *p; p++)
		{
			if (*p < '0' || *p > '9')
			{
				onlyNums = false;
				break;
			}
		}
		if (onlyNums)
		{
			for (char suffix : { '\0', 's', 'c' })
			{
				NzbgetRsBuf fBuf = nzbget_rs_tpr_diskstate_format_queue_file_state_path(
					g_Options->GetQueueDir(), PATH_SEPARATOR, filename, suffix);
				FileSystem::DeleteFile(fBuf.data ? fBuf.data : "");
				nzbget_rs_free(fBuf);
			}
		}
	}
}

inline bool DiskState::DownloadQueueExists()
{
	debug("Checking if a saved queue exists on disk");

	// An empty queue still has saved state: the IDs handed out before its entries
	// were deleted. Load it so both generators continue past those IDs.
	for (const char* name : { "queue", "history", "ids" })
	{
		NzbgetRsBuf qBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			g_Options->GetQueueDir(), PATH_SEPARATOR, name);
		bool exists = FileSystem::FileExists(qBuf.data ? qBuf.data : "");
		nzbget_rs_free(qBuf);
		if (exists)
		{
			return true;
		}
	}
	return false;
}

inline void DiskState::DiscardFile(int fileId, bool deleteData, bool deletePartialState, bool deleteCompletedState)
{
	// info and articles file
	if (deleteData)
	{
		NzbgetRsBuf fBuf = nzbget_rs_tpr_diskstate_format_queue_id_state_path(
			g_Options->GetQueueDir(), PATH_SEPARATOR, fileId, '\0');
		FileSystem::DeleteFile(fBuf.data ? fBuf.data : "");
		nzbget_rs_free(fBuf);
	}

	// partial state file
	if (deletePartialState)
	{
		NzbgetRsBuf fBuf = nzbget_rs_tpr_diskstate_format_queue_id_state_path(
			g_Options->GetQueueDir(), PATH_SEPARATOR, fileId, 's');
		FileSystem::DeleteFile(fBuf.data ? fBuf.data : "");
		nzbget_rs_free(fBuf);
	}

	// completed state file
	if (deleteCompletedState)
	{
		NzbgetRsBuf fBuf = nzbget_rs_tpr_diskstate_format_queue_id_state_path(
			g_Options->GetQueueDir(), PATH_SEPARATOR, fileId, 'c');
		FileSystem::DeleteFile(fBuf.data ? fBuf.data : "");
		nzbget_rs_free(fBuf);
	}
}

inline void DiskState::CleanupTempDir(DownloadQueue* downloadQueue)
{
	DirBrowser dir(g_Options->GetTempDir());
	while (const char* filename = dir.Next())
	{
		int id = 0, part = 0;
		int kind = nzbget_rs_dsp_classify_temp_file(filename, &id, &part);
		bool garbage = kind == 1;
		if (kind == 2)
		{
			garbage = true;
			for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
			{
				if (nzbInfo->GetFileList()->Find(id))
				{
					garbage = false;
					break;
				}
			}
		}

		if (garbage)
		{
			NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				g_Options->GetTempDir(), PATH_SEPARATOR, filename);
			FileSystem::DeleteFile(fullFnBuf.data ? fullFnBuf.data : "");
			nzbget_rs_free(fullFnBuf);
		}
	}
}

inline void DiskState::CleanupQueueDir(DownloadQueue* downloadQueue)
{
	// Prepare sorted id lists for faster search

	std::vector<int> nzbIdList;
	std::vector<int> fileIdList;

	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		nzbIdList.push_back(nzbInfo->GetId());

		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			fileIdList.push_back(fileInfo->GetId());
		}

		for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
		{
			fileIdList.push_back(completedFile.GetId());
		}
	}

	for (HistoryInfo* historyInfo : downloadQueue->GetHistory())
	{
		if (historyInfo->GetKind() == HistoryInfo::hkNzb ||
			historyInfo->GetKind() == HistoryInfo::hkUrl)
		{
			NzbInfo* nzbInfo = historyInfo->GetNzbInfo();
			nzbIdList.push_back(nzbInfo->GetId());

			for (FileInfo* fileInfo : nzbInfo->GetFileList())
			{
				fileIdList.push_back(fileInfo->GetId());
			}

			for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
			{
				fileIdList.push_back(completedFile.GetId());
			}
		}
	}

	std::sort(nzbIdList.begin(), nzbIdList.end());
	std::sort(fileIdList.begin(), fileIdList.end());

	// Do cleanup

	int deletedFiles = 0;

	DirBrowser dir(g_Options->GetQueueDir());
	while (const char* filename = dir.Next())
	{
		bool del = nzbget_rs_dsp_is_orphan_queue_file(
			filename,
			fileIdList.data(), fileIdList.size(),
			nzbIdList.data(), nzbIdList.size()) != 0;

		if (del)
		{
			NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				g_Options->GetQueueDir(), PATH_SEPARATOR, filename);
			detail("Deleting orphaned diskstate file %s", filename);
			FileSystem::DeleteFile(fullFnBuf.data ? fullFnBuf.data : "");
			nzbget_rs_free(fullFnBuf);
			deletedFiles++;
		}
	}

	if (deletedFiles > 0)
	{
		info("Deleted %i orphaned diskstate file(s)", deletedFiles);
	}
}

/* For safety:
 * - first save to temp-file (feeds.new)
 * - then delete feeds
 * - then rename feeds.new to feeds
 */
inline bool DiskState::SaveFeeds(Feeds* feeds, FeedHistory* feedHistory)
{
	debug("Saving feeds state to disk");

	StateFile stateFile("feeds", DISKSTATE_FEEDS_VERSION, true);

	if (nzbget_rs_dqh_diskstate_should_discard_feeds_file(
		feeds->empty() ? 1 : 0, feedHistory->empty() ? 1 : 0) != 0)
	{
		stateFile.Discard();
		return true;
	}

	StateDiskFile* outfile = stateFile.BeginWrite();
	if (!outfile)
	{
		return false;
	}

	// save status
	SaveFeedStatus(feeds, *outfile);

	// save history
	SaveFeedHistory(feedHistory, *outfile);

	// now rename to dest file name
	return stateFile.FinishWrite();
}

inline bool DiskState::LoadFeeds(Feeds* feeds, FeedHistory* feedHistory)
{
	debug("Loading feeds state from disk");

	StateFile stateFile("feeds", DISKSTATE_FEEDS_VERSION, true);

	if (!stateFile.FileExists())
	{
		return true;
	}

	StateDiskFile* infile = stateFile.BeginRead();
	if (!infile)
	{
		return false;
	}

	bool ok = false;
	int formatVersion = stateFile.GetFileVersion();

	// load feed status
	if (!LoadFeedStatus(feeds, *infile, formatVersion)) goto error;

	// load feed history
	if (!LoadFeedHistory(feedHistory, *infile, formatVersion)) goto error;

	ok = true;

error:

	if (!ok)
	{
		error("Error reading diskstate for feeds");
	}

	return ok;
}

inline bool DiskState::SaveFeedStatus(Feeds* feeds, StateDiskFile& outfile)
{
	debug("Saving feed status to disk");

	std::vector<const char*> urls;
	std::vector<uint32_t> filterHashes;
	std::vector<int> lastUpdates;
	urls.reserve(feeds->size());
	filterHashes.reserve(feeds->size());
	lastUpdates.reserve(feeds->size());
	for (FeedInfo* feedInfo : feeds)
	{
		urls.push_back(feedInfo->GetUrl());
		filterHashes.push_back(feedInfo->GetFilterHash());
		lastUpdates.push_back((int)feedInfo->GetLastUpdate());
	}
	WriteRsBuf(outfile, nzbget_rs_diskstate_format_feed_status(
		urls.data(), filterHashes.data(), lastUpdates.data(), urls.size()));

	return true;
}

inline bool DiskState::LoadFeedStatus(Feeds* feeds, StateDiskFile& infile, int formatVersion)
{
	debug("Loading feed status from disk");

	int size;
	if (infile.ScanLine("%i", &size) != 1) goto error;
	for (int i = 0; i < size; i++)
	{
		char url[1024];
		if (!infile.ReadLine(url, sizeof(url))) goto error;

		char filter[1024];
		if (formatVersion == 2)
		{
			if (!infile.ReadLine(filter, sizeof(filter))) goto error;
		}

		uint32 filterHash = 0;
		if (formatVersion >= 3)
		{
			if (infile.ScanLine("%u", &filterHash) != 1) goto error;
		}

		int lastUpdate = 0;
		if (infile.ScanLine("%i", &lastUpdate) != 1) goto error;

		for (FeedInfo* feedInfo : feeds)
		{
			if (nzbget_rs_dsp_feed_status_matches(
				feedInfo->GetUrl(),
				url,
				formatVersion,
				formatVersion == 2 ? feedInfo->GetFilter() : "",
				formatVersion == 2 ? filter : "",
				feedInfo->GetFilterHash(),
				filterHash) != 0)
			{
				feedInfo->SetLastUpdate((time_t)lastUpdate);
			}
		}
	}

	return true;

error:
	error("Error reading feed status from disk");
	return false;
}

inline bool DiskState::SaveFeedHistory(FeedHistory* feedHistory, StateDiskFile& outfile)
{
	debug("Saving feed history to disk");

	std::vector<int> statuses;
	std::vector<int> lastSeens;
	std::vector<const char*> urls;
	statuses.reserve(feedHistory->size());
	lastSeens.reserve(feedHistory->size());
	urls.reserve(feedHistory->size());
	for (FeedHistoryInfo& feedHistoryInfo : feedHistory)
	{
		statuses.push_back((int)feedHistoryInfo.GetStatus());
		lastSeens.push_back((int)feedHistoryInfo.GetLastSeen());
		urls.push_back(feedHistoryInfo.GetUrl());
	}
	WriteRsBuf(outfile, nzbget_rs_diskstate_format_feed_history(
		statuses.data(), lastSeens.data(), urls.data(), urls.size()));

	return true;
}

inline bool DiskState::LoadFeedHistory(FeedHistory* feedHistory, StateDiskFile& infile, int formatVersion)
{
	debug("Loading feed history from disk");

	int size;
	if (infile.ScanLine("%i", &size) != 1) goto error;
	for (int i = 0; i < size; i++)
	{
		int status = 0;
		int lastSeen = 0;
		int r = infile.ScanLine("%i,%i", &status, &lastSeen);
		if (r != 2) goto error;

		char url[1024];
		if (!infile.ReadLine(url, sizeof(url))) goto error;

		feedHistory->emplace_back(url, (FeedHistoryInfo::EStatus)(status), (time_t)(lastSeen));
	}

	return true;

error:
	error("Error reading feed history from disk");
	return false;
}

inline void DiskState::CalcFileStats(DownloadQueue* downloadQueue, int formatVersion)
{
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		nzbInfo->UpdateCurrentStats();
	}
}

inline bool DiskState::SaveAllFileInfos(DownloadQueue* downloadQueue)
{
	bool ok = true;
	StateFile stateFile("files", DISKSTATE_FILE_VERSION, true);
	if (!downloadQueue->GetQueue()->empty())
	{
		StateDiskFile* outfile = stateFile.BeginWrite();
		if (!outfile)
		{
			return false;
		}

		// save file-infos

		int fileCount = 0;
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			fileCount += nzbInfo->GetFileList()->size();
		}
		outfile->PrintLine("%i", fileCount);

		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			for (FileInfo* fileInfo : nzbInfo->GetFileList())
			{
				outfile->PrintLine("%i", fileInfo->GetId());
				SaveFileInfo(fileInfo, *outfile, false);
			}
		}

		// now rename to dest file name
		ok = stateFile.FinishWrite();
	}
	else
	{
		stateFile.Discard();
	}

	return ok;
}

inline bool DiskState::LoadAllFileInfos(DownloadQueue* downloadQueue)
{
	if (downloadQueue->GetQueue()->empty())
	{
		return true;
	}

	StateFile stateFile("files", DISKSTATE_FILE_VERSION, false, DISKSTATE_FILE_LEGACY_VERSION);
	StateDiskFile* infile = nullptr;
	bool useHibernate = false;

	if (stateFile.FileExists())
	{
		infile = stateFile.BeginRead();
		useHibernate = infile != nullptr;
		if (useHibernate)
		{
			int fileCount = 0;
			for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
			{
				fileCount += nzbInfo->GetFileList()->size();
			}
			int size = 0;
			int scanCnt = infile->ScanLine("%i", &size);
			useHibernate = scanCnt == 1 && size == fileCount;
		}
		if (!useHibernate)
		{
			stateFile.Discard();
		}
	}

	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		RawFileList brokenFileInfos;

		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			bool res = false;
			if (useHibernate)
			{
				int id = 0;
				infile->ScanLine("%i", &id);
				if (id == fileInfo->GetId())
				{
					res = LoadFileInfo(fileInfo, *infile, stateFile.GetFileVersion(), true, false);
				}
			}
			if (!res)
			{
				res = LoadFile(fileInfo, true, false);
			}
			if (!res)
			{
				brokenFileInfos.push_back(fileInfo);
			}
		}

		for (FileInfo* fileInfo : brokenFileInfos)
		{
			nzbInfo->GetFileList()->Remove(fileInfo);
		}
	}

	return true;
}

inline void DiskState::DiscardQuickFileInfos()
{
	StateFile stateFile("files", DISKSTATE_FILE_VERSION, false, DISKSTATE_FILE_LEGACY_VERSION);
	stateFile.Discard();
}

inline bool DiskState::LoadAllFileStates(DownloadQueue* downloadQueue, Servers* servers)
{
	NzbgetRsBuf cacheFlagBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		g_Options->GetQueueDir(), PATH_SEPARATOR, "acache");
	bool cacheWasActive = FileSystem::FileExists(cacheFlagBuf.data ? cacheFlagBuf.data : "");
	nzbget_rs_free(cacheFlagBuf);

	DirBrowser dir(g_Options->GetQueueDir());
	while (const char* filename = dir.Next())
	{
		int id = 0;
		char suffix = '\0';
		if (nzbget_rs_dqh_diskstate_parse_queue_file_id_and_suffix(filename, &id, &suffix) != 0)
		{
			if (suffix == 'c' || (g_Options->GetContinuePartial() && !cacheWasActive))
			{
				for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
				{
					for (FileInfo* fileInfo : nzbInfo->GetFileList())
					{
						if (fileInfo->GetId() == id)
						{
							// a damaged state file (cut short by a power loss: partial states
							// are written in place) costs only its own progress: the load left
							// blank articles behind, which then downloaded nothing, and the rest
							// of the queue dir's states weren't read
							int successArticles = fileInfo->GetSuccessArticles();
							int failedArticles = fileInfo->GetFailedArticles();
							int64 remainingSize = fileInfo->GetRemainingSize();
							int64 successSize = fileInfo->GetSuccessSize();
							int64 failedSize = fileInfo->GetFailedSize();
							if (!LoadFileState(fileInfo, servers, suffix == 'c'))
							{
								warn("Discarding damaged download state %s", filename);
								fileInfo->GetArticles()->clear();
								fileInfo->SetSuccessArticles(successArticles);
								fileInfo->SetFailedArticles(failedArticles);
								fileInfo->SetRemainingSize(remainingSize);
								fileInfo->SetSuccessSize(successSize);
								fileInfo->SetFailedSize(failedSize);
								NzbgetRsBuf dmgFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
									g_Options->GetQueueDir(), PATH_SEPARATOR, filename);
								FileSystem::DeleteFile(dmgFnBuf.data ? dmgFnBuf.data : "");
								nzbget_rs_free(dmgFnBuf);
								goto next;
							}
							if (m_lastFileStateVersion > DISKSTATE_FILE_VERSION)
							{
								// an earlier build of this branch wrote it: rewritten in the
								// format an nzbget without duplicate repair reads too
								SaveFileState(fileInfo, suffix == 'c');
							}
							fileInfo->GetArticles()->clear();
							fileInfo->SetPartialState(suffix == 'c' ? FileInfo::psCompleted : FileInfo::psPartial);
							goto next;
						}
					}
				}
			}
			else
			{
				NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
					g_Options->GetQueueDir(), PATH_SEPARATOR, filename);
				FileSystem::DeleteFile(fullFnBuf.data ? fullFnBuf.data : "");
				nzbget_rs_free(fullFnBuf);
			}
		}
		next:;
	}

	return true;
}

inline bool DiskState::SaveStats(Servers* servers, ServerVolumes* serverVolumes)
{
	debug("Saving stats to disk");

	StateFile stateFile("stats", DISKSTATE_STATS_VERSION, true);

	if (servers->empty())
	{
		stateFile.Discard();
		return true;
	}

	StateDiskFile* outfile = stateFile.BeginWrite();
	if (!outfile)
	{
		return false;
	}

	// save server names
	SaveServerInfo(servers, *outfile);

	// save stat
	SaveVolumeStat(serverVolumes, *outfile);

	// now rename to dest file name
	return stateFile.FinishWrite();
}

inline bool DiskState::LoadStats(Servers* servers, ServerVolumes* serverVolumes, bool* perfectMatch)
{
	debug("Loading stats from disk");

	StateFile stateFile("stats", DISKSTATE_STATS_VERSION, true);

	if (!stateFile.FileExists())
	{
		return true;
	}

	StateDiskFile* infile = stateFile.BeginRead();
	if (!infile)
	{
		return false;
	}

	bool ok = false;
	int formatVersion = stateFile.GetFileVersion();

	if (!LoadServerInfo(servers, *infile, formatVersion, perfectMatch)) goto error;

	if (formatVersion >=2)
	{
		if (!LoadVolumeStat(servers, serverVolumes, *infile, formatVersion)) goto error;
	}

	ok = true;

error:

	if (!ok)
	{
		error("Error reading diskstate for statistics");
	}

	return ok;
}

inline bool DiskState::SaveServerInfo(Servers* servers, StateDiskFile& outfile)
{
	debug("Saving server info to disk");

	std::vector<const char*> names;
	std::vector<const char*> hosts;
	std::vector<int> ports;
	std::vector<const char*> users;
	names.reserve(servers->size());
	hosts.reserve(servers->size());
	ports.reserve(servers->size());
	users.reserve(servers->size());
	for (NewsServer* newsServer : servers)
	{
		names.push_back(newsServer->GetName());
		hosts.push_back(newsServer->GetHost());
		ports.push_back(newsServer->GetPort());
		users.push_back(newsServer->GetUser());
	}
	WriteRsBuf(outfile, nzbget_rs_diskstate_format_server_info(
		names.data(), hosts.data(), ports.data(), users.data(), names.size()));

	return true;
}

inline bool DiskState::LoadServerInfo(Servers* servers, StateDiskFile& infile, int formatVersion, bool* perfectMatch)
{
	debug("Loading server info from disk");

	*perfectMatch = true;

	int size = 0;
	std::vector<int> refIds;
	std::vector<CString> refNameStrs;
	std::vector<CString> refHostStrs;
	std::vector<int> refPorts;
	std::vector<CString> refUserStrs;
	if (infile.ScanLine("%i", &size) != 1 || size < 0) goto error;
	refIds.reserve(size);
	refNameStrs.reserve(size);
	refHostStrs.reserve(size);
	refPorts.reserve(size);
	refUserStrs.reserve(size);
	for (int i = 0; i < size; i++)
	{
		char name[1024];
		if (!infile.ReadLine(name, sizeof(name))) goto error;

		char host[200];
		if (!infile.ReadLine(host, sizeof(host))) goto error;

		int port;
		if (infile.ScanLine("%i", &port) != 1) goto error;

		char user[100];
		if (!infile.ReadLine(user, sizeof(user))) goto error;

		refIds.push_back(i + 1);
		refNameStrs.emplace_back(name);
		refHostStrs.emplace_back(host);
		refPorts.push_back(port);
		refUserStrs.emplace_back(user);
	}

	{
		std::vector<const char*> srvNames;
		std::vector<const char*> srvHosts;
		std::vector<int> srvPorts;
		std::vector<const char*> srvUsers;
		srvNames.reserve(servers->size());
		srvHosts.reserve(servers->size());
		srvPorts.reserve(servers->size());
		srvUsers.reserve(servers->size());
		for (NewsServer* newsServer : servers)
		{
			srvNames.push_back(newsServer->GetName());
			srvHosts.push_back(newsServer->GetHost());
			srvPorts.push_back(newsServer->GetPort());
			srvUsers.push_back(newsServer->GetUser());
		}

		std::vector<const char*> refNames;
		std::vector<const char*> refHosts;
		std::vector<const char*> refUsers;
		refNames.reserve(refNameStrs.size());
		refHosts.reserve(refHostStrs.size());
		refUsers.reserve(refUserStrs.size());
		for (size_t i = 0; i < refNameStrs.size(); i++)
		{
			refNames.push_back(*refNameStrs[i]);
			refHosts.push_back(*refHostStrs[i]);
			refUsers.push_back(*refUserStrs[i]);
		}

		std::vector<int> outStateIds(servers->size(), 0);
		*perfectMatch = nzbget_rs_dsp_match_servers(
			srvNames.data(), srvHosts.data(), srvPorts.data(), srvUsers.data(), servers->size(),
			refIds.data(), refNames.data(), refHosts.data(), refPorts.data(), refUsers.data(), refIds.size(),
			outStateIds.data()) != 0;

		size_t idx = 0;
		for (NewsServer* newsServer : servers)
		{
			newsServer->SetStateId(outStateIds[idx++]);
		}
	}

	debug("******** MATCHING NEWS-SERVERS **********");
	for (NewsServer* newsServer : servers)
	{
		*perfectMatch = *perfectMatch && newsServer->GetStateId();
		debug("Server %i -> %i", newsServer->GetId(), newsServer->GetStateId());
		debug("Server %i.Name: %s", newsServer->GetId(), newsServer->GetName());
		debug("Server %i.Host: %s:%i", newsServer->GetId(), newsServer->GetHost(), newsServer->GetPort());
	}

	debug("All servers perfectly matched: %s", *perfectMatch ? "yes" : "no");

	return true;

error:
	error("Error reading server info from disk");

	return false;
}

inline bool DiskState::SaveVolumeStat(ServerVolumes* serverVolumes, StateDiskFile& outfile)
{
	debug("Saving volume stats to disk");

	outfile.PrintLine("%i", (int)serverVolumes->size());
	for (ServerVolume& serverVolume : serverVolumes)
	{
		const auto& articlesPerDays = serverVolume.GetArticlesPerDays();
		std::vector<NzbgetRsRpcArticlePair> articleEntries;
		articleEntries.reserve(articlesPerDays.size());
		for (const ServerVolume::Articles& articles : articlesPerDays)
		{
			articleEntries.push_back(NzbgetRsRpcArticlePair{ articles.failed, articles.success });
		}

		const ServerVolume::VolumeArray* bps = serverVolume.BytesPerSeconds();
		const ServerVolume::VolumeArray* bpm = serverVolume.BytesPerMinutes();
		const ServerVolume::VolumeArray* bph = serverVolume.BytesPerHours();
		const ServerVolume::VolumeArray* bpd = serverVolume.BytesPerDays();

		NzbgetRsDiskVolumeStatInput volInput{
			serverVolume.GetFirstDay(),
			static_cast<int>(serverVolume.GetDataTime()),
			static_cast<int>(serverVolume.GetCustomTime()),
			static_cast<int>(serverVolume.GetCountersResetTime()),
			serverVolume.GetTotalBytes(),
			serverVolume.GetCustomBytes(),
			bps->data(), bps->size(),
			bpm->data(), bpm->size(),
			bph->data(), bph->size(),
			bpd->data(), bpd->size(),
			articleEntries.data(), articleEntries.size()
		};
		WriteRsBuf(outfile, nzbget_rs_diskstate_format_volume_stat_item(&volInput));
	}

	return true;
}

inline bool DiskState::LoadVolumeStat(Servers* servers, ServerVolumes* serverVolumes, StateDiskFile& infile, int formatVersion)
{
	debug("Loading volume stats from disk");

	int size;
	std::vector<int32_t> srvStateIds;
	std::vector<int32_t> srvIds;
	if (infile.ScanLine("%i", &size) != 1) goto error;
	srvStateIds.reserve(servers->size());
	srvIds.reserve(servers->size());
	for (NewsServer* newsServer : servers)
	{
		srvStateIds.push_back(newsServer->GetStateId());
		srvIds.push_back(newsServer->GetId());
	}
	for (int i = 0; i < size; i++)
	{
		ServerVolume* serverVolume = nullptr;

		int slotIdx = nzbget_rs_dqh_diskstate_select_volume_server_slot(
			i, srvStateIds.data(), srvIds.data(), srvStateIds.size());
		if (slotIdx >= 0)
		{
			serverVolume = &serverVolumes->at(slotIdx);
		}

		int firstDay, dataTime, customTime, countersResetTime;
		uint32 High1, Low1, High2 = 0, Low2 = 0;
		if (formatVersion >= 4)
		{
			if (infile.ScanLine("%i,%i,%i,%i", &firstDay, &dataTime,&customTime, &countersResetTime) != 4) goto error;
			if (infile.ScanLine("%u,%u,%u,%u", &High1, &Low1, &High2, &Low2) != 4) goto error;

			if (serverVolume)
			{
				serverVolume->SetCustomTime(static_cast<time_t>(customTime));
				serverVolume->SetCountersResetTime(static_cast<time_t>(countersResetTime));
			}
		}
		else if (formatVersion == 3)
		{
			if (infile.ScanLine("%i,%i,%i", &firstDay, &dataTime,&customTime) != 3) goto error;
			if (infile.ScanLine("%u,%u,%u,%u", &High1, &Low1, &High2, &Low2) != 4) goto error;

			if (serverVolume)
			{
				serverVolume->SetCustomTime(static_cast<time_t>(customTime));
				time_t firstDayTime = static_cast<time_t>(firstDay * 86400);
				serverVolume->SetCountersResetTime(firstDayTime);
			}
		}
		else
		{
			if (infile.ScanLine("%i,%i", &firstDay, &dataTime) != 2) goto error;
			if (infile.ScanLine("%u,%u", &High1, &Low1) != 2) goto error;
		}
		if (serverVolume) serverVolume->SetFirstDay(firstDay);
		if (serverVolume) serverVolume->SetDataTime((time_t)dataTime);
		if (serverVolume) serverVolume->SetTotalBytes(Util::JoinInt64(High1, Low1));
		if (serverVolume) serverVolume->SetCustomBytes(Util::JoinInt64(High2, Low2));

		ServerVolume::VolumeArray* VolumeArrays[] = { 
			serverVolume ? serverVolume->BytesPerSeconds() : nullptr,
			serverVolume ? serverVolume->BytesPerMinutes() : nullptr,
			serverVolume ? serverVolume->BytesPerHours() : nullptr,
			serverVolume ? serverVolume->BytesPerDays() : nullptr };
		for (int k=0; k < 4; k++)
		{
			ServerVolume::VolumeArray* volumeArray = VolumeArrays[k];

			// the seconds, minutes and hours arrays have fixed sizes (the counters
			// index them by the clock), the days one grows: a count from the file
			// sized them, and a wrong one (damaged file) threw at every start or
			// left them too short for the counters
			int arrSize;
			if (infile.ScanLine("%i", &arrSize) != 1) goto error;
			int targetVolumeSize = 0;
			if (!nzbget_rs_dsp_calc_volume_slot_size(k, arrSize, &targetVolumeSize)) goto error;
			if (volumeArray) volumeArray->assign(targetVolumeSize, 0);

			for (int j = 0; j < arrSize; j++)
			{
				if (infile.ScanLine("%u,%u", &High1, &Low1) != 2) goto error;
				if (nzbget_rs_dqh_diskstate_can_store_volume_slot(
					volumeArray != nullptr ? 1 : 0,
					j,
					volumeArray ? volumeArray->size() : 0) != 0)
				{
					(*volumeArray)[j] = Util::JoinInt64(High1, Low1);
				}
			}
		}

		if (formatVersion >= 4)
		{
			uint32 arrSize;
			if (infile.ScanLine("%u", &arrSize) != 1) goto error;
			if (arrSize > 100000) goto error;

			ServerVolume::ArticlesArray articlesArr;
			articlesArr.reserve(arrSize);

			ServerVolume::Articles articles;
			while (arrSize--)
			{
				if (infile.ScanLine("%u,%u", &articles.failed, &articles.success) != 2) goto error;
				articlesArr.push_back(articles);
			}

			if (serverVolume) serverVolume->SetArticlesPerDays(std::move(articlesArr));
		}
	}

	return true;

error:
	error("Error reading volume stats from disk");

	return false;
}

inline void DiskState::WriteCacheFlag()
{
	NzbgetRsBuf flagBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		g_Options->GetQueueDir(), PATH_SEPARATOR, "acache");
	BString<1024> flagFilename = flagBuf.data ? flagBuf.data : "";
	nzbget_rs_free(flagBuf);

	StateDiskFile outfile;
	if (!outfile.Open(flagFilename, StateDiskFile::omWrite))
	{
		error("Error saving diskstate: Could not create file %s", *flagFilename);
		return;
	}

	outfile.Close();
}

inline void DiskState::DeleteCacheFlag()
{
	NzbgetRsBuf flagBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
		g_Options->GetQueueDir(), PATH_SEPARATOR, "acache");
	FileSystem::DeleteFile(flagBuf.data ? flagBuf.data : "");
	nzbget_rs_free(flagBuf);
}

inline void DiskState::AppendNzbMessage(int nzbId, Message::EKind kind, const char* text)
{
	NzbgetRsBuf logBuf = nzbget_rs_dqh_diskstate_format_nzb_log_path(
		g_Options->GetQueueDir(), PATH_SEPARATOR, nzbId);
	BString<1024> logFilename = logBuf.data ? logBuf.data : "";
	nzbget_rs_free(logBuf);

	StateDiskFile outfile;
	if (!outfile.Open(logFilename, StateDiskFile::omAppend))
	{
		error("Error saving log: Could not create file %s", *logFilename);
		return;
	}

	time_t tm = Util::CurrentTime();
	time_t rawtime = tm + g_Options->GetTimeCorrection();

	BString<100> time;
	Util::FormatTime(rawtime, time, 100);

	WriteRsBuf(outfile, nzbget_rs_diskstate_format_nzb_message_line(
		*time, (int)tm, (int)kind, text, LINE_ENDING));

	outfile.Close();
}

inline void DiskState::LoadNzbMessages(int nzbId, MessageList* messages)
{
	// Important:
	//   - Other threads may be writing into the log-file at any time;
	//   - The log-file may also be deleted from another thread;

	NzbgetRsBuf logBuf = nzbget_rs_dqh_diskstate_format_nzb_log_path(
		g_Options->GetQueueDir(), PATH_SEPARATOR, nzbId);
	BString<1024> logFilename = logBuf.data ? logBuf.data : "";
	nzbget_rs_free(logBuf);

	if (!FileSystem::FileExists(logFilename))
	{
		return;
	}

	StateDiskFile infile;
	if (!infile.Open(logFilename, StateDiskFile::omRead))
	{
		error("Error reading log: could not open file %s", *logFilename);
		return;
	}

	int id = 0;
	char line[1024];
	while (infile.ReadLine(line, sizeof(line)))
	{
		Util::TrimRight(line);

		int time = 0;
		int kindInt = (int)Message::mkError;
		size_t textOffset = 0;
		if (!nzbget_rs_dsp_parse_log_line(line, &time, &kindInt, &textOffset)) goto exit;
		messages->emplace_back(++id, (Message::EKind)kindInt, (time_t)time, line + textOffset);
	}

exit:
	infile.Close();
	return;
}

#ifndef NZBFILE_H
#define NZBFILE_H

#include <algorithm>
#include <set>

class NzbFile
{
public:
	NzbFile(const char* fileName, const char* category)
		: m_fileName{ fileName ? fileName : "" }
	{
		debug("Creating NZBFile");

		m_nzbInfo = std::make_unique<NzbInfo>();
		m_nzbInfo->SetFilename(fileName);
		m_nzbInfo->SetCategory(category);
		m_nzbInfo->BuildDestDirName();
	}

	bool Parse()
	{
		std::unique_ptr<NzbgetRsNzbFileParser, void (*)(NzbgetRsNzbFileParser*)> parser(
			nzbget_rs_nzbfile_parser_new(m_fileName.c_str()),
			nzbget_rs_nzbfile_parser_free);

		NzbgetRsCtype ct = RsCtype();
		NzbgetRsNzbFileSummary summary{};
		bool ok = nzbget_rs_nzbfile_parser_parse_file(
			parser.get(),
			g_Options ? g_Options->GetParIgnoreExt() : "",
			&ct,
			nullptr,
			[](void*) -> int
			{
				try
				{
					FileInfo tmp;
					return tmp.GetId();
				}
				catch (...)
				{
					return 0;
				}
			},
			[](const char* subject, const char* filename)
			{
				try
				{
					detail("Extracting a filename from Subject %s", subject);
					detail("Extracted Filename: %s", filename);
				}
				catch (...)
				{
				}
			},
			[](const char* const* filenames, size_t* indices, size_t count)
			{
				try
				{
					std::sort(indices, indices + count,
						[filenames](size_t a, size_t b)
						{
							return strcmp(filenames[a], filenames[b]) > 0;
						});
				}
				catch (...)
				{
				}
			},
			&summary) != 0;

		for (size_t i = 0; i < summary.messages_count; i++)
		{
			int kind = 0;
			const char* text = nullptr;
			if (nzbget_rs_nzbfile_parser_get_message(parser.get(), i, &kind, &text) && text)
			{
				m_nzbInfo->AddMessage(
					kind == 2 ? Message::mkError : Message::mkWarning,
					text);
			}
		}

		if (!ok)
		{
			return false;
		}

		m_nzbInfo->SetManyDupeFiles(summary.many_dupe_files != 0);
		m_nzbInfo->SetFileCount(summary.file_count);
		m_nzbInfo->SetTotalArticles(summary.total_articles);
		m_nzbInfo->SetFailedArticles(summary.failed_articles);
		m_nzbInfo->SetCurrentFailedArticles(summary.cur_failed_articles);
		m_nzbInfo->SetSize(summary.size);
		m_nzbInfo->SetRemainingSize(summary.remaining_size);
		m_nzbInfo->SetFailedSize(summary.failed_size);
		m_nzbInfo->SetCurrentFailedSize(summary.cur_failed_size);
		m_nzbInfo->SetParSize(summary.par_size);
		m_nzbInfo->SetParFailedSize(summary.par_failed_size);
		m_nzbInfo->SetParCurrentFailedSize(summary.par_cur_failed_size);
		m_nzbInfo->SetRemainingParCount(summary.remaining_par_count);
		m_nzbInfo->SetFullContentHash(summary.full_content_hash);
		m_nzbInfo->SetFilteredContentHash(summary.filtered_content_hash);

		for (size_t i = 0; i < summary.kept_files_count; i++)
		{
			NzbgetRsNzbKeptFileView view{};
			if (!nzbget_rs_nzbfile_parser_get_file(parser.get(), i, &view))
			{
				continue;
			}
			std::unique_ptr<FileInfo> fileInfo = std::make_unique<FileInfo>(view.id);
			fileInfo->SetNzbInfo(m_nzbInfo.get());
			fileInfo->SetSubject(view.subject ? view.subject : "");
			fileInfo->SetFilename(view.filename ? view.filename : "");
			fileInfo->SetTime(view.time);
			fileInfo->SetSize(view.size);
			fileInfo->SetRemainingSize(view.remaining_size);
			fileInfo->SetMissedSize(view.missed_size);
			fileInfo->SetTotalArticles(view.total_articles);
			fileInfo->SetMissedArticles(view.missed_articles);
			fileInfo->SetParFile(view.par_file != 0);

			for (size_t g = 0; g < view.groups_count; g++)
			{
				fileInfo->GetGroups()->push_back(view.groups_ptrs[g] ? view.groups_ptrs[g] : "");
			}

			ArticleList* articles = fileInfo->GetArticles();
			articles->reserve(view.articles_count);
			for (size_t a = 0; a < view.articles_count; a++)
			{
				std::unique_ptr<ArticleInfo> article = std::make_unique<ArticleInfo>();
				article->SetPartNumber(view.article_parts[a]);
				article->SetSize(view.article_sizes[a]);
				article->SetMessageId(view.article_msg_ids[a] ? view.article_msg_ids[a] : "");
				articles->push_back(std::move(article));
			}

			if (view.cut_short)
			{
				m_cutShortFiles.insert(fileInfo.get());
			}

			m_nzbInfo->GetFileList()->Add(std::move(fileInfo));
		}

		m_nzbInfo->UpdateMinMaxTime();

		if (g_Options && g_Options->GetServerMode())
		{
			for (FileInfo* fileInfo : m_nzbInfo->GetFileList())
			{
				g_DiskState->SaveFile(fileInfo);
				fileInfo->GetArticles()->clear();
			}
		}

		m_password = summary.password ? summary.password : "";
		m_metaName = summary.meta_name ? summary.meta_name : "";
		m_metaTitle = summary.meta_title ? summary.meta_title : "";
		m_category = summary.category ? summary.category : "";

		if (!m_metaName.empty() && m_nzbInfo)
		{
			m_nzbInfo->SetMetaName(m_metaName);
		}

		return true;
	}

	const char* GetFileName() const { return m_fileName.c_str(); }
	const std::string& GetCategoryFromFile() const { return m_category; }
	std::unique_ptr<NzbInfo> DetachNzbInfo() { return std::move(m_nzbInfo); }
	const std::string& GetPassword() const { return m_password; }
	const std::string& GetMetaName() const { return m_metaName; }
	const std::string& GetMetaTitle() const { return m_metaTitle; }

	void LogDebugInfo() { info(" NZBFile %s", m_fileName.c_str()); }

	static int DeclaredParts(const char* subject)
	{
		NzbgetRsCtype ct = RsCtype();
		return nzbget_rs_nzb_declared_parts(subject, &ct);
	}

	const std::set<FileInfo*>& GetCutShortFiles() const { return m_cutShortFiles; }

private:
	static NzbgetRsCtype RsCtype()
	{
#ifdef __GLIBC__
		return {*__ctype_b_loc(), (unsigned short)_ISspace, (unsigned short)_ISdigit,
			reinterpret_cast<const int*>(*__ctype_tolower_loc()), CHAR_MIN < 0};
#else
		static thread_local unsigned short classes[256];
		static thread_local int lower[384];
		for (int c = 0; c < 256; c++)
		{
			classes[c] = (isspace(c) ? 1 : 0) | (isdigit(c) ? 2 : 0);
		}
		for (int c = -128; c < 256; c++)
		{
			lower[c + 128] = c < 0 ? c : tolower(c);
		}
		return {classes, 1, 2, lower + 128, CHAR_MIN < 0};
#endif
	}

	std::unique_ptr<NzbInfo> m_nzbInfo;
	std::string m_fileName;
	std::string m_category;
	std::string m_password;
	std::string m_metaName;
	std::string m_metaTitle;
	std::set<FileInfo*> m_cutShortFiles;
};

#endif

#endif
#endif
